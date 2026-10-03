# TRD：通用 ACP 接入（分支 `research-acp`）

状态：v1.1（2026-09-30）。v1.0 经多轮冷启动检验后已没有阻塞问题，D19 确认采用 (a)；v1.1 补充了 D20（模型网关）和 D21（默认关闭 Claude 订阅登录）。需求和已确认的决策 D1–D18 见 [research.md](research.md) 的 §1 和 §11，术语见 [`CONTEXT.md`](../../CONTEXT.md)，本方案新增的术语见 §12。

## 0. 施工须知

- 开工前先读：`AGENTS.md`（重点是 §4–§8）、本文、research.md 的 §2–§3、`docs/opencode-unified/TRD.md`。OpenCode 的接入方式是和本方案最接近的先例。
- 按 §9 的里程碑顺序施工，每个里程碑提交一次或多次。每个里程碑结束时都要跑完 `AGENTS.md` §7 的全部命令。
- 硬约束：
  - Codex、OpenCode 原生 provider 的行为保持不变，它们现有的测试全部要通过。
  - 现有的 wire 字段、FRB 函数签名、DTO 字段，以及 `config.toml`、`state.toml`、`ui_state.json` 里已有的字段，都只能新增，不能修改。
  - 不依赖官方的 ACP crate（D3）。
  - 不读取 agent 的私有文件和凭据（R2）。
  - crate 根保留 `#![forbid(unsafe_code)]`。非测试代码不写 `unwrap()` / `expect()`。
  - 新依赖一律锁精确版本。
- 会产生写操作的实测（新建会话、发送 prompt、安装 agent、登录）只放在 M10 做，每次都要先征得用户同意。

## 1. 总体架构

```
                        主机（桌面 App，自有托管）                                         控制器（任意设备）
┌─────────────────────────────────────────────────────────────────────────────┐   ┌──────────────────────────────────┐
│ agent 子进程（claude-agent-acp / codex-acp / opencode acp / 自定义）          │   │ Flutter 会话界面（同一套）         │
│      ▲ stdin/stdout NDJSON，ACP v1；stderr → logs/acp-<name>.log             │   │  │ BridgeApi                     │
│ ACP Hub（host-svc acp/，只在桌面编译）                                        │   │  ▼                               │
│   AgentPeer · 进程监管 · HubSession（转录 / 待办 / 排队）· fs 处理器 · 认证    │   │ bridge：app_* 按 kind 分发         │
│   serve_ws：ws://127.0.0.1:<p>/acp（ACP v1 + _pcx/*） ◄─── pb: …:acp:<name> ──┼───┼─► engine/acp（ACP 客户端）         │
│   serve_meta：通用路由 + /history/v1 (acp/hub-v1)  ◄────── pb: …:meta:<name> ─┼───┼─► session_sync / meta_*           │
│   /acp/v1/*（远程管理，merge 进 generic_app 和 Codex serve()，任何 meta 都有） │   │    映射成 ThreadItem / AppEvent    │
│ 安装器（host-svc acp/install/）：清单 · 私有 Node · 下载校验 · npm ci · 审计   │   │    能力来自协商结果                │
└─────────────────────────────────────────────────────────────────────────────┘   └──────────────────────────────────┘
```

| 层 | 位置 | 职责 | 不做的事 |
|---|---|---|---|
| core `acp` | `crates/pocket-codex-core/src/acp/` | ACP v1 wire 类型、JSON-RPC 帧、`_pcx` 扩展类型（Hub 和控制器共用的全部结果类型都放在这里）、转录折叠（只有 Hub 调用，放在 core 是为了方便单元测试） | IO |
| Hub | `crates/pocket-codex-host-svc/src/acp/` | 启动和监管 agent 进程，作为唯一的 ACP 客户端；维护会话状态、转录、待办、排队；提供面向控制器的 WebSocket、fs 处理器、认证状态和历史源 | 翻译成 Codex 或 UI 的语义 |
| 安装器 | `crates/pocket-codex-host-svc/src/acp/install/` | 清单、平台检测、下载校验、解压、私有 Node、`npm ci`、解析启动规格、远程管理路由、审计 | 自动升级 |
| bridge 托管 | `crates/pocket-codex-bridge/src/engine/serve_acp.rs` 等 | 托管生命周期、relay 注册、登录终端、FRB | |
| bridge 引擎 | `crates/pocket-codex-bridge/src/engine/acp/` | 控制器侧的 ACP 客户端：映射成 ThreadItem/AppEvent，生成能力描述，接入历史缓存 | |
| Flutter | `apps/flutter/lib/src/` | 托管选择、agent 管理页、能力开关、新增的卡片 | 按 provider 名称判断功能 |

平台门控：
- host-svc 的 `acp::{hub, peer, process, session, server, pending, fs, auth, history, meta, install, launch, testing}` 全部加 `#[cfg(not(any(target_os = "android", target_os = "ios")))]`。
- core 的 `acp` 模块和 bridge 的 `engine/acp/` 在所有平台上都编译。
- bridge 的 `engine/{serve_acp, acp_manage, acp_terminal}.rs` 同样加这个门控，移动端另外编译一个同名的桩模块 `engine/acp_desktop_stub.rs`，对外函数签名一致，统一返回 `Err(anyhow!("ACP hosting is only available on desktop"))`。FRB 的函数和签名在所有平台上保持一致。
- `scripts/check_mobile_dependencies.py` 的禁止列表加上 `tar` 和 `zip`，由 CI 保证它们不会进入安卓产物。

主要数据流：
1. **托管**：`app_serve_start_acp(name, agent_id)` → `install::resolve_launch` → `AcpHub::start`（启动进程并 `initialize`）→ 绑定 ws 和 meta 两个回环端口 → 先注册 `meta`，再注册 `acp`。
2. **打开会话**：bridge 连到 `/acp` 后调用 `initialize`，再调用 `_pcx/session/attach {tail: 20}`。Hub 确保会话已加载（resume 或 load），返回末尾窗口和轮次摘要，然后补发这个会话的待办。
3. **发送**：`app_turn_start` → 应用配置项 → `_pcx/session/submit`。会话空闲时，Hub 调 `session/prompt`；运行中则排队。agent 发出的 `session/update` 先由 Hub 折叠进转录，再广播给已订阅的控制器，附带权威的 `_meta.pcx.itemId`；工具和计划的更新还附带折叠后的完整条目。bridge 不自己折叠，只按 id 合并，然后生成 AppEvent。
4. **审批**：agent 发来 `session/request_permission` → Hub 登记为待办，发给所有订阅者 → 第一个合法回应胜出 → Hub 回应 agent，并广播 `_pcx/request/resolved`。
5. **断线**：控制器断开后，Hub 照常运行，待办保留。控制器重连后重新 `initialize` 和 attach，待办会被补发。提交结果不确定的消息，按 `clientSubmissionId` 去重重发。
6. **历史缓存**：`app_history_prefetch` → meta 的 `/history/v1/window`（provider `acp/hub-v1`）增量同步 → `session_sync::save_history`。
7. **远程安装（安卓）**：`meta_acp_install` → 主机上任意一个 meta 的 `/acp/v1/agents/{id}/install` → 轮询 `/acp/v1/jobs/{id}` → `meta_acp_host` → 发现列表里出现新的 `acp:<name>`。

## 2. 决策清单

已确认的产品决策（research.md §11）：
- D1 有状态 Hub
- D2 只实现 v1
- D3 本地实现 wire 类型
- D4 每个实例一个进程
- D5 kind 为 `acp`
- D6 Codex-ACP 强制使用用户的 codex
- D7 Claude 使用适配器自带的引擎，并提供高级覆盖项
- D8 在主机侧登录
- D9 由 Hub 物化转录，经 `/history/v1` 提供
- D10 由 Hub 排队，隐藏补充
- D11 不声明 fs，但实现受限的 fs 处理器
- D12 内置清单锁定版本
- D13 私有 Node
- D14 远程管理默认开启
- D15 支持自定义 agent
- D16 两种 OpenCode provider 可以并存
- D17 安卓不做本机托管
- D18 只在 App 内托管
- D19 OpenCode-ACP 的数据库按大版本自动决定共享还是隔离（§11，2026-09-30 确认）
- D20 通用模型网关登录：Hub 声明 `auth._meta.gateway`；只要 agent 提供了网关类登录方法，就可以在主机桌面上配置网关地址和密钥，每次启动后由 Hub 自动登录（§4.2.12，2026-09-30 确认）
- D21 Claude 适配器默认加 `--hide-claude-auth` 启动，关闭 Claude.ai 订阅登录；主机桌面的高级设置里可以重新打开，打开时显示条款提示（§4.2.12，2026-09-30 确认）

本方案的技术决策：

| # | 决策 |
|---|---|
| T1 | 转录只由 Hub 折叠。Hub 转发的每条更新都带权威的条目 id；工具和计划的更新还带折叠后的完整条目。bridge 按 id 合并：文本块追加，工具和计划整条替换。这样 bridge 即使只持有末尾窗口，也能和 Hub 保持一致 |
| T2 | 控制器到 Hub 走 ACP v1 over WebSocket，路径 `/acp`，每帧一条 JSON-RPC 消息。bridge 复用 `pocket_codex_codex::client::AppClient`（`crates/pocket-codex-codex/src/client.rs:108`，自带 permessage-deflate 协商和回退） |
| T3 | ACP 没有定义的多端语义，放进 `_pcx/*` 扩展方法（§4.2.6） |
| T4 | bridge 分发：在每个 `app_*` 函数现有的 OpenCode 分支之前加一个 ACP 分支，不重构现有分支 |
| T5 | Hub 和安装器只在桌面编译 |
| T6 | ACP 设置写在独立文件 `<state_dir>/acp/agents.toml`，不动 `config.toml`（它是 `deny_unknown_fields`） |
| T7 | 远程管理路由是一个无状态的 `Router`，只在两处 merge：`generic_app` 和 Codex 的 `serve()`，都在 `.with_state(state)` 之后。`serve_meta` 基于 `generic_app` 构建，自动带上（§4.2.11）。实现由进程级注册的 `AcpManagement` 提供，没有注册时返回 404 |
| T8 | 两种安装方式：`archive`（下载、校验完整性、解压，不需要 Node）和 `npm`（私有 Node + 预生成的 lockfile + `npm ci --ignore-scripts`） |
| T9 | 完整性统一用 SRI 字符串（`sha256-…` 或 `sha512-…`）。清单由 `scripts/acp_catalog.py` 生成，CI 校验结构 |
| T10 | 私有 Node 固定为 v24.21.0，自带 npm 11.19.0；npm 从 11.11 起支持 lockfile 的 `libc` 字段 |
| T11 | Codex-ACP 用 `--omit=optional` 安装，不下载它自带的 Codex；启动时设 `CODEX_PATH`。按用户 Codex 的版本选适配器版本：2.0.1 ↔ `^0.159.1`，1.12.0 ↔ `^0.154.0` |
| T12 | 两个 OpenCode 大版本都走 `archive`：1.x 用 GitHub Release 包，2.x 用 npm 上的平台包 tarball，不跑 postinstall，直接执行 `package/bin/opencode`。x86_64 没有 AVX2 时选 `-baseline` 变体 |
| T13 | Linux 首期只支持 glibc。检测到 musl 时，清单里的 agent 报"平台不支持"；自定义 agent 不受这个限制 |
| T14 | Hub 转发 `session/update` 时，在 `params._meta.pcx` 里附上 `{seq, itemId?, created?, turn, generation, item?}`。<br>- `item` 只在 `tool_call`、`tool_call_update`、`plan` 这三种更新里出现，内容是折叠后的完整 HubItem。<br>- `seq` 是按会话递增的 u64（`HubSession.seq`），只给**广播给该会话全部订阅者**的通知编号：`session/update`、`_pcx/turn/*`、`_pcx/session/state`、`_pcx/session/generation`、`_pcx/session/loaded`、`loadFailed`、`_pcx/queue/failed`、`_pcx/request/resolved`，这些通知的参数里都带 `seq`。只发给单个连接的消息（`$/cancel_request`、标准 `session/load` 的重放）不占用 `seq`。<br>- `AttachResult.seq` 表示快照已经包含了哪一条之前的通知。<br>- `AppClient` 把响应和通知分别放在不同的通道（`client.rs:185-218`），bridge 无法从到达顺序判断先后，所以靠 `seq` 去重（§4.4.2） |
| T15 | 把 `event`、`item_event`、`bare_item`（`crates/pocket-codex-bridge/src/engine/opencode/events.rs:24-61`）原样移到新文件 `engine/app_events.rs`，改为 `pub(crate)`，OpenCode 和 ACP 共用；另外新增 `pub(crate) fn hub_event(kind: &str, raw: Value) -> AppEvent`，用于 `thread_id` 为 None 的事件（Hub 级别的 elicitation、`acp/sessions/changed`、`acp/hub/state`）。OpenCode 的测试必须保持通过 |
| T16 | `AppClient` 在把 JSON-RPC 错误交给调用方时只保留 message，丢掉了 code（`crates/pocket-codex-codex/src/client.rs:190-193`）。所以 Hub 发给控制器的每个错误，`message` 都以 `[acp.<code>] ` 开头，`data` 为 `{"pcxCode": "acp.<code>"}`；bridge 靠这个前缀识别错误类型（`engine/acp/mod.rs` 里的 `fn pcx_code(&anyhow::Error) -> Option<String>`）。不改 `pocket-codex-codex` |
| T17 | `AppClient::request` 固定 60 s 超时（`client.rs:66`），meta 客户端是 30 s（`crates/pocket-codex-bridge/src/engine/meta.rs:31`）。所以 Hub 对控制器的每个方法都有自己的回应期限，最长 45 s（§4.2.6 的期限表）。可能更慢的操作（加载会话、agent 类登录、远程开始托管）一律先返回当前状态或任务 id，完成后再发通知 |
| T18 | Hub 的协议逻辑和传输层分开：`AcpHub::open_connection` 返回进程内的 `HubConnection`，标准方法、`_pcx` 方法和待办路由都在它里面实现；`server.rs` 只负责在 WebSocket 帧和 `HubConnection` 之间转发。这样 M3 可以不经过 WebSocket 测完整个协议。`handle` 从不在读取路径上等待轮次或加载完成（§4.2.5） |
| T19 | 可测试性：bridge 托管用 `serve_acp::start_with(…, StartDeps)` 注入 relay 注册、agent 连接器、存储和状态目录；控制器引擎用 `acp::connect_url(service_key, ws_url, meta_url)` 直接连接一个本地 Hub；安装器用 `InstallContext` 注入目录、清单、npm、下载策略和连接器。这样 M5、M6、M7 的测试都不需要账号、relay、网络，也不会写入真实的状态目录 |

## 3. 协议基线

### 3.1 版本与 fixture

- 目标版本：ACP `protocolVersion: 1`，schema 为 `schema-v1.23.0`，来自规范仓库 commit `f05af18d9708f31c85fa62e172ac0968df042cf0`。
- M2 把 `schema/v1/schema.json` 和 `schema/v1/meta.json` 原样复制到 `crates/pocket-codex-core/tests/fixtures/acp/`，分别命名为 `schema-v1.23.0.json` 和 `meta-v1.23.0.json`。同目录下的 `README.md` 写明来源 URL 和 commit。
- 以后升级 schema，就是替换这两个文件并让契约测试通过，作为单独的变更提交。

### 3.2 Hub 向 agent 发送的 initialize

```json
{"protocolVersion":1,
 "clientCapabilities":{
   "fs":{"readTextFile":false,"writeTextFile":false},
   "terminal":false,
   "auth":{"terminal":true,"_meta":{"gateway":true}},
   "elicitation":{"form":{},"url":{}},
   "session":{"configOptions":{"boolean":{}}},
   "_meta":{"terminal-auth":true}},
 "clientInfo":{"name":"pocket-codex","title":"Pocket-Codex","version":"<CARGO_PKG_VERSION>"}}
```

- `auth.terminal` 和 `_meta["terminal-auth"]` 两种写法同时声明（research §3.1、§3.3）。
- `auth._meta.gateway` 是 Zed 和 JetBrains 用的扩展，不在 ACP 规范里。两个目标适配器都只在客户端声明了它时，才会返回网关类登录方法：claude-agent-acp v0.84.0 的 `src/acp-agent.ts:2357-2359`，codex-acp v2.0.1 的 `src/CodexAuthMethod.ts:48-66`。用法见 §4.2.12（D20）。
- 协商出的版本不是 1 时，结束进程并报 `acp.protocol_unsupported`。

### 3.3 用到的方法

Hub 调用 agent：

| 方法 | 何时调用 | 超时 |
|---|---|---|
| `initialize` | 进程启动后 | 60 s |
| `session/list` | 列出会话：翻页取全部，上限 500 条或 20 页 | 每页 30 s |
| `session/new` | 新建会话，固定传 `mcpServers: []` | 60 s |
| `session/resume` | 打开一个未加载、但 Hub 缓存的转录仍是最新的会话 | 60 s |
| `session/load` | 打开一个未加载、也不能 resume 的会话 | 300 s |
| `session/prompt` | 发送消息 | 不设超时，跟随轮次 |
| `session/cancel` | 停止执行 | 通知，无响应 |
| `session/set_config_option`、`session/set_mode` | 切换模型、推理强度、模式等 | 30 s |
| `session/close` | 空闲回收 | 30 s |
| `authenticate` | 用户选了 agent 类型的登录方法 | 600 s |

不调用 `session/delete`、`session/fork`、`logout`、`providers/*`、`nes/*`。

Hub 处理 agent 发来的消息：

| 方法 | 处理 |
|---|---|
| `session/request_permission`、`elicitation/create` | 登记为待办，交给控制器处理（§4.2.7） |
| `fs/read_text_file`、`fs/write_text_file` | Hub 在本地处理，只能访问会话目录（§4.2.8） |
| `terminal/*` 和其他请求 | 返回 `-32601` |
| `session/update`（通知） | 折叠进转录（§4.1.3），再转发给订阅者 |
| `elicitation/complete`（通知） | 结束对应的 URL 待办 |
| 其他通知 | 忽略 |

### 3.4 首批 agent 基线

| 清单 id | 显示名 | 版本 | 方式 | 启动命令 | 固定环境变量 | registry id |
|---|---|---|---|---|---|---|
| `claude-acp` | Claude Code | 0.84.0 | npm | `<node> <prefix>/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js [--hide-claude-auth]`。这个参数默认带上，`allow_subscription_login = true` 时去掉（D21） | 无；可选 `CLAUDE_CODE_EXECUTABLE`（D7 的高级项） | `claude-acp` |
| `codex-acp` | Codex（ACP） | 2.0.1 或 1.12.0，按 T11 选择 | npm，`--omit=optional` | `<node> <prefix>/node_modules/@agentclientprotocol/codex-acp/dist/index.js` | `CODEX_PATH=<用户的 codex>` | `codex-acp` |
| `opencode-acp` | OpenCode 1.x（ACP） | 1.18.33 | archive（GitHub Release） | `<dir>/opencode acp --hostname 127.0.0.1 --port 0 --no-mdns` | `OPENCODE_SERVER_PASSWORD=<每次启动随机生成>`；`OPENCODE_DB` 由 D19 决定 | `opencode` |
| `opencode2-acp` | OpenCode 2.x（ACP） | 2.0.20 | archive（npm 平台包 tarball） | `<dir>/package/bin/opencode acp` | `OPENCODE_DB` 由 D19 决定 | 无 |

各 agent 预期声明的能力见 research §3；OpenCode 2.0.18 的 initialize 实测响应见 research §3.5。

## 4. 模块设计

### 4.1 core（`pocket-codex-core`）

#### 4.1.1 服务键

- `src/service.rs`：新增 `ServiceKind::Acp`，线上字符串为 `"acp"`，同时更新 `as_key_segment`（`:60`）和 `FromStr`（`:80`）。仿照 `:193` 新增测试 `acp_keys_round_trip_alongside_existing_services`。
- 需要补分支的穷举 match：
  - `src/config.rs:247-275` 有两处 `ServiceKind::OpenCode | ServiceKind::Meta | ServiceKind::Unknown`，都加上 `ServiceKind::Acp`。
  - `crates/pocket-codex-cli/src/commands/ui.rs:212` 加 `ServiceKind::Acp => Color::Yellow`。
  - `crates/pocket-codex-cli/src/commands/services.rs:60-62` 和 `src/cli.rs:311-326` 不改，因为 CLI 不管理 ACP（D18）。
- account-proto：代码不改，仿照 `key.rs:143` 新增测试 `acp_keys_round_trip_without_merging_account_namespaces`。
- 兼容性：老版本 Rust 客户端会把 `acp` 反序列化成 `Unknown`，或者 `parse_key` 失败，在发现阶段就被丢弃。老版本 Flutter 的 `parseServiceKey` 会返回空字段，同样被过滤掉。

#### 4.1.2 `acp` 模块（新增，只使用 core 已有的依赖：serde、serde_json、anyhow、thiserror）

| 文件 | 内容 |
|---|---|
| `src/acp/mod.rs` | `pub const PROTOCOL_VERSION: u16 = 1;`，声明子模块并 re-export |
| `src/acp/rpc.rs` | JSON-RPC 帧 |
| `src/acp/types.rs` | ACP v1 子集类型 |
| `src/acp/update.rs` | `SessionUpdate` 和 `SessionNotification` |
| `src/acp/transcript.rs` | 转录折叠（§4.1.3） |
| `src/acp/pcx.rs` | `_pcx/*` 的方法名常量，以及 Hub 和控制器共用的全部参数和结果类型（§4.1.5）。bridge 在移动端也要用这些类型，所以它们不能放在只在桌面编译的 host-svc 模块里 |

`rpc.rs`：

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId { Number(i64), Text(String) }

#[derive(Clone, Debug, PartialEq)]
pub enum RpcMessage {
    Request { id: RequestId, method: String, params: Value },
    Notification { method: String, params: Value },
    Response { id: RequestId, result: std::result::Result<Value, RpcError> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[error("JSON-RPC error {code}: {message}")]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JSON-RPC and ACP error codes, plus the `_pcx` extension codes.
pub mod code {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
    pub const AUTH_REQUIRED: i64 = -32000;
    pub const RESOURCE_NOT_FOUND: i64 = -32002;
    pub const REQUEST_CANCELLED: i64 = -32800;
    pub const GENERATION_CHANGED: i64 = -32010;
    pub const HOST_ONLY: i64 = -32011;
    pub const SESSION_NOT_LOADABLE: i64 = -32012;
    pub const AGENT_UNAVAILABLE: i64 = -32013;
    pub const SESSION_LOADING: i64 = -32014;
}

/// Build a hub error whose message starts with `[acp.<pcx_code>] ` and whose
/// `data` is `{"pcxCode": "acp.<pcx_code>"}` (T16).
pub fn pcx_error(code: i64, pcx_code: &str, message: impl std::fmt::Display) -> RpcError;
/// Extract `acp.<code>` from a message produced by `pcx_error`.
pub fn pcx_code_of(message: &str) -> Option<&str>;

/// Parse one frame; accepts messages with or without `"jsonrpc": "2.0"`.
pub fn decode(bytes: &[u8]) -> std::result::Result<RpcMessage, RpcError>;
/// Serialize with `"jsonrpc": "2.0"` for a WebSocket text frame.
pub fn encode(message: &RpcMessage) -> String;
/// `encode` plus a trailing `\n` for stdio; serde_json never emits a raw newline.
pub fn encode_line(message: &RpcMessage) -> Vec<u8>;
```

`types.rs` 的约定：
- 结构体一律 `#[serde(rename_all = "camelCase")]`，不用 `deny_unknown_fields`，因为 ACP 只会新增字段。
- 可选字段写 `#[serde(default, skip_serializing_if = "Option::is_none")]`。
- `_meta` 写成 `#[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")] pub meta: Option<Value>`。
- 协议里的枚举值（tool kind、status、stopReason、permission option kind、plan 的 priority/status、auth method type）一律存成 `String`，已知值用常量比较，未知值原样保留。

需要的类型（字段名以 schema-v1.23.0 为准，§4.1.4 的契约测试会校验）：

| 类型 | 字段 |
|---|---|
| `Implementation` | `name`、`title?`、`version?` |
| `ClientCapabilities` | `fs{readTextFile, writeTextFile}`、`terminal`、`auth{terminal}`、`elicitation?{form?, url?}`、`session?`、`_meta?` |
| `InitializeRequest` / `InitializeResponse` | `protocolVersion: u16`、`clientCapabilities`、`clientInfo?` / `protocolVersion`、`agentCapabilities`、`authMethods[]`、`agentInfo?`、`_meta?` |
| `AgentCapabilities` | `loadSession`、`promptCapabilities{image, audio, embeddedContext}`、`mcpCapabilities?`、`sessionCapabilities{list?, resume?, close?, delete?, additionalDirectories?}`（对象存在即表示支持；`fork` 是 unstable，不在 v1.23.0 稳定 schema 里，不建模，反序列化时忽略）、`auth?`、`_meta?` |
| `AuthMethod` | `id`、`name`、`description?`、`type?`、`args[]`、`env{}`、`_meta?` |
| `NewSessionRequest` / `LoadSessionRequest` / `ResumeSessionRequest` / `CloseSessionRequest` | `cwd`、`mcpServers[]` / `sessionId`、`cwd`、`mcpServers[]` / 同 load / `sessionId` |
| `SessionSetup`（new、load、resume 共用的响应） | `sessionId?`、`modes?`、`configOptions?`、`_meta?` |
| `ListSessionsRequest` / `ListSessionsResponse` / `SessionInfo` | `cwd?`、`cursor?` / `sessions[]`、`nextCursor?` / `sessionId`、`cwd`、`title?`、`updatedAt?`、`_meta?` |
| `PromptRequest` / `PromptResponse` / `CancelNotification` | `sessionId`、`prompt[]` / `stopReason`、`_meta?` / `sessionId` |
| `ContentBlock`（按 `type` 手写 serde） | `Text{text}`、`Image{data, mimeType, uri?}`、`Audio{data, mimeType}`、`ResourceLink{uri, name, mimeType?, title?, description?, size?}`、`Resource{resource}`、`Unknown(Value)` |
| `ToolCall` / `ToolCallUpdate` | `toolCallId`、`title`、`kind?`、`status?`、`content[]`、`locations[]`、`rawInput?`、`rawOutput?`、`name?` / 除 `toolCallId` 外全部可选 |
| `ToolCallContent`（按 `type` 手写 serde） | `Content{content}`、`Diff{path, oldText?, newText}`、`Terminal{terminalId}`、`Unknown(Value)` |
| `ToolCallLocation`、`PlanEntry` | `path`、`line?` / `content`、`priority`、`status` |
| `SessionModeState` | `currentModeId`、`availableModes[{id, name, description?}]` |
| `ConfigOption` | `id`、`name`、`description?`、`category?`、`type`（`select` 或 `boolean`）、`currentValue: Value`、`options[]`（元素是 `{value, name, description?}` 或 `{group, name, options[]}`） |
| `SetConfigOptionRequest` | `sessionId`、`configId`、`value: Value`；boolean 类型额外带 `type: "boolean"` |
| `AvailableCommand`、`UsageUpdate` | `name`、`description`、`input?` / `used`、`size`、`cost?` |
| `PermissionOption` / `RequestPermissionRequest` | `optionId`、`name`、`kind` / `sessionId`、`toolCall: ToolCallUpdate`、`options[]` |
| `PermissionOutcome`（序列化成 `{"outcome": {...}}`） | `Selected{optionId}`、`Cancelled` |
| `ElicitationRequest` | `message`、`mode`（`form` 或 `url`）、`requestedSchema?`、`elicitationId?`、`url?`、`sessionId?`、`toolCallId?`、`requestId?`、`_meta?` |
| `ElicitationResponse` / `ElicitationComplete` | `action`（`accept`、`decline`、`cancel`）、`content?` / `elicitationId` |
| `ReadTextFileRequest` / `WriteTextFileRequest` | `sessionId`、`path`、`line?`、`limit?` → `{content}` / `sessionId`、`path`、`content` → `{}` |

`update.rs`：

```rust
pub struct MessageChunk { pub content: ContentBlock, pub message_id: Option<String> }

pub enum SessionUpdate {
    UserMessageChunk(MessageChunk),
    AgentMessageChunk(MessageChunk),
    AgentThoughtChunk(MessageChunk),
    ToolCall(ToolCall),
    ToolCallUpdate(ToolCallUpdate),
    Plan { entries: Vec<PlanEntry> },
    AvailableCommandsUpdate { available_commands: Vec<AvailableCommand> },
    CurrentModeUpdate { current_mode_id: String },
    ConfigOptionUpdate { config_options: Vec<ConfigOption> },
    /// Outer `None` = omitted (unchanged); `Some(None)` = explicit null (cleared).
    SessionInfoUpdate { title: Option<Option<String>>, updated_at: Option<Option<String>> },
    UsageUpdate(UsageUpdate),
    Unknown(Value),
}

pub struct SessionNotification { pub session_id: String, pub update: SessionUpdate, pub meta: Option<Value> }
```

`SessionUpdate` 按 `sessionUpdate` 字段的值手写 `Deserialize` 和 `Serialize`。未知的变体保存为 `Unknown`，原样序列化回去，并且不影响同一条消息里的其他字段。

#### 4.1.3 转录折叠（`src/acp/transcript.rs`）

```rust
pub const MAX_ITEMS: usize = 4_000;
pub const MAX_BYTES: usize = 48 * 1024 * 1024;
pub const MAX_ITEM_TEXT: usize = 1024 * 1024;
pub const MAX_RAW_JSON: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubItem {
    pub id: String,
    pub turn: u32,
    /// `user` | `agent` | `thought` | `tool` | `plan` | `notice`
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<ContentBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<Vec<PlanEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnInfo {
    pub turn: u32,
    pub user_item_id: Option<String>,
    pub user_preview: String,
    pub agent_preview: String,
    pub started_at_ms: Option<i64>,
    pub completed_at_ms: Option<i64>,
    pub stop_reason: Option<String>,
}

pub enum Applied {
    /// An item was created or changed; `delta` is the text appended by a chunk.
    Item { id: String, created: bool, delta: Option<String> },
    /// Session-level state (usage, title, config, modes, commands).
    Session,
    Ignored,
}

impl Transcript {
    pub fn new(generation: impl Into<String>) -> Self;
    pub fn generation(&self) -> &str;
    pub fn set_generation(&mut self, generation: impl Into<String>);
    /// Record the hub-originated user message of a live prompt; opens the next turn.
    pub fn begin_live_turn(&mut self, prompt: &[ContentBlock], now_ms: i64) -> (u32, String);
    pub fn apply(&mut self, update: &SessionUpdate, now_ms: Option<i64>) -> Applied;
    pub fn end_live_turn(&mut self, stop_reason: &str, now_ms: i64);
    /// Mark that earlier history exists but is not available (resume-only agents);
    /// sets `dropped_turns` to at least 1.
    pub fn mark_history_unavailable(&mut self);
    pub fn items(&self) -> &[HubItem];
    pub fn item(&self, id: &str) -> Option<&HubItem>;
    pub fn turns(&self) -> &[TurnInfo];
    pub fn current_turn(&self) -> u32;
    pub fn dropped_turns(&self) -> u32;
    /// Items older than `before` (or the newest), oldest first; returns `has_older`.
    pub fn window_before(&self, before: Option<&str>, limit: usize) -> (Vec<HubItem>, bool);
    /// Items of `turn` after `after`, oldest first; returns `has_more`.
    pub fn turn_items(&self, turn: u32, after: Option<&str>, limit: usize) -> (Vec<HubItem>, bool);
    /// After a reload: walk `previous` and `self` in order and reuse `previous`'s
    /// ids for items that are equal (see "equality" below), stopping at the
    /// first difference. Returns true when every item of `previous` except its
    /// last one matched, i.e. the reload only extended the conversation.
    pub fn adopt_ids_from(&mut self, previous: &Transcript) -> bool;
    pub fn approx_bytes(&self) -> usize;
}
```

折叠规则：
1. 轮次编号：第一条用户消息出现之前的条目归入轮次 0。之后每出现一条新的用户消息，`current_turn` 加 1。
2. `user_message_chunk`：
   - 如果存在实时轮次，直接忽略。`begin_live_turn` 已经记录了用户消息，而各 agent 回不回显这条消息并不一致。
   - 如果没有实时轮次：最后一个条目是 `user`，并且两者 `messageId` 相同或都没有 `messageId`，就追加到这个条目；否则开一个新轮次，新建 `user` 条目。
3. `agent_message_chunk` 和 `agent_thought_chunk`（kind 分别是 `agent` 和 `thought`）：
   - 带 `messageId`：用 `m:{kind}:{messageId}` 查找条目，找到就追加，找不到就新建。
   - 不带 `messageId`：最后一个条目的 kind 相同且也没有 `messageId`，就追加；否则新建。
4. 追加方式：text 块拼接到条目第一个 text 块的末尾，没有 text 块就新建一个；其他类型的块原样追加。文本超过 `MAX_ITEM_TEXT` 后，丢弃后续文本并设 `truncated = true`。
5. `tool_call`：`tc:{toolCallId}` 已存在，就用新提供的字段整体替换；不存在就在当前轮次里新建。
6. `tool_call_update`：条目存在就逐字段覆盖，其中 `content` 和 `locations` 是整个列表替换；条目不存在就新建一个，`title` 暂用 toolCallId。`rawInput` 或 `rawOutput` 序列化后超过 `MAX_RAW_JSON` 时，替换成 `{"_pcxTruncated": <原始字节数>}`。
7. `plan`：替换当前轮次的 `p:{turn}` 条目，不存在就新建。
8. 其余已知变体返回 `Applied::Session`，`Unknown` 返回 `Applied::Ignored`。
9. 预算：每新增一个条目都检查一次。条目数超过 `MAX_ITEMS`，或者总字节数超过 `MAX_BYTES` 时，从最早的轮次开始整轮删除，直到回到预算以内，并累加 `dropped_turns`。当前轮次永远不删。
10. 轮次信息：
    - `user_preview` 取用户文本的前 200 个字符，`agent_preview` 取该轮最后一个 agent 条目文本的前 200 个字符。
    - 实时轮次记录 `started_at_ms`、`completed_at_ms` 和 `stop_reason`；回放出来的轮次这三个字段为空，因为 ACP 不提供时间（research §2.5）。

条目 id：

| 条目 | id |
|---|---|
| user、agent、thought，带 messageId | `m:{kind}:{messageId}` |
| user、agent、thought，不带 messageId | `s:{turn}:{ordinal}`，ordinal 是本轮里第几个不带 id 的条目，从 0 开始 |
| 工具调用 | `tc:{toolCallId}` |
| 计划 | `p:{turn}` |
| Hub 插入的提示 | `n:{turn}:{ordinal}` |

界面上的轮次 id 统一是 `t{turn}`。

generation：
- Hub 启动时生成一个 `epoch`，是 16 位十六进制随机数。会话第一次物化时，generation 为 `{epoch}.0`。
- 重新 `session/load` 得到新的转录后，先调用 `adopt_ids_from(旧转录)`：
  - 返回 true：沿用旧的 generation。
  - 返回 false：换成 `{epoch}.{n+1}`，并广播 `_pcx/session/generation`。
- 条目相等的判断：
  - 工具条目：`toolCallId` 相同即相等。ACP 规定它在会话内唯一，而 `rawInput`、`rawOutput` 在实时和回放时经常不一样，不参与比较。
  - 其他条目：`kind`、`turn`、`content`、`plan` 都相同即相等。`id`、`messageId`、`observedAtMs`、`truncated` 不参与比较。所以实时轮次里 Hub 生成的用户条目，和回放出来的同一条用户消息能被认成同一个条目，沿用原来的 id。
- id 被沿用之后，折叠规则 3 按 `messageId` 查找条目时，查的是条目的 `message_id` 字段，不是 id。因此 `Transcript` 内部除了 id 索引，还维护一个 `(kind, messageId) → 下标` 的索引。
- generation 的计数器 `n` 存在 `HubSession` 里（`generation_seq: u32`），不存在 `Transcript` 里。转录被 LRU 丢弃后重新物化时，一律把 `n` 加 1，不会重复用到旧的 generation 字符串。
- 实时追加不会改变 generation。

#### 4.1.4 测试

单元测试（`transcript.rs`、`update.rs`、`rpc.rs`）：
- `decode_accepts_frames_without_jsonrpc_field`
- `encode_line_has_single_trailing_newline`
- `unknown_session_update_round_trips`
- `session_info_update_distinguishes_null_from_omitted`
- `chunks_with_message_id_merge`
- `chunks_without_id_merge_until_kind_changes`
- `live_turn_ignores_user_echo`
- `tool_call_upsert_and_patch`
- `plan_replaces_within_turn`
- `oversized_text_is_truncated`
- `oversized_raw_output_is_replaced`
- `budget_drops_oldest_whole_turns`
- `adopt_ids_keeps_generation_after_live_turns`
- `divergent_reload_bumps_generation`
- `ids_are_stable_across_identical_replays`
- `pcx_error_prefix_round_trips`

契约测试 `crates/pocket-codex-core/tests/acp_schema.rs`：
- §3.3 用到的每个方法名，都要出现在 `meta-v1.23.0.json` 里。
- `tests/fixtures/acp/messages/*.json` 为 §4.1.2 的每种请求、响应、通知至少提供一条手写样例，逐条执行：`decode` → 反序列化为对应类型 → 再序列化 → 结构校验。结构校验包括三点：
  - 顶层的 `required` 字段都存在。
  - 输出的每个属性名，要么出现在对应 `$defs` 的 `properties` 里（沿 `allOf`、`anyOf`、`oneOf` 展开），要么是 `_meta`。
  - 取值受 `const` 或 `enum` 约束的字段，值必须合法。

  校验器写在测试文件里，约 150 行，不引入新依赖。
- `tests/fixtures/acp/opencode-2.0.18-initialize.json`，也就是 research §3.5 的实测响应，要能完整反序列化为 `InitializeResponse`。

#### 4.1.5 共享的 `_pcx` 类型（`src/acp/pcx.rs`）

下面这些类型 Hub 和控制器都要用，统一放在 core。结构体都加 `#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]` 和 `#[serde(rename_all = "camelCase")]`；枚举另外按下面的写法指定 serde。

```rust
pub struct PcxCaps {
    pub list: bool, pub load: bool, pub resume: bool, pub close: bool,
    pub image: bool, pub embedded_context: bool,
    pub config_options: bool, pub modes: bool, pub commands: bool,
    pub queue: bool, pub steer: bool, pub url_elicitation: bool,
}

pub struct QueuedInfo { pub submission_id: String, pub text_preview: String }

pub struct AttachResult {
    pub session_id: String,
    pub cwd: String,
    pub title: Option<String>,
    pub updated_at: Option<String>,
    /// True while the hub is still loading the session; `items` is empty and
    /// `_pcx/session/loaded` (or `_pcx/session/loadFailed`) follows.
    pub loading: bool,
    pub generation: String,
    /// Sequence number of the last session notification already reflected in
    /// this snapshot (see T14 `seq`).
    pub seq: u64,
    pub items: Vec<HubItem>,
    pub has_older: bool,
    pub older_unavailable: bool,
    /// At most 2000, oldest first.
    pub turns: Vec<TurnInfo>,
    pub dropped_turns: u32,
    pub running: bool,
    pub active_turn: Option<u32>,
    pub queue: Vec<QueuedInfo>,
    pub config_options: Vec<ConfigOption>,
    pub modes: Option<SessionModeState>,
    pub commands: Vec<AvailableCommand>,
    pub usage: Option<UsageUpdate>,
}

pub struct WindowResult { pub generation: String, pub items: Vec<HubItem>, pub has_older: bool, pub has_more: bool, pub older_unavailable: bool }
pub struct SubmitResult { pub submission_id: String, pub queued: bool, pub position: u32, pub turn: Option<u32> }
pub struct RunningSession { pub session_id: String, pub running: bool, pub pending: u32, pub queue: u32 }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ProcessState {
    Stopped,
    Starting,
    Ready,
    Restarting { attempt: u32, retry_in_ms: u64 },
    Failed { message: String, stderr_tail: String },
}

pub struct AuthState {
    /// `unknown` | `ok` | `required` | `inProgress`
    pub status: String,
    pub methods: Vec<AuthMethodInfo>,
    pub message: Option<String>,
}

pub struct AuthMethodInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    /// `agent` | `terminal` | `gateway`
    pub kind: String,
    /// Whether a remote controller may start it.
    pub remote: bool,
    /// False when a legacy terminal method cannot be reproduced safely.
    pub available: bool,
    /// `_meta.gateway.protocol` of a gateway method (e.g. `anthropic`, `openai`), if given.
    pub gateway_protocol: Option<String>,
    /// For gateway methods: whether the host has a gateway configured for this method.
    pub gateway_configured: bool,
}

/// `initialize` result `_meta.pcx` and the `_pcx/hub/state` notification.
pub struct HubMeta {
    pub version: u32,
    pub agent: AgentIdentity,          // { id, name, version: Option<String>, pinned: bool, info: Option<Implementation> }
    pub caps: PcxCaps,
    pub auth: AuthState,
    pub process: ProcessState,
    pub default_config_options: Vec<ConfigOption>,
}

pub struct AgentStatus {
    pub id: String, pub name: String, pub description: String,
    /// `catalog` | `custom`
    pub source: String,
    pub pinned_version: Option<String>, pub installed_version: Option<String>,
    /// not_installed | installing | installed | failed | unsupported_platform | engine_missing | engine_incompatible
    pub state: String,
    pub detail: Option<String>, pub job_id: Option<String>,
    pub registry_version: Option<String>, pub hosted_names: Vec<String>,
    pub approx_size_mb: u32, pub needs_node: bool, pub remote_install_allowed: bool,
}

pub struct JobProgress {
    pub id: String,
    /// `install` | `host`
    pub kind: String,
    pub agent_id: String, pub version: String,
    /// install: queued | downloading | verifying | extracting | installing | validating | done | failed
    /// host:    queued | starting | done | failed
    pub state: String,
    pub bytes: u64, pub total: Option<u64>,
    pub message: Option<String>, pub error_code: Option<String>,
    /// Set when a `host` job is done.
    pub service_key: Option<String>,
}
```

`pcx.rs` 里还有 bridge 门面用的 `AcpSettingsView` 和 `CustomAgentDef`（定义见 §4.4.1，M6 加入）。`pcx.rs` 同时定义 §4.2.6 表格中每个 `_pcx` 方法的参数结构体（如 `AttachParams { session_id, cwd: Option<String>, tail: Option<u32> }`），以及方法名常量（如 `pub const SESSION_ATTACH: &str = "_pcx/session/attach";`）。

### 4.2 Hub（`pocket-codex-host-svc/src/acp/`，只在桌面编译）

| 文件 | 职责 |
|---|---|
| `mod.rs` | 声明模块（带 §1 的平台门控），re-export `AcpHub`、`HubOptions`、`LaunchSpec`、`serve_ws`、`serve_meta`、`AcpHistorySource`、`AcpSessionDirs`、`AcpError` |
| `error.rs` | `pub enum AcpError`，覆盖 §6 的全部错误代码，用 `thiserror` 实现；提供 `pub fn code(&self) -> &'static str` |
| `launch.rs` | `LaunchSpec`、`AgentConnector` trait、生产用的 `ProcessConnector` |
| `testing.rs` | 用 `#[cfg(any(test, feature = "acp-testing"))]` 门控：`FakeAgent`、`DuplexConnector`（§8.1） |
| `peer.rs` | 和 agent 之间的 NDJSON JSON-RPC 对端 |
| `process.rs` | 进程监管状态机 |
| `session.rs` | `HubSession` 状态机 |
| `hub.rs` | `AcpHub` 和 `HubConnection`：会话表、列表缓存、控制器连接表；标准方法和 `_pcx` 方法的全部处理逻辑（T18） |
| `server.rs` | WebSocket 适配层：在帧和 `HubConnection` 之间转发 |
| `pending.rs` | 权限和 elicitation 待办，以及"先答者胜" |
| `fs.rs` | 只能访问会话目录的 fs 处理器 |
| `auth.rs` | 认证状态、terminal 登录命令的构造 |
| `history.rs` | `AcpHistorySource` |
| `meta.rs` | `serve_meta` |
| `install/` | 安装器（§4.3） |

`crates/pocket-codex-host-svc/Cargo.toml` 的新增依赖：

```toml
[features]
# Exposes acp::testing (FakeAgent, DuplexConnector) to other crates' tests.
acp-testing = []

[target.'cfg(not(any(target_os = "android", target_os = "ios")))'.dependencies]
tar = "=0.4.46"
zip = { version = "=8.6.0", default-features = false, features = ["deflate"] }
flate2 = "=1.1.9"
semver = "=1.0.28"
sha2 = { workspace = true }
base64 = { workspace = true }
toml = { workspace = true }

# 合并进已有的 [target.'cfg(unix)'.dependencies]（Cargo.toml:40-41），不要再新建一张表：
# nix = { workspace = true, features = ["user", "signal", "fs"] }

[dev-dependencies]
# 已有的 tempfile、reqwest 保留，新增：
pocket-codex-host-svc = { path = ".", features = ["acp-testing"] }
tokio = { workspace = true, features = ["test-util"] }
```

- `flate2 1.1.9`、`semver 1.0.28`、`base64 0.22.1`、`sha2 0.10.9`、`toml 0.8.23` 都已经在 `Cargo.lock` 里，真正新引入的只有 `tar` 和 `zip`。改完要跑 AGENTS.md §7 的全量命令。
- 分两步加：M3 只加 `[features]`、nix 的 `signal` 和 `fs`、以及 dev-dependencies；安装器用的 `tar`、`zip`、`flate2`、`semver`、`toml`、`sha2`、`base64` 在 M5 加入，同一个里程碑里把 `tar`、`zip` 加进 `check_mobile_dependencies.py` 的禁止列表。
- 在 dev-dependencies 里让 crate 依赖它自己并打开 `acp-testing`，是为了让 `tests/` 目录下的集成测试也能用上 `acp::testing`。暂停时钟的测试需要 tokio 的 `test-util`，而工作区启用的 `full` 不包含它。

#### 4.2.1 启动规格与连接器（`launch.rs`）

```rust
#[derive(Clone, Debug)]
pub struct LaunchSpec {
    /// Catalog id or custom agent id.
    pub agent_id: String,
    pub display_name: String,
    /// Absolute path.
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Added on top of the App environment.
    pub env: BTreeMap<String, String>,
    pub env_remove: Vec<String>,
    pub version: Option<String>,
    /// True when the catalog pins this exact version.
    pub pinned: bool,
    /// Working directory of the agent process; `None` means the user's home.
    pub cwd: Option<PathBuf>,
    /// Appended after `args` only when launching the ACP agent (e.g. D21's
    /// `--hide-claude-auth`); never passed to terminal-login commands.
    pub launch_only_args: Vec<String>,
}

#[async_trait]
pub trait AgentConnector: Send + Sync {
    async fn connect(&self, spec: &LaunchSpec) -> Result<AgentIo, AcpError>;
}

pub struct AgentIo {
    pub reader: Box<dyn AsyncRead + Send + Unpin>,
    pub writer: Box<dyn AsyncWrite + Send + Unpin>,
    pub stderr: Option<Box<dyn AsyncRead + Send + Unpin>>,
    pub child: Option<ChildHandle>,
}

/// Owns the `tokio::process::Child` and, on Unix, its process-group id.
pub struct ChildHandle { /* child: tokio::process::Child, pgid: Option<i32> */ }
impl ChildHandle {
    pub fn pid(&self) -> Option<u32>;
    /// Close stdin, wait `grace`, then terminate the whole process tree.
    pub async fn terminate(self, grace: Duration);
}
```

`ProcessConnector` 的做法：
- 用 `tokio::process::Command::new(&spec.program)`，依次设置 `.args(&spec.args)`、`.args(&spec.launch_only_args)`、`.envs(&spec.env)`，再逐个执行 `env_remove`。
- 工作目录用 `spec.cwd`；为 `None` 时用 `HOME`（Windows 上是 `USERPROFILE`），再取不到就用 `std::env::temp_dir()`。会话自己的 cwd 由 `session/new` 另外传给 agent。
- stdin、stdout、stderr 都用管道，并设 `kill_on_drop(true)`。
- Unix 上用 `process_group(0)` 让 agent 成为独立进程组，因为 npm 类 agent 还会拉起子进程，例如 `codex app-server`。Windows 上用 `creation_flags(0x0800_0000)`（`CREATE_NO_WINDOW`）。
- `terminate` 的顺序：
  1. 关闭 stdin，等待 `grace`（默认 3 s）。
  2. Unix 上执行 `nix::sys::signal::killpg(pgid, SIGTERM)`，再等 2 s。
  3. 还没退出就执行 `killpg(pgid, SIGKILL)`。Windows 上执行 `taskkill /PID <pid> /T /F`。

#### 4.2.2 对端（`peer.rs`）

```rust
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
pub const STDERR_TAIL_BYTES: usize = 64 * 1024;

pub enum Inbound {
    Request { id: RequestId, method: String, params: Value },
    Notification { method: String, params: Value },
}
pub enum PeerExit { Eof, Io(String) }

impl AgentPeer {
    pub fn start(io: AgentIo, log_file: Option<PathBuf>) -> (Arc<Self>, mpsc::Receiver<Inbound>, JoinHandle<PeerExit>);
    pub async fn request(&self, method: &str, params: Value, timeout: Option<Duration>) -> Result<Value, AcpError>;
    pub async fn notify(&self, method: &str, params: Value) -> Result<(), AcpError>;
    pub async fn respond(&self, id: RequestId, result: Result<Value, RpcError>) -> Result<(), AcpError>;
    /// Lossy UTF-8 of the last `STDERR_TAIL_BYTES` of stderr.
    pub fn stderr_tail(&self) -> String;
}
```

- 读循环用 `read_until(b'\n')`，单行上限 `MAX_LINE_BYTES`。超过上限时，一直丢弃到下一个换行，记一条 `warn!`，再向 inbound 注入 `Notification { method: "_pcx/internal/oversized" }`。Hub 收到后，在当前轮次里插入一个 notice 条目："一条来自 agent 的消息超过 16 MiB，已丢弃"。
- 不是合法 JSON 的行：记录 `debug!` 后忽略。
- 请求 id 用数字，从 1 开始递增。请求超时后删掉对应的 pending，返回 `AcpError::Timeout`。
- stderr 一边写入环形缓冲，一边追加到日志文件 `paths::state_dir()?/logs/acp-<instance>.log`。启动时如果日志已经超过 1 MiB，就依次重命名为 `.1`、`.2`，最多保留两份。
- 写入端用单独的任务，按 `encode_line` 串行写出。

#### 4.2.3 进程监管（`process.rs`）

```
Stopped ──start──► Starting ──initialize ok──► Ready
   ▲                  │ 失败或 60 s 超时           │ 进程退出或 EOF
   │ stop             ▼                            ▼
   └──────────── Restarting(attempt) ◄──────── Crashed
                      │ 5 分钟内第 5 次失败
                      ▼
                    Failed（只能手动重启）
```

状态用 `ProcessState` 表示，定义见 §4.1.5。

- 重启间隔依次为 1、2、4、8、16 s，上限 30 s。5 分钟内第 5 次失败就进入 `Failed`。
- `Starting` 阶段依次执行：
  1. 调用 `HubOptions.launch`，得到这一次的 `LaunchSpec` 和网关配置。
  2. 启动进程，发送 `initialize`。
  3. 如果有网关配置，并且 agent 返回了 gateway 类方法，就执行网关 `authenticate`（§4.2.12）。
  4. 进入 `Ready`。

  网关登录必须在接受会话操作之前完成：两个适配器都在创建会话时就固定了路由（claude-agent-acp 的 `src/acp-agent.ts` 约 `:8692`，codex-acp 的 `src/CodexAcpClient.ts:839-859`），登录前创建或加载的会话不会走网关。网关登录失败时照常进入 `Ready`，但认证状态记为 required。
- 只有在 `Ready` 状态才接受会话操作。其他状态下，控制器的请求一律返回 `AGENT_UNAVAILABLE`；`_pcx/hub/state` 会带上当前状态，以及 stderr 的最后 2 KiB。
- 进程退出（Crashed）时依次处理：
  1. 所有会话设 `agent_loaded = false`，`phase` 设为 `Listed`（转录保留），所以之后的 attach 和 submit 都会先走加载。
  2. 运行中的会话结束实时轮次，`stop_reason = "_pcx_agent_exited"`，并广播 `_pcx/turn/completed`，error 为"agent 进程已退出"。
  3. 所有待办作废，逐条广播 `_pcx/request/resolved`。
  4. 清空排队的 prompt，广播 `_pcx/queue/failed`，附上原文，方便用户复制后重发。
  5. 进入 `Restarting`，按上面的间隔自动重启。
- `restart()` 等于先 stop 再 start，但不计入失败次数。登录完成后的"重新检测"、设置变更都调用它。

#### 4.2.4 会话（`session.rs`）

```rust
pub enum Phase { Listed, Loading, Idle, Running { turn: u32, submission: String }, Closing }

pub struct HubSession {
    pub id: String,
    pub cwd: String,
    pub title: Option<String>,
    pub updated_at: Option<String>,
    pub phase: Phase,
    pub agent_loaded: bool,
    /// Value of `updated_at` when `transcript` was materialized.
    pub transcript_updated_at: Option<String>,
    /// `n` in the `{epoch}.{n}` generation; survives LRU eviction of `transcript` (§4.1.3).
    pub generation_seq: u32,
    /// Last `seq` assigned to a broadcast session notification (T14).
    pub seq: u64,
    pub transcript: Option<Transcript>,
    pub queue: VecDeque<Queued>,
    pub pending: BTreeMap<String, Pending>,
    pub subscribers: HashSet<ConnId>,
    pub config_options: Vec<ConfigOption>,
    pub modes: Option<SessionModeState>,
    pub commands: Vec<AvailableCommand>,
    pub usage: Option<UsageUpdate>,
    pub last_activity_ms: i64,
    /// clientSubmissionId → result; 256 entries, 10 minutes.
    pub submissions: SubmissionLog,
}
/// Bounded FIFO of (client_submission_id, SubmitResult, recorded_at_ms); entries
/// older than 10 minutes or beyond 256 are dropped on insert.
pub struct SubmissionLog(VecDeque<(String, SubmitResult, i64)>);
pub struct Queued { pub submission: String, pub client_submission: String, pub prompt: Vec<ContentBlock>, pub queued_at_ms: i64 }
```

| 事件 | 条件 | 动作 | 结果状态 |
|---|---|---|---|
| attach | `agent_loaded`，并且有转录 | 无 | 不变 |
| attach | `agent_loaded`，但没有转录（按上面的 LRU 规则不会出现，这里只做兜底） | 按下面"未加载"的几行处理：能 resume 就 resume，否则 load | 同下 |
| attach | 未加载，有转录，`updated_at == transcript_updated_at`，agent 支持 resume | `session/resume` | Idle |
| attach | 未加载，agent 支持 `loadSession` | `session/load`，回放进一个新转录，再按 §4.1.3 的 `adopt_ids_from` 决定 generation | Loading → Idle |
| attach | 未加载，agent 只支持 resume、不支持 load，并且没有转录或转录已过期（`updated_at != transcript_updated_at`） | 丢弃过期的转录，`session/resume`，新建一个空转录（`dropped_turns` 记为 1，因此 `older_unavailable = true`，`generation_seq` 加 1）。会话可以继续对话，但看不到以前的历史 | Idle |
| attach | 未加载，既不能 load 也不能 resume | 返回 `SESSION_NOT_LOADABLE` | 不变 |
| submit | Listed（未加载） | 先按 attach 的规则加载；加载完成后按 Idle 处理；无法加载则返回 `SESSION_NOT_LOADABLE` | Loading → Running |
| submit | Idle | `begin_live_turn`，在后台任务里调用 `session/prompt` | Running |
| submit | Running 或 Loading | 加入队列，返回队列位置 | 不变 |
| prompt 返回 | Running | `end_live_turn(stopReason)`，广播 `_pcx/turn/completed`；队列不空就立刻提交下一条 | Idle 或 Running |
| prompt 报错 | Running | 结束轮次，`stop_reason = "_pcx_error"`，error 为错误信息；如果错误码是 `-32000`，同时把认证状态设为 required | Idle |
| cancel | Running | 发送 `session/cancel`；这个会话的待办按 §4.2.7 回应 cancelled；清空 Hub 的排队，广播 `_pcx/queue/failed`（reason `cancelled`，附原文），因为"停止执行"的意思是不再继续 | 不变，等 prompt 返回 |
| reload | 非 Running | 强制 `session/load` | Loading → Idle |
| 空闲回收 | Idle，无订阅、无待办、无排队，距上次活动超过 10 分钟，agent 支持 close | `session/close`，`agent_loaded = false`，转录保留在 LRU 中 | Listed |
| 转录 LRU | 转录总数超过 16 个，或总字节超过 256 MiB | 在不在运行、没有订阅者、没有待办的会话里，选最久未访问的：<br>- 已加载且 agent 支持 close：先 `session/close`，`agent_loaded = false`，再丢弃转录，进入 Listed。<br>- 未加载：直接丢弃转录。<br>- 已加载但 agent 不支持 close：跳过，不丢弃。<br>找不到可丢弃的会话时，允许暂时超出上限，记一条 warn | 见左 |

- 在 Loading 期间到达的 attach 不会重复发起 load，而是等待同一个加载任务，最多等 20 s（T17）。超时后先返回 `loading: true`；加载完成时，Hub 向订阅了这个会话的连接发送 `_pcx/session/loaded {sessionId}`，失败时发送 `_pcx/session/loadFailed {sessionId, error}`。加载本身仍受 `session/load` 的 300 s 超时约束。
- 空闲回收每 60 s 检查一次。
- `submit` 按 `clientSubmissionId` 做幂等：10 分钟内重复提交，返回第一次的结果。bridge 在"提交结果未知"时靠它安全地重发。
- 如果 agent 不支持 `session/list`：Hub 把自己创建过的会话记录在 `<state_dir>/acp/sessions/<instance>.json`，字段为 `[{sessionId, cwd, title, updatedAt}]`，最多 500 条，文件权限 0600。这是 Hub 自己的索引，不是 agent 的私有文件。

#### 4.2.5 Hub API（`hub.rs`）

```rust
/// What to launch and how to log in through a gateway; re-read on every start and restart,
/// so settings changes (D20 gateway, D21 flags) apply after `restart()`.
pub type LaunchProvider = Arc<dyn Fn() -> Result<(LaunchSpec, Option<GatewayAuth>), AcpError> + Send + Sync>;

/// D20. `headers` already contains `Authorization: Bearer <token>` plus extra headers.
#[derive(Clone)]
pub struct GatewayAuth { pub method_id: Option<String>, pub base_url: String, pub headers: BTreeMap<String, String>, pub provider_name: Option<String> }

pub struct HubOptions {
    pub instance: String,
    /// Production: `install::resolve_launch` plus the agent's gateway settings; tests return fixed values.
    pub launch: LaunchProvider,
    /// `<state_dir>/acp`; holds sessions/<instance>.json and run/.
    pub state_dir: PathBuf,
    pub log_file: Option<PathBuf>,
    pub connector: Arc<dyn AgentConnector>,
    pub terminal: Option<Arc<dyn TerminalLauncher>>,
}

pub struct HubInfo {
    pub agent_id: String,
    pub agent_name: String,
    pub agent_version: Option<String>,
    pub agent_info: Option<Implementation>,
    pub pinned: bool,
    pub caps: PcxCaps,
    pub auth: AuthState,
    pub process: ProcessState,
    pub pid: Option<u32>,
    pub program: PathBuf,
}

impl AcpHub {
    /// Call `options.launch`, spawn, wait for the first `Ready` or `Failed` (≤ 60 s), then
    /// wait for the first auth detection (≤ 15 s) so the returned `info().auth` is meaningful.
    pub async fn start(options: HubOptions) -> Result<Arc<Self>, AcpError>;
    pub fn info(&self) -> HubInfo;
    /// A transport-independent controller connection (T18): a cloneable handle
    /// plus the receiver of everything the hub sends to this controller.
    pub fn open_connection(self: &Arc<Self>) -> (HubConnection, mpsc::Receiver<RpcMessage>);
    pub async fn restart(&self) -> Result<(), AcpError>;
    /// Cancel running turns, wait up to `grace` for them, then terminate the process.
    pub async fn shutdown(&self, grace: Duration);
    pub fn running_sessions(&self) -> Vec<String>;
    pub fn session_cwd(&self, session: &str) -> Option<String>;
    pub async fn history_window(&self, query: &WindowQuery) -> anyhow::Result<HistoryWindow>;
    pub async fn authenticate(&self, method_id: &str) -> Result<AuthState, AcpError>;
    pub fn terminal_login(&self, method_id: &str) -> Result<(), AcpError>;
    pub async fn recheck_auth(&self) -> Result<AuthState, AcpError>;
}

pub struct AcpSessionDirs(pub Arc<AcpHub>);   // impl SessionDirResolver via session_cwd

#[derive(Clone)]
pub struct HubConnection { /* id: ConnId, hub: Arc<AcpHub>, tx: mpsc::Sender<RpcMessage> */ }
pub struct ConnId(pub u64);   // unique within the hub
impl HubConnection {
    pub fn id(&self) -> ConnId;
    /// Accept one message from the controller and return quickly. Requests
    /// are answered on the receiver returned by `open_connection`; anything
    /// that may wait (agent calls, loads, turns) runs in a spawned task.
    pub fn handle(&self, message: RpcMessage);
    /// Detach from every session; pending requests stay with the hub.
    pub fn close(&self);
}
```

- `open_connection` 返回的接收端容量是 4096 条（§4.2.6）。`HubConnection` 另有 `pub fn closed(&self) -> tokio_util::sync::CancellationToken`：出站通道写满时，Hub 取消这个 token 并把连接移出订阅者；`server.rs` 在读写任务里 `select!` 这个 token，被取消时以关闭码 1013 关闭 WebSocket。
- `handle` 是同步函数，**不会**在调用方的读取循环里等待。每个请求都在独立的 tokio 任务里处理，处理完把响应写进出站通道。因此：
  - 同一个连接上，控制器可以在 `session/prompt` 等待期间照常回应权限请求。
  - 读取循环不会被阻塞，WebSocket 的 ping 能及时回应（`AppClient` 每 15 s 发一次 ping，20 s 没有回应就断开，见 `client.rs:76-84`）。
- 同一个会话的通知，由会话自己的串行任务按折叠顺序写出，不受请求处理任务的并发影响。
- attach 和 reload 也在这个串行任务里完成：同一步里生成快照（带当前 `seq`）、把连接加入订阅者、把响应写进出站通道。所以这个连接之后收到的该会话通知，`seq` 一定大于快照里的 `seq`。
- M3 的测试直接用 `open_connection` 驱动整个协议，不经过 WebSocket。

会话列表：
- agent 支持 list 时，翻页取全部（最多 20 页、500 条），并和本进程里 Hub 创建的会话合并，按 `updatedAt` 倒序排列，缺少 `updatedAt` 的排在最后。结果缓存 5 s。
- agent 不支持 list 时，读 §4.2.4 的索引文件。
- 每次刷新后，把 `updatedAt` 发生变化的会话记录下来：对这些会话里有订阅者、并且不在 Running 的，广播 `_pcx/session/state`，由 bridge 决定是否 reload。这就是外部写入者的轮询检测（D9）。控制器打开会话期间，bridge 每 15 s 调用一次 `session/list`。
  - Hub 自己驱动的轮次也会改变 `updatedAt`。所以每个实时轮次结束后，Hub 在下一次列表刷新时，把该会话的新 `updatedAt` 直接记为基准值，同时写入 `updated_at` 和 `transcript_updated_at`，不算作外部变化。这样会话被空闲回收后再打开时，仍然可以走 `session/resume`，不用重新 load。
- 第一次请求列表时如果还没有缓存：最多等 5 s（§4.2.6）；等不到就返回空列表，`_meta.pcx.loading = true`，刷新完成后广播 `_pcx/sessions/changed`。

#### 4.2.6 面向控制器的协议（`server.rs`）

协议逻辑写在 `hub.rs` 的 `HubConnection` 里（T18）。`server.rs` 只提供 `pub async fn serve_ws(listener: TcpListener, hub: Arc<AcpHub>) -> anyhow::Result<()>`：
- 每个 WebSocket 连接调用一次 `hub.open_connection()`。
- 读任务：把收到的文本帧用 `rpc::decode` 解析后交给 `handle`。
- 写任务：把接收端里的消息用 `rpc::encode` 写回。
- 任一任务结束时调用 `close()`。

回应期限（T17）：Hub 在期限内一定会给出回应。agent 那边超时，或者 Hub 自己的等待超过期限时，返回 `pcx_error(INTERNAL_ERROR, "timeout", …)`；已经交给 agent 的操作继续在后台完成，结果通过通知广播。

| 控制器方法 | 期限 | 超过期限的处理 |
|---|---|---|
| `initialize`、`session/list`、`_pcx/sessions/running`、`_pcx/session/window`、`_pcx/session/detach` | 5 s | `session/list` 直接返回缓存，同时在后台刷新，刷新完成后广播 `_pcx/sessions/changed` |
| `_pcx/session/attach`、`_pcx/session/reload` | 20 s | 返回 `loading: true`，之后发 `_pcx/session/loaded` 或 `loadFailed` |
| `_pcx/session/submit` | 20 s | 会话未加载时立即返回 `queued: true`（§4.2.4） |
| `session/set_config_option`、`session/set_mode` | 30 s | 返回 `acp.timeout`；agent 之后发的 `config_option_update` 照常广播 |
| `session/new` | 45 s | 返回 `acp.timeout`。agent 之后如果创建成功，新会话会出现在列表里，并广播 `_pcx/sessions/changed` |
| `_pcx/auth/authenticate` | 立即 | 返回 `inProgress`（§4.2.9） |
| 标准 `session/prompt` | 不限 | 只给标准 ACP 客户端用，bridge 不调用 |

Hub 调用 agent 时，如果 agent 侧的超时（§3.3）比上表的期限长，就按上表的期限回应控制器，agent 侧的调用继续执行。

`serve_ws` 的要求：
- 只接受回环地址，否则 bail，和 `opencode/gateway.rs:128` 一样。
- 路由：`GET /acp`（WebSocket 升级）和 `GET /healthz`。
- 单帧和单条消息的上限都是 64 MiB，与 `AppClient` 一致。服务端不协商压缩；`AppClient` 发现对方不接受 permessage-deflate 时，会照常建立连接。
- 每个连接必须先调用 `initialize`，否则其他请求一律返回 `INVALID_REQUEST`。
- 每个连接只有一个写出任务，所以同一个会话的通知严格按 Hub 折叠的顺序送达。出站队列上限 4096 条；溢出时以关闭码 1013 断开连接，bridge 会重连并重新 attach。

`initialize` 的返回值：

```json
{"protocolVersion":1,
 "agentCapabilities":{"loadSession":true,"promptCapabilities":{"image":true,"audio":false,"embeddedContext":true},
                      "sessionCapabilities":{"list":{}},"mcpCapabilities":{}},
 "authMethods":[],
 "agentInfo":{"name":"pocket-codex-acp-hub","version":"<CARGO_PKG_VERSION>"},
 "_meta":{"pcx":{"version":1,
   "agent":{"id":"claude-acp","name":"Claude Code","version":"0.84.0","pinned":true,"info":{"name":"…","version":"…"}},
   "caps":{"list":true,"load":true,"resume":true,"close":true,"image":true,"embeddedContext":true,
           "configOptions":true,"modes":false,"commands":true,"queue":true,"steer":false,"urlElicitation":true},
   "auth":{"status":"ok","methods":[]},
   "process":{"state":"ready"},
   "defaultConfigOptions":[]}}}
```

- `promptCapabilities` 原样转发 agent 的值。
- `caps` 的来源：
  - `list`、`resume`、`close`：agent 的 `sessionCapabilities` 里有对应对象。
  - `load`：agent 的 `loadSession`。
  - `image`、`embeddedContext`：agent 的 `promptCapabilities`。
  - `configOptions`、`modes`：最近一次会话响应里带了 `configOptions`、`modes`。
  - `commands`：收到过 `available_commands_update`。
  - `queue`、`urlElicitation` 固定为 true；`steer` 固定为 false。`queue` 表示 Hub 会给多个控制器之间、或者和界面自己的本地排队撞上的提交兜底排队；界面不为它单独提供控件。
- `defaultConfigOptions`：Hub 记住的最近一次 `session/new`、`load` 或 `resume` 返回的 configOptions。还没有打开任何会话时，模型列表就用它。它按实例保存在 `<state_dir>/acp/defaults/<instance>.json`（0600，只在 agent id 相同时恢复），重启后不等会话打开就可用；见 `_pcx/hub/defaults`。

标准方法（Hub 对控制器表现为一个"虚拟 agent"）：

| 方法 | Hub 的行为 |
|---|---|
| `session/list {cwd?, cursor?}` | 返回合并后的列表，cursor 是 Hub 自己的 `o:<offset>`，每页 100 条；每个 SessionInfo 在 `_meta.pcx` 里带 `{running, pending, queue}` |
| `session/new {cwd, mcpServers?}` | 转发给 agent，但忽略控制器给的 `mcpServers`，统一传 `[]`。新会话进入 Idle，调用方自动订阅；然后广播 `_pcx/sessions/changed` |
| `session/load {sessionId, cwd}` | 先 attach，再把 Hub 的整个转录按 `session/update` 重放给这个连接，然后返回。供标准 ACP 客户端使用，bridge 不调用 |
| `session/prompt` | 语义同 `_pcx/session/submit`，但要等这一轮结束才返回 `{stopReason}`。供标准客户端使用 |
| `session/cancel` | 会话在 Running 时转发给 agent |
| `session/set_config_option`、`session/set_mode` | 转发给 agent，把结果写回会话状态，并以 `session/update` 广播 |
| 其他 agentMethods | 返回 `METHOD_NOT_FOUND` |

`_pcx` 方法（控制器 → Hub）：

| 方法 | 参数 | 结果 |
|---|---|---|
| `_pcx/session/attach` | `{sessionId, cwd?, tail?: u32}`，tail 默认 20，最大 100 | `AttachResult`（§4.1.5），最多等 20 s，超时就返回 `loading: true`（§4.2.4）。返回后，Hub 立即把这个会话的待办以请求形式发给该连接 |
| `_pcx/session/detach` | `{sessionId}` | `{}` |
| `_pcx/session/window` | `{sessionId, generation, before?: itemId, turn?: u32, after?: itemId, limit?: u32}`，limit 默认 60，最大 100 | `WindowResult`；generation 对不上时返回 `GENERATION_CHANGED`；会话还在加载时返回 `SESSION_LOADING` |
| `_pcx/session/submit` | `{sessionId, prompt: ContentBlock[], clientSubmissionId}` | `SubmitResult`；会话还没加载时，Hub 先在后台加载，并立即返回 `queued: true` |
| `_pcx/session/reload` | `{sessionId}` | `AttachResult`，和 attach 一样最多等 20 s |
| `_pcx/sessions/running` | `{}` | `{sessions: RunningSession[]}` |
| `_pcx/auth/authenticate` | `{methodId}` | 立即返回 `AuthState`（`status: "inProgress"`），结果之后通过 `_pcx/hub/state` 广播；terminal 类方法返回 `HOST_ONLY` |
| `_pcx/hub/defaults` | `{}` | `{configOptions: ConfigOption[]}`：新会话的默认配置项。Hub 已知时直接返回；一无所知、agent 支持 `session/close`、不需要登录、而且这个 agent 版本还没探测过时，Hub 在 `<state_dir>/acp/probe/` 里新建一个探测会话读出配置项，随即关闭它。探测会话记入 defaults 文件，`session/list` 永远不列出它（也不列出 cwd 为探测目录的会话）。探测拿到配置项后广播 `_pcx/hub/state` |

- `older_unavailable` 在 `dropped_turns > 0`、而且窗口已经到达最早保留的条目时为 true。
- 回应大小上限：attach、reload、window 的结果序列化后不超过 16 MiB（低于 `AppClient` 的 64 MiB 帧上限）。超出时，从最早的条目开始去掉，直到满足上限，同时把 `has_older` 置为 true。单个条目本身已经受 `MAX_ITEM_TEXT` 和 `MAX_RAW_JSON` 约束；图片块单张超过 4 MiB 时，Hub 在回应里换成一个 text 块 `[图片过大，未传输]`。
- 所有错误响应都用 `rpc::pcx_error` 构造（T16）。agent 返回的其他错误原样透传 message，加前缀 `[acp.agent_error] `。对应关系是：`GENERATION_CHANGED` → `acp.generation_changed`，`SESSION_LOADING` → `acp.session_loading`，`SESSION_NOT_LOADABLE` → `acp.session_not_loadable`，`AGENT_UNAVAILABLE` → `acp.agent_unavailable`，`HOST_ONLY` → `acp.host_only`。agent 返回的 `-32000` 转成 `acp.auth_required`；Hub 拒收图片时用 `INVALID_PARAMS` + `acp.images_unsupported`。

Hub → 控制器的通知：

| 方法 | 参数 |
|---|---|
| `session/update` | 标准形状，另在 `params._meta.pcx` 里带 `{seq, itemId?, created?, turn, generation, item?}`（T14）。下面这些会话级通知的参数里也都带 `seq`；`_pcx/sessions/changed` 和 `_pcx/hub/state` 是 Hub 级别的，不带 |
| `_pcx/session/loaded` | `{sessionId}` |
| `_pcx/session/loadFailed` | `{sessionId, error}` |
| `_pcx/turn/started` | `{sessionId, turn, submissionId, userItemId, startedAtMs}` |
| `_pcx/turn/completed` | `{sessionId, turn, stopReason, error?, completedAtMs, durationMs}` |
| `_pcx/request/resolved` | `{sessionId, requestId}` |
| `_pcx/session/state` | `{sessionId, running, queue, pending, title?, updatedAt?}` |
| `_pcx/session/generation` | `{sessionId, generation}` |
| `_pcx/queue/failed` | `{sessionId, reason, prompts: [{submissionId, text}]}` |
| `_pcx/sessions/changed` | `{}` |
| `_pcx/hub/state` | `HubMeta`（§4.1.5），与 initialize 结果里的 `_meta.pcx` 相同 |

Hub → 控制器的请求：`session/request_permission` 和 `elicitation/create`，参数保持标准形状，`params._meta.pcx.requestId` 是 Hub 的待办 id。

prompt 里的图片和文件：
- bridge 把 `app_turn_start` 的 `images` 转成 image 块。这些 `images` 就是界面现有的 `data:image/...;base64,...` URL（`apps/flutter/lib/src/bridge_api.dart:1589`）。
- agent 不支持 image（`promptCapabilities.image == false`）时，Hub 拒绝这次提交，错误为 `pcx_error(INVALID_PARAMS, "images_unsupported", "this agent does not accept images")`。界面在发送前就按能力禁用图片附件（§4.7.5）。

#### 4.2.7 待办（`pending.rs`）

```rust
pub struct Pending {
    /// "r{n}", unique within the hub.
    pub id: String,
    pub session_id: Option<String>,
    pub kind: PendingKind,
    pub agent_request: RequestId,
    pub created_ms: i64,
    pub sent_to: HashMap<ConnId, RequestId>,
}
pub enum PendingKind {
    Permission { tool_call: ToolCallUpdate, options: Vec<PermissionOption> },
    Form { message: String, schema: Value },
    Url { message: String, url: String, elicitation_id: String },
}
```

- 收到 agent 的请求后登记为待办，并发给这个会话当前的所有订阅者。每个连接用它自己的请求 id，记录在 `sent_to` 里。
- 新的订阅者 attach 成功后立即补发。没有订阅者时，待办一直保留。
- 第一个合法的回应胜出：Hub 回应 agent，再给其他已经收到这个请求的连接各发一条 `$/cancel_request {requestId}`，并广播 `_pcx/request/resolved {sessionId, requestId: <Hub 待办 id>}`。之后才到的回应一律忽略。
  - `$/cancel_request` 里的 `requestId` 按 ACP 的定义，填 Hub 发给该连接时用的 JSON-RPC 请求 id，也就是 `sent_to` 里记录的值。
  - bridge 收到后，按这个 id 找到 `AppClient` 的 token（token 就是请求 id 的字符串形式）。
- 什么样的回应算合法：
  - 权限：`optionId` 必须在 options 里，或者回应 `cancelled`。
  - 表单：`action` 必须是 `accept`、`decline` 或 `cancel`；`accept` 的 content 只能包含 schema 里定义的属性。
  - URL：`action` 必须是 `accept`、`decline` 或 `cancel`。
  - 控制器的回应本身是 JSON-RPC 响应，没法再"回应"它。所以回应不合法时，Hub 用一个新的请求 id 把同一个请求重新发给这个连接，并在 `params._meta.pcx.rejected` 里写明原因，同时更新 `sent_to`。待办继续保留。
  - bridge 在回应之前先在本地做同样的检查（optionId 在选项里、表单字段在 schema 里），不合法时直接向界面返回错误，不发出去。
- 会话被取消（cancel）时，这个会话的权限待办回应 `{"outcome":{"outcome":"cancelled"}}`，elicitation 待办回应 `{"action":"cancel"}`。
- 收到 `elicitation/complete {elicitationId}` 时，结束对应的 URL 待办，并广播 resolved。
- elicitation 只发给 initialize 时声明了对应模式的连接；bridge 两种模式都声明。
- 作用域是 `requestId`（不属于任何会话）的 elicitation 挂在 Hub 级别，`session_id` 为 `None`，发给所有连接，每个新连接 initialize 之后也会补发。这类请求通常是登录流程里的 device code，bridge 发出的 AppEvent 里 `thread_id` 为 None；界面的处理方式见 §4.7.5。

#### 4.2.8 fs 处理器（`fs.rs`）

- 允许访问的根目录只有会话的 `cwd`。会话不存在时返回 `RESOURCE_NOT_FOUND`。
- `fs/read_text_file {sessionId, path, line?, limit?}`：
  - path 必须是绝对路径，`canonicalize` 之后必须位于 `canonicalize(cwd)` 之下。
  - 文件不超过 16 MiB，按 UTF-8 读取，非法字节用替换字符。
  - `line` 从 1 开始，`limit` 是行数。返回 `{content}`。
- `fs/write_text_file {sessionId, path, content}`：
  - path 必须是绝对路径；父目录 canonicalize 之后必须位于根目录之下；目标本身是符号链接时拒绝。
  - content 不超过 32 MiB。先写到同一目录的临时文件，再 rename 过去；目标已存在时，在 Unix 上保留原来的权限位。返回 `{}`。
- 路径越界返回 `INVALID_PARAMS`，message 为 "path is outside the session directory"；文件不存在返回 `RESOURCE_NOT_FOUND`。
- 每次写入记一条 `info!` 日志，只记路径和字节数，不记内容。

#### 4.2.9 认证（`auth.rs`）

认证状态用 `AuthState` 和 `AuthMethodInfo` 表示，定义见 §4.1.5。

```rust
pub struct TerminalLaunch { pub program: PathBuf, pub args: Vec<String>, pub env: BTreeMap<String, String>, pub title: String }

pub trait TerminalLauncher: Send + Sync {
    /// Open a visible terminal that runs `launch` and writes its exit code to `status_file`.
    fn open(&self, launch: &TerminalLaunch, status_file: &Path) -> Result<(), AcpError>;
}
```

- 检测时机：进入 `Ready` 后，如果 agent 支持 list，就调用一次 `session/list {}`。成功记为 ok，返回 `-32000` 记为 required，其他错误记为 unknown。agent 不支持 list 时记为 unknown。之后任何请求返回 `-32000`，都把状态改为 required，并广播 `_pcx/hub/state`。
- 方法分类：
  - `type == "terminal"`：terminal 类，args 和 env 取自方法本身。
  - `_meta["terminal-auth"] = {command, args, label}`（旧写法）：也是 terminal 类。只有当 `command` 等于 `LaunchSpec.program` 的文件名（去掉 `.exe`）时才可用，此时 program 用 `LaunchSpec.program`，args 用这里给出的 args。否则 `available = false`，把 description 原样显示给用户。
  - 带 `_meta.gateway` 的方法：gateway 类，`remote = false`（密钥只能在主机桌面上配置），`gateway_protocol` 取 `_meta.gateway.protocol`。处理方式见 §4.2.12。
  - 其他：agent 类，`remote = true`。terminal 类一律 `remote = false`。
- terminal 登录（只在主机桌面进行，按规范写法）：program 为 `LaunchSpec.program`，args 为 `LaunchSpec.args` 后面追加 `method.args`，env 为 `LaunchSpec.env` 加上 `method.env`。
  - **不包含** `launch_only_args`。例如 claude-agent-acp 会把 `--cli` 之后的参数原样转给 Claude CLI（`src/index.ts:12-14`），而 CLI 不认识 `--hide-claude-auth`，会报 unknown option。
  - 旧写法的 args 里如果出现 `launch_only_args` 里的参数（适配器是用 `process.argv` 拼出这组参数的），先去掉这些参数再执行。
- Hub 把命令交给 `TerminalLauncher`（实现见 §4.4.1），状态文件放在 `<state_dir>/acp/run/<rand>.status`。之后每 2 s 检查一次，最长 15 分钟：
  - 退出码为 0：调用 `restart()`，然后重新检测。
  - 退出码非 0：`message` 设为"登录未完成（退出码 N）"。
  - 用户也可以随时点"重新检测"，效果等于 `recheck_auth()` = `restart()` 加检测。
- agent 类：`AcpHub::authenticate` 把状态设为 `inProgress` 并广播，然后在后台调用 `authenticate {methodId}`，超时 600 s。完成后重新检测，再广播一次 `_pcx/hub/state`。
  - 有的 agent 的 `authenticate` 什么也不做，直接返回成功（例如 OpenCode 没有声明 `_meta` 写法时，research §3.3）。这种情况下重新检测后仍是 required，就把 `message` 设为该方法的 description，比如"在终端里运行 `opencode auth login`"。
  - 我们的 initialize 声明了 `_meta["terminal-auth"]`，所以 OpenCode 一般会返回旧写法的 terminal 方法，走 terminal 登录。Codex 的 device code 会在这个过程中发出 URL elicitation，按 §4.2.7 处理。FRB 和 `_pcx` 调用只返回 `inProgress` 状态，不会阻塞等待结果（T17）。

#### 4.2.10 历史源（`history.rs`）

```rust
pub struct AcpHistorySource { hub: Arc<AcpHub> }

#[async_trait]
impl SessionHistorySource for AcpHistorySource {
    fn provider(&self) -> &'static str { "acp/hub-v1" }
    async fn read_window(&self, query: &WindowQuery) -> Result<HistoryWindow>;
}
```

| collection | 参数 | 返回 |
|---|---|---|
| `metadata` | 无 | `metadata = {sessionId, cwd, title, updatedAt, running, turns, olderUnavailable}`，不含文档 |
| `items` | `limit`；`projection`；`cursor`。与 Codex 适配器（`crates/pocket-codex-host-svc/src/history_sync.rs:171-177`、`:238-249`）语义相同：`projection` 省略时从**最新**的条目开始倒序，cursor 为 `b:<itemId>`，表示继续往更早翻；`projection = "asc"` 时从**最早**的条目开始正序，cursor 为 `a:<itemId>`，表示继续往更新翻 | 文档是 HubItem，以 item id 为键；`order` 按请求的方向排列；`metadata = {nextCursor?, hasOlder, olderUnavailable}` |
| `groups` | `cursor` 为 `t:<turn>`；`limit`；不接受 `asc`，始终从最新的轮次开始倒序，与 Codex 相同 | 文档是 TurnInfo，以 `t{turn}` 为键；`metadata = {nextCursor?}` |

`projection` 取 `"desc"` 时等同于省略。Codex 的预取会发送 `desc`（`crates/pocket-codex-bridge/src/engine/session_sync.rs:428-431`）。

- generation 就是转录的 generation。
- 会话还没有物化转录时，触发和 attach 相同的加载，但不订阅、不发 prompt。这是 D9 已接受的偏离。
- 加载最多等 25 s。超时就返回错误 `history is still loading`：router 返回 502，bridge 把这次预取记为失败；加载在后台继续，下一次请求直接命中。
- 会话无法加载时，错误信息写 `session cannot be loaded by this agent`。这句不含 "cursor"，所以 router 返回 502，不是 410。

#### 4.2.11 meta 服务（`meta.rs`）

```rust
pub async fn serve_meta(listener: TcpListener, store: Arc<ConfigStore>, host: Arc<HostStore>, uploads_dir: PathBuf, hub: Arc<AcpHub>) -> anyhow::Result<()>
```

- 把 `crates/pocket-codex-host-svc/src/lib.rs:155-177` 里 `serve_generic` 构建 router 的部分，提取成 `pub(crate) fn generic_app(store, host, uploads_dir, session_dirs) -> Router`。`serve_generic` 改为调用它，行为不变。
- `serve_meta` 在 `generic_app(…, Arc::new(AcpSessionDirs(hub.clone())))` 的基础上，再 merge `history_sync::router(Arc::new(AcpHistorySource::new(hub)))`。
- uploads 目录为 `paths::state_dir()?/acp/uploads/<instance>`。
- 远程管理入口：`acp::install::manage::router()`（§4.3.11）是无状态的 `Router`，所以要在 `.with_state(state)` 之后 merge，和 `history_sync::router` 的挂法相同（`lib.rs:116`）。**只改两处**：Codex 的 `serve()`（`lib.rs:103-118`）和 `generic_app`。`serve_meta` 基于 `generic_app` 构建，已经带上这些路由，不能再 merge 一次，否则 axum 会因路由重复而 panic。这样 Codex、OpenCode、ACP 的每一个 meta 服务都有远程管理入口。
  - 这两处 merge 都加桌面平台门控。
  - CLI 进程（`pocket-codex` 命令行）没有注册 `AcpManagement` 实现，这些路由统一返回 404。
  - `manage.rs` 在 M5 才有，所以这两处 merge 也在 M5 加。M4 的 `serve_meta` 先只挂通用路由和 history。

#### 4.2.12 模型网关与订阅登录（D20、D21）

背景：用户常用 New API 这类中转站作为模型网关，用 API key 访问模型，不用 Claude.ai 订阅。

**网关登录（D20）**

- 适配器接受的请求形状（两个适配器的源码都已核对）：
  - claude-agent-acp：`authenticate {methodId, _meta: {gateway: {baseUrl?, headers?}}}`，见 `src/acp-agent.ts:1432-1451`。它会把 baseUrl 和 headers 映射成 Claude Code 的环境变量，绕过标准登录。
  - codex-acp：`authenticate {methodId: "gateway", _meta: {gateway: {baseUrl, headers, providerName?}}}`，见 `src/CodexAuthMethod.ts:48-66`。它声明的协议是 `openai`，`restartRequired: "false"`，内部按 `wire_api = "responses"` 配置 provider。是否提供这个方法由同文件约 `:76-84` 的 `supportsGatewayAuth` 判断。
  - Claude 有两个网关方法：`gateway`（anthropic 协议）和 `gateway-bedrock`。`baseUrl` 必须是 http(s) 地址；它自己会设 `ANTHROPIC_AUTH_TOKEN="acp-proxy"`，并把 headers 映射成 `ANTHROPIC_CUSTOM_HEADERS`（`src/acp-agent.ts` 约 `:9559-9564`）。我们通过 headers 发的 `Authorization: Bearer <token>` 能不能覆盖它默认的 `Bearer acp-proxy`，要在 M10 第 15 项实测确认。如果覆盖不了，就改为同时发送 `x-api-key: <token>`，这种方式 New API 同样接受。
- Hub 的做法按能力决定，不按 agent 名分支：
  1. initialize 时声明 `auth._meta.gateway: true`（§3.2）。
  2. 返回的方法里带 `_meta.gateway` 的，归为 gateway 类（§4.2.9）。
  3. `agents.toml` 里给这个 agent 配了网关（§5.1）时，`HubOptions.launch` 会同时返回 `GatewayAuth`。Hub 在每次启动（包括崩溃后重启和 `restart()`）的 `Starting` 阶段，`initialize` 之后、进入 `Ready` 之前（§4.2.3），调用一次：

     ```json
     {"method":"authenticate","params":{"methodId":"<配置的 method_id，缺省为第一个 gateway 类方法>",
       "_meta":{"gateway":{"baseUrl":"<base_url>","headers":{"Authorization":"Bearer <token>", "...":"<extra_headers>"},
                           "providerName":"<provider_name，可选>"}}}}
     ```

  4. 成功后，`AuthState.status` 记为 ok。失败时记为 required，`message` 里写"网关登录失败：<错误信息>"，但不写出密钥。
- 配置了网关的 agent，界面的登录区显示"已配置网关"，不再提示订阅登录或 terminal 登录。
- 用户已经写在 `~/.claude/settings.json`（`env` 里的 `ANTHROPIC_BASE_URL` 等）或 `~/.codex/config.toml`（`model_providers`）里的网关配置照常生效：Claude 适配器会读取 user、project 和 local 三级设置（`src/acp-agent.ts:8710`），codex-acp 使用用户自己的 `CODEX_HOME`。Pocket-Codex 的网关配置是可选的，配了就以它为准。
- OpenCode 不提供网关类方法，继续用它自己的 provider 配置。原生的 Codex 和 OpenCode provider 不受影响。
- 密钥的处理：
  - 以明文保存在 `agents.toml`（权限 0600），与 `config.toml` 保存账号 token 的做法一致。
  - 不写日志，不进审计，不经远程管理路由返回。
  - FRB 读取设置时只返回 `has_token`，不返回密钥本身。
- 网关地址要求 https；只有回环地址和私有网段（`127.0.0.0/8`、`10/8`、`172.16/12`、`192.168/16`）允许 http，而且界面上要给出提示。

**订阅登录（D21）**

- 清单里 `claude-acp` 的 release 带条件参数：`allow_subscription_login` 为 false（默认）时，启动命令追加 `--hide-claude-auth`。
- 带这个参数时，适配器不再提供 Claude.ai 订阅登录；如果某一轮要由订阅额度付费，它会按未登录处理（`src/hide-claude-auth.ts`）。这时用户需要使用网关、API key 或 Console 登录。
- 主机桌面的 agent 管理页"高级"区里可以打开 `allow_subscription_login`。这个开关是按清单的 `conditional_args` 通用生成的（`AcpAgentFlagDto`），代码里不写死 agent 名。打开前弹出条款提示（Key `acp-flag-confirm-<setting>`，文案取 `confirm_key`，内容引用 research §3.1 的原文）；改动写进审计日志，之后重启该 agent 生效（`restart()` 会重新解析启动参数）。
- 这样，默认配置下 Pocket-Codex 不提供 Claude.ai 登录入口，D8 的法务问题只和用户主动打开的这个选项有关，M10 的发布不再被法务确认卡住。

### 4.3 安装器（`pocket-codex-host-svc/src/acp/install/`，只在桌面编译）

| 文件 | 职责 |
|---|---|
| `mod.rs` | 对外 API：`catalog()`、`parse_catalog()`、`agents_status(ctx)`、`start_install(..)`、`job(id)`、`uninstall(..)`、`resolve_launch(..)`、`settings(ctx)`、`save_settings(ctx, ..)`。除了 `catalog`、`parse_catalog`、`job`，其余函数都接收 `&InstallContext`（§4.3.8），不直接读 `paths::state_dir()` |
| `catalog.rs` | 解析内嵌的 `catalog/catalog.toml` |
| `locks.rs` | **生成文件**：`pub static LOCKS: &[(&str, &str)]`，内容是 lockfile 名和对应的 `include_str!`，由 §4.3.13 的脚本生成 |
| `catalog/pins.toml` | 维护者手工编辑的输入：id、名称、版本、包名、URL 模板、参数、环境变量、引擎版本范围 |
| `catalog/catalog.toml` | **生成文件**：带完整性哈希的清单 |
| `catalog/locks/<id>-<version>.json` | **生成文件**：npm 类 agent 的 `package-lock.json` |
| `platform.rs` | 平台检测 |
| `fetch.rs` | HTTPS 下载与完整性校验 |
| `archive.rs` | 安全解压 |
| `node.rs` | 私有 Node |
| `npm.rs` | `npm ci`，包括测试用的 `NpmRunner` trait |
| `store.rs` | 托管目录布局和 `installed.json` |
| `jobs.rs` | 安装任务状态机 |
| `resolve.rs` | 解析启动规格 |
| `settings.rs` | 读写 `agents.toml` |
| `registry_hint.rs` | 从 ACP Registry 获取新版本提示 |
| `manage.rs` | `/acp/v1/*` 路由和 `AcpManagement` trait |
| `audit.rs` | 审计日志 |

#### 4.3.1 清单格式

`catalog.toml` 的示例如下。其中的完整性哈希由脚本生成，不允许手工编辑：

```toml
schema = 1
generated_at = "2026-10-01T00:00:00Z"

[node]
version = "24.21.0"
[node.targets.darwin-aarch64]
url = "https://nodejs.org/dist/v24.21.0/node-v24.21.0-darwin-arm64.tar.gz"
integrity = "sha256-…"
root = "node-v24.21.0-darwin-arm64"
# darwin-x86_64、linux-aarch64、linux-x86_64 用 .tar.gz；windows-aarch64、windows-x86_64 用 .zip

[[agents]]
id = "claude-acp"
name = "Claude Code"
description = "Anthropic Claude Code，经 claude-agent-acp 适配器接入"
registry_id = "claude-acp"
approx_size_mb = 320
engine_override = { env = "CLAUDE_CODE_EXECUTABLE", setting = "engine_path" }
[[agents.releases]]
version = "0.84.0"
kind = "npm"
package = "@agentclientprotocol/claude-agent-acp"
entry = "node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js"
lock = "claude-acp-0.84.0"
omit = []
# D21：设置项为 false（默认）时追加这些参数
conditional_args = [{ setting = "allow_subscription_login", default = false, when_false = ["--hide-claude-auth"],
                      label_key = "acpSubscriptionLogin", confirm_key = "acpSubscriptionLoginConfirmBody" }]
platforms = ["darwin-aarch64", "darwin-x86_64", "linux-aarch64", "linux-x86_64", "windows-aarch64", "windows-x86_64"]

[[agents]]
id = "codex-acp"
name = "Codex（ACP）"
description = "OpenAI Codex，经 codex-acp 适配器接入，使用本机已安装的 codex"
registry_id = "codex-acp"
approx_size_mb = 60
external_engine = { env = "CODEX_PATH", binary = "codex", setting = "codex_binary", version_args = ["--version"] }
[[agents.releases]]
version = "2.0.1"
engine_range = ">=0.159.1, <0.160.0"
kind = "npm"
package = "@agentclientprotocol/codex-acp"
entry = "node_modules/@agentclientprotocol/codex-acp/dist/index.js"
lock = "codex-acp-2.0.1"
omit = ["optional"]
platforms = ["darwin-aarch64", "darwin-x86_64", "linux-aarch64", "linux-x86_64", "windows-aarch64", "windows-x86_64"]
[[agents.releases]]
version = "1.12.0"
engine_range = ">=0.154.0, <0.155.0"
kind = "npm"
package = "@agentclientprotocol/codex-acp"
entry = "node_modules/@agentclientprotocol/codex-acp/dist/index.js"
lock = "codex-acp-1.12.0"
omit = ["optional"]
platforms = ["darwin-aarch64", "darwin-x86_64", "linux-aarch64", "linux-x86_64", "windows-aarch64", "windows-x86_64"]

[[agents]]
id = "opencode-acp"
name = "OpenCode 1.x（ACP）"
registry_id = "opencode"
approx_size_mb = 200
[[agents.releases]]
version = "1.18.33"
kind = "archive"
args = ["acp", "--hostname", "127.0.0.1", "--port", "0", "--no-mdns"]
random_env = ["OPENCODE_SERVER_PASSWORD"]
data_family = "opencode-v1"
[agents.releases.targets.darwin-aarch64]
url = "https://github.com/anomalyco/opencode/releases/download/v1.18.33/opencode-darwin-arm64.zip"
integrity = "sha256-…"
cmd = "opencode"
[agents.releases.targets."linux-x86_64-baseline"]
url = "https://github.com/anomalyco/opencode/releases/download/v1.18.33/opencode-linux-x64-baseline.tar.gz"
integrity = "sha256-…"
cmd = "opencode"
# 其余目标：darwin-x86_64(-baseline)、linux-aarch64、linux-x86_64、windows-aarch64、windows-x86_64(-baseline)；windows 的 cmd 是 opencode.exe

[[agents]]
id = "opencode2-acp"
name = "OpenCode 2.x（ACP）"
approx_size_mb = 210
[[agents.releases]]
version = "2.0.20"
kind = "archive"
args = ["acp"]
data_family = "opencode-v2"
[agents.releases.targets.darwin-aarch64]
url = "https://registry.npmjs.org/@opencode/cli-darwin-arm64/-/cli-darwin-arm64-2.0.20.tgz"
integrity = "sha512-…"
cmd = "package/bin/opencode"
# 其余目标同理：@opencode/cli-<os>-<arch>[-baseline]；windows 的 cmd 是 package/bin/opencode.exe
```

```rust
/// `node` is None only in the M5(a) placeholder catalog (`schema = 1`, `agents = []`).
pub struct Catalog { pub schema: u32, pub generated_at: String, pub node: Option<NodePin>, pub agents: Vec<CatalogAgent> }
pub struct NodePin { pub version: String, pub targets: BTreeMap<String, ArchiveTarget> }
pub struct ArchiveTarget { pub url: String, pub integrity: String, pub root: Option<String>, pub cmd: Option<String> }
pub struct CatalogAgent {
    pub id: String, pub name: String, pub description: String,
    pub registry_id: Option<String>, pub approx_size_mb: u32,
    pub engine_override: Option<EngineOverride>, pub external_engine: Option<ExternalEngine>,
    /// Newest first.
    pub releases: Vec<Release>,
}
pub struct Release {
    pub version: String, pub engine_range: Option<String>, pub kind: ReleaseKind,
    pub args: Vec<String>, pub env: BTreeMap<String, String>, pub random_env: Vec<String>,
    pub data_family: Option<String>,
    pub conditional_args: Vec<ConditionalArgs>,
}
/// Boolean setting read from `[agents.<id>]` in agents.toml. `label_key` / `confirm_key` are
/// l10n keys the UI uses to render a switch (and a confirm dialog when turning it on).
pub struct ConditionalArgs {
    pub setting: String, pub default: bool,
    pub when_true: Vec<String>, pub when_false: Vec<String>,
    pub label_key: String, pub confirm_key: Option<String>,
}
pub enum ReleaseKind {
    Npm { package: String, entry: String, lock: String, omit: Vec<String>, platforms: Vec<String> },
    Archive { targets: BTreeMap<String, ArchiveTarget> },
}
pub struct ExternalEngine { pub env: String, pub binary: String, pub setting: String, pub version_args: Vec<String> }
pub struct EngineOverride { pub env: String, pub setting: String }

/// Parsed once from `include_str!`; the unit test `embedded_catalog_parses` guards it.
pub fn catalog() -> &'static Catalog;
/// Parse any catalog text; installer tests use their own small catalogs.
pub fn parse_catalog(text: &str) -> Result<Catalog, AcpError>;
```

- 所有结构体都用 `#[derive(Deserialize)]` 加 `#[serde(default)]`：`description` 缺省为 `""`，`args`、`env`、`random_env`、`omit`、`platforms` 缺省为空，`engine_range`、`data_family` 等 `Option` 缺省为 None。`ReleaseKind` 用 `#[serde(tag = "kind", rename_all = "lowercase")]`。

选择版本的规则：
- 没有 `external_engine` 的 agent，直接用 `releases[0]`。
- 有 `external_engine` 的 agent（codex-acp），先检测引擎版本（§4.3.8），再从新到旧选第一个 `engine_range` 能匹配的 release。
- 都匹配不上时报 `EngineIncompatible`，并在错误里列出清单支持的 Codex 版本范围。

#### 4.3.2 平台检测（`platform.rs`）

```rust
pub struct Platform {
    /// `darwin-aarch64` | `darwin-x86_64` | `linux-aarch64` | `linux-x86_64` | `windows-aarch64` | `windows-x86_64`
    pub key: &'static str,
    pub npm_os: &'static str,   // darwin | linux | win32
    pub npm_cpu: &'static str,  // arm64 | x64
    pub libc: Option<&'static str>, // Some("glibc") on Linux
    pub avx2: bool,
}
pub fn detect() -> Result<Platform, AcpError>;
/// Pure function behind `detect`, for tests on any runner.
pub fn detect_from(os: &str, arch: &str, musl_present: bool, avx2: bool) -> Result<Platform, AcpError>;
```

安装器里需要平台信息的函数都从 `InstallContext.platform` 读取，不自己调用 `detect()`。`InstallContext::production` 调用 `detect()` 填这个字段，测试用 `detect_from` 构造。平台不受支持时，这个字段是 `Err`，但 `production` 仍然成功：
- 清单 agent 的安装和启动，返回这个错误。
- `agents_status` 把清单 agent 的状态标为 `unsupported_platform`。
- 自定义 agent、设置读写不受影响。

- 操作系统和架构取自 `std::env::consts`，不在上面六种之内的一律报 `UnsupportedPlatform`。
- Linux 上存在 `/lib/ld-musl-x86_64.so.1` 或 `/lib/ld-musl-aarch64.so.1` 时，报 `UnsupportedPlatform`，message 为 "musl Linux is not supported yet"（T13）。
- `avx2`：在 `target_arch = "x86_64"` 上用 `std::is_x86_feature_detected!("avx2")` 检测，其他架构一律为 true。
- 查找 archive 目标：x86_64 且没有 AVX2 时，先找 `{key}-baseline`，找不到再找 `{key}`；其他情况只找 `{key}`。

#### 4.3.3 目录布局

托管目录在生产环境是 `paths::state_dir()?/acp/`，所有安装器函数都通过 `InstallContext.root` 访问它（§4.3.8），测试时换成临时目录。目录结构如下：

```
acp/
  agents.toml                      # 用户设置（§5.1），0600
  installed.json                   # 已安装记录（§5.2），0600
  node/v24.21.0/<archive root>/     # 私有 Node
  agents/<id>/<version>/            # npm 前缀目录，或 archive 解压目录
  tmp/                              # 下载和暂存目录；App 启动时清空
  npm-cache/                        # 私有 npm 缓存
  npmrc                             # 空文件，避免继承用户的 ~/.npmrc
  data/<data_family>/opencode.db    # OpenCode 隔离数据库（D19）
  uploads/<instance>/               # 附件上传
  sessions/<instance>.json          # 不支持 list 的 agent 的会话索引
  run/                              # terminal 登录用的包装脚本和状态文件
  registry-cache.json               # 注册表提示缓存
  audit.log                         # 审计日志，0600
```

Unix 上，`acp/` 目录本身的权限是 0700。

#### 4.3.4 下载与校验（`fetch.rs`）

```rust
pub struct FetchPolicy { pub allowed_hosts: Vec<String>, pub max_bytes: u64, pub allow_loopback_http: bool }
pub async fn download(url: &str, integrity: &str, dest: &Path, policy: &FetchPolicy, progress: &dyn Fn(u64, Option<u64>)) -> Result<(), AcpError>;
```

- 客户端用 host-svc 已有的 `reqwest`（带 rustls）。
- 重定向：自定义 `redirect::Policy`，最多 5 跳，每一跳都必须是 https，而且主机名在白名单里。
- 白名单：`nodejs.org`、`github.com`、`objects.githubusercontent.com`、`release-assets.githubusercontent.com`、`registry.npmjs.org`、`cdn.agentclientprotocol.com`。
- 超时：连接 15 s；连续 60 s 读不到数据就中断。
- 大小上限 1 GiB。有 `content-length` 时先检查它，同时按实际读到的字节数再检查一次。
- 边下载边按 SRI 前缀（`sha256` 或 `sha512`）算哈希，结果用 base64 比较。不一致时删除文件，报 `IntegrityMismatch`。
- `allow_loopback_http` 只在测试中置为 true，允许 `http://127.0.0.1:<port>`。生产代码里构造的值恒为 false。

#### 4.3.5 解压（`archive.rs`）

```rust
pub fn extract(archive: &Path, dest: &Path) -> Result<(), AcpError>;   // by suffix: .zip | .tar.gz | .tgz
```

以下条目一律拒绝，并报 `ArchiveRejected`：
- 绝对路径、包含 `..` 的路径、带 Windows 盘符的路径。
- 解析后指向 `dest` 之外的符号链接或硬链接。
- 解压后总大小超过 3 GiB，或条目数超过 200 000。

Unix 上保留权限位，但清除 setuid 和 setgid。`extract` 只负责解压到调用方给的 `dest`。安装流程（§4.3.9）先解压到暂存目录 `agents/<id>/<version>.staging-<rand>/`，自检通过后才 rename 到最终目录；Node 运行时同样先解压到 `node/<version>.staging-<rand>/`，校验 `node --version` 之后再 rename。

#### 4.3.6 私有 Node（`node.rs`）

```rust
pub struct NodeRuntime { pub node: PathBuf, pub npm_cli: PathBuf }
/// Uses `ctx.catalog.node` (version + targets), `ctx.platform`, `ctx.root` and `ctx.fetch`.
pub async fn ensure_node(ctx: &InstallContext, progress: &dyn Fn(JobProgress)) -> Result<NodeRuntime, AcpError>;
```

| 平台 | node 可执行文件 | npm 入口 |
|---|---|---|
| Unix | `<root>/bin/node` | `<root>/lib/node_modules/npm/bin/npm-cli.js` |
| Windows | `<root>/node.exe` | `<root>/node_modules/npm/bin/npm-cli.js` |

- 只装一次，路径记在 `installed.json` 的 `node` 字段里。
- 装完后执行 `node --version` 校验，输出必须等于 `v{ctx.catalog.node.version}`（生产清单里是 `v24.21.0`）。
- 测试用的小清单里，`[node]` 指向本地 HTTP 服务上的一个假 Node 压缩包，其中的 `bin/node` 是一个会打印对应版本号的脚本。`npm` 的执行已经由 `NpmRunner` 替换，所以测试里不会真的运行 Node。

#### 4.3.7 npm 安装（`npm.rs`）

- 暂存目录为 `agents/<id>/<version>.staging-<rand>/`，里面放两个文件：
  - `package.json`，内容为 `{"name":"pcx-<id>","private":true,"dependencies":{"<package>":"<version>"}}`。
  - 清单中对应的 lockfile，复制为 `package-lock.json`。
- 命令：

  ```
  <node> <npm-cli.js> ci --prefix <dir> --ignore-scripts --no-audit --no-fund --no-update-notifier
    --os=<npm_os> --cpu=<npm_cpu> [--libc=glibc] [--omit=<x>]...
    --registry=<registry> --cache=<acp>/npm-cache --userconfig=<acp>/npmrc
  ```

  其中 `--omit` 按 release 的 `omit` 字段逐项添加。
- 环境变量：把 node 所在目录加到 `PATH` 最前面，并设 `npm_config_update_notifier=false`。
- 超时 20 分钟。stdout 和 stderr 各保留最后 64 KiB，失败时报 `NpmFailed { log_tail }`。
- `registry` 默认为 `https://registry.npmjs.org/`，可以在 `agents.toml` 里换成镜像，但必须是 https。无论用哪个源，npm 都会按 lockfile 里的 `integrity` 校验每个包。
- 测试通过 `trait NpmRunner { async fn ci(&self, args: &NpmArgs) -> Result<(), AcpError>; }` 替换实际执行。

#### 4.3.8 解析启动规格（`resolve.rs`）

```rust
/// Everything the installer touches, injected so tests never use the real state
/// directory, network, npm or agents. Production: `InstallContext::production(..)`.
#[derive(Clone)]
pub struct InstallContext {
    /// Managed directory; production = `paths::state_dir()?/acp`.
    pub root: PathBuf,
    /// Production = `catalog()` (embedded); tests pass `parse_catalog(..)` output.
    pub catalog: Arc<Catalog>,
    /// Lock name → package-lock.json text; production = `locks::LOCKS`.
    pub locks: Arc<BTreeMap<String, String>>,
    pub npm: Arc<dyn NpmRunner>,
    pub fetch: FetchPolicy,
    /// Used by install validation; production = `ProcessConnector`.
    pub connector: Arc<dyn AgentConnector>,
    /// Production = `platform::detect()`. An `Err` (e.g. musl Linux) does not make the
    /// context fail: catalog install/resolve paths return this error, while custom
    /// agents, settings and `agents_status` keep working (T13).
    pub platform: Result<Platform, AcpError>,
    /// From the bridge's `serve::codex_locate()` (serve.rs:232: saved config → PATH).
    pub codex_binary: Option<PathBuf>,
    /// (agent_id, version) → whether a hosted instance currently uses it. Provided by the bridge.
    pub in_use: Arc<dyn Fn(&str, &str) -> bool + Send + Sync>,
}
impl InstallContext {
    pub fn production(codex_binary: Option<PathBuf>, in_use: Arc<dyn Fn(&str, &str) -> bool + Send + Sync>) -> Result<Self, AcpError>;
}
/// Launch an installed catalog agent or a custom agent.
pub fn resolve_launch(agent_id: &str, ctx: &InstallContext) -> Result<LaunchSpec, AcpError>;
/// Launch a specific release from a specific directory; used by `resolve_launch`
/// and by install validation (on the staging directory, before it is recorded).
pub fn resolve_release(agent: &CatalogAgent, release: &Release, dir: &Path, ctx: &InstallContext) -> Result<LaunchSpec, AcpError>;
pub fn engine_version(binary: &Path, args: &[String]) -> Result<semver::Version, AcpError>;
```

- **自定义 agent**：`command` 必须是绝对路径且文件存在；args 和 env 取自设置。
- **清单 agent**：`installed.json` 里必须有这个 agent 的记录，否则报 `NotInstalled`；有记录时，用记录里的目录调用 `resolve_release`。archive 类的 agent 如果在设置里配了 `binary` 覆盖，就改用那个路径。
- **npm 类**：program 为私有 node，args 为 `[<入口的绝对路径>] + release.args`。
- **archive 类**：program 为 `<dir>/<cmd>`，args 为 `release.args`。
- **外部引擎（codex-acp）**：
  1. 引擎路径的优先级：`agents.toml` 里的 `codex_binary` 最高，其次是 `ctx.codex_binary`。都没有时报 `EngineMissing`。只有带 `external_engine` 的 agent 才会走这一步。
  2. 执行 `<codex> --version`，超时 5 s，取输出里第一个形如 `\d+\.\d+\.\d+` 的版本号。
  3. 检查已安装的 release 的 `engine_range`。不匹配时报 `EngineIncompatible { found, required, suggested }`，`suggested` 是清单里能匹配当前引擎版本的 release，界面据此提示"安装 codex-acp X 以匹配你的 Codex"。
  4. 设置 `CODEX_PATH=<codex 的绝对路径>`。
- **引擎覆盖（Claude）**：`agents.toml` 的 `[agents.claude-acp] engine_path` 有值时，设置 `CLAUDE_CODE_EXECUTABLE`。清单字段 `setting` 指明从 `[agents.<id>]` 下的哪个键读取：Claude 是 `engine_path`，Codex 是 `codex_binary`，archive 类是 `binary`。
- **`random_env`**：每次启动为其中每个变量生成一个新的 32 字节十六进制随机值（用两个 uuid v4 拼成）。
- **`conditional_args`**：读取 `[agents.<id>]` 下的布尔设置项，缺省时用 `default`，按取值把 `when_true` 或 `when_false` 放进 `LaunchSpec.launch_only_args`（D21）。不放进 `args`，原因见 §4.2.9。
- **`data_family`**：按 D19 的结论（§11）决定是否设置 `OPENCODE_DB=<acp>/data/<data_family>/opencode.db`。
- `pinned`：清单 agent 的已安装版本等于清单版本时为 true；自定义 agent 或 registry 来源的版本为 false。

#### 4.3.9 安装任务（`jobs.rs`）

```
queued → downloading → verifying → extracting → installing → validating → done
                               任何一步失败 ↘ failed
```

进度用 `JobProgress` 表示，定义见 §4.1.5。

```rust
pub async fn start_install(agent_id: &str, version: Option<&str>, remote: bool, ctx: &InstallContext) -> Result<String, AcpError>;
pub fn job(id: &str) -> Option<JobProgress>;
pub fn uninstall(agent_id: &str, ctx: &InstallContext) -> Result<(), AcpError>;
```

- 同一个 agent 同时只能有一个任务，全局最多 2 个任务并发。
- 开始前检查磁盘：Unix 上用 `nix::sys::statvfs` 查询可用空间，要求至少是 `approx_size_mb` 的 2 倍。Windows 不预检查，直接依赖写入失败时报错。
- 需要 Node 的 agent，会先执行 `ensure_node`；清单没有 `[node]` 时报 `UnsupportedPlatform`。
- **validating 阶段**：
  1. 用 `resolve_release(agent, release, <暂存目录>, ctx)` 得到启动规格，再把 `cwd` 设为 `tmp/` 下一个新建的空目录。带 `data_family` 的 agent，无论 D19 怎么决定，都另设 `OPENCODE_DB=<这个空目录>/validate.db`，保证自检不会触碰用户的 OpenCode 数据库。然后启动 agent。
  2. 发送 §3.2 的 `initialize`，超时 60 s。
  3. 要求返回的 `protocolVersion` 为 1，并记录 `agentInfo.version`。
  4. 调用 `terminate` 结束进程。
  5. 不发送 `session/new`，也不发 prompt。
- **成功**：
  1. 把暂存目录 rename 到最终目录。
  2. 以"写临时文件再 rename"的方式更新 `installed.json`。
  3. `ctx.in_use(agent_id, 旧版本)` 为 false 时，立即删除旧版本目录；否则记入 `installed.json` 的 `gcPending`，等 Hub 停止或 App 下次启动时再删。
- **失败**：删除暂存目录，旧版本保持不动。
- 任务结束后，状态在内存里保留 1 小时。
- 清理工作由两个函数负责：
  - `pub fn startup_cleanup(ctx: &InstallContext)`：清空 `tmp/`，删除残留的 `*.staging-*` 目录，然后执行一次 `gc_sweep`。由 `acp_manage::register()` 在 App 启动时调用。
  - `pub fn gc_sweep(ctx: &InstallContext)`：删除 `gcPending` 里 `ctx.in_use` 为 false 的目录，并更新 `installed.json`。`serve_acp::stop` 结束 Hub 之后调用一次。
- `remote = true` 时的额外检查：版本必须是清单里锁定的版本，并且设置里的 `remote_management` 为 true；否则报 `RemoteManagementDisabled` 或 `VersionNotPinned`。
- 卸载：`uninstall(agent_id, ctx)` 在 `ctx.in_use` 为 true 时拒绝，报 `InUse`；否则删除目录并更新 `installed.json`。
- D16 的共存提示：把某个 OpenCode 数据族切到"共享"，而另一个数据族的 agent 已经安装时，设置保存前返回警告（`AcpSettingsDto` 保存接口返回 `warnings: Vec<String>`），由界面弹确认框。

#### 4.3.10 注册表提示（`registry_hint.rs`）

- 请求 `https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json`，最多每 6 小时一次，超时 30 s，响应体上限 4 MiB。只解析 `agents[].{id, version, distribution}`，结果缓存到 `acp/registry-cache.json`。
- 对设置了 `registry_id` 的清单 agent，用 semver 比较注册表版本和清单版本。注册表版本更新时，填入 `AgentStatus.registry_version`。
- 安装注册表版本是高级操作，只能在主机桌面上做，调用 `start_install(agent_id, Some(version), remote = false)`，其中 version 不等于清单版本：
  - 注册表条目给当前平台提供了带 `sha256` 的 `binary` 目标：按 archive 方式安装，完整性用那个 sha256。
  - 只提供 `npx`：执行 `npm install --save-exact --ignore-scripts <pkg>@<ver>`。这种情况下没有 lockfile，界面必须先弹出确认框，提示"传递依赖未锁定，未经 Pocket-Codex 验证"。
  - 其他情况报 `VersionNotPinned`。
  - 这类安装在 `installed.json` 里记为 `source = "registry"`。

#### 4.3.11 远程管理（`manage.rs`）

`AgentStatus` 的定义见 §4.1.5。

```rust
#[async_trait]
pub trait AcpManagement: Send + Sync {
    async fn agents(&self) -> Vec<AgentStatus>;
    async fn install(&self, agent_id: &str, remote: bool) -> Result<String, AcpError>;
    fn job(&self, id: &str) -> Option<JobProgress>;
    /// Validates synchronously (policy, installed, name clash) and returns a `host` job id.
    async fn host(&self, agent_id: &str, name: Option<String>, remote: bool) -> Result<String, AcpError>;
    fn remote_allowed(&self) -> bool;
}
/// Registered once by the desktop bridge (§4.4.1); without it every route returns 404.
pub fn set_management(management: Arc<dyn AcpManagement>);
pub fn router() -> Router;
```

| 路由 | 行为 | 错误 |
|---|---|---|
| `GET /acp/v1/agents` | `{remoteManagement, agents: [AgentStatus]}` | 404：没有注册实现 |
| `POST /acp/v1/agents/{id}/install` | 202 `{jobId}`；只安装清单锁定的版本 | 403 `acp.remote_management_disabled`；404 `acp.unknown_agent`（未知 id 或自定义 agent）；409 `acp.job_running`；422 `acp.unsupported_platform` / `acp.engine_missing` / `acp.engine_incompatible` |
| `GET /acp/v1/jobs/{id}` | `JobProgress` | 404 `acp.unknown_job` |
| `POST /acp/v1/agents/{id}/host`，body `{name?}` | 202 `{jobId}`，任务 kind 为 `host`，完成后 `serviceKey` 有值。启动 agent、检测登录、注册 relay 加起来可能超过 meta 客户端的 30 s 超时，所以做成任务 | 同步检查的错误：403 `acp.remote_management_disabled`；409 `acp.name_conflict`；424 `acp.not_installed`。启动过程中的错误记在任务的 `error_code` 里 |

- `{id}` 必须匹配 `^[a-z][a-z0-9-]{0,63}$`，请求体不超过 4 KiB。
- 这些 HTTP 路由都经 relay 进来，一律按 `remote = true` 处理。本机 FRB 直接调用安装器，按 `remote = false` 处理。
- 错误响应体为 `{"code":"acp.<…>","message":"…"}`。
- 所有 POST 都写审计日志。

#### 4.3.12 审计（`audit.rs`）

- 格式为 JSONL，文件 `acp/audit.log`，权限 0600。超过 1 MiB 时轮转，保留 `.1` 到 `.3`。
- 每条记录的字段为 `{ts, action, agentId, version?, remote, result, code?}`，其中 `action` 是 `install`、`uninstall`、`host`、`settings`、`custom_agent` 之一，`result` 是 `ok` 或 `error`。
- 不记录环境变量的值，也不记录 agent id 以外的路径。

#### 4.3.13 清单生成脚本（`scripts/acp_catalog.py`，只用 Python 标准库）

`python3 scripts/acp_catalog.py update`：由维护者在本机执行，需要网络，并且 PATH 上的 npm 版本不低于 11.11。
1. 用 `tomllib` 读取 `pins.toml`。
2. **Node**：下载锁定版本的 `SHASUMS256.txt`，记录 6 个包的 sha256。PATH 上有 `gpgv` 时再校验 `SHASUMS256.txt.sig`，没有就打印警告。
3. **archive 类 agent**：
   - GitHub 上的包：下载后算 sha256。如果 ACP Registry 也给了 sha256，两者必须一致，不一致就中止。
   - npm 平台包：从 npm registry 的元数据里读 `dist.integrity`。
4. **npm 类 agent**：在临时目录里写入和安装器**完全相同**的 `package.json`（§4.3.7：`{"name":"pcx-<id>","private":true,"dependencies":{"<package>":"<version>"}}`），然后执行 `npm install --package-lock-only --ignore-scripts --save-exact`，把生成的 `package-lock.json` 复制到 `catalog/locks/<id>-<ver>.json`。`npm ci` 会检查 package.json 与 lockfile 根节点是否一致，所以两边必须写同一份 package.json。
5. 写出 `catalog.toml` 和 `locks.rs`。

`python3 scripts/acp_catalog.py check`：离线运行，给 CI 用，检查以下几点：
- `catalog.toml` 能正常解析。
- 每个 release 都为自己的全部平台提供了完整性哈希。
- 每个 lock 文件都存在，并且 `lockfileVersion` 为 3。
- 每个包条目都有 `integrity`，`resolved` 都以 `https://registry.npmjs.org/` 开头。
- `locks.rs` 与 `catalog/locks/` 下的文件一一对应。

CI：在 `.github/workflows/ci.yml` 的 Rust job 里增加一步 `python3 scripts/acp_catalog.py check`。

### 4.4 bridge（`pocket_codex_bridge`）

#### 4.4.1 托管（`engine/serve_acp.rs`，新增）

```rust
pub const ACP_STOP_GRACE: Duration = Duration::from_secs(5);

struct AcpServe {
    device: String, name: String, key: String, agent_id: String,
    /// `LaunchSpec.version` (the installed release); used by `InstallContext.in_use`.
    agent_version: Option<String>,
    /// The `register` closure from `StartDeps`, reused by `reregister`.
    register_fn: Arc<dyn Fn(ServiceKind, &str, SocketAddr) -> Result<Option<Published>> + Send + Sync>,
    hub: Arc<AcpHub>,
    ws_local: SocketAddr, ws_task: JoinHandle<()>, register: Option<Published>,
    meta_key: String, meta_local: SocketAddr, meta_task: JoinHandle<()>, meta_register: Option<Published>,
}
pub struct AcpServeReport {
    pub device: String, pub name: String, pub service_key: String, pub listen_addr: String,
    pub meta_service_key: String, pub agent_id: String, pub agent_name: String,
    pub agent_version: String, pub auth: AuthState, pub reused: bool,
}

fn hosts() -> std::sync::MutexGuard<'static, HashMap<String, AcpServe>>;   // same pattern as serve_opencode.rs:90
pub fn is_hosting(name: &str) -> bool;
pub fn start(name: Option<String>, agent_id: String) -> Result<AcpServeReport>;
pub(super) fn status() -> Vec<ServeStatus>;
pub(super) fn local_endpoints(service_key: &str) -> Option<(String, String)>;
fn slot(host: &mut AcpServe, kind: ServiceKind) -> Result<&mut Option<Published>>;   // Acp | Meta
pub(super) fn deregister(name: &str, kind: &str) -> Result<()>;
pub(super) fn reregister(name: &str, kind: &str) -> Result<()>;
pub(super) fn stop(name: &str);
pub(super) fn stop_all();
pub fn hub(name: &str) -> Option<Arc<AcpHub>>;

/// Injected dependencies (T19). `start` = `start_with(.., StartDeps::production())`.
pub struct StartDeps {
    pub device: String,
    /// Production: require a signed-in account (same check as serve_opencode.rs:127).
    pub require_account: bool,
    pub connector: Arc<dyn AgentConnector>,
    /// Production: resolve the transport lazily (`transport::resolve_blocking()`) inside the
    /// closure, then `serve::register_service(&transport, &device, kind, name, local)`.
    /// Also used by the reuse path and by `reregister`, so tests never touch the relay.
    pub register: Arc<dyn Fn(ServiceKind, &str, SocketAddr) -> Result<Option<Published>> + Send + Sync>,
    /// Production: `install::resolve_launch`; tests return a fixed `LaunchSpec`.
    pub resolve: Box<dyn Fn(&str) -> Result<LaunchSpec> + Send + Sync>,
    /// Name → provider already hosting it (`"Codex"` / `"OpenCode"`). Production checks
    /// `serve::is_hosting_codex` and `serve_opencode::is_hosting`.
    pub other_hosting: Box<dyn Fn(&str) -> Option<&'static str> + Send + Sync>,
    /// Production: `serve::config_store()` / `serve::host_store()`; tests pass stores opened in a temp dir.
    pub stores: (Arc<ConfigStore>, Arc<HostStore>),
    /// Production: `paths::state_dir()?`; tests pass a temp dir (logs, acp/uploads, acp/run, acp/sessions).
    pub state_dir: PathBuf,
}
impl StartDeps {
    /// Fails when the stores or the state directory cannot be opened.
    pub fn production() -> Result<Self>;
}
pub fn start_with(name: Option<String>, agent_id: String, deps: StartDeps) -> Result<AcpServeReport>;
```

- 测试用的 `StartDeps`：`require_account = false`，`connector` 用 `FakeAgent` 的 `DuplexConnector`，`register` 返回 `Ok(None)`，`resolve` 返回一个固定的 `LaunchSpec`。这样 §8.2 的托管用例不需要账号和 relay。
- 移动端桩模块（§1）的函数一律声明为 `pub(crate)`：桩里的 `pub(super)` 只对 `acp_desktop_stub` 可见，`serve.rs` 调不到。
- 移动端桩模块的 `serve_acp`：`is_hosting` 返回 false，`status` 返回空列表，`local_endpoints` 返回 None，`start`、`deregister`、`reregister` 返回 §1 的错误，`stop` 和 `stop_all` 返回 `()`、什么也不做（签名与桌面版一致）。桩模块里没有 `hub()` 和 `start_with`，这两个只在桌面代码里用。
- `AcpServeReport` 定义在所有平台都编译的 `engine/acp/mod.rs` 里，`serve_acp` re-export 它，所以桌面和桩的签名能保持一致。

`acp_manage` 是 FRB 函数和桌面实现之间的门面，参数和返回值只用所有平台都能编译的类型（core 的 `pcx` 类型，以及下面两个也放在 `core::acp::pcx` 里的类型）。§4.4.5 的托管、管理、登录类 FRB 函数全部只调用这里的函数，不直接碰 host-svc：

```rust
// core::acp::pcx
pub struct AcpSettingsView { pub remote_management: bool, pub npm_registry: Option<String>, pub claude_engine_path: Option<String>, pub codex_binary: Option<String>, pub binary_overrides: Vec<(String, String)>, pub opencode_data: Vec<(String, String)>, pub gateways: Vec<GatewayView>, pub flags: Vec<AgentFlagView> }
/// (agent_id, setting, label_key, confirm_key, value); generated from every catalog agent's `conditional_args`.
pub struct AgentFlagView { pub agent_id: String, pub setting: String, pub label_key: String, pub confirm_key: Option<String>, pub value: bool }
/// Same semantics as `AcpGatewayDto` (token is write-only).
pub struct GatewayView { pub agent_id: String, pub method_id: Option<String>, pub base_url: String, pub token: Option<String>, pub has_token: bool, pub provider_name: Option<String>, pub extra_headers: Vec<(String, String)>, pub clear: bool }
pub struct CustomAgentDef { pub id: String, pub name: String, pub command: String, pub args: Vec<String>, pub env: Vec<(String, String)> }

// engine::acp_manage (desktop) and engine::acp_desktop_stub::acp_manage (mobile), same signatures
pub fn register();
pub fn install_context() -> Result<InstallContextHandle>;   // desktop only; not in the stub
pub fn agents() -> Result<Vec<AgentStatus>>;
pub fn install(agent_id: &str, version: Option<&str>) -> Result<String>;
pub fn job(id: &str) -> Option<JobProgress>;
pub fn uninstall(agent_id: &str) -> Result<()>;
pub fn settings() -> Result<AcpSettingsView>;
pub fn save_settings(view: AcpSettingsView, force: bool) -> Result<(bool, Vec<String>)>;
pub fn custom_agents() -> Result<Vec<CustomAgentDef>>;
pub fn put_custom_agent(def: CustomAgentDef) -> Result<()>;
pub fn delete_custom_agent(id: &str) -> Result<()>;
pub fn auth_terminal(name: &str, method_id: &str) -> Result<()>;
pub fn auth_agent(name: &str, method_id: &str) -> Result<AuthState>;
pub fn auth_recheck(name: &str) -> Result<AuthState>;
```

- 桩的实现：`register` 什么也不做，`job` 返回 None，其余函数都返回 §1 的错误。
- `InstallContextHandle` 就是 host-svc 的 `InstallContext`，只在桌面代码里出现。
- FRB 的 DTO（§4.4.5）和这两个类型之间的转换写在 `api/bridge.rs` 里。FRB 不直接暴露元组，所以 `binary_overrides` 和 `opencode_data` 在 DTO 里是结构体列表。
- 桩的组织方式：`engine/acp_desktop_stub.rs` 里写 `pub mod serve_acp { … }` 和 `pub mod acp_manage { … }` 两个子模块；`engine/mod.rs` 在移动端用 `pub use acp_desktop_stub::{serve_acp, acp_manage};`。`acp_terminal` 只被 `serve_acp` 使用，移动端不需要桩。

`start` 的步骤，仿照 `serve_opencode::start`（`serve_opencode.rs:124-263`）：
1. 要求已登录账号，和 `:127` 相同。
2. 实例名默认取 `agent_id`，并做 sanitize。
3. 名字冲突检查：`serve::is_hosting_codex(&name)` 或 `serve_opencode::is_hosting(&name)` 为真时，报错 "`{name}` is already hosting {Codex|OpenCode} on this device; choose another name"。本注册表里已有同名实例时：agent 相同就复用（`reused = true`，并按需 reregister）；agent 不同就报同样的错误。
4. 先调用一次 `StartDeps.resolve`（生产环境即 `install::resolve_launch(agent_id, &ctx)`），确认能得到启动规格，失败就直接报错。之后每次启动和重启，Hub 都会通过第 5 步的 `launch` 闭包重新解析。`ctx` 由 `acp_manage::install_context()` 构造，即 `InstallContext::production(codex_binary, in_use)`：
   - `codex_binary` 取 `serve::codex_locate()`（`serve.rs:232`，先查保存的配置再查 PATH，返回 `Option<String>`）。
   - `in_use` 按 `hosts()` 里各实例的 `agent_id` 和 `agent_version`（即 `LaunchSpec.version`，也就是清单里的 release 版本，不是 agent 自报的 `agentInfo.version`）判断。
5. 调用 `AcpHub::start(HubOptions { instance, launch, state_dir, log_file: logs/acp-<name>.log, connector, terminal: Some(DesktopTerminal) })`。
   - `launch` 闭包每次调用时都会：用 `StartDeps.resolve` 重新解析启动规格，读 `agents.toml` 里这个 agent 的网关配置，生成 `GatewayAuth`。
   - uploads 目录只交给 meta 服务（第 8 步），Hub 不需要它。失败时，错误里带上 `stderr_tail` 的最后 2 KiB。
6. 调用两次 `bind_loopback`：一个给 ws，一个给 meta。现在 `bind_loopback` 是 `serve_opencode.rs:346` 里的私有函数，把它原样移到 `serve.rs`，改成 `pub(super)`，供两个模块共用。
7. ws 任务：`rt.spawn(serve::supervise("the ACP hub", ws_local, ws_std, move |l| pocket_codex_host_svc::acp::serve_ws(l, hub.clone())))`。
8. meta 任务：`rt.spawn(serve::supervise("the ACP meta service", meta_local, meta_std, move |l| pocket_codex_host_svc::acp::serve_meta(l, store, host_store, uploads, hub.clone())))`。
9. 先 `transport::resolve_blocking()`，再注册 `ServiceKind::Meta`（best-effort），最后注册 `ServiceKind::Acp`，这一步决定成败。遇到冲突时，中止两个任务，调用 `hub.shutdown(ACP_STOP_GRACE)` 后报错。
10. 插入 `hosts()`。

其他函数：
- `stop(name)`：先调 `hub.shutdown(ACP_STOP_GRACE)`，再中止任务并注销两个注册。会不会中断运行中的轮次，由界面事先二次确认（§4.7）。
- `status()` 生成的 `ServeStatus`：
  - `provider = "acp"`，`pid` 为 agent 进程号，`alive = (process == Ready)`。
  - `app_*` 字段填 ws 的地址和键；`api_*` 为空；`meta_*` 照常填。
  - `codex_binary = Some(program)`，`provider_version` 为 agent 版本，`provider_verified = pinned`。
  - 新增字段 `agent_id`、`agent_name`，见 §4.4.4。
- `serve.rs` 要改的地方：
  - 名字冲突检查（`:691`）加上 `serve_acp::is_hosting`。
  - `serve_status`（`:942`）追加 `serve_acp::status()`。
  - `local_endpoints`（`:959`）在 OpenCode 之后接着查 `serve_acp::local_endpoints`。
  - `serve_deregister`、`serve_reregister`、`serve_stop`、`serve_stop_all`（`:1012-1094`）都加上 `serve_acp::is_hosting(name)` 分支；`stop_all` 也要调用 `serve_acp::stop_all()`。
- `serve_opencode.rs:135` 的名字冲突检查同样加上 `serve_acp::is_hosting`。

新增 `engine/acp_terminal.rs`，实现 `TerminalLauncher` 的桌面版本 `DesktopTerminal`：
- 在 `<state_dir>/acp/run/` 下写一个权限 0700 的包装脚本：macOS 用 `.command`，Linux 用 `.sh`，Windows 用 `.cmd`。脚本执行登录命令，然后把退出码写进状态文件。
- 参数写进脚本时逐个转义：POSIX 用单引号，内部的单引号写成 `'\''`；Windows 用双引号包住，并转义 `"`、`%`、`^`。环境变量写成 `export K='v'` 或 `set "K=v"`。
- 打开终端的方式：
  - macOS：`open -a Terminal <script>`
  - Windows：`cmd.exe /c start "Pocket-Codex login" cmd.exe /k <script>`
  - Linux：依次尝试 `x-terminal-emulator -e`、`gnome-terminal --`、`konsole -e`、`xfce4-terminal -x`、`xterm -e`。都不可用时报 `NoTerminal`，错误信息里附上可以直接复制的命令行。

新增 `engine/acp_manage.rs`，提供 `AcpManagement` 的实现：
- `install`：调用 `install::start_install`。
- `host`：在 `spawn_blocking` 里调用 `serve_acp::start`。
- `agents`：合并清单、已安装记录和 `hosts()`。
- 注册时机：在 `init_bridge`（`crates/pocket-codex-bridge/src/api/bridge.rs:62`）里，`runtime` 初始化（`engine/runtime.rs:32`）之后，桌面平台上调用一次 `engine::acp_manage::register()`，它内部执行 `set_management`。移动端的桩模块里 `register()` 什么也不做。
- `install_context()`（供 `serve_acp::start` 和 `AcpManagement` 共用）也写在 `acp_manage.rs` 里，门面函数的完整列表见本节前面的代码块。

#### 4.4.2 控制器引擎（`engine/acp/`，新增，所有平台都编译）

| 文件 | 职责 |
|---|---|
| `mod.rs` | `is_acp(key)`（写法同 `opencode/mod.rs:46`，比较 `ServiceKind::Acp.as_key_segment()`）、连接注册表 `conns()`、`connect`、`disconnect`、`is_connected`、`probe_reason`、`subscribe_events`、事件循环、重连 |
| `state.rs` | 每个连接共享的状态：`HubMeta`（§4.1.5）、每个会话的 `SessionView`（按 id 索引的条目表、generation、轮次、运行状态、配置项、模式、命令、用量、`pre_plan_mode`、翻页游标）、待办表（hub requestId → AppClient token、种类和选项）、未确认的提交（clientSubmissionId → 提交内容） |
| `mapping.rs` | 纯函数：HubItem → ThreadItem；TurnInfo → TurnSummary；SessionInfo → ThreadMeta；配置项 → ModelInfo 和 ThreadRuntimeConfig；权限和 elicitation → AppEvent；答案 → ACP 回应 |
| `events.rs` | 把收到的通知和请求翻译成 `Vec<AppEvent>` |
| `ops.rs` | `thread_list`、`thread_start`、`thread_resume`、`model_list`、`running_sessions`、`config_options`、`set_config_option`、`slash_commands` |
| `history.rs` | `thread_read`、`thread_older_page`、`thread_turn_items`、`thread_turn_page`、`thread_reload`，以及历史预取 |
| `turns.rs` | `turn_start`、`turn_interrupt`、`respond_approval`、`respond_permission_option`、`respond_user_input`、`respond_elicitation_url`、`auth_state`、`auth_authenticate` |
| `engine_tests.rs` | `#[cfg(test)]`：在进程内启动 Hub（用 `DuplexConnector` 接 fake agent）并连接真实的 `serve_ws`，做端到端测试 |
| `live_tests.rs` | `#[cfg(test)]`：设置 `PCX_ACP_LIVE=<清单 id>` 时才运行的只读实测 |

连接：
1. 先用 `serve::local_endpoints(key)` 查本机托管的实例，找到就连 `ws://{ws}/acp`；否则用 `runtime::subscribe_service(key, local_port, transport)` 建隧道，连 `ws://{local_addr}/acp`。
2. `AppClient::connect(url)` 之后，用 `tokio::time::timeout(10 s, client.request("initialize", …))` 发送 initialize。`AppClient` 自身的 60 s 超时是上限（T17）。参数：

   ```json
   {"protocolVersion":1,
    "clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false,
                          "elicitation":{"form":{},"url":{}},"session":{"configOptions":{"boolean":{}}}},
    "clientInfo":{"name":"pocket-codex-controller","version":"<CARGO_PKG_VERSION>"}}
   ```

3. 解析返回值里的 `_meta.pcx` 存入 `HubMeta`，启动事件循环，然后发出 AppEvent `acp/capabilities`。界面在 `appConnect` 返回之后才订阅事件（`app_session_screen.dart:3533-3534`），所以 `subscribe_events` 在每个新订阅者开始时，都先补发一次 `acp/capabilities` 和当前的 `acp/hub/state`。

测试入口（T19）：`pub(crate) fn connect_url(service_key: &str, ws_url: &str, meta_url: Option<Url>) -> Result<()>` 跳过 `local_endpoints` 和 relay，直接连接给定的地址。`meta_url` 有值时，`thread_start` 查默认项目用它，否则用 `meta::endpoint`。`engine_tests.rs` 在回环地址上启动进程内的 Hub（FakeAgent）、`serve_ws` 和 `serve_meta`，然后调用 `connect_url`。

事件循环：
- 收到通知：交给 `events::translate`，结果广播出去（`broadcast::channel(1024)`）。
- 收到请求（`session/request_permission` 或 `elicitation/create`）：登记待办，AppEvent 的 `request_id` 用 hub 的 `requestId`，同时保存 AppClient 的 token 以便回应；然后发出审批或输入事件。
- 收到 `$/cancel_request`：删除对应的待办，发出 `serverRequest/resolved`。

重连：
1. `AppClient::is_alive()` 为 false 时，按 1 s 到 30 s 的指数退避重连，然后重新 `initialize`。
2. 对重连前 attach 过的每个会话，重新 attach，tail 为 20。
3. 如果 generation 变了，发出 `acp/session/generation`，界面据此重新读取。
4. 会话之前在运行、现在不在运行了：用 `turns` 里最后一轮的 `stop_reason` 合成一个 `turn/completed`。
5. 所有没有得到确认的提交，用原来的 `clientSubmissionId` 重新 submit（Hub 会去重），把结果补发出去。

`SessionView` 的维护（T1，bridge 不自己折叠）：
- `SessionView.items` 是 `IndexMap` 风格的有序表：`Vec<HubItem>` 加上 `HashMap<String, usize>`，按 id 索引，最多保留 2000 条。超出时丢掉最早的条目，被丢掉的可以再通过 window 读回来。
- 发出 attach 或 reload 请求之前，先把这个会话标记为"同步中"。在此期间，事件循环收到的该会话通知不立即应用，而是暂存起来（最多 4096 条，超出就放弃暂存，等响应到了再重新 attach 一次）。
- attach 或 reload 成功时：
  1. 用 `AttachResult.items` 整体替换，并记下 generation、turns、配置项、模式、命令、用量和 `seq`。
  2. 暂存的通知里，`seq` 小于等于快照 `seq` 的丢弃，其余按 `seq` 顺序应用。
  3. 解除"同步中"标记。
- 平时收到的通知，`seq` 小于等于 `SessionView.seq` 的一律丢弃（重复）；应用后更新 `SessionView.seq`。如果 `seq` 跳号，说明漏了通知，重新 attach 一次。
- 收到 `session/update` 时，按 `_meta.pcx` 处理：
  - 带 `item`（工具或计划）：按 `item.id` 整条替换；表里没有就追加。
  - message 或 thought chunk：按 `itemId` 找到条目，把 chunk 的文本块追加到第一个 text 块、其他块追加到末尾；表里没有这个 id（`created == true`，或者条目已经在窗口之外），就用 chunk 新建一个条目，`kind` 取自变体，`turn` 取自 `_meta.pcx.turn`。
  - 其他变体：只更新会话级状态。
- `_meta.pcx.generation` 和 `SessionView.generation` 不一致时，说明错过了一次重载：清空视图，发出 `acp/session/generation`。
- attach 返回 `loading: true` 时，`thread_read` 等待 `_pcx/session/loaded`，最多 300 s，然后重新 attach；收到 `_pcx/session/loadFailed` 时，返回 `[acp.session_not_loadable]` 或者原始错误。
- `pcx_code(&anyhow::Error) -> Option<String>` 遍历 `err.chain()`，对每一层的 Display 调用 `rpc::pcx_code_of`，返回第一个找到的 `acp.<code>`；需要按错误码走不同分支时都用它（T16）。
- 错误要保证前缀能到达 Flutter：
  - bridge 把 Hub 的错误交给 FRB 时，不要再套 `.context()`，前缀保持在最外层 message 的开头。
  - `AcpError` 的 Display 输出 `[acp.<code>] <message>`。
  - `meta_acp_*` 收到非 2xx 响应时，把响应体 `{"code","message"}` 转成 `anyhow!("[{code}] {message}")`，不走 `meta.rs:175` 那种通用格式。

#### 4.4.3 分发（`api/bridge.rs`）

在下列每个函数里，现有 `opencode::is_opencode` 分支之前插入 `if acp::is_acp(&service_key) { return …; }`。没有 OpenCode 分支的函数，就加在函数开头。

| 函数（行号） | ACP 实现 |
|---|---|
| `app_connect`（778）、`app_is_connected`（787）、`app_disconnect`（795）、`app_probe_reason`（820） | `acp::connect / is_connected / disconnect / probe_reason` |
| `app_events`（950） | `acp::subscribe_events`，转发方式与 OpenCode 分支相同 |
| `app_thread_list`（998） | `acp::thread_list`：调用 Hub 的 `session/list`，翻页取全部，上限 500 |
| `app_model_list`（1017） | `acp::model_list`（§4.5.3） |
| `app_thread_start`（1041） | `acp::thread_start(model, cwd)`：<br>1. 确定 cwd：`cwd` 为 None 时，取主机的默认项目（`meta::project_config(service_key)` 的 `default_project`，即 `meta_project_config` 用的那套逻辑）；也没有时返回 `[acp.cwd_required] choose a project folder first`。<br>2. 调用 `session/new`。<br>3. 立即调用 `_pcx/session/attach {tail: 0}`，拿到 generation 并建立 `SessionView`。<br>4. 传了 model 时，再调用 `set_config_option`。<br>approval、sandbox、tier 参数一律忽略 |
| `app_turn_steer`（1065） | 返回 `Err("steer is not supported by ACP agents")`。界面已按能力把入口隐藏 |
| `app_respond_approval`（1088） | `acp::respond_approval(decision)`（§4.5.4） |
| `app_respond_user_input`（1106） | `acp::respond_user_input(answers_json)`（§4.5.5） |
| `app_thread_resume`（1119） | `acp::thread_resume`：attach，并补发待办 |
| `app_thread_read`（1147） | `acp::thread_read`：attach（tail 20），按下面的规则组装 `ThreadHistory`。`include_turn_pages` 参数忽略，ACP 不返回 turn_pages。<br>- `history_epoch = Some(generation)`，必须和 history sync 的 source generation 相同，否则 `save_history` 会拒绝保存（`session_sync.rs:309-313`）。<br>- `first_turn_id` 取 `turns` 第一项的 `t{turn}`；`dropped_turns > 0` 时为 None，因为最早的轮次已经不在转录里。<br>- `turn_pages = []`，`has_older` 取自 AttachResult，`older_unavailable` 同名取值。<br>- `running` 和 `active_turn_id`（`t{turn}`）取自 AttachResult。<br>- `tokens_used` 和 `context_window` 取自 usage 的 `used` 和 `size`。<br>- `model` 和 `reasoning_effort` 按 §4.5.3 取值，`config_confirmed = true`。<br>- `cwd` 取自 AttachResult。<br>- 其余 `Option` 字段为 None |
| `app_thread_older_page`（1201） | `acp::thread_older_page`：以当前最早的条目为 `before`，limit 60 |
| `app_thread_turn_items`（1215） | `acp::thread_turn_items`：按 `turn` 取窗口，最多翻 5 页，把这一轮全部取回 |
| `app_thread_turn_page`（714） | `acp::thread_turn_page(turn_id, load_more, delta_only)`：<br>- `load_more == false`：从这一轮开头取一页（limit 60）。<br>- `load_more == true`：以上次返回的最后一条为 `after`，继续取下一页。<br>- `delta_only == Some(true)` 时只返回本次新读到的条目；否则返回这一轮到目前为止累计读到的全部条目，与 `bridge.rs:719` 的约定一致。<br>- `has_more` 取 `WindowResult.has_more` |
| `app_thread_runtime_config`（1235，sync） | `acp::thread_runtime_config`：只读缓存 |
| `app_rate_limits`（1259）、`app_git_diff`（1268）、`app_compact`（1277）、`app_set_thread_name`（1305）、`app_force_resume`（1609） | 返回 `Err("… is not supported by ACP agents")` |
| `app_thread_summary`（1291） | `Ok(None)` |
| `app_turn_start`（1325） | `acp::turn_start(text, images, model, collaboration_mode, reasoning_effort)` |
| `app_turn_interrupt`（1366） | `acp::turn_interrupt`：发送 `session/cancel` |
| `app_capabilities`（1410，sync） | `acp::capabilities(&key)`（§4.4.4） |
| `app_running_threads`（1430） | `acp::running_sessions`：调用 `_pcx/sessions/running`。把原来的 `!is_opencode → Err` 改成 `!is_opencode && !is_acp → Err` |
| `app_history_sync_prepare`（2201）、`app_history_prefetch`（2209） | 调用 `session_sync::prepare` 和 `session_sync::prefetch`（§4.4.6） |
| `app_history_cached`（2193）、`app_history_focus`（2217） | 不改，两者本来就和 provider 无关 |
| `meta_*` | 不改。ACP 主机的 meta 提供通用路由，文件链接通过 `AcpSessionDirs` 找到会话目录 |

#### 4.4.4 能力与托管状态 DTO（只新增字段）

`AppCapabilitiesDto`（`bridge.rs:1379`）新增以下字段，Dart 的 `AppCapabilities`（`bridge_api.dart:232`）同步新增：

| 字段 | codex | opencode | acp |
|---|---|---|---|
| `agent_name: String` | `""` | `""` | Hub 返回的 agent 名称 |
| `steer: bool` | true | true | false |
| `rename: bool` | true | true | false |
| `compact: bool` | true | true | false |
| `git_diff: bool` | true | true | false |
| `images: bool` | true | true | `caps.image` |
| `config_options: bool` | false | false | `caps.configOptions` |
| `slash_commands: bool` | false | false | `caps.commands` |
| `approval_options: bool` | false | false | true |
| `url_elicitation: bool` | false | false | true |
| `running_via_threads: bool` | false | true | true |
| `history_prefetch: bool` | true | false | true |
| `session_reload: bool` | false | false | true |

ACP 已有字段的取值：`provider = "acp"`；`fast`、`permission_presets`、`guardian`、`rate_limits`、`takeover`、`external_writer_monitor`、`local_sessions`、`approve_always_persists_project`、`child_sessions` 都是 false；`plan_mode` 在某个 `mode` 配置项里有值为 `plan` 时为 true；`effort_label = "effort"`；`multi_select_questions = true`。

- 还没连上 Hub 时，`acp::capabilities` 返回保守的默认值：`provider = "acp"`，除 `running_via_threads`、`history_prefetch` 外，所有可选能力都是 false。连上之后，事件循环发出 `acp/capabilities`，界面据此刷新缓存（§4.7.5）。
- Dart 的 `AppCapabilities` 构造函数里，新字段都给默认值：bool 为 false，`agentName` 为 `''`。这样现有的调用方不用改也能编译。`codex` 和 `openCode` 两个常量显式写出新字段的值。
- Codex 和 OpenCode 的新字段在现有的 `app_capabilities` 函数体里按上表赋值，已有字段的计算方式不变。

`ServeStatus`（`serve.rs:132`）和 `AppServeStatusDto`（`bridge.rs:357`）新增 `agent_id: Option<String>` 和 `agent_name: Option<String>`，默认都是 None。Dart 的 `AppServeStatus` 同步新增 `agentId`、`agentName` 和 `bool get isAcp => provider == 'acp'`。

`ThreadHistory`（`app_session.rs:1491`）和 `ThreadHistoryDto`（`bridge.rs:637`）新增 `older_unavailable: bool`，带 `#[serde(default)]`。`OlderPage`（`app_session.rs:564`）和 `OlderPageDto`（`bridge.rs:744`）也新增同名字段，这样往前翻页翻到底时，界面也能显示提示。

#### 4.4.5 新增的 FRB 函数（`api/bridge.rs`；除 `app_auth_state` 标了 `#[frb(sync)]` 以外，都是普通的 `pub fn`）

托管与本机管理（只在桌面可用，移动端返回 §1 的错误）：

```rust
pub fn app_serve_start_acp(name: Option<String>, agent_id: String) -> Result<AcpServeDto>
pub fn acp_agents() -> Result<Vec<AcpAgentDto>>
pub fn acp_install(agent_id: String, version: Option<String>) -> Result<String>        // job id
pub fn acp_job(job_id: String) -> Result<Option<AcpJobDto>>
pub fn acp_uninstall(agent_id: String) -> Result<()>
pub fn acp_settings() -> Result<AcpSettingsDto>
/// `force == false` returns the D16 warnings without saving when there are any.
pub fn acp_settings_set(settings: AcpSettingsDto, force: bool) -> Result<AcpSaveDto>
pub fn acp_custom_agents() -> Result<Vec<AcpCustomAgentDto>>
pub fn acp_custom_agent_put(agent: AcpCustomAgentDto) -> Result<()>
pub fn acp_custom_agent_delete(id: String) -> Result<()>
pub fn acp_auth_terminal(name: String, method_id: String) -> Result<()>
pub fn acp_auth_agent(name: String, method_id: String) -> Result<AcpAuthDto>
pub fn acp_auth_recheck(name: String) -> Result<AcpAuthDto>
```

远程管理（任何控制器都能用，走对方主机的 meta）：

```rust
pub fn meta_acp_agents(service_key: String) -> Result<AcpAgentsDto>
pub fn meta_acp_install(service_key: String, agent_id: String) -> Result<String>
pub fn meta_acp_job(service_key: String, job_id: String) -> Result<Option<AcpJobDto>>
pub fn meta_acp_host(service_key: String, agent_id: String, name: Option<String>) -> Result<String>   // host job id; poll meta_acp_job
```

会话：

```rust
pub fn app_respond_permission_option(service_key: String, request_id: String, option_id: String) -> Result<()>
pub fn app_respond_elicitation_url(service_key: String, request_id: String, accept: bool) -> Result<()>
pub fn app_config_options(service_key: String, thread_id: String) -> Result<Vec<AcpConfigOptionDto>>
pub fn app_set_config_option(service_key: String, thread_id: String, config_id: String, value: String, boolean: bool) -> Result<()>
pub fn app_slash_commands(service_key: String, thread_id: String) -> Result<Vec<AcpCommandDto>>
pub fn app_thread_reload(service_key: String, thread_id: String) -> Result<()>
#[frb(sync)]
pub fn app_auth_state(service_key: String) -> Option<AcpAuthDto>                       // cached HubMeta.auth; None before connect
pub fn app_auth_authenticate(service_key: String, method_id: String) -> Result<AcpAuthDto> // agent-type only; returns inProgress
```

DTO：

```rust
pub struct AcpServeDto { pub device: String, pub name: String, pub service_key: String, pub listen_addr: String, pub meta_service_key: String, pub agent_id: String, pub agent_name: String, pub agent_version: String, pub auth: AcpAuthDto, pub reused: bool }
pub struct AcpAuthDto { pub status: String, pub methods: Vec<AcpAuthMethodDto>, pub message: Option<String> }
pub struct AcpAuthMethodDto { pub id: String, pub name: String, pub description: String, pub kind: String, pub remote: bool, pub available: bool }
pub struct AcpAgentDto { pub id: String, pub name: String, pub description: String, pub source: String, pub pinned_version: Option<String>, pub installed_version: Option<String>, pub state: String, pub detail: Option<String>, pub job_id: Option<String>, pub registry_version: Option<String>, pub hosted_names: Vec<String>, pub approx_size_mb: u32, pub needs_node: bool, pub remote_install_allowed: bool }
pub struct AcpAgentsDto { pub remote_management: bool, pub agents: Vec<AcpAgentDto> }
pub struct AcpJobDto { pub id: String, pub kind: String, pub agent_id: String, pub version: String, pub state: String, pub bytes: u64, pub total: Option<u64>, pub message: Option<String>, pub error_code: Option<String>, pub service_key: Option<String> }
pub struct AcpEnvVarDto { pub name: String, pub value: String }
pub struct AcpCustomAgentDto { pub id: String, pub name: String, pub command: String, pub args: Vec<String>, pub env: Vec<AcpEnvVarDto> }
pub struct AcpSettingsDto { pub remote_management: bool, pub npm_registry: Option<String>, pub claude_engine_path: Option<String>, pub codex_binary: Option<String>, pub binary_overrides: Vec<AcpBinaryOverrideDto>, pub opencode_data: Vec<AcpDataModeDto>, pub gateways: Vec<AcpGatewayDto>, pub flags: Vec<AcpAgentFlagDto> }
/// One `conditional_args` switch from the catalog (D21), e.g. claude-acp / allow_subscription_login.
pub struct AcpAgentFlagDto { pub agent_id: String, pub setting: String, pub label_key: String, pub confirm_key: Option<String>, pub value: bool }
/// D20. Reading returns `token: None` plus `has_token`; writing with `token: None` keeps the stored token,
/// `Some("")` deletes it. `clear == true` removes the whole gateway entry.
pub struct AcpGatewayDto { pub agent_id: String, pub method_id: Option<String>, pub base_url: String, pub token: Option<String>, pub has_token: bool, pub provider_name: Option<String>, pub extra_headers: Vec<AcpEnvVarDto>, pub clear: bool }
pub struct AcpBinaryOverrideDto { pub agent_id: String, pub path: String }
pub struct AcpSaveDto { pub saved: bool, pub warnings: Vec<String> }
pub struct AcpDataModeDto { pub family: String, pub mode: String /* auto | shared | isolated */ }
/// `role`: `model` | `effort` | `mode` | `other`, classified by §4.5.3; the generic panel shows `mode` and `other`.
pub struct AcpConfigOptionDto { pub id: String, pub name: String, pub description: String, pub category: String, pub role: String, pub kind: String, pub current_value: String, pub options: Vec<AcpConfigValueDto> }
pub struct AcpConfigValueDto { pub value: String, pub name: String, pub description: String, pub group: Option<String> }
pub struct AcpCommandDto { pub name: String, pub description: String, pub hint: Option<String> }
```

改完之后，在 `apps/flutter` 目录下执行 `flutter_rust_bridge_codegen generate`（codegen 版本 2.12.0），重新生成 `lib/src/rust/` 和 `frb_generated.rs`。

#### 4.4.6 历史缓存（`engine/session_sync.rs`）

- `prepare`：`:119` 原来只接受 `codex/app-server-v2`，改成接受 `codex/app-server-v2` 或 `acp/hub-v1`，并在现有的 60 s 缓存条目里记下 provider。
- `request`（`:154`）只给 Codex 引擎用，不改；如果 namespace 的 provider 是 ACP，返回错误。
- 新增 `pub fn sync_window(service: &str, query: &WindowQuery, running: bool) -> Result<HistoryWindow>`，暴露现有的私有 `sync`（`:196`），ACP 引擎通过它调用。
- `prefetch`（`:411`）：provider 是 ACP 时：
  1. 调用 `sync_window`，参数 `collection = "items"`、`limit = 20`、`projection` 省略。这会从最新的条目开始倒序取，和 Codex 的预取（`:430`）一样取的是会话末尾。
  2. 按 `order` 反转成时间正序，把文档反序列化成 HubItem 后映射为 ThreadItem。
  3. 再做一次 `metadata` 查询，补齐 `running`。
  4. 组成 `ThreadHistory`，其中 `history_epoch` 为窗口的 generation，然后 `save_history`。
- `acp::thread_read` 每次读取后也调用 `save_history`，和 OpenCode 的做法（`opencode/history.rs:87`）一致。`ThreadHistory` 没有 title 字段，会话标题只出现在 `ThreadMeta` 里。
- `cached_history`、磁盘配额和 namespace 都不改。

### 4.5 映射

#### 4.5.1 HubItem → ThreadItem（`engine/acp/mapping.rs`）

| HubItem | item_type | title | text | images |
|---|---|---|---|---|
| user | `userMessage` | `""` | text 块拼接；`resource_link` 渲染成 `@{name}`，`resource` 渲染成 `@{uri}` | image 块转成 `data:{mimeType};base64,{data}`；单张超过 4 MiB 时丢弃，并在 text 末尾追加 `[图片过大，未显示]` |
| agent | `agentMessage` | `""` | text 块拼接 | 规则同上 |
| thought | `reasoning` | `""` | text 块拼接 | |
| tool，`kind = execute` | `commandExecution` | `rawInput.command`（字符串，或数组用空格连接），没有时用 `title` | content 里的文本块；`terminal` 内容写成 `[terminal {id}]`；再加上 `rawOutput` 里的 `output`、`stdout`、`stderr` 字符串。`status = failed` 时末尾追加 `\n[error]`；`rawOutput.exitCode` 是数字时追加 `\n[exit N]` | |
| tool，`kind ∈ {edit, delete, move}` | `fileChange` | 只有一个 diff 时用 path，多个时用 `N files` | 每个 diff 合成统一 diff：`--- a/{path}`、`+++ b/{path}`、`@@ -1,{old} +1,{new} @@`，旧内容每行前缀 `-`，新内容每行前缀 `+`。新文件（`oldText` 为 null）只有 `+` 行。总长上限 256 KiB | |
| tool，`kind = fetch` | `webSearch` | `title` | `rawInput` 的 JSON | |
| tool，`kind = think` | `reasoning` | `title` | content 里的文本块 | |
| 其他 tool | `dynamicToolCall` | `name`，没有时用 `title` | JSON `{input: rawInput, content: 文本, output: rawOutput, locations}`，上限 256 KiB | |
| plan | `plan` | `""` | 调用 `app_session::encode_plan(&json!({"plan": entries.map(\|e\| {"step": e.content, "status": e.status})}))`（`app_session.rs:3028`） | |
| notice | `dynamicToolCall` | `"Pocket-Codex"` | 提示文字 | |

- 工具的 `status` 为 `pending` 或 `in_progress` 时，按运行中显示，与 OpenCode 相同。
- `turn_id = "t{turn}"`。`turn_completed_at` 取 `completed_at_ms / 1000`，单位是秒（`app_session.rs:4033`）。`turn_duration_ms = completed_at_ms - started_at_ms`，只有实时轮次才有这两个值。
- TurnInfo → `TurnSummary { turn_id: "t{turn}", user_text: user_preview, assistant_text: agent_preview, loaded: 窗口里有这一轮的条目 }`。
- SessionInfo → `ThreadMeta { id, preview: title 或 "", name: title, cwd, updated_at: updatedAt 按 RFC 3339 解析为秒，解析失败时为 0 }`。

#### 4.5.2 Hub 的消息 → AppEvent（`engine/acp/events.rs`）

AppEvent 统一用 T15 移出来的 `event`、`item_event`、`bare_item` 构造，所以 raw 的形状和 OpenCode 引擎完全一致。

| 收到的消息 | AppEvent |
|---|---|
| `_pcx/turn/started` | `turn/started`，raw `{"threadId", "turnId": "t{turn}", "turn": {"id": "t{turn}", "status": "inProgress"}}` |
| `_pcx/turn/completed` | `turn/completed`，raw `{"threadId", "turn": {"id", "status", "error"?: {"message"}}}`。status 的对应关系：`end_turn` → `completed`；`cancelled` → `interrupted`；`max_tokens`、`max_turn_requests`、`refusal`、`_pcx_agent_exited`、`_pcx_error` → `failed`，error.message 取原因 |
| `session/update` agent/thought chunk，item 新建 | `item/started`，text 为空 |
| `session/update` agent chunk 追加 | `item/agentMessage/delta`，text 为增量 |
| `session/update` thought chunk 追加 | `item/reasoning/textDelta`，text 为增量 |
| `tool_call` / `tool_call_update` | 用 `_meta.pcx.item`（折叠后的完整条目）按 §4.5.1 映射。未完成（`pending`、`in_progress`）时发 `item/started`，完成（`completed`、`failed`）时发 `item/completed`，都带完整的 title 和 text |
| `plan` | `item/completed`，item 类型为 `plan`，带完整 text |
| `_pcx/turn/completed` 到达时 | 在它之前，先给这一轮每个还没发过 completed 的 agent 和 thought 条目补一个 `item/completed`，带完整 text |
| `usage_update` | `thread/tokenUsage/updated`，raw `{"threadId", "tokenUsage": {"last": {"totalTokens": used}, "modelContextWindow": size}}`。这个形状和 `apps/flutter/lib/src/context_status.dart:25-39` 的解析方式一致 |
| `session_info_update` 带 title | `thread/name/updated`，raw `{"threadId", "name"}` |
| `config_option_update`、`current_mode_update`、`available_commands_update` | 更新 `SessionView`，然后发 `acp/config/updated`，raw `{"threadId"}` |
| `_pcx/sessions/changed` | `acp/sessions/changed`，raw `{}`。界面目前不处理 `thread/started`，所以用新的事件，并在 §4.7.5 里加处理：收到后调用 `_loadThreads()` |
| `_pcx/session/state` | `updatedAt` 变了、会话不在运行、并且界面当前打开了它时，发 `acp/session/changed`，raw `{"threadId"}` |
| `_pcx/session/generation` | `acp/session/generation`，raw `{"threadId"}` |
| `_pcx/request/resolved` | `serverRequest/resolved`，raw `{"threadId", "requestId"}`（与 `opencode/events.rs:69` 相同） |
| `_pcx/queue/failed` | `acp/queue/failed`，raw `{"threadId", "reason", "prompts"}` |
| `_pcx/hub/state` | `acp/hub/state`，raw 原样透传 |
| `session/request_permission` | 见 §4.5.4 |
| `elicitation/create`，mode 为 form | 见 §4.5.5 |
| `elicitation/create`，mode 为 url | `acp/elicitation/url`，raw `{"threadId"?, "message", "url", "host"}`，`request_id` 为 Hub 的待办 id。Hub 级别（没有会话）的 elicitation，`thread_id` 为 None |

#### 4.5.3 配置项 → 模型、推理强度、模式

- 配置项的来源：当前会话 `SessionView.config_options`，没有时用 `HubMeta.defaultConfigOptions`。两者都为空（新对话第一次发送之前）时，`model_list` 调用 `_pcx/hub/defaults`，结果写回缓存的 `HubMeta`；旧版 Hub 没有这个方法时返回空列表。界面在没有模型时，收到 `process.state == ready` 的 `acp/hub/state` 会重新读取模型列表。
- 模型选项：`category == "model"` 的 select 项；没有 category 时，取 `id == "model"` 的项。
- 推理强度选项：`category == "thought_level"` 的项；没有 category 时，依次找 `id` 为 `effort`、`reasoning_effort`、`thought_level` 的项。
- `model_list`：模型选项的每个值生成一个 `ModelInfo`：
  - `id = value`，`display_name = name`，`description = description 或 ""`。
  - `supported_reasoning_efforts` 取推理强度选项的全部值，`default_reasoning_effort` 取它的 `currentValue`。
  - `is_default = (value == 模型选项的 currentValue)`。
  - `supported_service_tiers` 为空，所以界面不显示 Fast。
- `thread_runtime_config`：`model` 取模型选项的 currentValue，`reasoning_effort` 取推理强度选项的 currentValue，其他字段为 None，`confirmed_by_update = true`。
- `turn_start` 在 submit 之前按顺序应用配置：
  1. model 与当前值不同：`set_config_option(模型选项 id, model)`。
  2. reasoning_effort 与当前值不同：同上，改推理强度选项。
  3. `collaboration_mode == "plan"`，并且某个 `mode` 类选项有 `plan` 这个值：先把当前值记进 `pre_plan_mode`，再设成 `plan`。
  4. `collaboration_mode == "default"`，并且 `pre_plan_mode` 有值：恢复成这个值。
  5. 其他情况不动。
- 通用面板（§4.7）列出除模型、推理强度以外的所有选项，select 用下拉框，boolean 用开关，都通过 `app_set_config_option` 设置。

#### 4.5.4 权限 → 审批卡片

AppEvent：
- `kind`：`toolCall.kind ∈ {edit, delete, move}` 时为 `item/fileChange/requestApproval`，否则为 `item/commandExecution/requestApproval`。
- `title = toolCall.title`；`text` 取 toolCall 第一个文本内容；`request_id` 为 Hub 的待办 id。
- `raw`：

  ```json
  {"threadId":"…","command":"<title；rawInput.command 是字符串时拼在后面>","cwd":"<会话 cwd>",
   "reason":"<text>","changes":[{"path":"…"}],
   "acpOptions":[{"optionId":"…","name":"…","kind":"allow_once"}]}
  ```

  `changes` 取自 `locations` 和 diff 内容里的 path。

`respond_approval(decision)`：用 `acpOptions` 里第一个 kind 符合的选项回应：
- `accept` → `allow_once`
- `acceptForSession` → `allow_always`
- `decline` → `reject_once`；没有时用任意一个 `reject_*`
- `cancel` → `{"outcome":{"outcome":"cancelled"}}`
- 找不到对应的选项时返回错误。

`respond_permission_option(option_id)`：直接用 option_id 回应。

#### 4.5.5 elicitation → 输入卡片

form 模式：
- AppEvent 的 `kind = item/tool/requestUserInput`，raw 为 `{"threadId", "title": message, "questions": [...]}`。字段和 OpenCode 的 `form_event` 一致（`opencode/mapping.rs:481-537`）。
- 从 `requestedSchema.properties` 生成 questions，`required` 里的字段排在前面，其余按原顺序：

  | 属性类型 | 生成的问题 |
  |---|---|
  | string，带 `enum` 或 `oneOf` | 单选，选项的 label 取 `title`，没有就取值本身 |
  | string | 自由输入（`isOther = true`，`options = []`） |
  | number、integer | 自由输入，回答时按数字解析 |
  | boolean | 两个选项："是 / Yes"、"否 / No" |
  | array，且 items 带 enum | `multiSelect = true` |
  | 其他 | 加一个 `__unsupported` 问题，文案改成"此表单包含暂不支持的字段，请在主机上完成，或取消。"，与 OpenCode 相同，只能取消 |

- `respond_user_input(answers_json)`：`{id: [labels]}` 按属性类型还原成 content：string 取第一个 label；number 和 integer 解析成数字；boolean 按"是"或"否"转换；array 取全部 label。然后回应 `{"action":"accept","content":…}`。解析失败时返回错误，界面显示后保留卡片。空答案回应 `{"action":"cancel"}`。

url 模式：
- 事件是 `acp/elicitation/url`。
- `respond_elicitation_url(accept)`：accept 时回应 `{"action":"accept"}`，否则回应 `{"action":"decline"}`。

### 4.6 backend 与 CLI

- `crates/pocket-codex-backend/src/api.rs`：
  - `ServicesQuery`（`:270-274`）新增字段 `#[serde(default)] include_acp: bool`。
  - `own_services`（`:312`）新增参数 `include_acp`，过滤条件（`:331`）加上 `|| (include_acp && nsid.service.kind == ServiceKind::Acp)`。
  - 仿照 `opencode_services_are_listed_only_on_request`（`:466`），新增测试 `acp_services_are_listed_only_on_request`。
- `crates/pocket-codex-bridge/src/engine/account.rs:349`：查询参数改为 `?include_opencode=true&include_acp=true`。
- 部署：账号模式下，要先部署新的 backend，ACP 服务才会出现在发现列表里。老 backend 会忽略这个参数，只是不列出 ACP，不会报错。自托管模式直接读 relay 的键，不受影响。
- CLI：只做 §4.1.1 里为了能编译通过的 match 补分支，不新增命令。

### 4.7 Flutter（`apps/flutter/lib/src/`）

#### 4.7.1 服务键与 provider 判断

- `service_key.dart`：
  - `_isKind`（`:46`）加上 `'acp'`，`isSessionKind`（`:51`）也加上 `'acp'`。
  - 新增 `bool isAcpKey(String key) => parseServiceKey(key).kind == 'acp';`。
  - 在 `test/service_key_test.dart` 补充对应的用例。
- `bridge_api.dart` 的 `AppServeStatus` 新增 `bool get isCodex => provider == 'codex';` 和 `bool get isAcp => provider == 'acp';`。
- 下面这些地方把 `!isOpenCode` 当成了"是 Codex"，全部改成 `isCodex`，或者按 provider 分三种情况处理：
  - `home_screen.dart:383`：`codex` 的判断改成 `local.any((h) => h.isCodex)`。
  - `services_screen.dart:297-305`：kind 按 provider 取 `opencode`、`acp` 或 `app`；只有 Codex 才合成 `api` 这一行。
  - `services_screen.dart:383-399`：`localAppAddr` 和 `localApiAddr` 只收录 `isCodex` 的主机；`localTunnels` 给 ACP 主机加上 `(name, kind: 'acp')`。
  - `services_screen.dart:581-585`：provider 按 kind 映射，`'acp'` 映射为 `'acp'`；协议标签用 `'ACP'`。
  - `services_screen.dart:1602`：`host.isCodex` 时用 `appReachableLocalProvider`，其他情况都用 `appReachableProvider`。
  - `codex_setup_screen.dart:159`、`local_host_dialog.dart:173` 和 `:205`：原来的 `!isOpenCode` 改成 `isCodex`，`isOpenCode` 保持不变，再为 `isAcp` 增加一个分支。
  - `providers.dart:304`：改成 `if (caps.runningViaThreads)`，其中 `caps = api.appCapabilities(serviceKey)`。这个分支拿到 `appRunningThreads` 的结果后，如果 `caps.historyPrefetch` 为真（即 ACP），就按 `:321-329` 相同的轮换规则，对最多 2 个运行中的会话调用 `appHistoryPrefetch`，然后再 `return`。OpenCode 的 `historyPrefetch` 为 false，行为不变。这一处在 M9 修改。
- `widgets/provider_badge.dart`：
  - `provider` 可取 `codex`、`opencode`、`acp`，并新增可选参数 `label`。
  - ACP 的底色用 `scheme.primaryContainer`；文字取 `label`，为空时用 `l10n.providerAcp`。
  - `ProviderBadge.forKey(key, {badgeKey, label})`：`isAcpKey` 时 provider 为 `'acp'`。
  - `label` 的来源（远程的 `ServiceEntry` 不带 agent 名称，所以统一按下面的规则取）：
    - `services_screen.dart:1679`（本机主机卡片）：传 `host.agentName`。
    - `services_screen.dart:1222`（`_CapabilityRow`，只有 provider 和服务键）：给 `_CapabilityRow` 新增可选参数 `label`。调用方在本机主机列表里找到同一服务键时传 `host.agentName`；找不到时传 `api.appCapabilities(key).agentName`，已连接时有值，未连接时为空，显示通用的"ACP"。
    - `app_session_screen.dart:6788`（当前会话）：传 `_caps.agentName`。
    - `app_session_screen.dart:6762`（切换到其他服务的下拉项）：传 `ref.read(bridgeApiProvider).appCapabilities(otherKey).agentName`。
- 错误显示（`error_format.dart`）：
  - `friendlyError`：去掉开头的 `[acp.<code>] ` 前缀。
  - 新增 `String? acpErrorCode(Object error)`：返回这个前缀里的 `acp.<code>`，界面按它选 §6 的文案。
  - ACP 的名字冲突错误和 OpenCode 一样，按原文显示；`isHostNameConflict` 不改。
- 新增 `lib/src/hosting_support.dart`，提供 `bool hostingSupportedPlatform()`，逻辑与 `services_screen.dart:1453-1458` 的 `_hostingSupported` 相同。设置页和 agent 管理页用它；已有的私有 getter 保持不动。
- 生效时间：`_isKind` 加 `'acp'` 和新增 `isAcpKey` 在 M1 完成。`isSessionKind` 加 `'acp'` 放到 M8，因为在 bridge 引擎（M7）完成之前，界面不应该把 ACP 服务当成可以打开的会话。

#### 4.7.2 托管对话框

- `local_host_dialog.dart`：
  - 把字段 `bool _openCode` 换成 `String _provider = 'codex'`。`_openCode` 在 `local_host_opencode.dart:25` 被赋值，所以不能只加一个同名 getter：用 `rg -n "_openCode" apps/flutter/lib/src/widgets` 找到全部读写位置，读的地方改成 `_provider == 'opencode'`，写的地方改成给 `_provider` 赋值。
  - `_providerPicker`（`local_host_opencode.dart:7-30`）增加第三个选项：`ButtonSegment(value: 'acp', label: Text(l10n.providerAcp, key: const Key('provider-acp')))`。选中 ACP 时调用 `_loadAcpAgents()`。
  - `build`（`:205`）新增分支：已有主机 `existing.isAcp` 时显示 `_acpExisting(existing)`；新建时 `_provider == 'acp'` 显示 `_acpForm()`。
  - 开始按钮的 onPressed 按 `_provider` 分别调用 `_start`、`_startOpenCode` 或 `_startAcp`。
- 新建 `widgets/local_host_acp.dart`，写成 `part of 'local_host_dialog.dart'`，内容是 `extension _AcpHost on _LocalHostDialogState`。状态字段加在 `_LocalHostDialogState` 里：`List<AcpAgent>? _acpAgents; String? _acpAgentId; AcpJob? _acpJob; Timer? _acpPoll; AcpAuth? _acpAuth; final _acpName = TextEditingController();`。
- `_acpForm()`：
  - agent 下拉框（Key `acp-agent-picker`），列出 `acpAgents()` 返回的全部 agent，每项带一个状态标签。
  - 选中 agent 后显示：已安装版本、清单版本、大小，以及 state/detail。`engine_incompatible` 时显示建议的版本；`registry_version` 有值时显示"注册表有新版本（未经 Pocket-Codex 验证）"。
  - 安装或升级按钮（Key `acp-install-btn`）：调用 `acpInstall(id)`，之后每 500 ms 调一次 `acpJob` 轮询，用 `LinearProgressIndicator`（Key `acp-install-progress`）显示进度。失败时用 `friendlyError` 显示原因。
  - 实例名输入框（Key `acp-name`），默认值是 agent id。
  - "管理 agent…"链接，跳转到 `/settings/acp`。
- `_startAcp()`：
  1. 调用 `appServeStartAcp(name:, agentId:)`。
  2. 调用 `setAutoHostAcp(AutoHostAcpPrefs(name, agentId))`，并 invalidate `localServeListProvider` 和 `servicesProvider`。
  3. 返回结果里 `auth.status == 'required'` 时，对话框不关闭，切到已有主机的视图并显示登录区；否则关闭对话框。
  4. 失败时用 `friendlyError(e)` 原样显示，与 `_startOpenCode` 相同。名字冲突错误的格式是 "`{name}` is already hosting … on this device"。
- `_acpExisting(host)` 依次显示：
  - 标签（`label: host.agentName`）
  - agent id 和版本；`providerVerified == false` 时显示"未锁定版本"（Key `acp-unpinned`）
  - ws 的地址和键、meta 的地址和键
  - 登录区
  - `ProjectFoldersEditor`
- 登录区 `_acpAuthSection(host)`：
  - 用 `appAuthState(host.appServiceKey)` 读状态。
  - 有 gateway 类方法并且 `gateway_configured` 时，显示"已配置模型网关"和网关地址（不显示密钥），其余登录按钮收进"其他登录方式"。有 gateway 类方法但还没配置时，显示"配置模型网关"按钮，跳转到 `/settings/acp`，打开该 agent 的网关对话框。
  - 每个方法一个按钮，Key 为 `acp-login-<methodId>`：terminal 类且 `available` 的，调用 `acpAuthTerminal(name, id)`；agent 类的，调用 `acpAuthAgent(name, id)`。
  - 一个"重新检测"按钮（Key `acp-recheck-btn`），调用 `acpAuthRecheck(name)`。
- `_stopAcp()`：
  1. `appRunningThreads(host.appServiceKey)` 不为空时，先弹确认框（Key `acp-stop-confirm`，文案 `l10n.acpStopRunningConfirm`）。
  2. 调用 `appServeStop(name)` 和 `removeAutoHostAcp(name)`。
  3. 把 `appServiceKey` 加进 `pendingRemovalProvider`。

#### 4.7.3 Agent 管理页（`screens/acp_agents_screen.dart`，新增）

- 路由：在 `router.dart` 中加 `GoRoute(path: '/settings/acp', builder: (c, s) => AcpAgentsScreen(serviceKey: s.uri.queryParameters['svc']))`。
- 外壳：`UtilityPage(route: '/settings', title: l10n.acpAgentsTitle, parent: UtilityParent(title: l10n.settingsTitle, route: '/settings'))`，写法同 `codex_setup_screen.dart:329-332`。

本机模式（`serviceKey == null`，只在桌面可进入）：
- 每个 agent 一个 `AcpAgentTile`，显示名称、来源、清单版本和已安装版本、状态、注册表提示。
  - 操作按钮：安装、升级、卸载。卸载前要确认；有托管实例在用时，按钮禁用并说明原因。
  - "安装注册表版本"放在"更多"菜单里，点击后先弹警告确认框（Key `acp-registry-install-confirm`）。
- 设置卡片：
  - 远程管理开关（Key `acp-remote-toggle`），下面附说明文字 `l10n.acpRemoteManagementHint`。
  - npm 镜像地址，只接受 https。
  - Codex 可执行文件路径。
  - Claude 引擎路径，放在"高级"折叠区里。
  - 每个 OpenCode 数据族的模式：自动、共享、隔离（D19）。选"共享"时弹风险确认框（Key `acp-shared-data-confirm`）。
  - 保存时先调用 `acpSettingsSet(settings, force: false)`：返回 `saved == false` 并带 warnings（D16 共存提示）时，弹确认框，用户确认后再用 `force: true` 保存。
  - archive 类 agent 的可执行文件覆盖，放在每个 agent 卡片的"高级"菜单里，对应 `binary_overrides`。
- 模型网关（D20）：每个 agent 卡片的"网关"按钮（Key `acp-gateway-<id>`）打开编辑对话框，字段依次是：
  - 按钮始终可用，因为 agent 没在托管时无法知道它提供哪些登录方式。
  - 登录方法：agent 正在托管时，下拉框列出它返回的 gateway 类方法，按 `gateway_protocol` 标注（例如"Anthropic 协议"、"OpenAI 协议"）；两个 Claude 网关方法的名称都是"Custom model gateway"，所以必须按协议区分。没有在托管时显示"自动（第一个网关方法）"。
  - agent 启动后如果发现它没有 gateway 类方法，`AuthState.message` 写"该 agent 没有提供网关登录方式，网关配置未生效"。
  - 网关地址：输入框下方按协议显示提示。`anthropic` 填根地址，例如 `https://relay.example.com`，Claude Code 会自己拼上 `/v1/messages`；`openai` 填到 `/v1`，例如 `https://relay.example.com/v1`，而且网关需要支持 `/v1/responses`（用户的 New API 中转站已确认支持）。
  - 密钥：密码框。已保存过时显示"已保存，留空表示不修改"。
  - provider 名称：可选。
  - 额外请求头：每行一个 `KEY=VALUE`。
  - 对话框底部写明："密钥只保存在这台主机的 agents.toml（权限 0600），不会发送给其他设备。"保存后，对正在托管的实例调用 `acpAuthRecheck`，让新配置生效。
- 清单开关（D21）：`settings.flags` 里属于这个 agent 的每一项，都在它卡片的"高级"区显示一个 `Switch`：Key 为 `acp-flag-<agentId>-<setting>`，标签取 `label_key`。Claude Code 的 `allow_subscription_login` 默认关闭。打开时，如果有 `confirm_key`，先弹确认框（§4.2.12）；确认后保存，并提示"重启该 agent 后生效"。
- 自定义 agent：
  - 列表来自 `acpCustomAgents()`，编辑时也用它的数据预填。"添加"按钮 Key 为 `acp-custom-add`。
  - 添加和编辑用同一个对话框，字段为 id、名称、命令（必须是绝对路径）、参数（每行一个）、环境变量（每行一个 `KEY=VALUE`）。
  - 对话框里写明："环境变量以明文保存在主机的 agents.toml（权限 0600）"。

远程模式（`serviceKey != null`，所有平台都能进入，包括安卓）：
- 数据来自 `metaAcpAgents(serviceKey)`。
- `remoteManagement == false` 时，显示提示条 `l10n.acpRemoteDisabled`，所有操作按钮禁用。
- 安装：调用 `metaAcpInstall`，之后每 1 s 调一次 `metaAcpJob` 轮询。
- 已安装但还没托管的 agent，显示"开始托管"按钮（Key `acp-host-btn`）：调用 `metaAcpHost(serviceKey, id)` 拿到任务 id，每 1 s 调一次 `metaAcpJob` 轮询。状态变成 `done` 后 invalidate `servicesProvider`，失败时显示 `error_code` 对应的文案。
- 远程模式下不显示设置卡片和自定义 agent。

入口：
- 设置页：
  - 桌面布局在 `settings_screen.dart:349-359` 的 Codex 卡片旁边，新增 `GroupCard(title: 'ACP')`，里面放一行 `_SettingsRow(key: Key('acp-agents-btn'), …)`。
  - 紧凑布局在 `:158-164` 旁边加一个 `ListTile`。
  - 这两个入口只在 `hostingSupportedPlatform()` 为真时显示。
- 服务页：远程设备的主机分组菜单里，新增"管理 ACP agent"（Key `host-acp-manage`），跳转到 `/settings/acp?svc=<该设备任意一个会话服务键>`。bridge 会从这个键推导出 meta 键。

#### 4.7.4 自动恢复托管（`ui_prefs.dart`、`home_screen.dart`）

- `UiPrefs` 新增 `final List<AutoHostAcpPrefs> autoHostAcp;`，默认值为 `const []`。
  - JSON 键为 `autoHostAcp`，值是 `[{name, agentId}]` 这样的列表；缺少 `agentId` 的条目直接忽略。
  - `copyWith` 增加 `List<AutoHostAcpPrefs>? autoHostAcp` 参数。
  - 加载时的合并规则见本节最后一条。
- 新增 `class AutoHostAcpPrefs { const AutoHostAcpPrefs({required this.name, required this.agentId}); … }`。
- `UiPrefsStore` 新增 `setAutoHostAcp(AutoHostAcpPrefs p)`（按 name 替换或追加）和 `removeAutoHostAcp(String name)`。
- `home_screen.dart`：
  - 新增静态标志 `_autoHostAcpAttempted`，在 `debugResetAutoHost()` 里一并重置。
  - `_restoreHosting` 现在有三处提前返回（`:370`、`:374`、`:385`），条件是 Codex 和 OpenCode 都已经尝试过、或者都没有偏好。这些条件都要加上 ACP：只有三种 provider 都"已尝试或没有偏好"时，才提前返回。
  - 处理完 OpenCode 后，遍历 `prefs.autoHostAcp`：本机还没有同名主机的，调用 `api.appServeStartAcp(name: p.name, agentId: p.agentId)`，每个都单独 try/catch。只要真的尝试过，就把 `_autoHostAcpAttempted` 置为 true。
- 合并规则（`ui_prefs.dart:245-262`）：`autoHostAcp` 按 `name` 合并。先放 `loaded` 里的条目，再用 `raced` 里的同名条目覆盖、不同名的追加。不能因为 `raced` 不为空就丢掉磁盘上已有的条目。

#### 4.7.5 会话界面（`screens/app_session_screen.dart`）

- **能力刷新**：`appConnect` 返回后，以及收到 kind 为 `acp/capabilities` 的 AppEvent 时，都把 `_capsCache` 置空并 `setState`。
- **会话列表刷新**：收到 `acp/sessions/changed` 时调用 `_loadThreads()`。
- **按能力隐藏入口**：
  - `_caps.steer` 为 false：隐藏"补充"入口，不调用 `appTurnSteer`（`:3051`、`:3663`）。
  - `_caps.gitDiff` 为 false：不请求 git diff（`:3318`），也不显示对应的角标。
  - `_caps.compact` 为 false：隐藏压缩入口（`:3465`）。
  - `_caps.rename` 为 false：隐藏重命名入口（`:4309`）。
  - `_caps.planMode` 为 false：隐藏 Plan 选项（`:8926-8940`、`:9478`）。
  - `_caps.images` 为 false：禁用图片附件入口 `_pickImages`（`:7668`）和粘贴图片。
- **通用配置面板**：新增 `screens/app_session/acp_config_panel.dart`，类名 `AcpConfigOptionsPanel(serviceKey, threadId)`。
  - `_caps.configOptions` 为真时显示，放在 `_turnSettingsPanel`（`:8653`）的推理强度滑块之后，以及 `_showConfigSheet`（`:8875`）里。
  - 数据来自 `appConfigOptions`，只显示 role 为 `mode` 或 `other` 的项。select 用下拉框，boolean 用 `Switch`，修改后调用 `appSetConfigOption`。
  - 收到 `acp/config/updated` 时重新读取。
- **Slash 命令**：新增 `screens/app_session/acp_slash_menu.dart`。`_caps.slashCommands` 为真、并且输入框内容以 `/` 开头时，在输入框上方弹出命令列表（数据来自 `appSlashCommands`）。选中一项后，把输入框内容替换成 `/{name} `。
- **卡片**（`_chatPane`，`:5718-5733`）：
  - 待办列表 `_approvals` 除了收录 `thread_id` 等于当前会话的请求，还收录 `thread_id` 为 None 的请求。这类是 Hub 级别的 elicitation，通常是登录用的 device code，所以同一个服务下每个打开的会话里都会显示；任何一处回答后，其余位置会随 `serverRequest/resolved` 一起消失。
  - kind 为 `acp/elicitation/url` 的待办，显示新卡片 `UrlElicitationCard`（写在 `composer_cards.dart` 里）。
    - 卡片显示 message，并以醒目的方式显示 URL 的 host（`Uri.parse(url).host`）。
    - "打开"按钮（Key `acp-url-open`）：先调用 `widgets/links.dart:34` 已有的打开链接函数，再调用 `appRespondElicitationUrl(accept: true)`。
    - "拒绝"按钮（Key `acp-url-decline`）：调用 `appRespondElicitationUrl(accept: false)`。
  - `ApprovalCard` 新增可选回调 `onOption(AppEvent, String optionId)`。raw 里有 `acpOptions` 时，按选项顺序渲染按钮，Key 为 `acp-option-<optionId>`：
    - `allow_once` 用 `FilledButton`。
    - `allow_always`、`reject_always` 先弹确认框（Key `acp-option-confirm`）。
    - 其余用 `TextButton`。
    - 没有 `acpOptions` 时，行为和现在完全一样。
  - `_decideOption(prompt, optionId)`：先从 `_approvals` 里移除这张卡片，再调用 `appRespondPermissionOption`。
- **提示条**：
  - 收到 `acp/hub/state`，`auth.status == 'required'` 时，显示登录提示条（Key `acp-auth-banner`）。主机就是本机并且是桌面时，提示条上显示登录按钮，行为同 §4.7.2；否则显示 `l10n.acpLoginOnHost`，另外如果有 agent 类方法，显示"在主机上开始登录"，调用 `appAuthAuthenticate`。
  - `process` 为 Restarting 或 Failed 时，显示进程状态提示条（Key `acp-process-banner`）。
- **更早的历史**：`older_unavailable` 为真，并且已经滚到最早一条时，在列表顶部显示 `l10n.acpOlderUnavailable`（Key `acp-older-unavailable`）。
- **外部写入与重载**：
  - 收到 `acp/session/changed` 时，显示"有新内容，点击刷新"标签（Key `acp-reload-chip`）；点击后调用 `appThreadReload`，再重新读取历史。
  - 收到 `acp/session/generation` 时，直接重新读取历史，做法和断线重连一样。
- **排队失败**：收到 `acp/queue/failed` 时，弹出 SnackBar，写明有几条排队消息没有发出，并提供"复制原文"操作。

#### 4.7.6 BridgeApi 与 Fake

- `bridge_api.dart`：
  - 新增 Dart 模型：`AcpServeResult`、`AcpAuth`、`AcpAuthMethod`、`AcpAgent`、`AcpAgents`、`AcpJob`、`AcpSettings`、`AcpBinaryOverride`、`AcpDataMode`、`AcpSaveResult`、`AcpCustomAgent`、`AcpEnvVar`、`AcpConfigOption`（含 `role`）、`AcpConfigValue`、`AcpCommand`。字段和 §4.4.5 的 DTO 一一对应，名称改成 lowerCamelCase。
  - 为 §4.4.5 的每个函数加一个抽象方法，命名规则和现有方法相同（例如 `appServeStartAcp({String? name, required String agentId})`）。
  - `AppCapabilities` 按 §4.4.4 增加字段；`codex` 和 `openCode` 两个常量补上新字段的取值；新增 `static const acpDefault`，所有可选能力都为 false。
- `bridge_api_rust.dart`：逐个字段映射到 FRB，写法同 `:234`、`:260`。
- `test/fake_bridge_api.dart`：
  - 新增 `List<AcpAgent> acpAgents`、`Map<String, AcpJob> acpJobs`、`AcpAuth acpAuth`，以及 `final List<(String?, String)> acpServeCalls`。
  - `appServeStartAcp` 的行为：名字被非 ACP 主机占用时抛 `StateError`；否则生成键 `pcx:local:acp:$n`，并加入一个 `AppServeStatus(provider: 'acp', agentId: …, agentName: …)` 和一个 `ServiceEntry(kind: 'acp')`。
  - `appCapabilities` 按 kind 返回：`isAcpKey` 时返回 `acpCaps`（可在测试里配置，默认是一份启用了全部 ACP 能力的配置），`isOpenCodeKey` 时返回 `openCode`，其余返回 `codex`。

#### 4.7.7 文案（`lib/l10n/app_en.arb` 和 `app_zh.arb` 同步新增）

- **托管与 agent**：`providerAcp`、`acpAgentLabel`、`acpAgentsTitle`、`acpManageHost`、`acpInstall`、`acpInstalling`、`acpInstallFailed`、`acpUpgrade`、`acpUninstall`、`acpUninstallInUse`
- **agent 状态**：`acpNotInstalled`、`acpUnsupportedPlatform`、`acpEngineMissing`、`acpEngineIncompatible`（占位符 `found`、`required`）、`acpRegistryNewer`（占位符 `version`）、`acpRegistryInstallWarning`、`acpUnpinned`
- **远程管理**：`acpRemoteManagement`、`acpRemoteManagementHint`、`acpRemoteDisabled`、`acpHostStart`
- **登录**：`acpLogin`、`acpLoginOnHost`、`acpRecheck`、`acpAuthRequired`
- **模型网关与订阅登录**：`acpGateway`、`acpGatewayConfigured`、`acpGatewayConfigure`、`acpGatewayUnsupported`、`acpGatewayMethod`、`acpGatewayUrl`、`acpGatewayToken`、`acpGatewayTokenKept`、`acpGatewayProvider`、`acpGatewayHeaders`、`acpGatewayNote`、`acpGatewayInsecureHttp`、`acpGatewayFailed`（占位符 `message`）、`acpSubscriptionLogin`、`acpSubscriptionLoginConfirmTitle`、`acpSubscriptionLoginConfirmBody`、`acpRestartToApply`
- **设置与自定义 agent**：`acpCustomAgent`、`acpCustomCommand`、`acpCustomArgs`、`acpCustomEnv`、`acpCustomEnvNote`、`acpNpmRegistry`、`acpCodexBinary`、`acpClaudeEnginePath`、`acpDataMode`、`acpDataAuto`、`acpDataShared`、`acpDataIsolated`、`acpSharedDataWarning`
- **会话界面**：`acpOptionConfirmTitle`、`acpOptionConfirmBody`、`acpUrlTitle`、`acpUrlOpen`、`acpUrlDecline`、`acpQueueFailed`（占位符 `count`）、`acpOlderUnavailable`、`acpSessionChanged`、`acpSessionNotLoadable`、`acpStopRunningConfirm`、`acpProcessRestarting`、`acpProcessFailed`

## 5. 配置与状态 schema

### 5.1 `<state_dir>/acp/agents.toml`（新文件，权限 0600）

```toml
schema = 1
remote_management = true
# npm_registry = "https://registry.npmmirror.com/"

[agents.claude-acp]
# engine_path = "/opt/homebrew/bin/claude"   # D7(b)：CLAUDE_CODE_EXECUTABLE
allow_subscription_login = false             # D21：false 时带 --hide-claude-auth

[agents.claude-acp.gateway]                  # D20：可选；配置后 Hub 每次启动自动网关登录
base_url = "https://relay.example.com"
token = "sk-…"                               # 以 Authorization: Bearer <token> 发送；只在本机保存
# method_id = "gateway"                      # 缺省为 agent 返回的第一个 gateway 类方法
# provider_name = "new-api"                  # codex-acp 用作 provider 名称
# extra_headers = { "X-Custom" = "…" }

[agents.codex-acp]
# codex_binary = "/usr/local/bin/codex"      # 优先于 config.toml 的 [codex] binary 和 PATH

[agents.opencode-acp]
# binary = "/path/to/opencode"               # archive 类 agent 的可执行文件覆盖

[data]
opencode-v1 = "auto"                         # auto | shared | isolated（D19）
opencode-v2 = "auto"

[[custom]]
id = "gemini"                                # ^[a-z][a-z0-9-]{0,63}$，不能和清单 id 重名
name = "Gemini CLI"
command = "/opt/homebrew/bin/gemini"
args = ["--acp"]
env = {}
```

- Rust 结构体 `AcpSettings` 用 `#[serde(default)]`，**不加** `deny_unknown_fields`，这样老版本能读新版本写出的文件。
- 保存时，先把磁盘上的文件读成 `toml::Value`，只改写本方案定义的键，其余键原样保留，然后写临时文件再 rename。这样老版本保存设置时，不会把新版本加的键弄丢。文件不存在时使用默认值。
- 修改 `remote_management`、自定义 agent、网关配置和 `allow_subscription_login` 都要写审计日志；审计记录只写"改了哪一项"，不写地址以外的值，更不写密钥。

### 5.2 `<state_dir>/acp/installed.json`（新文件，权限 0600）

```json
{"schema":1,
 "node":{"version":"24.21.0","path":"node/v24.21.0/node-v24.21.0-darwin-arm64"},
 "agents":{
   "claude-acp":{"version":"0.84.0","kind":"npm","path":"agents/claude-acp/0.84.0","source":"catalog",
                 "integrity":"<lockfile 的 sha256>","agentVersion":"0.84.0","installedAt":"2026-10-01T00:00:00Z"}},
 "gcPending":["agents/claude-acp/0.83.0"]}
```

### 5.3 `ui_state.json`

只新增 `autoHostAcp` 键（§4.7.4），已有的键不变。

### 5.4 不改动的文件与迁移

- 不改动：`config.toml`、`state.toml`、`pocket-codex-host.json`、`pocket-codex-threads.json`。
- 迁移：所有 ACP 数据都放在新文件里，不需要迁移。
- 降级：老版本 App 会忽略 `acp/` 目录；但老版本重写 `ui_state.json` 时会丢掉 `autoHostAcp`，之后再升级回来，需要重新托管一次。这一点写进已知限制。

## 6. 错误处理

错误码有三个用途，格式都是 `[acp.xxx] message`：
- 作为 `AcpError::code()` 的返回值和 HTTP 错误体里的 `code`；
- 作为 Hub 发给控制器的 JSON-RPC 错误 message 的前缀（T16）；
- 作为 FRB 错误信息的前缀。

界面用 `friendlyError` 去掉前缀后显示，再用 `acpErrorCode` 按错误码选择下表的文案（§4.7.1）。

| 错误码 | 场景 | 界面文案（zh） | 处理 |
|---|---|---|---|
| `acp.not_installed` | 托管时 agent 还没安装 | 该 agent 尚未安装 | 显示安装按钮 |
| `acp.unsupported_platform` | 平台或 libc 不支持 | 当前系统暂不支持该 agent | 建议改用自定义 agent |
| `acp.engine_missing` | 找不到 codex | 未找到 Codex，请先安装 Codex 或指定路径 | 跳转到 Codex 设置 |
| `acp.engine_incompatible` | Codex 版本不在支持范围内 | 本机 Codex {found} 不在支持范围 {required} | 有建议版本时显示"安装匹配的适配器" |
| `acp.protocol_unsupported` | 协商出的版本不是 1 | 该 agent 使用了不支持的 ACP 版本 | 结束 |
| `acp.agent_start_failed` | 进程启动失败 | agent 启动失败 | 附上 stderr 的最后 2 KiB |
| `acp.initialize_timeout` | 60 s 内没有完成 initialize | agent 启动超时 | 同上 |
| `acp.agent_unavailable` | 进程正在重启或已失败 | agent 正在重启 / 已停止 | 显示提示条，可以重试 |
| `acp.auth_required` | agent 返回 `-32000` | 需要登录 | 显示登录区 |
| `acp.host_only` | 远程发起 terminal 登录 | 该登录方式只能在主机上完成 | 显示说明 |
| `acp.session_not_loadable` | agent 不能恢复这个会话 | 该 agent 不支持打开历史会话 | 会话只读，显示空历史和说明 |
| `acp.generation_changed` | 翻页时 generation 变了 | —（内部） | 重新读取 |
| `acp.timeout` | 请求超时 | 请求超时 | 可以重试 |
| `acp.images_unsupported` | agent 不接受图片 | 该 agent 不支持图片 | 保留草稿 |
| `acp.download_failed` | 网络或重定向被拒 | 下载失败 | 可以重试 |
| `acp.integrity_mismatch` | 完整性校验不一致 | 校验失败，已放弃安装 | 不自动重试 |
| `acp.archive_rejected` | 压缩包内容不安全 | 安装包内容异常 | 结束 |
| `acp.npm_failed` | `npm ci` 失败 | 依赖安装失败 | 附上日志末尾 |
| `acp.disk_space` | 磁盘空间不足 | 磁盘空间不足（需要约 {n} MB） | 结束 |
| `acp.validation_failed` | 装完后握手失败 | 安装后自检失败 | 保留旧版本 |
| `acp.in_use` | 卸载或删除时仍有实例在用 | 仍有托管实例在使用 | 先停止托管 |
| `acp.job_running` | 同一个 agent 已有任务在跑 | 正在安装中 | 显示进度 |
| `acp.remote_management_disabled` | 主机关闭了远程管理 | 主机已关闭远程管理 | 在主机上打开开关 |
| `acp.version_not_pinned` | 远程安装未锁定的版本，或注册表条目不可安装 | 该版本不能远程安装 | — |
| `acp.no_terminal` | Linux 上找不到终端程序 | 未找到终端程序，请手动运行以下命令 | 显示可复制的命令 |
| `acp.agent_error` | agent 返回的其他 JSON-RPC 错误 | agent 返回错误：{message} | 显示原文 |
| `acp.session_loading` | 会话还在加载时请求翻页 | 正在加载历史… | bridge 等 `_pcx/session/loaded` 后重试 |
| `acp.cwd_required` | 新建会话时既没传 cwd，主机也没有默认项目 | 请先选择项目目录 | 打开项目选择 |
| `acp.unknown_agent`、`acp.unknown_job` | 远程管理路由收到未知 id | 主机上没有这个 agent / 任务 | 刷新列表 |
| `acp.name_conflict` | 远程开始托管时名字被占用（只出现在 HTTP 里） | 该名字已被主机上的其他实例使用 | 换一个名字 |

本机托管时的名字冲突，仍然用 bridge 的原文（"`{name}` is already hosting … on this device"），不加前缀。

## 7. 安全

- **网络暴露**：
  - Hub 的 WebSocket 和 meta 服务都只监听回环地址，远程访问只能经 relay。
  - Hub 的 WebSocket **本身没有鉴权**，这和现有的 Codex app-server、OpenCode 网关一样：能访问 relay 上这个键的人，就能驱动 agent。账号模式下，relay 凭据只能访问本账号的 `pcxu:<user>:…` 命名空间；自托管模式依赖 `MSG_HEADER_KEY`。
- **远程管理**：
  - 远程只能安装清单锁定的版本、启动已安装的 agent。
  - 自定义命令、注册表版本和设置修改，都只能在主机桌面上操作。
  - 主机上有开关控制，默认开启（D14）。所有写操作都写审计日志。
  - 路由里没有任何能执行任意命令的入口。
- **供应链**：
  - 只走 HTTPS，并限定主机白名单。
  - 用 SRI 校验完整性。npm 类 agent 用 lockfile 锁定全部依赖，并加 `--ignore-scripts`。
  - 使用私有的 npmrc 和缓存。
  - 不静默升级。
  - 解压时检查路径穿越、链接越界、总大小和条目数。
- **文件访问**：
  - agent 的 fs 请求只能访问会话目录，并拒绝符号链接逃逸。
  - 上传的附件由 meta 服务写入 `acp/uploads/<instance>`（与现有 `/uploads` 路由相同）。图片通过 `data:` URL 随 prompt 发送，Hub 不读取任何上传文件。
- **登录**：
  - terminal 登录只在主机上执行 agent 自己的程序，参数逐个转义。
  - 旧写法的 command 必须与 agent 程序同名。
  - Pocket-Codex 不实现任何账号登录界面，也不接触凭据。
- **凭据与日志**：
  - 不读取 agent 的凭据文件。
  - `OPENCODE_SERVER_PASSWORD` 每次启动随机生成，不写日志，也不进 `installed.json`。
  - 审计日志不记录环境变量的值。
  - stderr 日志可能包含用户内容，文件权限设为 0600。
  - `agents.toml` 里自定义 agent 的环境变量以明文保存，界面上会说明。
- **合规**（D8、D21）：
  - 默认配置下，Claude 适配器带 `--hide-claude-auth` 启动，Pocket-Codex 不提供 Claude.ai 订阅登录，推荐的用法是模型网关或 API key（D20）。
  - 用户在主机桌面上主动打开订阅登录时，只会启动未修改的 Claude Code 自带登录，并且事先显示条款提示。这个选项是否合规，仍以法务意见为准，但它不影响默认发布。
  - M10 时在 README 的 ACP 章节里写明：用户需要遵守各 agent 和所用模型网关的使用条款。
- **网关密钥**（D20）：
  - 以明文保存在主机的 `agents.toml`（权限 0600）。
  - 只在 Hub 调用 `authenticate` 时，通过 stdio 发给本机的 agent 进程。
  - 不写日志，不进审计，不经 FRB 读回，不经远程管理路由返回。
- **资源上限**：

  | 项 | 上限 |
  |---|---|
  | agent 单行消息 | 16 MiB |
  | 单个会话的转录 | 4 000 条 / 48 MiB |
  | Hub 缓存的转录 | 16 个 / 256 MiB |
  | 控制器 WebSocket 消息 | 64 MiB |
  | 控制器出站队列 | 4 096 条 |
  | 下载 | 1 GiB |
  | 解压 | 3 GiB / 20 万条目 |
  | npm 安装 | 20 分钟 |
  | fs 读 / 写 | 16 MiB / 32 MiB |

## 8. 测试方案

### 8.1 测试替身

- **Fake agent**：新建 `crates/pocket-codex-host-svc/src/acp/testing.rs`，用新 feature `acp-testing` 门控，写法是 `#[cfg(any(test, feature = "acp-testing"))]`。
  - 提供 `FakeAgent::spawn(script: FakeScript) -> (DuplexConnector, FakeHandle)`，它在 `tokio::io::duplex` 上跑一个脚本化的 ACP agent。
  - `FakeScript` 可以配置：initialize 的响应、会话列表（每页条数）、每个会话回放时发出的 update、是否要求认证，以及收到 prompt 后执行的步骤列表。
  - 步骤包括：`Update(Value)`、`RequestPermission { options }`（阻塞，直到收到回应）、`Elicit(Value)`、`Sleep(ms)`、`Finish(stopReason)`、`Crash`。
  - `FakeHandle` 能取到 agent 收到的全部请求，并能在测试里主动触发崩溃。
  - `crates/pocket-codex-bridge/Cargo.toml` 的 `[dev-dependencies]` 加 `pocket-codex-host-svc = { workspace = true, features = ["acp-testing"] }`。host-svc 自己的集成测试通过 §4.2 里的自依赖拿到这个 feature。
  - 用到暂停时钟的测试加 `#[tokio::test(start_paused = true)]`，依赖 host-svc 的 dev-dependency `tokio` 的 `test-util` feature。
- **真实进程**：`crates/pocket-codex-host-svc/tests/fixtures/acp/fake_agent.py`，只用 Python 标准库。
  - 实现 `initialize`、`session/list`、`session/new`，`session/prompt` 回显后返回 `end_turn`。
  - 带参数 `--spawn-child` 时，额外启动一个 sleep 60 s 的子进程，用来验证整个进程树都被结束。
  - 测试开始时检查 `python3` 是否存在，找不到就打印原因后跳过。
- **历史 fixture**：
  - M2 手写 Claude、Codex、OpenCode 形状的回放序列，放在 `tests/fixtures/acp/replays/*.jsonl`，每行一条 `session/update` 的 params。形状依据 research §3 引用的源码。
  - M10 实测后，用录到的真实序列替换（只录只读的 load 回放，内容做脱敏）。

### 8.2 各层用例

| 层 | 文件 | 用例 |
|---|---|---|
| core | `src/acp/*` 单元测试、`tests/acp_schema.rs` | §4.1.4 全部 |
| Hub | `crates/pocket-codex-host-svc/tests/acp_hub.rs` | 见下 |
| 安装器 | `crates/pocket-codex-host-svc/tests/acp_install.rs` | 见下 |
| bridge 托管 | `engine/serve_acp.rs` 的单元测试 | 见下 |
| bridge 引擎 | `engine/acp/engine_tests.rs`、`mapping.rs` 的单元测试 | 见下 |
| Flutter | `test/acp_*.dart` | 见下 |

**Hub**（`tests/acp_hub.rs`）：
- `initialize_negotiates_v1_and_reports_caps`
- `rejects_protocol_other_than_v1`
- `list_merges_pages_and_sorts_by_updated_at`
- `attach_loads_once_for_concurrent_callers`
- `attach_prefers_resume_when_transcript_is_current`
- `attach_returns_loading_after_20s_then_notifies_loaded`（暂停时钟）
- `submit_to_unloaded_session_loads_then_prompts`
- `submit_while_running_queues_and_drains`
- `duplicate_client_submission_is_idempotent`
- `error_responses_carry_pcx_code_prefix`
- `update_meta_carries_item_for_tool_and_plan`
- `authenticate_returns_in_progress_then_broadcasts_state`
- `hub_level_elicitation_is_sent_to_every_connection`
- `cancel_answers_pending_permission_with_cancelled`
- `permission_first_valid_answer_wins_and_others_get_cancel_request`
- `invalid_option_is_resent_with_new_id_and_pending_kept`
- `attach_snapshot_seq_orders_against_live_notifications`
- `pending_is_resent_to_late_subscriber`
- `form_and_url_elicitation_route_by_client_mode`
- `elicitation_complete_resolves_url_pending`
- `crash_fails_running_turn_clears_pending_and_restarts_with_backoff`
- `five_crashes_in_five_minutes_enter_failed`
- `auth_required_detected_from_list_and_new`
- `legacy_terminal_auth_requires_matching_command`
- `gateway_authenticate_runs_before_ready`：FakeAgent 返回带 `_meta.gateway` 的方法。设置了网关时，每次启动（包括重启后）都在第一个会话操作之前，收到带 `baseUrl` 和 `Authorization` 头的 `authenticate`
- `restart_rereads_launch_provider`：修改 `LaunchProvider` 的返回值后调用 `restart()`，新进程用的是新参数和新网关
- `gateway_failure_reports_required_without_leaking_token`
- `fs_read_write_confined_to_session_cwd_and_rejects_symlink_escape`
- `oversized_line_becomes_notice_item`
- `idle_session_is_closed_after_ten_minutes`（用 tokio 的暂停时钟）
- `history_source_windows_items_and_groups`
- `divergent_reload_broadcasts_generation`
- `slow_consumer_is_disconnected_with_1013`
- `ws_rejects_non_loopback_listener`
- `process_connector_spawns_and_terminates_tree`（真实进程，依赖 python3）

**安装器**（`tests/acp_install.rs`）：
- `embedded_catalog_parses`
- `platform_key_and_baseline_selection`
- `musl_is_unsupported`
- `download_verifies_sha256_and_sha512`
- `integrity_mismatch_deletes_file`
- `redirect_to_unlisted_host_is_rejected`
- `archive_rejects_traversal_symlink_escape_and_size_bombs`
- `npm_install_builds_expected_command`（通过 `NpmRunner`）
- `validation_failure_keeps_previous_version`
- `upgrade_defers_gc_while_hosted`
- `codex_release_selected_by_engine_version`
- `remote_install_requires_toggle_and_pinned_version`
- `audit_log_rotates_and_omits_env`
- `settings_round_trip_preserves_unknown_keys`
- `validation_runs_from_staging_dir_with_empty_cwd`
- `shared_data_mode_returns_coexistence_warning`
- `conditional_args_add_hide_claude_auth_by_default`：参数只出现在 `launch_only_args` 里
- `terminal_login_excludes_launch_only_args`：spec 写法和旧写法的登录命令里都没有 `--hide-claude-auth`
- `gateway_token_is_write_only_and_not_audited`
- `gateway_url_requires_https_except_private_networks`
- `in_use_predicate_blocks_uninstall_and_defers_gc`

下载类的测试用一个本地 HTTP 服务配合 `allow_loopback_http = true`。

**bridge 托管**（`serve_acp.rs` 单元测试）：
- `start_registers_meta_then_acp`
- `name_reported_by_other_hosting_is_refused`（通过 `StartDeps.other_hosting` 注入）
- `is_hosting_is_true_after_start_with`（Codex 和 OpenCode 那一侧的冲突检查只是调用 `serve_acp::is_hosting`，由这条用例加上 §8.4 第 14 项覆盖）
- `same_name_same_agent_is_reused`
- `stop_shuts_down_hub_and_forgets_host`：停止后 `is_hosting` 为 false，FakeAgent 观察到 stdin 已关闭。注销本身依赖 `Published::stop`，没有测试构造函数，由 §8.4 第 1 项和第 14 项人工覆盖
- `status_reports_agent_fields`

**bridge 引擎**：
- `engine_tests.rs`：
  - `connect_reads_caps_and_emits_capabilities_event`
  - `thread_list_maps_session_info`
  - `thread_read_maps_items_and_turns`
  - `older_page_and_turn_page_use_windows`
  - `turn_start_applies_config_then_submits`
  - `stream_emits_started_delta_completed_in_order`
  - `permission_maps_to_approval_and_decisions_map_to_options`
  - `form_elicitation_round_trip`
  - `url_elicitation_accept_and_decline`
  - `interrupt_sends_cancel`
  - `reconnect_reattaches_and_resubmits_unknown_submission`
  - `chunks_merge_by_hub_item_id_beyond_tail`
  - `thread_read_waits_for_loaded_notification`
  - `thread_start_without_cwd_uses_default_project`
  - `turn_page_delta_only_returns_new_items`
  - `pcx_code_is_parsed_from_error_message`
  - `generation_change_emits_event`
  - `usage_maps_to_token_usage_shape`
  - `steer_rename_compact_diff_are_unsupported`
- `mapping.rs` 单元测试：
  - `hub_item_mapping_table`，逐行覆盖 §4.5.1
  - `stop_reason_mapping`
  - `config_option_roles`

**Flutter**：
- `test/acp_hosting_test.dart`：
  - 选中 ACP 后显示 agent 列表
  - 安装进度和失败提示
  - 开始托管后记住 autoHostAcp
  - 需要登录时显示登录区
  - 停止前的确认
  - 已有 ACP 主机显示 agent、版本和键，并且没有 API 这一行
- `test/acp_agents_screen_test.dart`：
  - 本机模式：安装、升级、卸载、设置、自定义 agent 的增删改、共享数据确认
  - 远程模式：远程管理关闭时禁用操作，安装和开始托管的调用
- `test/acp_session_test.dart`：
  - 补充、重命名、压缩、diff、Fast、权限预设都隐藏
  - Plan 按能力显示
  - 配置面板的读取和设置
  - slash 菜单
  - `acpOptions` 按钮和确认框
  - URL 卡片
  - 登录提示条
  - older_unavailable 提示
  - reload 标签
  - 排队失败的 SnackBar
- `test/acp_hosts_test.dart`：
  - 首页和服务页显示 ACP 主机及其标签
  - 冷启动时恢复 autoHostAcp
  - 远程设备的"管理 ACP agent"入口
- `test/service_key_test.dart`：新增 `acp` 相关用例
- 现有的 Codex 和 OpenCode 测试全部保持通过

### 8.3 实测（M10，逐项征得用户同意）

- **只读**：`PCX_ACP_LIVE=<清单 id> cargo test -p pocket_codex_bridge acp_live -- --nocapture --test-threads=1`
  - 前提是本机已安装并托管了对应的 agent。
  - 执行内容：initialize、list、打开一个已有会话（load 或 resume）、翻页、history sync。
  - 不创建会话，不发 prompt。
- **写入**：再加上 `PCX_ACP_LIVE_WRITE=1`。
  - 在 `$TMPDIR/pocket-acp-e2e` 下新建测试会话，每个 agent 最多发 3 条很短的 prompt。
  - 覆盖：流式输出、运行中排队、权限拒绝、停止执行。
- **安装**：在用户同意后，在 macOS 上用 App 安装全部 4 个清单 agent，记录每个用了多久、占多少磁盘。

### 8.4 人工检查项（M10）

| # | 检查项 |
|---|---|
| 1 | 桌面：托管对话框选 ACP → 安装 Claude Code → 看到进度 → 开始托管 → 主机名旁显示"Claude Code"标签 |
| 2 | 需要登录时：点登录 → 系统终端打开 agent 自带的登录 → 完成后自动重新检测，状态变为已登录 |
| 3 | 会话列表里能看到终端里直接用 CLI 创建的会话（Claude、Codex），打开后历史完整、能翻页 |
| 4 | 发消息：流式输出、工具卡片、审批按钮按 agent 给的选项显示，拒绝后 agent 继续 |
| 5 | 运行中再发一条：进入队列，上一轮结束后自动发出 |
| 6 | 停止执行后，服务仍然可用 |
| 7 | 模型和推理强度选择器可用；通用配置面板能切换 mode |
| 8 | 手机断网 30 s 再恢复：运行中的轮次没有中断，待办重新出现 |
| 9 | 两台控制器同时打开同一个会话：一边审批，另一边的卡片消失 |
| 10 | 安卓：在服务页进入"管理 ACP agent"→ 远程安装 OpenCode 2.x → 开始托管 → 打开会话 |
| 11 | 关闭远程管理开关后，安卓上的安装按钮被禁用，并显示原因 |
| 12 | Codex-ACP：本机 codex 版本不兼容时提示升级，兼容时能正常工作 |
| 13 | 重启 App：autoHostAcp 自动恢复托管 |
| 14 | Codex、OpenCode 原生 provider 回归：托管、打开会话、发送都和以前一样 |
| 15 | 模型网关（D20）：给 Claude Code 和 Codex（ACP）分别配置 New API 网关后，不做任何订阅登录也能正常对话；密钥不出现在日志、审计和远程设备上；删掉网关配置后，界面回到需要登录的状态 |
| 16 | 订阅登录（D21）：默认情况下登录区没有 Claude.ai 登录；在高级设置里打开并确认条款后，重启 agent，登录区出现订阅登录 |

## 9. 里程碑

每个里程碑结束时执行下面这组命令（即 `AGENTS.md` §7），记作"§7 全量"：

```bash
export PATH=$HOME/.cargo/bin:$PATH
cargo fmt --check \
  -p pocket-codex-core -p pocket-codex-codex -p pocket-codex-pb \
  -p pocket-codex-api-proxy -p pocket-codex-host-svc -p pocket-codex-cli \
  -p pocket_codex_bridge -p pocket-codex-account-proto -p pocket-codex-store \
  -p pocket-codex-auth -p pocket-codex-backend
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
python3 scripts/check_mobile_dependencies.py
cd apps/flutter && fvm flutter pub get \
  && dart format --output=none --set-exit-if-changed lib test integration_test \
  && fvm flutter analyze && fvm flutter test
```

- `check_mobile_dependencies.py` 用 `cargo tree --target aarch64-linux-android` 检查安卓产物的依赖边界，不需要 NDK，本机也能跑。从 M3 起，它用来确认桌面专用的依赖没有漏进移动端。
- 改了 bridge 的 FRB API 时，先在 `apps/flutter` 目录下执行 `flutter_rust_bridge_codegen generate`（2.12.0）。
- 改了清单时，还要执行 `python3 scripts/acp_catalog.py check`。
- 移动端编译检查：现有的 `check_mobile_dependencies.py` 只跑 `cargo tree`，不会编译；唯一的安卓构建在 `release.yml:516`，只在打 tag 或手动触发时运行。桩模块在桌面上根本不编译，所以 M6 要在 `.github/workflows/ci.yml` 里新增一个 `android-check` job：
  - 运行在 `ubuntu-latest` 上，这个镜像预装了 Android NDK，路径在 `$ANDROID_NDK_LATEST_HOME`。
  - 先执行 `rustup target add aarch64-linux-android`。
  - 再设置环境变量 `CC_aarch64_linux_android=$ANDROID_NDK_LATEST_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang` 和 `AR_aarch64_linux_android=$ANDROID_NDK_LATEST_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar`。
  - 最后执行 `cargo check -p pocket_codex_bridge --target aarch64-linux-android --locked`。
  - 从 M6 起，每个里程碑的 PR 都要看这个 job 的结果。本地没有 NDK 时可以不跑，以 CI 为准。
  - 把这个 job 加进 `ci.yml` 里汇总结果的 `ci-result` 的 `needs`。在 `scripts/ci_affected.py` 的判断里，只要 bridge、core、host-svc 有改动就运行它。
  - CI 全局设置的 `RUSTFLAGS=-D warnings` 对这个 job 同样生效，所以桩模块在安卓目标上也不能有任何警告，例如未使用的参数要以 `_` 开头。

| M | 目标 | 要改的文件 | 验收 | 依赖 |
|---|---|---|---|---|
| M0 | 文档 | `docs/acp-integration/research.md`、`TRD.md` | 用户确认；冷启动检验没有阻塞问题 | — |
| M1 | 新 kind `acp` 全链路打通（但界面还不会打开它） | `core/src/service.rs`、`core/src/config.rs`、`cli/src/commands/ui.rs`、`account-proto/src/key.rs`（测试）、`backend/src/api.rs`、`bridge/src/engine/account.rs`、Flutter 的 `service_key.dart`（只改 `_isKind`、新增 `isAcpKey`）及其测试 | §4.1.1 和 §4.6 的测试通过；老 kind 的测试不变；§7 全量通过 | M0 |
| M2 | core 的 ACP 类型、折叠、`_pcx` 共享类型和契约测试 | `core/src/acp/*`（含 §4.1.5 的 `pcx.rs`）、`core/src/lib.rs`（加 `pub mod acp`）、`core/tests/acp_schema.rs`、`core/tests/fixtures/acp/**`。复制 schema fixture 需要访问 GitHub raw（公开、只读），按 §3.1 固定的 commit 下载 | §4.1.4 全部通过；§7 全量通过 | M1 |
| M3 | Hub 核心：对端、进程、会话、待办、fs、认证，以及用 `HubConnection` 实现的完整协议（T18） | `host-svc/src/acp/{mod,error,launch,peer,process,session,hub,pending,fs,auth,testing}.rs`、`host-svc/Cargo.toml`（只加 `[features]`、nix 的 `signal`/`fs` 和 dev-dependencies，安装器依赖留到 M5）、`host-svc/src/lib.rs`（`pub mod acp`，带平台门控） | §8.2 Hub 用例中除 `history_source_windows_items_and_groups`、`slow_consumer_is_disconnected_with_1013`、`ws_rejects_non_loopback_listener` 以外的全部通过，这些用例直接通过 `open_connection` 驱动；§7 全量通过 | M2 |
| M4 | WebSocket 适配层、历史源和 meta | `host-svc/src/acp/{server,history,meta}.rs`、`host-svc/src/lib.rs`（提取 `generic_app`，`serve_generic` 行为不变） | §8.2 Hub 用例全部通过；OpenCode 和 Codex 的 host-svc 测试不变 | M3 |
| M5 | 安装器、清单和远程管理路由 | `host-svc/src/acp/install/**`、`host-svc/Cargo.toml`（`tar`、`zip`、`flate2`、`semver`、`toml`、`sha2`、`base64` 在这一步加入）、`host-svc/src/lib.rs`（在 `generic_app` 和 Codex `serve()` 两处 `.with_state` 之后 merge `manage::router()`）、`scripts/acp_catalog.py`、`scripts/check_mobile_dependencies.py`（禁止列表加 `tar`、`zip`）、`.github/workflows/ci.yml`（加 check 步骤） | 分两步验收：<br>(a) 代码：提交一份占位清单 `catalog.toml`，内容只有 `schema = 1`、`generated_at` 和 `agents = []`，没有 `[node]`；`locks.rs` 为空表。§8.2 安装器用例全部通过（用测试自带的小清单），`acp_catalog.py check` 通过（占位清单也要能通过）。<br>(b) 数据：维护者执行 `acp_catalog.py update`（需要网络，执行前征得用户同意），生成 §3.4 的 4 个 agent 和 Node 的真实哈希与 lockfile，并提交；`check` 通过 | M3 |
| M6 | bridge 托管和本机或远程管理的 FRB | `core/src/acp/pcx.rs`（新增 `AcpSettingsView`、`CustomAgentDef`）、`bridge/src/engine/{serve_acp,acp_terminal,acp_manage,acp_desktop_stub}.rs`、`engine/serve.rs`（移入 `bind_loopback`、分发、冲突检查）、`engine/serve_opencode.rs`（冲突检查）、`engine/mod.rs`、`api/bridge.rs`（§4.4.5 的托管与管理部分、DTO，以及在 `init_bridge`（`:62`）里调用 `acp_manage::register()`）、`crates/pocket-codex-bridge/Cargo.toml`（dev-dependency：host-svc 的 `acp-testing`）、`engine/acp/mod.rs`（先只放 `AcpServeReport`）、FRB 重新生成、Dart 的 `bridge_api.dart`、`bridge_api_rust.dart`、`fake_bridge_api.dart` | §8.2 bridge 托管用例通过（通过 `start_with` 注入依赖，不需要账号和 relay，T19）；Flutter 能编译，现有测试通过；新增的 CI `android-check` job 通过（`.github/workflows/ci.yml` 在这个里程碑修改） | M4、M5 |
| M7 | bridge 引擎、分发、能力和历史缓存 | `bridge/src/engine/acp/**`、`engine/mod.rs`（声明 `acp`、`app_events`）、`engine/meta.rs`（新增 `project_config_at(base: &Url)`，供 `connect_url` 的 `meta_url` 使用；现有按服务键解析地址的函数改为调用它）、`engine/app_events.rs`（T15）、`engine/opencode/events.rs`（改为从 app_events 导入）、`engine/session_sync.rs`、`engine/app_session.rs`（`older_unavailable`）、`api/bridge.rs`（§4.4.3 分发、§4.4.4 能力、§4.4.5 的会话部分）、FRB 重新生成、Dart 的 BridgeApi 同步 | §8.2 bridge 引擎用例通过（通过 `connect_url` 连接进程内的 Hub 和 meta，T19）；OpenCode 引擎测试不变 | M6 |
| M8 | Flutter 托管与管理界面 | §4.7.1（`providers.dart` 除外）到 §4.7.4、§4.7.6、§4.7.7 涉及的文件，包括 `service_key.dart`（`isSessionKind` 加 `'acp'`）、`error_format.dart`、`hosting_support.dart`、`widgets/local_host_acp.dart`、`router.dart`、`settings_screen.dart`、`services_screen.dart`、`home_screen.dart`、`ui_prefs.dart`、`screens/acp_agents_screen.dart` | §8.2 中 `acp_hosting_test`、`acp_agents_screen_test`、`acp_hosts_test`、`service_key_test` 通过；现有 widget 测试通过 | M7 |
| M9 | Flutter 会话界面 | §4.7.5 涉及的文件，包括 `app_session_screen.dart`、`composer_cards.dart`、`screens/app_session/acp_config_panel.dart`、`acp_slash_menu.dart`，以及 `providers.dart`（§4.7.1 的运行中清单和预取） | `acp_session_test` 通过；现有会话测试通过 | M8 |
| M10 | 实测、文档和发布前检查 | 本文 §13 的验证记录、`CONTEXT.md`（§12 的术语）、`AGENTS.md`（路线图新增第 16 条）、`README.md`（Status 表和 ACP 章节）、替换实测的回放 fixture | §8.3 和 §8.4 全部完成；§7 全量通过；macOS 构建（CI）通过；安卓 APK 在真机上作为控制器完成 §8.4 第 10、11 项；默认配置下 `claude-acp` 带 `--hide-claude-auth` 启动（D21），界面上不出现 Claude.ai 登录；用用户的 New API 网关完成 §8.4 第 15 项（需要用户提供网关，并征得同意） | M9 |

每个里程碑的"接口"和"测试用例"，分别以 §4 和 §8.2 中对应的小节为准。

## 10. 风险与已知限制

| 风险 | 缓解 |
|---|---|
| codex-acp 运行时不检查 Codex 版本，版本不配时只会报透传的错误 | 安装和托管前按 `engine_range` 检查（§4.3.8），并给出建议的适配器版本 |
| Claude 适配器自带的引擎和用户的 `claude` 共用 `~/.claude`，版本可能不一致 | 提供 D7(b) 高级选项；M10 实测两个版本交替使用 |
| Anthropic 条款的边界 | 默认带 `--hide-claude-auth`，不提供订阅登录（D21）；用户主动打开订阅登录时才会涉及，是否合规以法务意见为准（D8） |
| 网关登录用的 `auth._meta.gateway` 不在 ACP 规范里，适配器升级后可能改变形状 | 按能力处理：没有 gateway 类方法就不显示入口；每次清单升级时，核对两个适配器的 `GatewayAuthMeta`；用户在 agent 自己的配置文件里写的网关继续可用 |
| Codex（ACP）经 New API 使用时，要求网关支持 Codex 使用的 OpenAI Responses 接口 | 用户确认（2026-09-30）其 New API 中转站支持 `/v1/responses`；M10 第 15 项仍要做端到端实测 |
| 全量回放体积很大（codex-acp #516） | 转录预算，超出后整轮删除并显示 `older_unavailable`；load 超时 300 s |
| v1 的 `messageId` 可选且不保证跨 load 稳定，重载后 id 可能变化 | 用 generation 机制兜底：id 变化时换 generation，控制器重新读取窗口 |
| OpenCode ≥2.0.4 的 `session/new` 缺少用户配置（#50236） | M10 实测。如果影响使用，就在清单 release 上加 quirk（例如固定 2.0.3），不在代码里写针对 OpenCode 的分支 |
| OpenCode 1.x 子代理的权限请求不转发（#48232） | 写进已知限制；界面上的停止执行可以恢复 |
| OpenCode 1.x 和 2.x 共用数据库，2.x 首次运行会改写 1.x 的数据 | D19 |
| npm lockfile 跨平台可能装错平台包 | 用 npm ≥ 11.11 生成 lockfile，安装时带 `--os`、`--cpu`、`--libc`；M10 在 CI 的三个平台上各做一次安装和握手 |
| 账号模式需要新的 backend 才会列出 ACP | 发布说明里写明；老 backend 不会报错 |
| Linux 上找不到终端程序 | 显示可复制的命令 |
| 同一个会话同时在终端和 Hub 里写 | 靠轮询 updatedAt 提示刷新，界面写明不保证一致性 |

已知限制（M10 写进 README）：
- 不支持补充（steer）、重命名、压缩、git diff、子会话。
- 回放出来的轮次没有时间和时长。
- 超出转录预算的更早历史不可见。
- 只支持 glibc 版 Linux。
- 降级后 `autoHostAcp` 会丢失。
- terminal 登录只能在主机上完成。

## 11. D19：OpenCode-ACP 的数据库隔离（2026-09-30 确认采用 (a)）

背景（研究时读源码确认，未实测）：
- OpenCode 1.x 和 2.x 默认共用同一个数据目录和 `opencode.db`。
- 2.x 首次运行时会清空 `event` 表，并改写已迁移会话的 `session_message`。之后再用 1.x 打开这个库，行为未经验证。
- 两个版本都支持用 `OPENCODE_DB` 单独指定数据库文件，登录信息和配置继续共用。

| 选项 | 说明 |
|---|---|
| **(a) 自动（推荐）** | 主机上检测到的用户 OpenCode 大版本（先查 PATH，再查 `~/.opencode/bin/opencode`，取 `--version` 的主版本号）与 agent 的 `data_family` 相同时，共用用户的数据库，能看到已有会话；不同或检测不到时，设 `OPENCODE_DB=<acp>/data/<family>/opencode.db` 隔离。主机桌面上可以手动切换；切到"共享"而大版本又不一致时，弹风险确认框 |
| (b) 始终隔离 | 最安全，但看不到用户已有的 OpenCode 会话，违背 R2 |
| (c) 始终共享 | 历史最全，但可能破坏用户的数据 |

以本机为例：本机装的是 OpenCode 2.0.18，所以按 (a)，`opencode2-acp` 会共用数据库，`opencode-acp`（1.x）会隔离。

施工细节（`install/resolve.rs`）：
- `agents.toml` 的 `[data]` 里，某个数据族的值为 `auto` 时，每次启动 agent 都重新检测一次用户的 OpenCode 大版本，检测命令超时 5 s。
- 检测结果和 `data_family` 的对应关系：`opencode-v1` 对应主版本 1，`opencode-v2` 对应主版本 2。
- 值为 `shared` 时，不设置 `OPENCODE_DB`；值为 `isolated` 时，设置 `OPENCODE_DB=<acp>/data/<data_family>/opencode.db`，并事先创建这个目录，权限 0700。
- 无论选哪一项，安装后的自检都使用临时数据库（§4.3.9）。

## 12. 术语与文档更新（M10）

`CONTEXT.md` 新增以下术语：
- **自有托管（Owned Hosting）**：主机启动并拥有 provider 进程，停止托管时结束该进程。ACP 托管采用这种方式。
- **ACP Hub**：主机上唯一的 ACP 客户端，把一个 agent 进程里的会话复用给多个控制器。
- **Agent 清单（Catalog）**：随 App 发布、锁定了版本和完整性的可安装 agent 列表。
- **托管目录（Managed Directory）**：`<state_dir>/acp/`，App 安装的 agent 和私有 Node 都放在这里。
- **转录（Transcript）**：Hub 从 ACP 更新中物化出的、有预算上限的会话条目序列。

`CONTEXT.md` 修改以下术语：
- **服务提供方**：说明提供方可以是"通过 ACP 接入的 agent"。在界面上，ACP 实例显示 agent 的名称。

`AGENTS.md` §9 新增第 16 条路线图，写明 ACP 通用接入的范围、D1–D19 的要点，并链接到本文。README 的 Status 表新增一行 ACP。

## 13. 验证记录

施工完成后，按 OpenCode TRD §7 的格式，在这里补写自动验证结果、实测结果和人工检查结果。

### 13.1 施工进度

分支 `feat-acp`（从 `research-acp` 的 `7461280` 拉出）。每个里程碑结束时本地跑完 §9 的"§7 全量"命令块（改清单时加跑 `python3 scripts/acp_catalog.py check`，改 FRB API 时先重新生成绑定）；"CI 全部通过"指 draft PR #4 上除 Claude Code Review 以外的检查，M6 起包括 `android cargo check`。

| M | commit | 新增的测试 | 验证 | 偏差 |
|---|---|---|---|---|
| M1 | `a5a3257` | `acp_keys_round_trip_alongside_existing_services`（core）、`acp_keys_round_trip_without_merging_account_namespaces`（account-proto）、`acp_services_are_listed_only_on_request`（backend）、`service_key_test.dart` 的 "ACP keys parse…" | §7 全量通过（2026-09-30）；CI 全部通过 | 无 |
| M2 | `3c235c1` | §4.1.4 的 16 个单元测试（`rpc.rs`、`update.rs`、`transcript.rs`），另加 `known_variants_round_trip`、`process_state_uses_state_tag_and_camel_case_fields`；契约测试 `tests/acp_schema.rs`：`every_used_method_exists_in_meta`、`message_samples_round_trip_and_match_schema`（45 条样例）、`validator_rejects_undeclared_properties_and_bad_enums`、`opencode_2_0_18_initialize_deserializes`、`replay_fixtures_fold_into_transcripts` | §7 全量通过（2026-10-01）；CI 全部通过 | 见 §13.2 的 M2 条目 |
| M3 | `42dd121` | `tests/acp_hub.rs` 中除 M4 的 3 个用例以外的全部 32 个（含 `process_connector_spawns_and_terminates_tree`，用 `tests/fixtures/acp/fake_agent.py`）；单元测试 `display_code_and_rpc_agree`、`backoff_doubles_and_caps`、`fifth_failure_within_window_gives_up`、`permission_answers_are_validated`、`form_answers_stay_within_schema`、`line_and_limit_select_lines`、`quoting_and_redaction`、`sessions_sort_newest_first_with_missing_last` | §7 全量通过（2026-10-01）；`acp_hub` 连续跑 6 次都通过；CI 全部通过 | 见 §13.2 的 M3 条目 |
| M4 | `1c0488a` | `history_source_windows_items_and_groups`（同时经 `serve_meta` 读 `/history/v1/capabilities` 和 `/healthz`）、`slow_consumer_is_disconnected_with_1013`（真实 WebSocket）、`ws_rejects_non_loopback_listener`；`acp_hub` 共 35 个用例，连续跑 3 次都通过 | §7 全量通过（2026-10-01）；CI 全部通过 | 见 §13.2 的 M4 条目 |
| M5(a) | `2cb3e14` | `tests/acp_install.rs` 的 §8.2 安装器用例全部 21 个，另加 `management_routes_answer_404_until_registered`；单元测试 `ids_follow_the_pattern`。占位清单 `catalog.toml` 和空的 `locks.rs` 通过 `acp_catalog.py check` | §7 全量通过（2026-10-01） | 见 §13.2 的 M5 条目 |
| M5(b) | `1c027a2` | `embedded_catalog_parses` 现在覆盖真实清单：4 个 agent、5 个 release、3 个 lockfile（都是 `lockfileVersion: 3`，`resolved` 全部来自 `https://registry.npmjs.org/`）；`acp_catalog.py check` 通过；`acp_catalog.py verify-node` 用 gpgv 和 Node.js 发布密钥校验了 SHASUMS256.txt 的签名，并核对 6 个 Node 包的哈希 | `acp_catalog.py update` 于 2026-10-01 执行（临时目录在 `$TMPDIR/opencode` 下，用完已删除）；§7 全量通过；CI 全部通过 | 见 §13.2 的 M5(b) 条目 |
| M6 | `d541d89` | bridge 托管：`start_registers_meta_then_acp`、`name_reported_by_other_hosting_is_refused`、`is_hosting_is_true_after_start_with`、`same_name_same_agent_is_reused`、`stop_shuts_down_hub_and_forgets_host`、`status_reports_agent_fields`；`acp_terminal` 的 `scripts_quote_every_argument`。FRB 重新生成；Dart 的 `BridgeApi`、`RustBridgeApi`、`FakeBridgeApi` 加上托管与管理接口；CI 新增 `android cargo check` job | §7 全量通过（2026-10-01）；CI 全部通过，含 android-check | 见 §13.2 的 M6 条目 |
| M7 | `d7595d6` | `engine/acp/engine_tests.rs` 的 §8.2 用例全部 19 个（经 `connect_url` 连进程内的 Hub、`serve_ws` 和 `serve_meta`；重连用例经一个可断开的 TCP 代理）；`mapping.rs` 的 `hub_item_mapping_table`、`stop_reason_mapping`、`config_option_roles`，另加 `approvals_and_forms_round_trip`；`acp_keys_are_recognized_in_both_namespaces`。`live_tests.rs` 只在设置 `PCX_ACP_LIVE` 时运行，本轮没有运行。OpenCode 引擎测试未改动并通过；FRB 重新生成，Dart 的 `BridgeApi` 同步 | §7 全量通过（2026-10-01）；ACP 引擎用例连续跑 3 次都通过；CI 全部通过 | 见 §13.2 的 M7 条目 |
| M8 | `6b51c9e` | `test/acp_hosting_test.dart`（7 个：agent 列表、安装进度和失败、开始托管并记住 autoHostAcp、需要登录时的登录区、已配置网关时收起其他登录方式、停止前的确认、已有主机的 agent/版本/键且没有 API 行）、`test/acp_agents_screen_test.dart`（9 个：本机安装/升级/卸载、注册表版本警告、设置与共享数据/共存确认、网关只写密钥、保留已存密钥、订阅登录开关、自定义 agent 增删改、远程管理关闭时禁用、远程安装与开始托管）、`test/acp_hosts_test.dart`（6 个：首页打开 ACP 主机、冷启动恢复 autoHostAcp、autoHostAcp 的读写与合并、本机 ACP 主机卡片与能力行、远程 ACP 行的标签、远程设备的管理入口）、`service_key_test` 补充 `isSessionKind('acp')`；现有 widget 测试全部通过 | §7 全量通过（2026-10-01）；CI 全部通过 | 见 §13.2 的 M8 条目 |
| M9 | `df88bae` | `test/acp_session_test.dart`（12 个：补充/重命名/压缩/diff/Fast/权限预设隐藏、Plan 与配置面板按能力显示（桌面弹出层）、手机配置面板入口、没有 Plan 时不显示高级入口、slash 菜单、`acpOptions` 按钮与"始终"确认、URL 卡片、登录提示条、进程状态提示条、older_unavailable 提示、reload 标签与 generation 重读、排队失败的 SnackBar）；现有会话测试（含 OpenCode 的运行中清单）全部通过 | §7 全量通过（2026-10-01）；CI 全部通过 | 见 §13.2 的 M9 条目 |

### 13.2 施工偏差

实现时与本文不一致、但不影响 D1–D21、T1–T19、对外契约和安全模型的地方，逐条记在这里。

- M2：已知 `type`/`sessionUpdate` 的内容块、工具内容或 update，如果字段对不上对应变体，也保存为 `Unknown(Value)` 并原样序列化，而不是让整条消息反序列化失败。
- M2：`Transcript` 在 §4.1.3 的接口之外多了 `turn_info`、`live_turn`、`push_notice`（Hub 插入 notice 用）；同一个 `messageId` 在不同轮次重复出现时，新条目的 id 加 `~n` 后缀保证唯一。
- M2：会话级通知的参数结构体（`SessionLoadedParams` 等）都带 `seq`，与 §4.2.6"会话级通知都带 seq"一致；`RequestResolvedParams.session_id` 为可选，留给 Hub 级待办。
- M2：只带 `_meta` 的响应（`AuthenticateResponse` 等）在契约测试里用 `CapabilityMarker` 往返。
- M3：Hub 的全部状态放在一把同步互斥锁里，持锁期间不 await；状态变更和它引起的通知在同一次加锁里写进各连接的出站通道，所以通知顺序与折叠顺序一致，attach 的快照与通知按 `seq` 有序。§4.2.4 写的"会话自己的串行任务"由这把锁代替，效果相同。
- M3：`AgentPeer::start` 不返回 inbound 接收端，改为接收一个同步回调，在读循环里按行处理完再读下一行。这样 `session/prompt` 的响应不会跑在它之前的 `session/update` 前面（否则 `_pcx/turn/completed` 可能早于最后几条更新）。peer 只保留同步的 `notify_now`、`respond_now`。
- M3：`hub.rs` 拆成 `hub.rs`（进程监管、连接表）、`ops.rs`（控制器方法、会话加载、排队、空闲回收、LRU）、`inbound.rs`（agent 发来的消息、待办路由、崩溃清理）三个文件；`GatewayAuth` 定义在 `auth.rs` 并从 `acp` 模块导出。
- M3：首次启动（`AcpHub::start`）失败时直接返回错误，不进入自动重启；自动重启只在曾经 Ready 过的进程退出后发生。
- M3：`session/prompt` 因进程退出而失败时，由退出清理统一以 `_pcx_agent_exited` 结束轮次，prompt 任务本身不再结束它。
- M3：网关登录分三种结果：成功（状态 ok，跳过认证检测）、`authenticate` 失败（required，message 为"网关登录失败：…"，密钥替换为 `***`，跳过检测）、agent 没有网关方法（照常检测，message 保留"该 agent 没有提供网关登录方式，网关配置未生效"）。
- M3：Hub 自己开始的轮次会额外广播一条 `session/update`（`user_message_chunk`，`_meta.pcx.item` 为完整的用户条目），让其他控制器也能显示这条用户消息；超长行插入的 notice 以 `sessionUpdate: "_pcx_notice"` 广播，`_meta.pcx.item` 为 notice 条目。
- M3：收到 `elicitation/complete` 时，仍在等待的 URL elicitation 以 `{"action":"accept"}` 回应 agent。
- M3：`HubSession` 的待办统一存在 Hub 级的表里（带 `session_id`），没有按会话分表；`HubSession` 另加 `replay`、`load_rx`、`baseline_pending`、`materialized`、`last_access` 字段。会话按 `updatedAt` 排序时用 chrono 解析 RFC 3339，host-svc 因此新增对工作区已有的 `chrono` 的依赖（不引入新 crate）。
- M4：`serve_meta` 和 `serve_ws` 一样，监听地址不是回环时直接报错。`metadata` 集合按 §4.2.10 把元数据放在窗口的 `metadata` 里，不产生文档。`items` 集合额外支持 `group = "t{turn}"`，只返回该轮的条目（与 Codex 适配器的 `group` 语义一致）。`groups` 接受 `projection` 为空、`desc` 或 `summary`，`asc` 报错。
- M4：host-svc 的 dev-dependencies 新增工作区已有的 `tokio-tungstenite`，用于 WebSocket 传输层测试。
- M5：`zip` 只开 `deflate-flate2`（不带压缩用的 zopfli），`tar` 关掉默认的 `xattr`；实际新进入 `Cargo.lock` 的只有 `tar`、`zip`、`filetime`、`typed-path`，都只在桌面目标上编译，`check_mobile_dependencies.py` 已禁止 `tar`、`zip` 进入安卓依赖树。
- M5：`InstallContext` 多一个 `opencode_major` 注入点（D19 的 `auto` 需要知道用户 OpenCode 的主版本），生产环境检测 PATH 和 `~/.opencode/bin/opencode`，测试里返回固定值，从而不会执行用户的 opencode。
- M5：`AcpSettingsView`、`AgentFlagView`、`GatewayView`、`CustomAgentDef` 提前在 M5 加进 `core::acp::pcx`（§9 写的是 M6），因为设置的读写接口和 §8.2 的设置类用例都在 M5。`claude_engine_path` 和 `codex_binary` 分别对应清单里带 `engine_override` 和 `external_engine` 的 agent，代码里不写 agent 名。
- M5：`ensure_node` 和下载的进度回调是 `Fn(state, bytes, total)`，不是 `Fn(JobProgress)`；任务表里的 `JobProgress` 由 `jobs.rs` 统一更新。`archive.rs` 另有 `extract_with_limits`，供测试使用较小的上限。
- M5：解压时，条目路径经过已存在的符号链接（"穿过链接写入"）一律拒绝；全部写完后再逐个 `canonicalize` 新建的符号链接，解析到目标目录以外的也拒绝。
- M5：审计记录多一个可选字段 `item`，只写设置项的名字（如 `gateway`、`allow_subscription_login`），不写值。
- M5：注册表版本的 npm 安装用 `node_modules/<pkg>/dist/index.js` 作为入口；注册表 archive 安装把 `cmd`、`args` 记进 `installed.json`（`InstalledAgent` 新增 `entry`、`cmd`、`args`，只在 `source = "registry"` 时写入）。`agents_status` 只读注册表缓存，`registry_hint::refresh` 由 bridge 在后台调用（M6），测试不访问网络。
- M5(b)：本机 PATH 上的 npm 是 10.9.3，低于 lockfile `libc` 字段要求的 11.11，脚本按设计改用从 nodejs.org 下载、并用 SHASUMS256.txt 校验过的 Node 24.21.0 自带的 npm 11.19 生成 lockfile（与安装器运行 `npm ci` 的 npm 同一版本），只在临时目录里执行。脚本新增 `verify-node` 子命令和 gpgv 签名校验（密钥环取自 nodejs/release-keys）；PATH 上没有 gpgv 时只打印警告。
- M5(b)：OpenCode 1.18.33 的 9 个 GitHub 包都已下载并计算 sha256，与 GitHub API 的 `digest` 一致；压缩包里的可执行文件在根目录（`cmd = "opencode"`，Windows 为 `opencode.exe`）。OpenCode 2.0.20 的 9 个平台包只取 npm 元数据里的 `dist.integrity`（sha512），没有下载；`cmd` 按研究结论写作 `package/bin/opencode[.exe]`，M10 实测安装时确认。
- M5(b)：`jobs.rs` 的磁盘空间换算在 Linux 上触发 `clippy::useless_conversion`（`fsblkcnt_t` 在 macOS 是 u32、在 Linux 是 u64），只在 Linux 上对这个函数加了带 reason 的 allow。
- M6：`StartDeps` 多一个 `gateway` 注入点（生产环境读 `agents.toml` 的网关，测试返回 None）和 `after_stop`（生产环境在 Hub 停止后执行 `install::gc_sweep`，测试为 None，避免碰真实状态目录）；`resolve` 用 `Arc` 而不是 `Box`，因为 Hub 的 `launch` 闭包每次启动都要再调用它。
- M6：`AcpAuthMethodDto` 在 §4.4.5 的字段之外，补上 `gateway_protocol` 和 `gateway_configured`，与 `AuthMethodInfo` 一致，界面的网关登录区（§4.7.2）要用。
- M6：远程 `host` 只接受清单 agent；自定义 agent 运行的是用户自己的命令，只能在主机桌面上开始托管（对应 §7"自定义命令只能在主机桌面上操作"），远程请求返回 `acp.unknown_agent`。
- M6：`meta_acp_*` 把 `/acp/v1` 的 `{code, message}` 错误体转成 `[acp.<code>] message`；没有 JSON 错误体的 404（对方主机没有注册管理实现，例如旧版本）报"this host does not support remote ACP management"。
- M6：`scripts/ci_affected.py` 新增输出 `android`：全量运行，或者受影响的测试集合里包含 bridge（bridge 依赖 core 和 host-svc，所以这两个 crate 改动时也会触发）。
- M7：引擎返回 `acp::Capabilities`，`AppCapabilitiesDto` 在 `api/bridge.rs` 里组装。连上 Hub 之前按 §4.4.4 返回保守值；`multi_select_questions`、`approval_options`、`url_elicitation`、`session_reload` 只在连上之后为 true。
- M7：回应权限或 elicitation 之后，引擎立即删掉这条待办；如果 Hub 认为答案无效，会用新的请求 id 重发，界面就再显示一张卡片（带 `_meta.pcx.rejected`）。
- M7：重连时，每条待办记下送达它的连接代次。重新 attach 并重新提交之后，再发一次 `_pcx/sessions/running` 作为收尾往返：Hub 在回应它之前重发的请求，此时都已进入接收队列。处理完队列后，仍属于旧代次的待办视为在断线期间已经解决，删除并发出 `serverRequest/resolved`。重新提交被 Hub 拒绝时（不是断线），发出 `acp/queue/failed`。
- M7：`turn_start` 提交时如果连接断开，提交保留在"未确认"表里并返回成功，由重连后的重新提交完成（Hub 按 `clientSubmissionId` 去重）；其他错误照常返回。
- M7：`thread_reload` 在 generation 变化时由引擎自己发出 `acp/session/generation`：Hub 的 `_pcx/session/generation` 发生在重载快照之前，它的 `seq` 已包含在快照里，会被丢弃。`_pcx/session/window` 返回 `acp.generation_changed` 时也发出这个事件。
- M7：`running_sessions` 返回正在运行或有排队提示的会话。`AcpCommandDto.hint` 取自 `input.hint`。
- M7：§4.4.6 的 ACP 预取写在 `engine/acp/history.rs` 的 `prefetch_history`，由 `session_sync::prefetch` 在协商到 `acp/hub-v1` 时调用；能力缓存条目多记一个 provider，`session_sync::request` 遇到 ACP provider 时报错。
- M7：`live_tests.rs`（只在设置 `PCX_ACP_LIVE` 时运行）用已安装的 agent 在测试进程内启动 Hub、`serve_ws` 和 `serve_meta` 并通过 `connect_url` 连接，不需要先在 App 里托管，也不需要中继；Hub 的会话索引和日志写到临时目录。`PCX_ACP_LIVE_WRITE=1` 时在 `$TMPDIR/pocket-acp-e2e` 新建一个会话并发送一条短提示。
- M7：测试用的 FakeAgent 在脚本给了 `configOptions` 时，`session/set_config_option` 会更新对应项的当前值并返回全部选项（更接近真实 agent）。Dart 侧新增 `AppCapabilities.acpDefault`，`FakeBridgeApi` 新增 `acpCaps`、`acpConfig`、`acpCommands`、`acpAnswers`、`acpConfigSets`、`acpReloads`、`acpAuthState`。
- M8：服务页没有现成的"主机分组菜单"，远程设备的"管理 ACP agent"放在设备详情标题右侧新增的溢出菜单里（Key `host-menu`，菜单项 Key `host-acp-manage`），跳转到 `/settings/acp?svc=<该设备第一个会话服务键>`。放成按钮时窄屏会溢出。
- M8：`/settings/acp` 另外接受 `gateway=<agentId>`，加载后直接打开该 agent 的网关对话框（托管对话框里"配置模型网关"按钮用它）。
- M8：§4.7.7 之外新增的文案键：`acpManageAgents`、`acpUninstallConfirm`、`acpInstalled`、`acpInstalledVersion`、`acpPinnedVersion`、`acpSizeMb`、`acpHostedAs`、`acpSourceCustom`、`acpRegistryInstall`、`acpOtherLogins`、`acpGatewayAuto`、`acpGatewayProtocol`、`acpGatewayUrlHintAnthropic`、`acpGatewayUrlHintOpenai`、`acpGatewayClear`、`acpAdvanced`、`acpCustomAdd`、`acpCustomId`、`acpCustomName`、`acpCustomDelete`、`acpBinaryOverride`、`acpSettingsSaved`、`acpReload`。按 `acp.<code>` 选文案的逻辑集中在新文件 `lib/src/acp_errors.dart`。
- M8：托管对话框读取已有主机的登录状态用 `appAuthState`，只有这台控制器已连上该服务时才有值；没有值时只显示"重新检测"。开始托管返回 `auth.status == required` 时，对话框留在新主机的详情上，网关地址从 `acpSettings()` 读取。清单 agent 未安装时"开始托管"按钮禁用。
- M8：agent 管理页本机模式的 agent 列表只列清单 agent，自定义 agent 列在自己的卡片里；远程模式"开始托管"的 Key 为 `acp-host-btn`（按 §4.7.3），每个 agent 卡片各有一个。会话界面里的 ACP 标签（`app_session_screen.dart`）随 M9 修改。
- M9：所有 `acp/*` 事件在 `_onEvent` 开头由 `_onAcpEvent` 统一处理（`acp/elicitation/url` 也在这里加入待办，没有会话的 Hub 级卡片在每个打开的会话里显示）。`acp/config/updated` 让配置面板和 slash 菜单重新读取。
- M9：§4.7.5 写的"高级"入口里只有 Plan 一项，所以 `planMode` 为 false 时整个"高级"入口隐藏；手机端的配置表单多一项"Agent"（`opt-acp-options`），打开装着 `AcpConfigOptionsPanel` 的底部表单。
- M9：`_caps.steer` 为 false 时，异步问题的回答也改用 `appTurnStart`（Hub 会排队）。`_caps.images` 为 false 时，图片菜单项禁用、粘贴图片忽略；拖放文件的路径没有改，Hub 会以 `acp.images_unsupported` 拒绝图片。
- M9：登录提示条在本机桌面托管时提供 terminal（可用的）和 agent 类方法；远程时只提供 `remote` 的 agent 类方法，调用 `appAuthAuthenticate`。排队失败的 SnackBar 用已有的 `copy` 文案作为"复制原文"按钮。
- M9：会话界面里的 ACP 标签显示 agent 名称，放在 `Flexible` 里，名称长时和标题一起收缩，不会撑破侧栏的服务切换行。
- M9：`runningSessionInventoryProvider` 按 `appCapabilities(key).runningViaThreads` 走 `appRunningThreads`（OpenCode 和 ACP），`historyPrefetch` 为真（ACP）时按原来的轮换规则预取最多 2 个运行中会话；OpenCode 的行为不变。
- M1–M9 之后：新对话在第一次发送前拿不到模型等配置项（ACP 只在会话响应里给出配置项，Hub 原来只记在内存里）。新增 `_pcx/hub/defaults`、`defaults/<instance>.json` 持久化和一次性的隐藏探测会话（`host-svc/src/acp/defaults.rs`）；探测要求 agent 支持 `session/close`，否则只靠持久化。`session/set_config_option` 返回空配置项时不再清空默认值。
- M1–M9 之后：按 AGENTS.md"每完成一个里程碑就更新 README Status 表和路线图"的规则，README 的 Status 表加了一行 ACP、AGENTS.md 路线图加了第 16 条，两处都写明实测尚未完成。M10 的其余文档项（`CONTEXT.md` 的 §12 术语、README 的 ACP 章节、用实测回放替换 fixture）仍留给 M10。
