# TRD：OpenCode 复用统一托管与会话界面（分支 `fix-opencode`）

状态：v1.0（grill 评审通过：T1–T6 均按推荐确认）。需求见 [PRD](PRD.md)，决策见 [ADR-0002](../adr/0002-opencode-shared-session-ui.md)，术语见 [`CONTEXT.md`](../../CONTEXT.md)。

## 1. 总体架构

```
           主机（桌面 App，托管 OpenCode 实例）                         控制器（任意设备）
┌────────────────────────────────────────────────────────┐   ┌──────────────────────────────────────┐
│ OpenCode 后台服务 127.0.0.1:49374 (Basic, 用户已有)       │   │ Flutter 会话界面（同一套）             │
│        ▲ Basic 注入                                      │   │   │ BridgeApi app_* / meta_*           │
│ OpenCode 网关 (host-svc, 回环, 路由白名单)  ◄── pb 注册 ──┼───┼─► bridge: 按服务类型分发              │
│   key: …:opencode:<name>                                │   │     ├─ Codex 引擎（现有，不改）       │
│ meta 服务 (host-svc, 通用 fs/上传/文件链接)  ◄── pb 注册 ─┼───┼─►   └─ OpenCode 引擎（新增）          │
│   key: …:meta:<name>                                    │   │         映射成现有 DTO + 能力描述      │
└────────────────────────────────────────────────────────┘   └──────────────────────────────────────┘
```

- **分发的接缝**：在 `crates/pocket-codex-bridge/src/api/bridge.rs` 中，每个 `app_*` 函数先按服务键的 kind 分发，`opencode` 交给 `engine::opencode`，其他仍走原来的 Codex 代码。FRB 函数签名和 DTO 形态都不变，Codex 引擎不做重构。
- **OpenCode 引擎是一个深模块**：接口就是现有的 `app_*` 语义（列会话、读历史、翻页、开始/补充/中断、回复待办、事件流）。内部封装了 HTTP/SSE、轮次合成、条目映射、增量覆盖和缺口补齐。
- **主机侧不做语义翻译**：网关只负责回环监听、路由白名单、注入 Basic 凭据和限流。meta 服务复用通用路由。

## 2. 协议基线（本机实测 v2.0.18）

| 用途 | 路由 |
|---|---|
| 身份 / 契约 | `GET /api/info`、`GET /openapi.json` |
| 会话 | `GET/POST /api/session`（`directory`、`parentID=null`、`limit`、`cursor`、`order`）、`GET/PATCH /api/session/{id}`、`GET /api/session/active` |
| 消息 | `GET /api/session/{id}/message`（`limit`≤200、`cursor`、`order`、`type`） |
| 执行 | `POST …/prompt`（`text`、`files[{uri,name}]`、`delivery: queue\|steer`）、`POST …/interrupt`、`POST …/compact`、`POST …/model`、`POST …/agent` |
| 待办 | `GET /api/permission/request`、`POST …/permission/{rid}/reply {decision: once\|always\|reject}`、`GET /api/form`、`POST …/form/{fid}/reply {answer}`、`DELETE …/form/{fid}` |
| 目录 / 模型 | `GET /api/project`、`GET /api/model`、`GET /api/agent`、`GET /api/vcs/diff`（`location[directory]`） |
| 事件 | `GET /api/event`（SSE，易失、不回放；`server.connected`、`session.*`） |

契约检查：`REQUIRED_ROUTES` 加上关键 schema 指针。2.0.18 视为已验证；契约满足的其他版本放行，并标记为未验证（`verified=false`）；契约不满足则拒绝，并列出缺失项。

## 3. 模块设计

### 3.1 core（`pocket-codex-core`、`pocket-codex-account-proto`）

- `ServiceKind::OpenCode`，线上字符串为 `"opencode"`，同步更新 `FromStr`、`Display`、`parse_key`，以及 account-proto 的规范解析。
- 需要补分支的穷举 match：`cli/commands/ui.rs`、`core/config.rs`、`cli/.../services.rs`，以及 backend `own_services`（列出 opencode 服务）。
- 兼容性：旧客户端无法解析新 kind，会在发现阶段直接丢弃，不会误用。

### 3.2 host-svc：`src/opencode/`（移植参考分支的 v2 部分，并做精简）

| 文件 | 内容 | 来源 |
|---|---|---|
| `mod.rs` | `Error`/`Result`、`BasicCredentials`、`validate_origin` | 从参考分支 v1 中抽出 |
| `discovery.rs` | 读取 `service.json`（O_NOFOLLOW、0600、16 KiB、回环、pid/版本交叉校验） | 原样移植，版本检查改为走契约 |
| `contract.rs` | 用 `/openapi.json` 校验路由和 schema 指针，返回 `ContractReport{version, verified, missing}` | 移植后放宽版本规则 |
| `sse.rs` | `FrameBudget`（8 MiB）、帧解析、心跳、45 s 空闲超时 | 移植 |
| `forms.rs` | 表单答案校验和类型转换 | 原样移植 |
| `gateway.rs` | **新写**：axum 反向代理，只监听回环；白名单为 §2 表中的路由，外加 `/api/session/{id}/message/{mid}` 和 `/api/session/{id}/diff`；注入 Basic；请求体上限 32 MiB；SSE 流式透传；其他路由返回 404。**不开放** credential、integration、pty、shell、config、experimental、fs、worktree 等路由 | 新写 |
| `meta.rs` | `serve_opencode_meta(listener, host_store, resolver)`：只挂载通用路由（`/healthz`、`/fs/*`、`/uploads`、`/host/local-probe`、`/projects`），`/fs/thread-file` 通过 `SessionDirResolver` 查询 OpenCode 会话目录 | 新写，从 `lib.rs` 拆出通用路由 |

- `lib.rs` 把通用路由提取为 `generic_routes()`，Codex 的 `serve()` 行为保持不变。
- `/uploads` 的目录改为由调用方传入：Codex 仍用 `codex_home()/pocket-codex-uploads`，OpenCode 用 `paths::state_dir()/opencode-uploads`。

### 3.3 bridge：托管（`engine/serve_opencode.rs`，新增）

- 注册表 `opencode_hosts(): HashMap<name, OpenCodeServe>`，和 Codex 的 `hosts()` 分开。由于 meta 键是按实例名推导的（`…:meta:<name>`），同一台主机上 OpenCode 实例**不允许**与 Codex 实例同名（T6），两边都会检查对方的注册表；OpenCode 默认实例名为 `opencode`。
- `opencode_serve_start(name, binary_override)` 步骤：
  1. 要求已登录账号。
  2. `discovery` 探测后台服务。失败时解析二进制（显式路径 → 保存的配置 → PATH → `~/.opencode/bin/opencode`），执行 `opencode service start`（20 s 超时），然后再探测一次。
  3. 契约检查。
  4. 在回环地址上绑定网关和 meta。
  5. 先注册 meta（best-effort），最后注册 `opencode` 键（决定性的一步）；遇到冲突时回滚本地任务。
- 健康监控：每 15 s 探测一次 `/api/info`。上游 pid 变了（OpenCode 被重启）时，重新读取 `service.json` 并更新网关上游，不重新注册。
- 停止：只停网关、meta，并注销注册，**不碰 OpenCode 进程**。
- `serve_status()` 同时返回两个注册表的内容。DTO 追加字段 `provider`（默认 `"codex"`）和 `provider_version`、`provider_verified`；OpenCode 实例的 `app_service_key` 填 opencode 键，api 相关字段留空。
- 新增 FRB 函数：`app_serve_start_opencode`、`app_serve_stop_provider(provider, name)`；`app_serve_deregister/reregister` 用 kind=`opencode` 定位实例，并新增 `app_serve_reregister_provider(provider, name, kind)` 来区分同名实例的 meta。
- `serve::local_endpoints(key)` 扩展为可以匹配 opencode 键，本机连接走回环地址。

### 3.4 bridge：会话引擎（`engine/opencode/`，新增）

| 子模块 | 职责 |
|---|---|
| `client.rs` | 基于隧道的无凭据 v2 HTTP 客户端（移植自 V2Client，去掉目录绑定，改为按调用传入 directory）；JSON 上限 64 MiB，超时 30 s |
| `session.rs` | 按服务键建立的会话注册表：连接、SSE 任务、待办表、活跃集合、增量覆盖、每个线程的历史窗口 |
| `turns.rs` | **轮次合成**：从一条 user 消息开始，到下一条 user 消息之前为止，算作一轮；`turn_id` 取 user 消息 id；`completed_at` 和 `duration` 取这一轮最后一条 assistant 的 `time.completed`，以及 `idle` 消息。 |
| `mapping.rs` | 消息 → `ThreadItem`（见 §3.5）、会话 → `ThreadMeta`、模型 → `ModelInfo`，以及待办 → `AppEvent` |
| `events.rs` | SSE → Codex 形态的 `AppEvent`（见 §3.6），50 ms 合并一次；断线后指数退避重连，重连后做一次尾部刷新 |

**分发表**（`api/bridge.rs`）：

| `app_*` | OpenCode 实现 |
|---|---|
| `app_connect/disconnect/is_connected/probe*` | 走隧道或本机回环，调用 `GET /api/info` 并做契约检查 |
| `app_thread_list` | 按 `order=desc`、`parentID=null` 分页列出所有目录的会话，上限 500 条；`cwd` 取 `location.directory` |
| `app_thread_start(cwd,…)` | `POST /api/session {location:{directory}}`；不传 permissions（C13） |
| `app_thread_resume` | 不发网络请求（OpenCode 没有 resume 语义），只刷新待办 |
| `app_thread_read` | 读尾部窗口（`order=desc&limit=60`），合成轮次；轮次摘要来自 `type=user` 分页（上限 500）；`running` 来自 `/active` |
| `app_thread_older_page / turn_page` | 用游标向更早的内容翻页；跳到尚未加载的轮次时，最多连续翻 5 页，仍然没到就返回缺口，由界面给出显式重试 |
| `app_turn_start` | 按需依次调用 `/model`、`/agent`（Plan 用 `plan`，否则用 `build`；自定义 agent 保持不动），然后 `POST /prompt {delivery: queue}` |
| `app_turn_steer` | `POST /prompt {delivery: steer}` |
| `app_turn_interrupt` | `POST /interrupt` |
| `app_respond_approval` | `accept → once`、`acceptForSession → always`、`decline → reject` |
| `app_respond_user_input` | 按 `forms.rs` 把答案转换并校验后回复；答案为空表示取消（DELETE） |
| `app_model_list` | `/api/model`，过滤 `enabled`；`supported_reasoning_efforts` 取 `variants[].id`；`supported_service_tiers` 为空 |
| `app_set_thread_name / app_compact / app_git_diff` | `PATCH` title / `POST compact` / `GET /api/vcs/diff`，把结果拼成统一 diff 文本 |
| `app_rate_limits`、本机会话、强制恢复 | 返回“不支持”，界面按能力隐藏入口 |
| `app_history_cached / prefetch / sync_prepare` | 见 §3.7 |

**新增 FRB**：`app_capabilities(service_key) -> AppCapabilitiesDto`，同步调用，按服务键 kind 返回静态能力：

```
provider: "codex"|"opencode"
fast, permissionPresets, guardian, rateLimits, takeover, externalWriterMonitor,
localSessions, planMode, effortLabel("effort"|"variant"), approveAlwaysPersistsProject,
multiSelectQuestions, childSessions
```

### 3.5 条目映射（消息 → ThreadItem）

| OpenCode | item_type | title | text |
|---|---|---|---|
| `user` | `userMessage` | | `text`；`files` 中的图片放进 images |
| assistant `text` part | `agentMessage` | | text |
| assistant `reasoning` part | `reasoning` | | text |
| tool `shell` / `shell` 消息 | `commandExecution` | `input.command` | 输出，外加 `[exit N]` |
| tool `edit` / `write` / `apply_patch` | `fileChange` | path，或“N files” | metadata 里的 diff；没有 diff 时由 old/new 合成 |
| tool `todowrite` | `plan` | | `encode_plan` 格式 |
| tool `webfetch` / `websearch` | `webSearch` | url / query | JSON |
| tool `subagent` / `task` | `collabAgentToolCall` | agent: description | JSON，其中包含 `receiverThreadIds:[metadata.sessionID]`，用于打开子会话 |
| tool `question` | `dynamicToolCall` | question | 问题和答案 |
| 其他 tool（read、grep、glob、skill、mcp…） | `dynamicToolCall` | 工具名，外加主要参数 | JSON：input、content、error |
| `compaction` | `contextCompaction` | 状态（进行中时为 `inProgress`） | summary |
| `synthetic` / `system` / `idle` / `*-switched` | 不显示（idle 只用来计算轮次时长） | | |

- 条目 id：text/reasoning 用 `msgID:kind:ordinal`，tool 用 `msgID:toolID`，保证增量覆盖和历史里的 id 一致。
- 工具状态映射：`streaming/running` 显示为运行中，`error` 在 text 末尾追加 `[error] …`。

### 3.6 事件映射（SSE → AppEvent）

| OpenCode 事件 | AppEvent.kind |
|---|---|
| `session.execution.started` | `turn/started`（`turnId` 取当前轮的 user 消息 id） |
| `session.execution.succeeded/failed/interrupted` | `turn/completed`（`turn.status`：completed / failed / interrupted） |
| `session.text.started/delta/ended` | `item/started`、`item/agentMessage/delta`、`item/completed` |
| `session.reasoning.*` | 同上，item 类型为 `reasoning`，事件为 `item/reasoning/textDelta` |
| `session.tool.input.started`、`tool.called`、`tool.progress` | `item/started`（类型按 §3.5） |
| `session.tool.success/failed` | `item/completed` |
| `session.shell.started/ended` | `item/started` / `item/completed`（`commandExecution`） |
| `session.compaction.started/ended` | 对应的 `contextCompaction` 条目 |
| `session.usage.updated` | `thread/tokenUsage/updated`（context window 取模型的 `limit.context`） |
| `session.renamed` | `thread/name/updated` |
| `session.created` | 刷新侧边栏（`thread/started`） |
| `session.permission.asked` | server request，kind 为 `item/commandExecution/requestApproval`；raw 为 `{command: action + resources, cwd, reason: message, persistsProject: bool}`；`request_id` 取 permission id |
| `session.permission.replied`、`form.replied/cancelled` | `serverRequest/resolved`（界面移除对应卡片） |
| `session.form.created` | kind 为 `item/tool/requestUserInput`；raw.questions 由表单字段转换而来（boolean 变成“是/否”两个选项，multiselect 带 `multiSelect:true`，数字和字符串为自由输入，不支持的字段加提示） |

- 用户消息的回显不作为 AppEvent 发出，这和 Codex 界面忽略 userMessage 回显的行为一致。

### 3.7 历史缓存

- OpenCode 不走 `/history/v1`。控制器用 `session_cache` 的现有命名空间（服务键里带 kind，天然隔离），缓存每个会话最近一次读到的有界 `ThreadHistory` 快照：最多 200 条，总配额共用 512 MB。
- `app_history_cached` 读这份快照；`sync_prepare` 返回 false。
- 显示缓存时处于只读状态，并标出已过期；连接成功后用实时数据替换。

### 3.8 Flutter

- **服务键**：`service_key.dart` 识别 `opencode`；新增 `isSessionKind(kind)`（app 或 opencode），替换所有 `kind == 'app'` 的主机筛选（home、services、welcome、local_sessions）。
- **首页**：候选主机包括两种 kind，排序规则不变；在主机名旁显示提供方标识。
- **服务页**：会话能力分组包括 opencode，显示可达性（调用 `appProbe`，它会按 kind 分发）和提供方标识；OpenCode 实例不显示 API 行。
- **托管对话框**：顶部新增提供方分段选择器。选择 OpenCode 时，显示探测到的服务地址、版本和验证状态、二进制路径（仅在需要启动服务时使用），以及实例名；不显示端口、代理和内置引擎。已有实例的视图共用同一个组件。
- **`ui_prefs.dart`**：追加字段 `autoHostOpenCode {name, binaryOverride}`；启动时两种都会恢复。
- **会话界面**：在 `_AppSessionState` 中读取 `appCapabilities`。
  - 按能力隐藏：Fast、权限预设、Guardian、限额、接管 / 外部写入监控、本机会话、Codex 安装提示。
  - Plan 开关映射为 agent。
  - 推理强度选择器改名为“变体”。
- **待办卡片**：
  - `ApprovalCard`：当 `persistsProject` 为真时，“本会话允许”改为“始终允许（项目）”，并弹出二次确认。
  - `UserInputCard`：支持 `multiSelect`。
- **子会话**：`collabAgentToolCall` 卡片带有 `receiverThreadIds` 时，显示“查看子会话”，进入同一个会话界面的只读模式：顶部显示“子会话 · 只读”横幅，输入框禁用，可以返回父会话（T5）。
- **运行中会话**：`runningSessionInventoryProvider` 在 opencode 服务上改用 `appThreadList` 结果里的 running 标记（来自 `/active`，每 5 s 轮询一次），不调用 `metaSessions`。
- **文案**：共用区域改为中性表述，en/zh ARB 同步更新。

## 4. 迭代里程碑（每个都可以独立验证，每个一个或多个提交）

| M | 内容 | 验收 |
|---|---|---|
| M0 | PRD / TRD / ADR / CONTEXT | 评审通过 |
| M1 | `ServiceKind::OpenCode` 贯通 core、account-proto、backend、CLI、Dart | 单元测试：键的解析和往返、旧 kind 行为不变 |
| M2 | host-svc `opencode/`：discovery、contract、sse、forms、gateway、meta 拆分 | axum 假上游测试：鉴权注入、白名单、SSE 透传、契约放宽规则；Codex `serve()` 回归 |
| M3 | bridge 托管 `serve_opencode.rs` 和 FRB；Dart `BridgeApi` 与 Fake | 用假 OpenCode 做端到端测试：启动、注册、状态、停止时不杀进程 |
| M4 | bridge 会话引擎：映射、轮次、事件、分发、能力 | 基于真实 shape 的 fixture 做映射测试；假上游测试列表、读取、翻页、发送、中断、待办、SSE |
| M5 | Flutter：服务键、首页、服务页、托管对话框、能力开关、待办卡片、文案 | widget 测试（Fake）；Codex 现有测试全部通过 |
| M6 | 历史缓存、附件、文件链接、子会话、重命名 / 压缩 / diff | 单元测试和 widget 测试 |
| M7 | 实测、全量 CI、macOS 构建、PR | 见 §5 |

## 5. 验证方案

- 每个里程碑都跑 AGENTS.md §7 的全部命令（fmt 按 -p 列表，clippy `-D warnings`，test `--locked`，dart format / analyze / test）。
- **只读实测**：对本机后台服务做发现、契约、列表、历史和 SSE 订阅。
- **写入实测**（T4）：使用本机 OpenCode 服务及其默认模型，在临时目录 `$TMPDIR/pocket-opencode-e2e` 中新建测试会话，发送 3–5 条很短的提示词，覆盖流式输出、排队 / 补充、权限（回复 reject）和中断。
- **构建**：macOS 桌面 `fvm flutter build macos`。

## 6. 风险

| 风险 | 缓解 |
|---|---|
| `files[].uri` 是否支持 `data:` 图片没有实测 | 优先上传到主机，改传 `file://` 路径；实测后再决定 |
| SSE 易失，事件会丢 | 重连后做尾部刷新，并对执行状态做 `/active` 对账 |
| 轮次摘要需要列出所有 user 消息 | 上限 500，超出部分标记 `has_older` |
| OpenCode 升级导致契约变化 | 契约检查明确报出缺失项；fixture 使用实测 OpenAPI |
| 网关绕过路由白名单 | 按“方法 + 路径模板”精确匹配，并加测试覆盖 |

## 7. 验证记录（2026-09-28）

**自动验证**：AGENTS.md §7 的全部命令都通过，结果为 Rust 395 passed / 0 failed，Flutter 612 passed，analyze 无问题，fmt 无改动。

**实测**：直接调用 App 用到的同一套引擎函数，对接本机 OpenCode 2.0.18 后台服务，走真实账号中转。使用默认免费模型，只在 `$TMPDIR/pocket-opencode-e2e` 下新建会话，共发送 5 条提示词。

| # | 项 | 结果 | 证据 |
|---|---|---|---|
| 1 | 托管 | 通过（引擎层） | `opencode:opencode` 和 meta 都已注册，版本 2.0.18，状态为已验证；界面上的显示由 widget 测试覆盖 |
| 2 | 自动恢复 | 通过（CI 构建的真实 App） | 在 `ui_state.json` 中临时写入 `autoHostOpenCode` 后启动 App，日志显示 `opencode:opencode` 与 `meta:opencode` 已在中转注册；验证后已恢复原文件。截图无法捕获窗口（缺少屏幕录制权限），因此界面显示未经目视确认 |
| 3 | 会话列表与翻页 | 通过 | 15 个根会话、9 个目录；读取到 85 条、10 个轮次，更早一页 90 条；另外通过中转临时隧道成功握手 |
| 4 | 新建与流式 | 通过 | 收到 7 次增量，`turn/completed` 状态为 completed，回复正确 |
| 5 | 排队与补充 | 通过 | 运行中补充（steer）返回轮次 id，排队的 prompt 也被接受 |
| 6 | 权限拒绝 | 通过 | 本机 OpenCode 默认全部放行，所以给测试会话单独设置规则 `pocket-e2e: ask`；审批卡片出现，拒绝后待办清零。表单（布尔 + 多选）也回答成功 |
| 7 | 停止执行 | 通过 | `turn/completed` 状态为 interrupted，服务仍可达 |
| 8 | 离线缓存 | 通过 | 停止托管后缓存有 18 条，状态为非运行中；`opencode service status` 仍返回服务地址 |
| 9 | 双托管 | 通过 | Codex `default` 和 OpenCode `opencode` 同时运行并都能打开；两个方向的同名托管都被拒绝 |
| 10 | Codex 回归 | 通过（不含模型调用） | 双托管时 Codex 能连接并列出 187 个线程；Codex 的自动化测试全部通过；没有实发 Codex 模型轮次，以免消耗 Codex 额度 |

复现命令（`live_*` 测试，按需启用）：

    PCX_OPENCODE_LIVE_SUPPORT="$HOME/Library/Application Support/io.github.ackingyou.pocketCodex" \
      cargo test -p pocket_codex_bridge opencode_live -- --nocapture --test-threads=1

**已知限制**：
- 会话链接只能访问会话目录和项目根目录下的文件。
- 运行中排队的 prompt，要等下一次读取才能得到准确的轮次 id。
- 桌面目视确认（2026-09-28，CI 构建的真实 App）：启动后自动恢复 OpenCode 托管并打开该实例，主机名旁显示 “OpenCode” 标识。侧边栏按目录（quiet-pixel、stellar-star）列出 OpenCode 已有会话，最近的会话能打开并显示历史。模型选择器显示 `claude-opus-5-5-thinking · 极高`（即变体）；Fast 与权限预设不显示，而同一 App 里的 Codex 主机仍然显示它们。服务页，以及托管对话框、审批卡片、停止按钮的点击流程，已由用户在桌面 App 上手动确认（2026-09-28）。

**Android（控制器）**：OpenCode 支持走共享的 Dart 界面和桥接层，Android 不需要单独改代码；托管仍然只在桌面端（PRD §5.1）。临时 CI 为 arm64-v8a、armeabi-v7a、x86_64 三个 ABI 构建了 Release APK（run 36337018447），并通过 `scripts/verify_android_apk.py` 校验（签名、ABI、页对齐）。由于本仓库没有发布密钥，这批 APK 使用 debug 签名，覆盖安装已装的正式签名版本会失败，需要先卸载。尚未在真机上运行。
