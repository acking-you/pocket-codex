# 调研方案：通用 ACP 接入（分支 `research-acp`）

状态：v1.0，§11 的 D1–D21 已于 2026-09-30 全部按推荐确认。调研日期 2026-09-30。技术方案见 [TRD](TRD.md)（按本文确认结果编写）。术语沿用 [`CONTEXT.md`](../../CONTEXT.md)，现有 OpenCode 接入见 [TRD](../opencode-unified/TRD.md) 和 [ADR-0002](../adr/0002-opencode-shared-session-ui.md)。

约定：方括号里的键（如 [P1]、[A6]）对应 §12 的一手来源，每个来源都固定了 commit、版本或抓取日期。**未验证** 表示没能从一手来源或本机实测确认；**推断** 表示从源码读出、但没有实际运行确认。

## 0. 结论摘要

1. ACP 稳定线是 `protocolVersion: 1`，唯一的标准传输是 stdio。`session/list`、`session/load`、`session/resume`、`session/close` 都已稳定；v2 仍是草案，会删掉 `session/load`、`fs/*`、`terminal/*`，并重做消息模型 [P1][P5]。首期只实现 v1。
2. 四个目标 agent 都能走 ACP，但只有 OpenCode 是原生实现。Claude Code 和 Codex 都要通过 `agentclientprotocol` 组织维护的 npm 适配器，而且两个适配器**默认都带一份自己的引擎**：Claude Code 2.1.284、`@openai/codex` ^0.159.1 [A1][A6]。
3. 三个关键 agent 都支持 `session/list` 和完整回放的 `session/load`，Claude 和 Codex 还能列出 CLI 自己创建的会话。但回放一次就是全量，没有分页，条目也没有时间戳 [P5][A1][A6][A10][A11]。
4. ACP 的前提是"一个客户端启动一个 agent 子进程"，没有多客户端挂接，没有断线重连，也没有排队或补充（steer）的语义 [P5]。Pocket-Codex 要求"手机断线后主机继续跑、多台设备同时看"，所以主机必须有一个**有状态的 ACP Hub**：它是唯一的 ACP 客户端，持有进程、会话、待办和有界转录，再把会话复用给多个控制器。
5. 推荐架构（§8 方案 C）：主机侧的 Hub 对控制器仍然讲 ACP v1（走 WebSocket），并加上 `_pcx/*` 扩展；bridge 新增 ACP 引擎，把 ACP 映射成现有的 ThreadItem/AppEvent 和能力描述。这延续了 ADR-0002 的原则：主机不伪装成 Codex，协议翻译放在 bridge。
6. 安装渠道以官方 ACP Registry 为参考，但 registry 每小时直接往 main 推新版本，npx 分发也没有哈希 [P6]。所以安装器要用 Pocket-Codex 内置的白名单清单锁定版本和完整性，并自带私有 Node 运行时。
7. 官方 ACP Rust crate 会开启 `serde_json/preserve_order`，这会改变 `json_digest` 依赖的键顺序（`crates/pocket-codex-core/src/history_sync.rs:342-345`），也会增加安卓端体积。推荐沿用项目对 Codex 的做法：在本地实现 v1 的 wire 类型，用官方 schema 做契约测试。
8. 安卓不适合当宿主：Android 10 起应用不能执行运行时下载的文件，Claude Code 也没有 Android 构建 [A15][A5]。安卓只做控制器，负责远程查看安装状态、触发安装和启动托管。

## 1. 需求回顾

已确认的需求（详见目标说明）：

| # | 需求 |
|---|---|
| R1 | ACP 是新增的通用 provider，现有的 Codex、OpenCode 原生接入保持不变 |
| R2 | 在 ACP 能力范围内，历史越全越好；不读 agent 私有文件，不写针对某个 agent 的解析逻辑 |
| R3 | 首批：Claude Code、Codex、OpenCode 1.x、OpenCode 2.x，全部走同一套实现；差异只体现在清单、配置和能力协商上 |
| R4 | App 帮用户安装 agent、适配器和依赖，覆盖 macOS / Windows / Linux 主机；安卓能远程查看、安装、升级 |
| R5 | 交付调研方案（本文）和施工级 TRD |

## 2. ACP 协议现状

### 2.1 版本与稳定性

| 项 | 现状 | 来源 |
|---|---|---|
| 稳定线 | `protocolVersion: 1`（整数，只在 breaking change 时递增）；新功能通过 capability 增加 | [P1] README、`schema/v1/schema.json` |
| 草案 | v2，2026-07-20 发布；官方要求用版本协商**加** feature flag 门控，并继续支持 v1 | [P5] `/announcements/acp-v2-draft`、`/protocol/v2/migration` |
| 最新产物 | schema-v1.23.0、schema-v2.0.0-alpha.5（都是 2026-09-18）；Rust `agent-client-protocol` 2.2.0、`agent-client-protocol-schema` 1.9.1；TS `@agentclientprotocol/sdk` 1.5.1 | [P2][P3] |
| 功能生命周期 | RFD：Draft → Active → Preview → Completed（即稳定化）；不稳定的功能放在 `schema.unstable.json` 和 Rust 的 `unstable_*` feature 中 | [P5] `/rfds/about` |
| 节奏 | 没有官方节奏；实际上 v1 schema 每 1–4 周发一版 | [P2] |

wire 兼容性只由 `initialize` 协商出的版本和能力决定，与产物版本号无关 [P1]。

### 2.2 传输

- stdio：客户端把 agent 作为子进程启动，走 NDJSON（JSON-RPC 2.0，每条消息占一行，消息内不能有换行），stderr 用来输出日志 [P5] `/protocol/v1/transports`。
- Streamable HTTP + WebSocket：RFD 处于 Active 状态，定位是 v1 的增量功能。约定是单一的 `/acp` 端点，`GET` 加 `Upgrade` 就升级为 WebSocket。按 RFD 原文，会话可以在断线后通过 `session/load` 重新挂上，但**断线期间的消息不会重放** [P5] `/rfds/streamable-http-websocket-transport`。官方 crate `agent-client-protocol-http` 2.2.0 已经实现了服务端和客户端 [P4]。
- 结论：Pocket-Codex 与 agent 之间只能用 stdio。控制器到主机这一段可以借用 RFD 里 WebSocket profile 的形态，但断线、多端这些语义要自己定义。

### 2.3 方法与门控（v1）

| 方法 | 方向 | 门控 | 备注 |
|---|---|---|---|
| `initialize` | C→A | — | 交换 `clientCapabilities` / `agentCapabilities`；未声明的能力一律视为不支持 |
| `authenticate` / `logout` | C→A | `authMethods` / `auth.logout` | 见 §2.7 |
| `session/new` | C→A | — | `cwd`（绝对路径）、`mcpServers[]` |
| `session/load` | C→A | `loadSession` | 先把整段对话作为 `session/update` 重放，全部发完才返回 |
| `session/list` | C→A | `sessionCapabilities.list` | `{cwd?, cursor?}` → `{sessions[], nextCursor?}`，2026-03-09 稳定 |
| `session/resume` | C→A | `sessionCapabilities.resume` | 只恢复上下文，**不**重放，2026-04-22 稳定 |
| `session/close` / `session/delete` | C→A | `sessionCapabilities.close` / `.delete` | close = 取消 + 释放资源；delete 会从 list 中移除 |
| `session/fork` | C→A | unstable | Rust 需要 `unstable_session_fork` |
| `session/prompt` | C→A | `promptCapabilities` | 整个 turn 期间都挂起，返回 `{stopReason}` |
| `session/cancel` | C→A（通知） | — | 挂起的权限请求必须以 `cancelled` 回应 |
| `session/set_config_option` | C→A | 会话响应里带 `configOptions` | 模型选择也改走这里（category `model`）；`set_model` 已于 2026-06-01 删除 |
| `session/set_mode` | C→A | 会话响应里带 `modes` | 已被 config options 取代，v2 删除 |
| `session/request_permission` | A→C | — | 见 §2.5 |
| `fs/read_text_file` / `fs/write_text_file` | A→C | `fs.readTextFile` / `.writeTextFile` | v2 删除 |
| `terminal/*` | A→C | `terminal` | v2 删除 |
| `elicitation/create` | A→C | `elicitation.form` / `.url` | 2026-07-24 稳定 |
| `$/cancel_request` | 双向 | — | 接收方必须回应原请求（结果或 `-32800`） |

来源：[P1] `schema/v1/meta*.json`、`schema.json`；[P5] `/protocol/v1/*`、`/rfds/updates`。

### 2.4 `session/update` 变体（v1 稳定集合共 11 个）

`user_message_chunk`、`agent_message_chunk`、`agent_thought_chunk`（每个都带单个 ContentBlock 和可选的 `messageId`）、`tool_call`、`tool_call_update`（打补丁，只有 `toolCallId` 是必填）、`plan`（每次整体替换）、`available_commands_update`、`current_mode_update`、`config_option_update`、`session_info_update`（`title`、`updatedAt`）、`usage_update`（`used`、`size`、`cost?`）[P1]。v1 的 `SessionUpdate` 是封闭的 oneOf，不稳定的变体必须由客户端声明能力后才会下发。

### 2.5 历史语义（决定 R2 能做到什么程度）

- `session/load` 必须重放"entire conversation"，但 v1 没有逐项规定是否包含 thought、tool call、plan；v2 迁移指南才明确"与实时流相同" [P5]。实际行为以各 agent 为准，见 §3。
- 没有分页，也没有局部加载，每次都是全量。v2 的 `replayFrom` 目前只有 `start` 一个值 [P5] `/rfds/v2/session-resume-replay`。
- **所有条目都没有时间戳**，唯一的时间字段是会话级的 `updatedAt`。v1 的 `messageId` 是可选的，而且**不要求跨 load 保持稳定**；v2 才要求必填且稳定 [P5] `/rfds/message-id`。
- `session/list` 不规定排序，也没有"运行中"字段；cursor 是不透明的，不能持久化 [P5] `/protocol/v1/session-list`。
- agent 进程退出后会话怎么处理，规范**没有定义**。能不能用 load/resume 恢复，取决于 agent 是否持久化了会话 [P5]。

### 2.6 权限、工具调用与 elicitation

- `session/request_permission {sessionId, toolCall, options[]}`，`options[].kind` 取 `allow_once`、`allow_always`、`reject_once`、`reject_always` 之一（只作 UI 提示）。回应是 `{outcome:"selected", optionId}` 或 `{outcome:"cancelled"}`。"always"的选择由哪一方记住，规范没有规定 [P5] `/protocol/v1/tool-calls`。
- `ToolKind` 有 `read`、`edit`、`delete`、`move`、`search`、`execute`、`think`、`fetch`、`switch_mode`、`other`；状态有 `pending`、`in_progress`、`completed`、`failed`。内容有三种：`content`、`diff{path, oldText?, newText}`、`terminal{terminalId}`；另有 `locations[]`、`rawInput`、`rawOutput` [P1]。
- elicitation 的 form 模式用受限的 JSON Schema 收集非敏感数据；url 模式要求客户端显示目标 host 并取得用户同意 [P5] `/protocol/v1/elicitation`。

### 2.7 配置项、命令、计划、用量、认证

- Config options（`select` 或 `boolean`，category 为 `mode`、`model`、`model_config`、`thought_level` 或 `_自定义`）：设置后，响应和 `config_option_update` 返回**完整**列表。boolean 类型要等客户端声明 `session.configOptions.boolean` 后才会下发 [P5]。
- Slash 命令通过 `available_commands_update` 通告，调用方式就是把 `/name args` 当作普通 prompt 发出去 [P5]。
- 认证 [P5] `/protocol/v1/authentication`：
  - `agent` 类型：客户端调用 `authenticate{methodId}`，登录流程由 agent 自己完成。
  - `terminal` 类型：要求客户端声明 `auth.terminal`。客户端用同一个 agent 程序另开一个交互式终端，**追加** `args`，退出码为 0 即成功，然后重连并重新 `initialize`。
  - 需要认证时，请求返回 `-32000`。目前没有查询认证状态的方法，`auth/status` 还停在 RFD Draft。
  - `env_var` 类型已于 2026-07-27 删除。
  - 注意：registry 的 `AUTHENTICATION.md` 写的是"replace"默认参数，和规范（追加）冲突，以规范为准 [P6]。

### 2.8 扩展与并发

- 扩展方法和通知以 `_` 开头（例如 `_zed.dev/...`），自定义数据放在 `_meta` 里；不认识的请求返回 `-32601`，不认识的通知忽略 [P5] `/protocol/v1/extensibility`。
- 一个连接上可以有多个会话；**不支持多个客户端挂接同一个进程或会话**。v1 没有排队或补充语义，prompt 在整个 turn 期间挂起；v2 也明确"不规定 queueing / steering" [P5] `/rfds/v2/prompt`。

## 3. 目标 agent 支持矩阵

| | Claude Code | Codex | OpenCode 1.x | OpenCode 2.x |
|---|---|---|---|---|
| 入口 | 适配器 `@agentclientprotocol/claude-agent-acp` 0.84.0 [A1][A2] | 适配器 `@agentclientprotocol/codex-acp` 2.0.1（TypeScript）[A6] | 原生 `opencode acp`，1.18.33 [A10] | 原生 `opencode acp`，npm `@opencode/cli` 2.0.20 [A11][A12] |
| 自带引擎 | 是：SDK 0.3.284 附带 Claude Code 2.1.284 原生二进制；可用 `CLAUDE_CODE_EXECUTABLE` 覆盖（`src/acp-agent.ts:1720`、`:8749`）[A1][A3] | 是：依赖 `@openai/codex` ^0.159.1，作为 `codex app-server` 子进程运行，不在进程内链接；可用 `CODEX_PATH` 覆盖（`src/CodexJsonRpcConnection.ts:21-25`）[A6] | 自身即引擎 | 自身即引擎；每个 ACP 连接启动一个私有的 `serve --stdio` server，不连用户的后台服务 [A11][A12] |
| 运行时 | Node ≥22 | Node（未声明 engines） | Bun 单文件二进制 | Bun 单文件二进制（npm 包含 `postinstall`） |
| 共享数据 | `~/.claude`（设置、transcript）[A1] | `CODEX_HOME`（登录、config、rollout、state DB）[A6] | OpenCode 数据目录 | 同一数据目录（实测，§3.5） |
| 登录 | terminal auth：`--cli auth login …`；另有 gateway [A1] | API key、浏览器登录、device code（需要 URL elicitation）、gateway [A6] | 事先运行 `opencode auth login`；`authenticate` 什么也不做 [A10] | 同 1.x [A11] |
| list / load | ✅（每页 1000）/ ✅ 完整回放，含工具调用、子代理、计划 [A1] | ✅（包括 CLI 和 VS Code 的会话）/ ✅ 分页流式回放 [A6] | ✅（只列根会话，每页 100）/ ✅ [A10] | ✅（服务端分页，100）/ ✅（每批 200）[A11] |
| resume/close/delete/fork | ✅/✅/✅/✅ | ✅/✅/✅/✅ | ✅/✅/❌/✅ | ✅/✅/✅/✅ |
| 客户端 fs/terminal | 不用 | 不用 | 获批后调用 `fs/write_text_file` 回写，**不检查**客户端是否声明（推断）[A10] | 只有客户端声明了才回写 [A11] |
| 模式与配置项 | `default`、`acceptEdits`、`plan`、`auto`、`bypassPermissions`；model、effort、fast [A1] | `read-only`、`workspace-write`、`agent`（Auto review，默认）、`agent-full-access`；model、effort、fast [A6] | agent、model | `model`、`effort`（有变体时）、`mode`；也支持 `set_mode` [A11] |
| 权限选项 | 四种都有 [A1] | once/always/reject_once（网络规则还会用到 reject_always）[A6] | once/always/reject [A10] | 同 1.x |
| 私有扩展 | `_meta.claudeCode.promptQueueing`、`_session/steering`、终端输出 `_meta` [A1] | `_meta.steering.supported` [A6] | — | `_meta["opencode/child-session-updates"]` [A11] |
| Registry 条目 | `claude-acp`（npx，license 写 `proprietary`） | `codex-acp`（npx） | `opencode`（binary，6 个平台，都带 sha256） | **不在** registry（2.x 没有 GitHub Release 资产） |

registry 数据按 2026-09-30 的 CDN 复核 [P7]。

### 3.1 Claude Code 要点

- Claude Code CLI 本身**没有**原生 ACP（issue #24411 仍是 open）[A5]。适配器已经改过两次名，旧的 `@zed-industries/*` 包都已 deprecated [A2]。
- 平台来自 SDK 的 optionalDependencies：darwin x64/arm64、linux x64/arm64（glibc 和 musl）、win32 x64/arm64。linux-arm64 包解压后约 242 MB [A3]。
- 认证：只有客户端声明了 terminal auth（规范写法或 `_meta["terminal-auth"]` 写法），才会返回 `claude-ai-login` / `console-login`。检测到 `SSH_*` / `NO_BROWSER` 时改为返回 `claude-login`，进入 TUI 执行 `/login` [A1]。
- **合规**（需法务确认；D21 之后，默认配置已不涉及这个问题）：Anthropic 的条款写着"Anthropic does not permit third-party developers to offer Claude.ai login into their own applications"，同时允许"an end user signing in to the unmodified Claude Code binary with their own Claude subscription, including where a platform hosts Claude Code" [A4]。结论：Pocket-Codex 不能提供自己的 Claude.ai 登录入口，只能启动未修改的 Claude Code 自带的登录。
- 已知问题：#883（`mcpServers` 里的 stdio MCP 从未传给模型）、#976（后续 prompt 会杀掉后台子代理）[A2]。ACP 创建的会话在 `claude --resume` 选择器里默认被隐藏（#84421）[A5]。

### 3.2 Codex 要点

- openai/codex **没有**原生 ACP：155 个 workspace 成员里没有 acp 相关条目，#30052 仍是 open [A8]。
- 旧的 Rust 版 `zed-industries/codex-acp` 已于 2026-07-22 归档，它直接链接 `codex-core` 等 crate [A7]，是 AGENTS.md §8.1 禁止的形态。现行的 TS 版改为走外部的 `codex app-server` 子进程。
- 与 §8.1 冲突的地方：不设 `CODEX_PATH` 时，适配器会用 npm 依赖里自带的 Codex（0.159.x），和用户 PATH 里的 `codex`（本机是 0.154.0）无关；上游还说明其他版本"may not be compatible" [A6]。两个版本共写同一个 `~/.codex` 时，state DB 和 rollout 是否兼容，**未验证**。
- 远程登录：只有 device code 加 URL elicitation 能完全在手机上完成 [A6]。宿主机已经 `codex login` 过的话，会直接复用（推断）。
- 已知问题：#516，超大会话（约 610 MiB）回放时丢了最近的消息；#310、#477、#406，`config.toml` 里的沙箱和审批设置被模式覆盖 [A6]。

### 3.3 OpenCode 1.x 要点

- `opencode acp` 从 v0.15.10 开始提供（PR #2947），1.x 和 2.x 都保留 [A10]。
- 进程内起一个 HTTP server（`--port 0`、`--hostname 127.0.0.1`）。全局配置里的 `server.hostname` 可能让它监听 `0.0.0.0` [A10]，TRD 要把启动参数固定下来。
- 认证只认 `_meta["terminal-auth"]` 写法，不支持规范的 `auth.terminal`（实测见 §3.5）。
- 已知问题：#48232，Task 子代理的权限请求不转发给 ACP 客户端，子代理会一直挂起 [A13]。

### 3.4 OpenCode 2.x 要点

- 实现位于 `packages/cli/src/acp/*`（v2.0.18 = `cd9a14a6`）。ACP 进程启动私有 server `opencode serve --stdio --port 0`，带随机 Basic 密码，stdin 断开就退出。官方文档写明"It does not connect to the shared background service"[A11][A12]。
- 与 ADR-0002 的关系：原生 OpenCode provider 是附接用户的后台服务，ACP 版是自有私有 server，两者共享存储。两个 server 同时写 SQLite 会不会冲突，**未验证**。
- 已知问题：#50236，从 2.0.4 起 `session/new` 返回的目录缺少用户配置的 provider、agent 和默认模型；#38121，表单和提问还没接到 elicitation [A13]。
- 分发：npm `@opencode/cli`（12 个平台包，含 musl 和 baseline，有 `postinstall`）、`opencode.ai/v2/install`、brew、AUR、Docker [A12]。官方迁移文档说明 V1 和 V2 共用 `opencode` 命令名，默认不再并存安装 [A12]。

### 3.5 本机实测（2026-09-30，macOS 15.7.4 arm64）

已安装的程序：`claude` 2.1.220、`codex-cli` 0.154.0（两者都没有 acp 子命令）、`opencode` v2.0.18（`~/.opencode/bin`）、OpenChamber 内置的 opencode v2.0.16、`node` v22.20.0 / `npx` 10.9.3。`codex-acp`、`claude-agent-acp`、`gemini` 都没安装，按约束没有下载，所以它们的能力数据来自源码和 registry 的每日 protocol matrix [P9]。

方法：在空目录下启动 `opencode acp`，经 stdin 发两行 NDJSON，读 stdout，stdin 关闭后进程以 0 退出，事后检查没有残留进程。

```json
{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false,"auth":{"terminal":true}},"clientInfo":{"name":"acp-probe","title":"ACP probe","version":"0.0.1"}}}
{"jsonrpc":"2.0","id":1,"method":"session/list","params":{}}
```

结果（2.0.18，initialize 耗时 972 ms）：

```json
{"protocolVersion":1,
 "agentCapabilities":{"loadSession":true,"mcpCapabilities":{"http":true,"sse":false},
   "promptCapabilities":{"embeddedContext":true,"image":true},
   "sessionCapabilities":{"close":{},"delete":{},"fork":{},"list":{},"resume":{}},
   "_meta":{"opencode/child-session-updates":true}},
 "authMethods":[{"id":"opencode-login","name":"Login with opencode","description":"Run `opencode auth login` in the terminal"}],
 "agentInfo":{"name":"OpenCode","version":"2.0.18"}}
```

- 声明了 `auth.terminal`，返回的 authMethods 里仍然没有 `type:"terminal"`。
- `session/list {}` 返回 94 条并带 `nextCursor`，每条只有 `sessionId`、`cwd`、`title`、`updatedAt`。列表跨越所有项目，还包含 OpenChamber 后台服务刚创建的会话，说明存储是共享的。
- `session/list {"cwd":"<空目录>"}` 返回 0 条，不带 `nextCursor`。
- 2.0.16 的 initialize 除版本号外逐字段一致，耗时 1619 ms。

## 4. 安装渠道

| 渠道 | 覆盖 | 完整性 | 适用 |
|---|---|---|---|
| ACP Registry `https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json` [P6][P7] | 41 个 agent；binary 只有 6 个平台键（`darwin/linux/windows` × `aarch64/x86_64`），不区分 glibc/musl，也没有 Android | binary 的 `sha256` 是可选的；npx/uvx 没有哈希；**没有签名**；每小时的 cron 把新版本直接提交到 main | 发现 agent、提示新版本 |
| npm registry | Claude、Codex 适配器，OpenCode 2.x | 每个 tarball 带 `dist.integrity`（sha512），但依赖范围会漂移：codex-acp 的依赖都是 `^` 范围 [A6]；claude-agent-acp 的直接依赖是精确版本，传递依赖仍会漂移 [A2] | 用私有 Node 加锁定版本安装 |
| GitHub Release | OpenCode 1.x（zip/tar.gz，含 musl 和 baseline）[A10] | registry 为 6 个目标都给了 sha256 | 直接下载 |
| nodejs.org 官方包 | Node 运行时，全平台 | `SHASUMS256.txt`（带 GPG 签名） | 私有 Node 运行时 |

Zed 的做法可以借鉴 [P8]：
- 拉取 registry，最多每小时刷新一次；只处理 `binary` 和 `npx`，忽略 `uvx`。
- binary 装进按"版本 + URL + sha256"命名的目录；有 sha256 就校验，没有的话改用 GitHub Release asset 的 `digest`。
- npx 用 `npm install <pkg> --save-exact` 装到 `external_agents/registry/npx/<id>`，再用 Zed 自带的 node 运行。

安装体积：Claude 的平台二进制约 242 MB [A3]，OpenCode 和 Codex 的原生二进制都是数十到上百 MB（**未逐一测量**）。安装器要显示进度，并检查磁盘空间。

## 5. 可借鉴的实现

| 实现 | 借鉴点 | 来源 |
|---|---|---|
| Zed `agent_server_store.rs` / `agent_registry_store.rs` | registry 拉取和缓存、binary 与 npx 的安装布局、sha256 校验、清理旧版本；SSH 远程项目由远端 headless server 解析和安装 agent，agent 跑在远端 | [P8] |
| ACP proxy-chains RFD + `agent-client-protocol-conductor` | "位于客户端和 agent 之间的 ACP 组件"这一定位，Hub 就是一个带扇出的 proxy | [P5] `/rfds/proxy-chains`；[P4] |
| `agent-client-protocol-http` | 单一 `/acp` 端点、WebSocket 升级的形态 | [P4] |
| 本仓库 OpenCode 接入 | bridge 深模块引擎、能力描述、待办重发、fake 上游测试 | `docs/opencode-unified/TRD.md` |
| 第三方移动和远程客户端（Happy、Runmote、VACP 等） | 只有项目自述，**未验证**，不作为设计依据 | [P5] `/get-started/clients` |

## 6. 现有代码的接缝

| 接缝 | 现状 | ACP 接入的影响 |
|---|---|---|
| 服务键 | `ServiceKind` 有 `#[serde(other)] Unknown`，但 `FromStr` 是严格匹配的（`crates/pocket-codex-core/src/service.rs:36-89`）；`pcxu:` 键的解析走 core（`crates/pocket-codex-account-proto/src/key.rs:93-122`） | 在 core 里新增 kind；老客户端会把它解析成 Unknown 并丢弃 |
| 后端列服务 | OpenCode 要客户端显式传 `include_opencode` 才列出（`crates/pocket-codex-backend/src/api.rs:267-274`、`:331-332`） | 新 kind 也需要一个开关，避免老客户端看到 |
| bridge 分发 | 没有 provider trait；约 30 个 `app_*` 函数都以 `if opencode::is_opencode(&key)` 开头（`crates/pocket-codex-bridge/src/api/bridge.rs:778-2214`） | 在每个函数现有的 OpenCode 分支前面加一个 ACP 分支（TRD 的 T4），Codex 和 OpenCode 的实现不动 |
| 能力描述 | `app_capabilities` 的全部字段都由一个布尔值推出（`crates/pocket-codex-bridge/src/api/bridge.rs:1409-1427`）；Dart 侧是静态常量（`apps/flutter/lib/src/bridge_api.dart:232-282`） | ACP 的能力来自协商结果，是动态的：DTO 要追加字段，Dart 要按服务键缓存 |
| 托管 | Codex 自己启动并监管进程（`crates/pocket-codex-bridge/src/engine/serve.rs:649-899`）；OpenCode 附接用户服务、停止时不碰进程（`crates/pocket-codex-bridge/src/engine/serve_opencode.rs:124-513`）；实例名跨 provider 唯一（`crates/pocket-codex-bridge/src/engine/serve.rs:689-693`） | ACP 是自有托管，形态接近 Codex |
| 待办重发 | OpenCode 在 `thread_resume` 时重新发出待办（`crates/pocket-codex-bridge/src/engine/opencode/ops.rs:100-128`） | Hub 保存待办，控制器重连后重发 |
| 历史同步 | `SessionHistorySource` 只有两个方法（`crates/pocket-codex-host-svc/src/history_sync.rs:20-29`），router 只挂在 Codex 的 `serve()` 上（`crates/pocket-codex-host-svc/src/lib.rs:116-118`）；bridge 只接受 `codex/app-server-v2`（`crates/pocket-codex-bridge/src/engine/session_sync.rs:96-119`） | 新增 `acp/hub-v1` 的 source 和控制器投影 |
| 配置 | `Config` 是 `deny_unknown_fields`（`crates/pocket-codex-core/src/config.rs:40-67`） | ACP 设置不能新增为 `config.toml` 的顶层表，否则老版本读这个文件会失败 |
| 二进制查找 | Codex：显式路径 → config → PATH（`crates/pocket-codex-bridge/src/engine/serve.rs:546-577`）；macOS GUI 通过登录 shell 取 PATH（`apps/flutter/macos/Runner/ShellEnvironment.swift:5-51`） | 托管目录里的 agent 用绝对路径，不依赖 PATH |
| 安装 | 仓库里没有任何下载或校验 provider 程序的代码 | 安装器从零开始写 |
| 测试 | fake 上游（`crates/pocket-codex-host-svc/tests/opencode.rs`）、fake 连接（`crates/pocket-codex-bridge/src/engine/opencode/engine_tests.rs:124-152`）、Dart fake（`apps/flutter/test/fake_bridge_api.dart:449-529`） | 照搬为 fake ACP agent 和 fake Hub |

## 7. 映射可行性

ACP 到现有 DTO 的映射（在 bridge 里做，和 OpenCode 的 `mapping.rs` 同层）：

| ACP | ThreadItem / AppEvent | 说明 |
|---|---|---|
| `user_message_chunk` | `userMessage` | 按 `messageId`（没有就按连续段）合并；图片放进 images |
| `agent_message_chunk` / `agent_thought_chunk` | `agentMessage` / `reasoning`，外加 delta 事件 | 同上 |
| `tool_call` + `tool_call_update` | 按 `kind` 映射：`execute` → `commandExecution`；`edit`/`delete`/`move` → `fileChange`（用 diff 内容合成统一 diff）；`fetch` → `webSearch`；其余 → `dynamicToolCall` | 以 `toolCallId` 为键做 upsert；`failed` 时在末尾追加 `[error]` |
| `plan` | `plan`（沿用 `encode_plan`） | 整体替换 |
| `usage_update` | `thread/tokenUsage/updated` | `size` 作为 context window |
| `session_info_update` | `thread/name/updated` | |
| `config_option_update` / `current_mode_update` | 运行时配置更新 | 模型和 effort 选择器走现有控件，其余选项用通用面板 |
| `request_permission` | `item/commandExecution/requestApproval`，raw 里带 `acpOptions` | 卡片按 options 动态生成按钮 |
| `elicitation/create`（form / url） | `item/tool/requestUserInput` / 新的 URL 卡片 | form 的转换规则与 OpenCode 表单相同 |
| prompt 响应的 `stopReason` | `turn/completed`：`end_turn` → completed，`cancelled` → interrupted，其余 → failed 并附原因 | |

降级（能力为 false 时隐藏入口）：
- 补充（steer）：v1 没有这个语义。
- 重命名：没有对应方法。
- 压缩：compaction 还是 unstable。
- 限额、Guardian、Fast 或权限预设：这些是 Codex 专有控件。agent 自己提供的 fast、mode 选项会出现在通用配置面板里。
- 子会话：没有标准语义。
- 外部写入监控：只能靠轮询 `updatedAt` 后重新加载。
- 轮次时长：回放出来的历史没有时间，只有 Hub 亲眼看到的实时轮次才有时长。

## 8. 候选架构

### 方案 A：主机侧 ACP 客户端，直接翻译成 Pocket 的 DTO

主机启动 agent，把 ACP 翻译成 ThreadItem/AppEvent，再通过自定义 API 发给控制器；bridge 只做转发。

### 方案 B：主机侧只做网关，bridge 里的 ACP 引擎负责翻译

主机只把 stdio 桥接成 WebSocket（参照 OpenCode 网关），bridge 是真正的 ACP 客户端。

### 方案 C（推荐）：主机侧有状态的 ACP Hub，bridge 里的 ACP 引擎负责翻译

```
               主机（桌面 App，自有托管）                                        控制器（任意设备）
┌──────────────────────────────────────────────────────────────────┐   ┌───────────────────────────────────┐
│ agent 子进程：claude-agent-acp / codex-acp / opencode acp / 自定义 │   │ Flutter 会话界面（同一套）          │
│        ▲ stdio NDJSON，ACP v1（Hub 是唯一的 ACP 客户端）           │   │   │ BridgeApi app_*                 │
│ ACP Hub（host-svc）                                               │   │   ▼                               │
│   进程监管 · 会话注册表 · 有界转录 · 待办 · 排队                  │   │ bridge：按 kind 三路分发            │
│   对控制器：ACP v1 over WebSocket + `_pcx/*` 扩展  ◄── pb 注册 ───┼───┼─► ACP 引擎（新增）：ACP → DTO      │
│   key: …:acp:<name>                                               │   │    能力描述来自协商结果            │
│ meta：通用路由 + /history/v1（acp/hub-v1）+ /acp/v1/agents ◄── pb ─┼───┼─► 历史同步、远程安装管理          │
│ 安装器：白名单清单 · 托管目录 · 完整性校验                        │   │                                   │
└──────────────────────────────────────────────────────────────────┘   └───────────────────────────────────┘
```

- Hub 对 agent 是标准的 ACP 客户端。对控制器，它表现为一个"虚拟 agent"：`initialize`、`session/new`、`session/prompt`、`session/cancel`、`set_config_option`、`request_permission` 都保持 ACP 语义。多端挂接、有界窗口、运行中清单、待办重发这些 ACP 没有定义的部分，放在 `_pcx/*` 扩展方法里。
- Hub 只做 ACP 层面的归并（按 `messageId` 合并 chunk、按 `toolCallId` upsert），不做 Codex 语义的翻译。映射到 DTO 的工作留在 bridge，与 ADR-0002 一致。

### 对比

| 维度 | A | B | C |
|---|---|---|---|
| 手机断线后轮次继续运行 | ✅ | ❌ stdio 与连接绑定，进程或轮次会丢；即使网关保住进程，ACP 也没法重新挂接挂起中的请求 | ✅ |
| 多控制器同时看同一会话 | ✅ | ❌ 每个连接各起一个进程，实时事件互相看不到 | ✅ Hub 扇出 |
| 待办保留与重发 | ✅ | ❌ | ✅ |
| 有界窗口和磁盘缓存 | ✅ | ❌ 每次打开都要全量回放，再经 relay 传输 | ✅ 回放只发生在主机本地 |
| 忠于上游、主机不做语义翻译（ADR-0002） | ❌ 主机要编码 UI 语义，DTO 必须主机和控制器同步升级 | ✅ | ✅ 主机只做 ACP 归并 |
| 将来直连远程 ACP agent（官方 HTTP/WS 传输） | ❌ | ✅ | ✅ bridge 本来就是 ACP 客户端 |
| 主机复杂度 | 高 | 低 | 中高 |
| bridge 复杂度 | 低 | 中 | 中 |

结论：B 直接违背 Pocket-Codex 的核心体验（断线继续、多端同看）；A 违背 ADR-0002；推荐 C。

## 9. 安卓分析

- **安卓当宿主：不可行，不建议做。**
  - Android 10 起，targetSdk ≥ 29 的应用不能执行 app home 里可写的文件（W^X），所以运行时下载 agent 这条路走不通，只能把二进制作为 native lib 打进 APK [A15]。这既违背 R4"App 帮用户装、不打进安装包"的前提，也会让 APK 暴涨（仅 Claude 就约 242 MB）。
  - Claude Code 没有 Android 构建（#50270，`linux-arm64-android` 包在 npm 上是 404）[A5]。
  - OpenCode 只有社区方案（PR #33010 / #48405，都是 open）[A13]。
  - Codex 的 musl 静态二进制理论上可行，**未验证**。
  - `nodejs-mobile` 最新是 18.x，低于 Claude 适配器要求的 Node 22 [A16]。
- **安卓作为控制器（纳入范围）**：
  - 会话界面：共用 Dart 界面和 bridge 引擎，与 OpenCode 的经验一致，不需要安卓专用代码。
  - 远程管理：通过主机 meta 的 `/acp/v1/agents*` 路由，查看安装状态和版本，触发安装或升级（仅限白名单锁定的版本），查看进度和错误，以及启动托管。
  - 前提：主机上至少有一个在线的托管实例（任意 provider），因为 meta 是控制器能连到主机的唯一通道。
  - 登录：Codex 的 device code 可以在手机上完成；Claude 和 OpenCode 的 terminal 登录只能在主机上做，手机上只显示"需要在主机上登录"及原因。

## 10. 风险

| 风险 | 缓解 |
|---|---|
| ACP 迭代快，v2 会删掉 `session/load`、`fs/*` 等 | 只用 v1 稳定面；wire 类型在本地实现，用固定版本的官方 schema 做契约测试；v2 的迁移另立项 |
| 适配器自带的引擎和用户的 CLI 共写 `~/.codex` / `~/.claude`，版本不一致 | 由 §11 的 D6/D7 决定；TRD 里做版本检测和提示 |
| 全量回放体积大（codex-acp #516） | Hub 设回放预算，超出后只保留尾部，并标出"更早的历史超出回放上限" |
| Anthropic 条款限制第三方提供 Claude.ai 登录 | 不做自己的登录入口，只启动未修改的 Claude Code 自带登录；**需法务确认** |
| registry 每小时自动推版本、npx 没有哈希，存在供应链风险 | 内置白名单清单锁定版本和完整性（D12） |
| OpenCode 2 的私有 server 与后台服务共写 SQLite（未验证） | TRD 实测；出问题时提示用户不要同时用两种 OpenCode provider |
| OpenCode 1.x 子代理的权限请求不转发（#48232），会一直挂起 | 界面显示运行时长并提供停止执行；在 TRD 已知限制里写明 |
| OpenCode ≥2.0.4 的 `session/new` 缺少用户配置（#50236） | TRD 实测，必要时作为数据驱动的 quirk 处理 |
| `session/load` 有副作用：会话进入 agent 内存，还会连接 MCP | 只在用户打开会话时 load；空闲时用 `session/close` 释放；不发 prompt |
| 用户同时在终端里用 CLI 写同一个会话 | 轮询 `updatedAt` 后重新加载；Hub 不能保证这种场景下的一致性，界面上要写明 |
| terminal auth 有规范和 `_meta` 两种写法 | 两种都声明，按 agent 返回的写法处理 |
| 引入 `preserve_order` 导致历史指纹不一致；`config.toml` 是 `deny_unknown_fields` | 不依赖官方 crate（D3）；ACP 设置放在独立文件 |
| 远程安装等于在主机上远程执行 | 只允许白名单里锁定的版本，主机设开关，写审计日志（D14） |

## 11. 决策清单（2026-09-30 已确认，均采用推荐项）

| # | 决策 | 选项 | 推荐 |
|---|---|---|---|
| D1 | 总体架构 | A 主机翻译成 DTO / B 纯网关 / **C 有状态 Hub** | **C**，理由见 §8 |
| D2 | 协议版本 | **只实现 v1** / 同时实现 v2 草案 | **只实现 v1**；映射层按 upsert 模型设计，方便以后迁移 |
| D3 | wire 类型 | **本地实现 v1 子集，加官方 schema 契约测试** / 依赖官方 crate | **本地实现**，避开 `preserve_order` 和安卓体积问题 |
| D4 | 托管语义 | **自有托管：每个实例一个 agent 进程，多会话复用** / 每个会话一个进程 | **每个实例一个进程**。停止托管时先取消运行中的轮次（界面二次确认），再结束进程 |
| D5 | 服务键 | **新 kind `acp`，`pcx:<device>:acp:<name>`** / 每种 agent 一个 kind | **`acp`**。agent 身份放在能力描述里，实例名默认取清单 id，跨 provider 唯一；后端加 `include_acp` 开关 |
| D6 | Codex-ACP 与 §8.1 | **(a) 强制设 `CODEX_PATH` 指向用户的 codex，并按用户版本选一个兼容的 codex-acp 版本** / (b) 用适配器自带的 Codex（需要修订 §8.1）/ (c) 首批不做 | **(a)**，符合 §8.1，也没有两个版本共写 `~/.codex` 的问题；版本不兼容时提示升级，不回退 |
| D7 | Claude Code 引擎 | **(a) 用适配器依赖的 Claude Code（随适配器锁定版本）** / (b) 用 `CLAUDE_CODE_EXECUTABLE` 指向用户的 claude | **(a)**，(b) 作为高级选项。§8.1 只约束 Codex；(b) 的兼容性未验证 |
| D8 | 认证与合规 | **主机侧完成：主机桌面打开终端做 terminal auth；支持 URL elicitation；不把 PTY 转发到手机；不提供自有的 Claude.ai 登录入口** / 实现 PTY 转发 | **推荐前者**；Claude 部分需要法务确认 |
| D9 | 历史策略 | **Hub 对 list 全量分页（上限 500）；打开会话时 load，物化成有界转录，经 `/history/v1` 的 `acp/hub-v1` 和控制器磁盘缓存提供窗口；超出预算时只保留尾部；不支持 list 的 agent 只显示经 Hub 创建的会话；外部写入靠轮询 `updatedAt` 触发重新加载** / 每次打开都经 relay 全量回放 | **前者**。同时接受一处偏离：ACP 没有无副作用的只读读取，所以读取要 load，但不发 prompt |
| D10 | 运行中发送 | **Hub 负责排队，隐藏补充** / 把 Claude、Codex 私有的 `_meta` steering 做成 quirk | **前者**；等 steering 进入标准后再加 |
| D11 | 客户端 fs/terminal | **不声明；但实现只能访问会话根目录的 `fs/*` 处理器**（兼容 OpenCode 1.x 不检查能力就回写的行为）/ 声明 fs | **前者**；terminal 不声明也不实现 |
| D12 | 安装来源与锁定 | **内置白名单清单（随 App 发布）：binary 锁 sha256，npm 包附预生成的 lockfile 用 `npm ci`，Node 锁 sha256；registry 只用来提示新版本** / 允许在主机上确认后安装 registry 最新版 / 完全跟 registry（Zed 的做法） | **前者**；"安装 registry 最新版"作为主机桌面上的高级选项，远程不可用 |
| D13 | Node 运行时 | **始终用托管目录里的私有 Node** / 优先用系统 Node | **私有 Node**，避开 nvm 和 GUI 应用 PATH 不一致的问题 |
| D14 | 安卓远程管理 | **meta 路由支持查看状态、安装或升级（仅白名单锁定版本）、查看进度、启动托管；主机开关"允许控制器远程管理 ACP agent"默认开启；自定义命令和未锁定版本只能在主机桌面操作；写审计日志** / 默认关闭 / 每次都要主机确认 | **前者**。有 relay 访问权限的控制器本来就能让 agent 执行任意命令，安装锁定版本几乎没有增加攻击面 |
| D15 | 自定义 agent | **主机桌面可以添加自定义 ACP agent（命令、参数、环境变量）** / 只允许清单里的 agent | **前者**，这样才满足"任何支持 ACP 的 agent 只靠配置就能接入" |
| D16 | OpenCode 在两种 provider 下并存 | **原生 OpenCode 和 OpenCode-ACP 可以同时托管；安装器检测到 1.x 和 2.x 共存时给出警告** / 禁止并存 | **前者**，数据目录兼容性在 TRD 中实测 |
| D17 | 安卓本机托管 | **不做** / 做 | **不做**，理由见 §9 |
| D18 | CLI | **首期只在桌面 App 托管** / CLI 同步新增 | **只在 App 托管**，与 OpenCode 一致 |
| D19 | OpenCode-ACP 的数据库（写 TRD 时补充） | **(a) 按用户本机 OpenCode 的大版本自动决定共享或隔离** / (b) 始终隔离 / (c) 始终共享 | **(a)**，详见 [TRD](TRD.md) §11 |
| D20 | 模型网关（用户使用 New API 中转站，补充） | **(a) 通用网关登录：声明 `auth._meta.gateway`，在主机上配置网关地址和密钥，由 Hub 自动登录** / (b) 只做环境变量注入 / (c) 只依赖 agent 自己的配置 | **(a)**；agent 自己的配置照常生效，详见 TRD §4.2.12 |
| D21 | Claude 订阅登录（补充） | **(a) 默认加 `--hide-claude-auth`，高级设置里可以打开** / (b) 保持现状 | **(a)**；默认配置下不涉及 D8 的条款问题 |

## 12. 来源

协议与生态：
- [P1] 规范仓库 https://github.com/agentclientprotocol/agent-client-protocol ，main @ `f05af18d9708f31c85fa62e172ac0968df042cf0`（2026-09-30）：`schema/v1/{schema.json,schema.unstable.json,meta.json}`、`schema/v2/*`、`README.md`
- [P2] https://github.com/agentclientprotocol/agent-client-protocol/releases （2026-09-30 查询）
- [P3] https://crates.io/crates/agent-client-protocol 、https://crates.io/crates/agent-client-protocol-schema （依赖与 feature 于 2026-09-30 经 crates.io API 查询）、https://www.npmjs.com/package/@agentclientprotocol/sdk
- [P4] Rust SDK https://github.com/agentclientprotocol/rust-sdk ，main @ `b55cdc72e06f00b469f8739e57c44539d5dfcbf9`：`agent-client-protocol-http`、`agent-client-protocol-conductor`
- [P5] 规范站点 https://agentclientprotocol.com （2026-09-30 抓取）：`/protocol/v1/{transports,initialization,session-setup,session-list,prompt-turn,tool-calls,file-system,terminals,elicitation,authentication,extensibility,cancellation,session-config-options,slash-commands,agent-plan}`、`/protocol/v2/migration`、`/rfds/{about,updates,message-id,streamable-http-websocket-transport,proxy-chains,v2/prompt,v2/session-resume-replay,get-auth-state}`、`/get-started/{architecture,clients}`
- [P6] Registry 仓库 https://github.com/agentclientprotocol/registry ，main @ `4d35a5296bbf7f5a5abb45dd54220693034ffe79`：`README.md`、`FORMAT.md`、`AUTHENTICATION.md`、`agent.schema.json`
- [P7] https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json （`Last-Modified: Wed, 30 Sep 2026 00:57:18 GMT`；本文在 2026-09-30 复核 `claude-acp`、`codex-acp`、`opencode` 条目）
- [P8] Zed 文档 https://zed.dev/docs/ai/external-agents ；源码 `crates/project/src/{agent_registry_store.rs,agent_server_store.rs}` @ `decbf641b18f1982b3475c037e7c5c554471574f`
- [P9] https://github.com/agentclientprotocol/registry/blob/main/.protocol-matrix/latest.json （generatedAt 2026-09-29T11:23:21Z）

Agent：
- [A1] https://github.com/agentclientprotocol/claude-agent-acp/tree/v0.84.0 （`bdb50ad9`）：`src/acp-agent.ts`（:1720-1755、:2346-2540、:2580-2666、:8749）、`src/session-mode.ts`、`src/permissions/*`、`src/tool-calls/renderer.ts`、`src/hide-claude-auth.ts`
- [A2] https://registry.npmjs.org/@agentclientprotocol/claude-agent-acp （0.84.0：`engines.node >=22`，依赖为精确版本）；deprecated 的 `@zed-industries/claude-code-acp`、`@zed-industries/claude-agent-acp`；issues https://github.com/agentclientprotocol/claude-agent-acp/issues （#517、#883、#976）
- [A3] https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk/0.3.284 （`claudeCodeVersion` 2.1.284、optionalDependencies）
- [A4] https://code.claude.com/docs/en/legal-and-compliance （2026-09-30 抓取）
- [A5] https://github.com/anthropics/claude-code/issues/24411 、/50270 、/84421
- [A6] https://github.com/agentclientprotocol/codex-acp/tree/v2.0.1 （`7a8e00fe`）：`src/CodexJsonRpcConnection.ts:21-25`、`src/CodexAuthMethod.ts`、`src/AgentMode.ts`、`src/CodexAcpServer.ts`、`src/CodexAcpClient.ts:1153-1204`、`readme-dev.md`；npm https://registry.npmjs.org/@agentclientprotocol/codex-acp （2.0.1：依赖 `@openai/codex ^0.159.1`）；issues #516、#310、#477、#406
- [A7] https://github.com/zed-industries/codex-acp （2026-07-22 归档）：`Cargo.toml`@v0.16.0
- [A8] https://github.com/openai/codex/blob/main/codex-rs/Cargo.toml （2026-09-30）；issues #2785、#30052、#41293、#16385
- [A9] https://registry.npmjs.org/@openai/codex/latest （0.159.2，平台 optionalDependencies）
- [A10] https://github.com/anomalyco/opencode/tree/v1.18.33 ：`packages/opencode/src/cli/cmd/acp.ts`、`src/cli/network.ts`、`src/acp/{service,permission}.ts`；PR #2947（commit `f3f21194`）
- [A11] https://github.com/anomalyco/opencode/tree/v2.0.18 （`cd9a14a6`）：`packages/cli/src/commands/handlers/acp.ts`、`src/services/standalone.ts`、`src/acp/*`；npm https://registry.npmjs.org/@opencode/cli （2.0.20）
- [A12] https://opencode.ai/v2/docs/cli/acp/ 、https://opencode.ai/v2/docs/migrate-v1/ 、https://opencode.ai/v2/install
- [A13] https://github.com/anomalyco/opencode/issues/48232 、/50236 、/38121 、/33010 、/48405
- [A14] https://github.com/google-gemini/gemini-cli/tree/v0.62.0/packages/cli/src/acp （仅作为原生实现参考）
- [A15] https://github.com/termux/termux-packages/wiki/Termux-and-Android-10
- [A16] https://registry.npmjs.org/nodejs-mobile-react-native （18.20.4）

本机实测：见 §3.5。
