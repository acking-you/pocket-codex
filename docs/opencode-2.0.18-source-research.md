# OpenCode 2.0.18 适配核查

日期：2026-09-27。状态：**已完成官方源码与隔离服务的只读核查；未实现 2.0.18 适配，未完成 Pocket 桌面连接验收。**

本文件纠正此前把 v1.18.32 的验证外推至用户 OpenCode 2.0.18 的问题。它补充而不覆盖 [v1.18.32 研究](opencode-api-research.md)；两套协议不能只通过增加 `/api` 前缀互换。

## 1. 证据和范围

- 官方仓库 `anomalyco/opencode` 的 `refs/tags/v2.0.18` 真实存在，`git ls-remote --tags` 与 GitHub Git Ref API 均指向 **`cd9a14a6b688d4021bee381dfd39d2cef9c0f862`**。以下源码链接固定到该 SHA，不使用浮动分支。GitHub REST `releases/tags/v2.0.18` 本轮返回 404，因此不把 release 记录或“最新发布”当作证据。[tag], [source-tree]
- 本机安装包元数据为 `ai.opencode.desktop`、版本 `2.0.18`，`Resources/opencode-cli.version` 同为 `2.0.18`。本轮只读核实了该安装包与其中编译产物，并运行其中的 CLI，未读取用户服务注册文件、供应商密钥或现有会话。
- 官方源码完整克隆于 `/tmp/opencode-2.0.18-research.Vf1XBN/source`，detached HEAD 等于上述 SHA。临时研究路径不是项目源码或交付物。
- **文档存在版本滞后**：同一 tag 的 `packages/web/src/content/docs/server.mdx` 仍列 `/global/health`、`/doc` 和可覆盖用户名的旧协议；其说法不能覆盖这一版真正构建的 `packages/server`、`packages/protocol` 与运行时接口。[old-server-doc], [api], [server-routes]
- **生成的 OpenAPI 也需交叉核查**：tag 中 `packages/protocol/openapi.json` 有 113 个 path，实际 2.0.18 服务 `/openapi.json` 有 115 个。运行时多 `/api/pair`、`/auth/connect/{code}`；二者已经出现在该 tag 的 `groups/server.ts`，不能仅按提交的生成文件判不存在。[server-group], [generated-openapi]

## 2. 结论摘要

1. 2.0.18 的服务识别为 `GET /api/info`，规范为 `GET /openapi.json`。`/api/global/health` 和 `/api/doc` **不是**这些旧路由的可用替代。本轮带正确认证实测均 404；未认证时的 401 来自前置鉴权，不能证明路由存在。[server-group], [web-ui], [server-process]
2. 原生会话、消息、事件、权限、问题和执行接口均发生了结构性变化，不是 URL 前缀修补。需要独立 v2 wire types 与行为适配，保留旧适配器和 Codex 路径。[session-group], [message-group], [event-group], [permission-group], [form-schema]
3. OpenCode 桌面采用官方共享后台服务机制。只读 `Service.discover` 是接入契约；`Service.ensure`、`Service.stop` 可能替换/终止用户服务，不能在 Pocket 的 attached 连接流程中调用。[desktop-service], [service-client]
4. 正规认证来源有用户输入服务密码、官方本机服务 discovery、用户明确操作的配对流程；它们与模型供应商凭据不同。不能从 `auth.json` 寻找服务密码，也不能为了连通而关闭服务认证。[service-client], [auth], [pairing], [desktop-pairing]

## 3. 桌面服务、发现和认证

### 3.1 桌面不是匿名的独占 sidecar

`packages/desktop/src/main/service/background-service.ts` 调用 `@opencode/client/service` 的 `Service.ensure`，启动命令包含 `serve --service`，可采用已有兼容后台服务。初次启动携带桌面 CLI 的版本条件；返回结果必须有 `auth.type === "basic"`。`0.0.0.0` 被规范化为用于本地连接的 `127.0.0.1`。[desktop-service]

主进程保存 endpoint/password，renderer 的 ready 数据只包含 URL。对精确匹配 sidecar origin 的请求，主进程加入 `Basic base64(opencode:password)`；其它 origin 不注入。因此从桌面 UI 看到 URL 并不意味着另一客户端可以匿名连接。[sidecar-credentials]

正式渠道 `service.json`、默认端口 `0xc0de` 即 `49374` 在 CLI 的 `service-config.ts` 明确定义；本机先前观测的 `49374` 与此一致，但端口本身不能证明进程身份或所有权。[service-config]

### 3.2 只读 discovery 可用，ensure 不可冒用

官方 `@opencode/client` 明确注释：服务注册文件包含 `url`、`pid`、`version`、私有服务 `password`，以 0600 权限写入，它是完整 discovery contract。默认位置为 `$XDG_STATE_HOME/opencode/service.json`，未设 XDG 时为 `~/.local/state/opencode/service.json`；CLI 其它 channel 可以选其它注册文件。**本次研究没有读取用户的这些文件。**[service-client], [service-registration], [service-config]

只读发现的必要流程：[service-client]

1. 读取并验证注册文件字段，构造 Basic `opencode:<password>`。
2. 对其 origin 请求 `/api/info`，验证 JSON 类型、`pid` 与注册文件一致，若注册文件有 `version` 也须一致。
3. 检查 readiness、调用者要求的版本兼容条件，返回 endpoint；发现失败返回不可用，不启动其它服务。

`Service.ensure` 除启动外，还可对版本不匹配服务执行 stop；持续超时恢复路径也会 terminate。`terminate` 先 SIGTERM，等待后可 SIGKILL，并删除仍匹配的注册文件。CLI `opencode pair` 内部也先调用 `Service.ensure`，所以它不是严格只读命令。**Pocket attached 模式不能直接复用 ensure/stop/pair CLI 作为“无副作用探测”。**[service-client], [cli-pair]

Pocket 若实现自动发现，应额外保护凭据边界：只在用户主动“连接本机 OpenCode”时使用此契约；校验文件类型、所有者/权限和注册 endpoint 为本机 loopback；拒绝跨 origin 重定向；凭据仅驻留 Rust 内存，不经 FRB 返回、不写日志/relay key/配置导出。以上为本项目安全建议，不声称上游 discovery 已替 Pocket 完成这些验证。

### 3.3 2.0.18 认证协议

`ServerAuth` 用户名固定 `opencode`，允许服务密码或官方签发的 session token 作为 Basic 密码。HTTP 中间件读取 Basic header、`auth_token` query，或有效同源 session cookie；没有在这条 API 鉴权路径实现 `Authorization: Bearer ...`。应使用 Basic header，不使用 query 传 secret。Cookie 名带端口以隔离同主机多个服务，并限制带 Origin 的 cookie 请求为同 host。[auth], [authorization]

CLI 普通 `serve` 从 `OPENCODE_PASSWORD` 读取，兼容 `OPENCODE_SERVER_PASSWORD`；缺少时生成随机密码。共享 `--service` 使用自己的服务配置/持久密码。普通前台模式未显式提供密码时会打印生成密码，因此 Pocket owned 模式必须设置随机内存密码并脱敏进程输出。不能沿用“缺少环境变量表示无认证”或 `OPENCODE_SERVER_USERNAME` 可覆盖用户名的旧假设。[cli-env], [cli-server-process]

### 3.4 配对是显式授予访问，不是认证绕过

- 已认证客户端 `POST /api/pair` 获得随机单次 code，5 分钟有效。[server-group], [pairing]
- `GET /auth/connect/:code` 是唯一相应免凭据兑换路由。带 `Accept: application/json` 获得 `{ "token": "..." }`；浏览器 HTML 导航则设置 HttpOnly session cookie 并重定向 `/`。[authorization], [server-handler]
- token 作为 Basic 的密码、用户名仍 `opencode`；有效期 30 天，以服务密码派生密钥签名，服务密码轮换会撤销所有此类 token。token 没有会话/目录级细分权限。[auth]
- 官方桌面“设置 / 配对”的页面通过主进程 `createPairing()` 调用已认证客户端，不需要用户暴露主服务密码。生成 code 前需用户主动选择；源码没有显示兑换后再等待第二次审批的环节，持有未过期 code 即具有兑换资格。[desktop-pairing], [pairing-ui], [pairing-client]

未来 Pocket 可接受用户明确提供的 pairing link 并在内存兑换，不应把 link/code/token放聊天、日志或诊断截图；服务密码与 token 均视为 secret。本轮没有创建或兑换配对。

## 4. 核心协议差异

| 能力 | Pocket 当前 v1.18.32 假设 | 2.0.18 原生契约 |
| --- | --- | --- |
| 身份/就绪 | `/global/health`，`healthy/version` | `/api/info`，`version/pid/urls/paths`；未就绪还有非 200 状态，不能只解析 version 判 ready。 |
| 接口规范 | `/doc` | `/openapi.json`。 |
| 会话列表 | `/session` 返回数组，最近 100 条 | `/api/session` 返回 `{data,cursor:{previous,next}}`，默认最近 50，支持 `order`、`cursor`、`directory`/`project`、`parentID`。 |
| 会话详情 | 顶层 `directory`、`title` | `{data:Session.Info}`；目录为 `location.directory`，title 可缺省，`outcome/time.idle` 为完成状态。 |
| 运行状态 | `/session/status` | `/api/session/active` 返回 `{data:{[id]:{type:"running"}}}`，仅该进程拥有的前台 drain；缺席为 inactive，不应把 pending inbox 或工具后台化等同正在前台运行。 |
| 历史 | 数组 `{info,parts}`，`before` 与 `X-Next-Cursor` | `/api/session/:id/message` 返回 `{data,cursor}`；query `order/cursor/limit`，limit 1–200；消息为 `type` discriminated union。 |
| 继续对话 | `/prompt_async`，`{parts:[{type:"text",text}]}`，204 | `/api/session/:id/prompt`，`{text,...}`，返回 `{data:SessionInbox.User}`；持久接收与实际执行分离。 |
| 停止 | `/abort` 返回 true | `/api/session/:id/interrupt` 返回 `{interrupted:boolean}`，仅中断该服务拥有的执行；默认不要发送 `resume=true`。 |
| 权限待办 | `/permission`，`permission/patterns/always` | session-scoped `/api/session/:id/permission`，`{data}`；request为 `action/resources/save/source`。 |
| 权限响应 | `/permission/:request/reply`，`{reply}` | `/api/session/:id/permission/:request/reply`，`{decision:"once"|"always"|"reject",message?}`，204。 |
| 用户问题 | `/question`，`questions[]` 与二维 answers | session forms，`/api/session/:id/form`，answer是按 field key 的有类型字典；取消用 DELETE form。 |
| SSE | `/event`，`properties`，`message.part.*` | `/api/event`，`data`，原生 `session.text.*`/`session.step.*`/`session.tool.*`；跨所有 server locations。 |

各行直接依据：[server-group], [server-process], [session-group], [session-schema], [message-group], [message-schema], [prompt-input], [permission-group], [permission-schema], [form-schema], [event-group]。

### 4.1 历史、提交与位置

消息不能把 v2 数据强制塞成 v1 `{info,parts}`：user直接含 `text/files/agents/skills`，assistant含 `content[]` 的 text/reasoning/tool；另有 agent/model/location切换、system、synthetic、skill、shell、compaction、idle记录。tool自身有 streaming/running/completed/error状态与内容。未知类型须保持可见/可诊断，不能导致整页丢失。[message-schema], [message-group]

v2 `prompt` 的可选 `id`、`delivery`、`resume` 与 inbox语义需要独立验证；持久接收响应不等于模型完成。断网接受状态不明时仍不得自动重发。会话位置由服务根据 session解析，调用方目录不是访问控制；Pocket 必须从 `location.directory` 核对自己的授权范围，不能只依赖 query/header。[session-group], [session-location], [location-schema]

### 4.2 权限的 always 语义也变了

2.0.18 在 `always` 且 request有 `save` 时调用 `PermissionSaved.add` 按 project写入数据库，后续评估加载这些 saved rules，并重新评估待处理请求。**原 UI“直到 OpenCode 实例重启才失效”的说明在 v2 是错误的安全承诺。** 必须展示这是 project范围持久保存的权限（仅上游提供save时），默认仍单次批准；不能无声映射旧说明。[permission-core], [permission-saved]

Forms支持 string/number/integer/boolean/multiselect/external，以及 required、条件显示 `when`、默认值等；不是把 v1问题选项改一个字段名。只支持子集时应明确不可操作提示，不得提交缺字段、错误类型或自动接受外部链接。[form-schema]

### 4.3 SSE 与恢复

`/api/event` 是跨所有 locations的易失流；官方契约明确慢消费者溢出会断流、断开期间会丢事件。当前容量4096。帧仅写 `data: <JSON>\n\n`，没有 SSE `id:`；15秒心跳是 `: heartbeat`注释，不是旧 `server.heartbeat`业务事件。[event-group], [event-feed], [event-handler]

v2文本增量使用 `data.sessionID/assistantMessageID/ordinal/delta`，全文终态为 `session.text.ended` 的 `text`；reasoning有同类事件，execution、step、tool也有独立生命周期。不能使用v1的 `partID/field/properties` reducer。scope过滤须考虑 `location`、session和重连代次，不能将其它会话内容送入Flutter。[session-events]

另有实验性 `/api/experimental/session/:id/log?after=<seq>&follow=true` durable log；它不是 `/api/event` 的重放。文本delta明确是 ephemeral，Text.Ended才是可重放全文边界。首期应先以有界历史重读、当前消息/状态/审批/forms校准恢复，不在未验证前宣称完整断点续传。[session-group], [session-events]

### 4.4 旧接口兼容开关

本 tag实际 `makeDefaultApi` 只组装新 protocol groups；CLI `serve`没有发现启用v1 HTTP routes的开关。源码仍有 `V1Migration`（数据迁移）和 CLI `run/v1.ts`（命令边界兼容），**它们不是旧HTTP服务器接口兼容层**。结合隔离实测，不能指望给2.0.18追加启动flag便恢复v1 API。此结论仅针对本tag/本二进制，不推断所有未来版本。[api], [server-routes], [cli-v1], [migration-group]

## 5. 本地隔离只读验证

本轮于2026-09-27运行安装包内 `/Applications/OpenCode.app/Contents/Resources/opencode-cli`，SHA256 `8fccb375dd271323c287fb4dac4bc61aee9faa39b3d294d9e8b95687e3cc7645`。独立HOME与XDG目录，不继承用户配置/供应商密钥；显式loopback、独立端口和随机内存密码。未调用 `serve --service`、discovery、ensure或用户现有服务。只结束本次启动且持有句柄的owned子进程，结果确认已退出。

证据保存在 `/tmp/pocket-opencode-2018-research.5lhm6n/` 的 `probe.mjs`、`results.json`、`openapi.json`。本研究已读回脱敏 `results.json`；未把token/password写入文档。结果：

| 只读请求 | 实测 |
| --- | --- |
| `/api/info`（正确Basic） | 200，version `2.0.18`，pid与owned child一致。 |
| `/api/info`（无认证） | 401。 |
| `/global/health`、`/doc` | 200 `text/html`，是Web UI fallback，不是健康或规范。 |
| `/api/global/health`、`/api/doc`（正确Basic） | 404，否定“只需要/api前缀”的假设。 |
| `/openapi.json` | 200，OpenAPI3.1.0、115 paths。 |
| `/api/session?limit=1` | 200，`{"data":[],"cursor":{"previous":null,"next":null}}`；无游标的运行时形状包含null，应兼容null和缺省。 |
| `/api/event` | 200，首帧keys `id/type/data`，type `server.connected`，无SSE `id:`行。 |

进程日志扫描未发现该随机密码或Basic值。本次未发任何POST，未调用模型、创建会话、pair、权限回复、forms回复、interrupt或dispose；没有读取用户现有会话。**因此上述结果不能宣称已验证历史内容、增量生成、审批闭环、停止语义或已安装Pocket的连通性。**

## 6. Pocket 施工落点和顺序

先完成基于上述协议的设计与契约，不再通过猜端口/前缀反复试用户服务。建议施工分成可验收的小阶段，以下为计划，不是已完成代码：

1. **协商和错误分层**：`crates/pocket-codex-host-svc/src/opencode/client.rs:125` 的 `health` 与 `client.rs:134` 的 `capabilities` 固定v1；引入明确的v1/v2协议选择、JSON/content-type/schema验证、401/404/非JSON/未就绪区分。origin与API版本分离，不放宽路径校验就声称支持v2。保留v1契约测试。
2. **只读v2闭环**：在该目录 `protocol.rs`定义独立v2 info/session/message/page类型与边界，`client.rs`实现info、list、detail、history/active。补null游标、排序、单页大小、directory与session身份测试。使用既有crate，不引入新crate。
3. **事件与恢复**：`sse.rs`共用SSE传输解析，但分开v1 `properties`与v2 `data` wire envelope；`crates/pocket-codex-bridge/src/engine/opencode/mod.rs`实现v2 reducer/snapshot校准、跨location过滤与断线只读状态，测试delta/full重复和乱序。
4. **写入闭环**：按native inbox实现prompt与未知接受状态核对；interrupt不触碰进程；权限明确session和持久always语义；Forms只发送当前权威待办的正确类型答案。更新`opencode/protocol.rs`、`client.rs`与bridge测试。
5. **attached发现与owned托管分离**：`crates/pocket-codex-core/src/opencode/mod.rs`、`crates/pocket-codex-cli/src/commands/opencode.rs`补官方只读注册文件发现与凭据内存生命周期；不调用上游ensure/stop替换用户共享服务。原首版仅实现附接托管；若另行扩展owned托管，只能管理本应用自行启动且能证明所有权的独立进程，不能将本次研究夹具当作产品已实现owned的证据。
6. **relay与Flutter能力呈现**：`opencode/gateway.rs`当前公开v1形状路由（约100–115行），须定义有版本的Pocket gateway契约，不能让gateway伪称自己是未经转换的上游v2；同时校准`apps/flutter/lib/src/opencode_api.dart`、`opencode_controller.dart`、`screens/opencode_screen.dart`。保持独立入口和现有首页/Codex逻辑不变，展示版本/认证原因，修复always提示和Forms交互。
7. **真实打包前验收**：先真实2.0.18隔离实例，再已安装客户端对用户授权的attached服务做只读连接/列表/历史，再经明确确认的新测试会话检验prompt/SSE/权限/forms/interrupt；最后验证Codex并回归DMG。未通过前不得再次称“可用桌面包”。

落点行号取自本轮当前分支，不是固定提交；施工前需重新读取。现有测试落点包括 `crates/pocket-codex-host-svc/tests/opencode_http.rs`、`opencode_gateway.rs`，`crates/pocket-codex-bridge/src/engine/opencode/tests.rs`，`apps/flutter/test/opencode_controller_test.dart`、`opencode_screen_test.dart`。

## 7. 开工与交付门槛

- 契约门槛：2.0.18固定源码与实际规范交叉校验，不能只断言path存在；黄金fixture覆盖native messages、events、权限和forms，且v1.18.32回归不变。
- 安全门槛：attached连接/断开/退出不发送进程信号、不调用上游ensure/stop/dispose、不写用户OpenCode配置；凭据不出现在URL、FRB、日志、截图、relay配置和错误正文；本机发现只向验证的loopback origin发送认证。
- 行为门槛：列表与历史真实有界分页，SSE增量与全文不重复，断线恢复不丢已读内容，current待办被其它客户端处理后正确刷新，prompt不自动重发，interrupt只作用所选session。
- UI门槛：安装后从现有独立入口可连接；能区分认证失败、未知协议与网络错误；保留原Codex会话和首页；v2持久授权文案不得沿用v1临时实例说明。
- 验证门槛：项目AGENTS规定的全套Rust fmt/clippy/test、Flutter format/analyze/test；原Codex兼容测试；隔离真实2.0.18、真实v1.18.32；macOS arm64构建并启动桌面包实际点击验收。不能以mock测试、CLI探测或构建成功替代桌面端到端结果。
- 当前剩余限制：没有实施v2 adapter；尚未通过Pocket客户端连接用户的2.0.18服务；没有真实模型和权限/forms/停止闭环；没有验证其它OpenCode 2.x版本。上述工作未完成时只报告研究/只读探测通过。

[tag]: https://api.github.com/repos/anomalyco/opencode/git/ref/tags/v2.0.18
[source-tree]: https://github.com/anomalyco/opencode/tree/cd9a14a6b688d4021bee381dfd39d2cef9c0f862
[old-server-doc]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/web/src/content/docs/server.mdx#L39
[api]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/protocol/src/api.ts#L155
[server-routes]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/server/src/routes.ts#L174
[generated-openapi]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/protocol/openapi.json
[server-group]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/protocol/src/groups/server.ts#L35
[web-ui]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/cli/src/services/web-ui.ts#L16
[server-process]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/server/src/process.ts#L178
[desktop-service]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/desktop/src/main/service/background-service.ts#L30
[sidecar-credentials]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/desktop/src/main/service/sidecar-credentials.ts#L7
[service-client]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/client/src/effect/service.ts#L19
[service-registration]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/cli/src/services/service-registration.ts#L38
[service-config]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/cli/src/services/service-config.ts#L29
[auth]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/server/src/auth.ts#L16
[authorization]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/server/src/middleware/authorization.ts#L31
[cli-env]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/cli/src/env.ts#L7
[cli-server-process]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/cli/src/server-process.ts#L81
[pairing]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/server/src/pairing.ts#L8
[server-handler]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/server/src/handlers/server.ts#L27
[desktop-pairing]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/desktop/src/main/service/pairing.ts#L3
[pairing-ui]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/app/src/settings/pairing/pairing.tsx#L63
[pairing-client]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/app/src/servers/connect/pairing.ts#L14
[cli-pair]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/cli/src/commands/handlers/pair.ts#L14
[session-group]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/protocol/src/groups/session.ts#L181
[session-schema]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/schema/src/session.ts#L32
[message-group]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/protocol/src/groups/message.ts#L9
[message-schema]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/schema/src/session-message.ts#L72
[prompt-input]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/schema/src/prompt-input.ts#L30
[permission-group]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/protocol/src/groups/permission.ts#L100
[permission-schema]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/schema/src/permission.ts#L25
[permission-core]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/core/src/permission.ts#L295
[permission-saved]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/core/src/permission/saved.ts#L1
[form-schema]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/schema/src/form.ts#L37
[event-group]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/protocol/src/groups/event.ts#L45
[event-feed]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/server/src/event-feed.ts#L9
[event-handler]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/server/src/handlers/event.ts#L14
[session-events]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/schema/src/session-event.ts#L394
[session-location]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/server/src/middleware/session-location.ts#L17
[location-schema]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/schema/src/location.ts#L9
[cli-v1]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/cli/src/run/v1.ts#L28
[migration-group]: https://github.com/anomalyco/opencode/blob/cd9a14a6b688d4021bee381dfd39d2cef9c0f862/packages/protocol/src/groups/migration.ts#L18
