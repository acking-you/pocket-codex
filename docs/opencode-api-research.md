# OpenCode 官方接口与源码核查

日期：2026-09-27。用途：为 [OpenCode 集成设计](opencode-integration-design.md) 提供事实依据；本文件不是已完成实现或兼容性承诺。

> 基线限制：本文记录的是先前针对 **1.18.32** 的核查，不代表用户安装的 **2.0.18**，也不再作为“当前最新版本”的依据。2.0.18 已更换服务发现、健康检查、会话/消息模型、事件、审批和问题协议；不能只给旧接口加 `/api` 前缀。适配前必须先阅读 [2.0.18 源码与实测研究](opencode-2.0.18-source-research.md)。此前的 1.18.32 夹具结果不能替代 2.0.18 桌面验收。

## 1. 来源和推荐基线

本次查询官方仓库 `anomalyco/opencode` 的 GitHub Release API，返回最新非预发布版本为 **v1.18.32**。tag 实际指向 **`545f51d26cc39a907d2867492d498d9607ea5fa4`**，下文源码链接均固定到此 tag SHA。Release API 的 `target_commitish` 则是它的父提交 `f5ce4f881e477c7b75421cea2d20939f0ddd71fb`；本次读取的接口源码来自该父提交，另已比对两个提交仅有 `bun.lock` 和各 `package.json` 版本改动，所引接口文件和行号不变。开发分支查询得到 `a42f393c850bec0c0f395fb91bf19b1ee8b31666`，不以浮动 `dev` 作为实现和测试依据。

- [官方发布记录 v1.18.32](https://github.com/anomalyco/opencode/releases/tag/v1.18.32)。
- [官方 Server 文档](https://opencode.ai/docs/server/)，以及[对应固定版本的文档源文件][server-doc]。
- [固定版本 server 入口][server-entry]：实际构建 Effect HttpApi；旧文章常见的 `packages/opencode/src/server/routes/session.ts` 在该版本已不存在，真实实现位于 `server/routes/instance/httpapi/`。

**建议首个验证基线是 v1.18.32 的官方文档所列无 `/api` 前缀接口。** 这组路由在该版本仍真实存在。源码也有 `/api/*` 新接口及 `/experimental/*` 路由，不能混用两套消息类型。没有证据支持直接宣称“所有 1.x”或某个更早版本以上均兼容。连接时同时检查健康版本、`/doc` 的目标路由和 schema；无效或未知形状应明确拒绝，不能仅凭 HTTP 200 判断兼容。

## 2. 连接、凭据和目录作用域

| 项目 | 核实结果 |
| --- | --- |
| 健康检查 | `GET /global/health` 返回 `{ "healthy": true, "version": "..." }`，见[实现][health]。 |
| API 描述 | 官方服务暴露 `/doc`；服务自身通过 `OpenApi.fromApi(PublicApi)` 生成 schema，见[入口][server-entry]。 |
| 认证 | 非空 `OPENCODE_SERVER_PASSWORD` 开启 HTTP Basic；用户名来自 `OPENCODE_SERVER_USERNAME`，默认 `opencode`，见[认证配置][auth]。 |
| 每请求上下文 | 无已识别 session 时，目录优先级为 `directory` query、`x-opencode-directory` header、服务进程 cwd，见[工作区路由][routing]。 |
| session 路由 | 已存在 session 的 `directory` 和 `workspaceID` 可优先于调用方 query，用来选择实际工作区，见[工作区路由][routing]。 |
| 会话列表过滤 | `/session` 处理器仅在显式提供 `directory` query 时把当前目录传入列表过滤；只发目录 header 不等于列表按目录过滤，见[列表处理器][session-handler]。 |

目录是路由和查询上下文，**不是 OpenCode 的访问控制边界**。知道另一个 session ID 的调用者可能被路由到该 session 自身目录。Pocket 服务必须对目标 session、允许的目录、`workspaceID` 做独立约束，并把来源标识和目录纳入缓存键。第一期可只支持明确选定的本机目录，遇到 OpenCode 自身的远程 workspace 明确报不支持，而不是无意转发到另一个执行环境。

现有服务的密码由用户提供，不能读取模型供应商的 `auth.json` 代替服务密码。只向已确认 origin 发送 `Authorization`，禁用携带凭据的跨 origin 重定向。源码还接受 `auth_token` query，但本项目不应使用 URL 传凭据；见[认证中间件][authorization]。Basic 本身不加密，应通过 loopback、已核实的安全隧道或验证证书的 HTTPS 传输；不能默认忽略证书错误或把普通远端 HTTP 当安全通道。

## 3. 会话、历史和继续对话

接口 schema 见[会话路由声明][session-group]，行为以[处理器][session-handler]为准。

| 需求 | 请求 | 响应和注意事项 |
| --- | --- | --- |
| 会话列表 | `GET /session?directory=<编码后的绝对路径>&limit=100` | `Session[]`，按更新时间降序；另支持 `scope=project`、`path`、`roots`、`start`、`search`。 |
| 会话详情 | `GET /session/{sessionID}` | `Session`，包含目录等原始身份信息；与本地连接作用域核对后才允许操作。 |
| 批量状态 | `GET /session/status?directory=...` | `{ [sessionID]: SessionStatus }`；以服务状态为准，断开 SSE 不等于 idle。 |
| 历史尾页 | `GET /session/{sessionID}/message?directory=...&limit=20` | `{ info: Message, parts: Part[] }[]`，页内为从旧到新顺序。 |
| 历史前页 | 上述请求加 `before=<不透明游标>` | 游标来自响应 `X-Next-Cursor`，响应同时可带 `Link: <...>; rel="next"`；无游标表示当前页没有更早页。 |
| 单条消息 | `GET /session/{sessionID}/message/{messageID}` | `{ info, parts }`；用于重连校准活跃消息。 |
| 异步继续 | `POST /session/{sessionID}/prompt_async?directory=...` | 接受后 `204`，模型结果经 SSE 和历史读取；`204` 不是模型成功结束。 |
| 同步继续 | `POST /session/{sessionID}/message` | 等待生成后返回消息 JSON；它不是 token SSE。首期交互采用异步接口。 |
| 停止执行 | `POST /session/{sessionID}/abort?directory=...` | 返回 `true`；处理器调用该 session 的 prompt cancel，不是服务进程退出接口。 |

`/session` 的 `start` 在[数据库查询][session-list]中是 `time_updated >= start`，**不是向旧会话翻页的游标**。默认上限是 100，公开的该路由没有 `before`/`cursor`。首期列表可以提供有界最近列表和服务端标题搜索，必须展示范围限制；不能根据 100 条结果断言全部会话已加载。不要把 `/experimental/session` 的数值游标伪装成 `/session` 的稳定能力。

历史消息与列表不同，当前版本已支持真正的 `before` 分页。见[历史响应头处理][history-handler]和[消息分页查询][message-page]。不传 `limit` 或传 `0` 会读取全历史，应禁止走默认无限读取路径。客户端只使用服务器给的游标值构造同 origin 请求，不直接访问 `Link` 中未经校验的任意 URL。单页消息数有界不代表单条工具输出字节数有界，FRB 前仍需限制正文、附件及响应总字节。

最小 prompt 请求体：

```json
{
  "parts": [{ "type": "text", "text": "继续处理当前任务" }]
}
```

可选字段包括 `messageID`、`model: { providerID, modelID }`、`agent`、`noReply`、`system`、`variant`、`format`；`tools` 已标注废弃。见[PromptInput][prompt-schema]。不应把 Codex 的 model/effort/approval preset 原样发送给 OpenCode。首期可保留该会话 OpenCode 默认模型配置，后续再提供对应模型选择。

`prompt_async` 在后台执行，失败通过 `session.error` 事件报告，见[异步处理器][prompt-handler]。`messageID` 可用于提交结果核对，但上述接口没有声明请求幂等性保证。网络中断造成接受状态不明时，保留草稿和待确认状态，读取消息核对，**不能自动重发 prompt**。

## 4. SSE 事件与恢复

`GET /event?directory=...` 返回 `text/event-stream`，初始 `server.connected`，约 10 秒心跳 `server.heartbeat`，并按实例目录/工作区过滤实时事件。载荷格式为 JSON：

```json
{
  "id": "事件标识",
  "type": "message.part.delta",
  "properties": {
    "sessionID": "ses_...",
    "messageID": "msg_...",
    "partID": "prt_...",
    "field": "text",
    "delta": "新增文本"
  }
}
```

关键区别：该版本 **JSON 载荷有 `id`，但 SSE frame 的 `id` 明确是 `undefined`**。实现是实时监听队列，没有在这个接口读取 `Last-Event-ID` 或回放历史。不能承诺断点事件重放。见[SSE 处理器][events]。`/global/event` 是另一种 `{ directory?, payload }` 包裹格式，不应与 `/event` 的直接事件解析器混用，见[全局事件实现][global-events]。

需要处理的事件至少包括：

- `message.updated`、`message.removed`。
- `message.part.updated`、`message.part.delta`、`message.part.removed`；增量按 session/message/part 定位，全量 part 更新按 ID 替换，不能再次追加相同全文。
- `session.created`、`session.updated`、`session.deleted`、`session.status`、`session.error`。
- `permission.asked`、`permission.replied`；`question.asked`、`question.replied`、`question.rejected`。
- `server.connected`、`server.heartbeat`、`server.instance.disposed`。

消息/part 事件字段见[消息事件 schema][message-events]；权限和问题事件见[权限 schema][permission-schema]、[问题 schema][question-schema]。未知事件应保留连接并记录脱敏诊断，不猜测其业务意义。

推荐恢复流程是先建立并暂存新流事件，再读取有界尾页、活跃消息、状态和审批/问题待办。快照与 delta 没有共同事务游标，不能简单把快照后缓存的所有 delta 再追加一遍；先合并完整对象、将受影响的消息标脏，再按单条消息重读校准。正常流中可即时显示 delta；重连时用服务器完整对象替换已知消息，并去重事件/对象标识。流断开后显示连接不确定，禁止由旧缓存自动恢复可写状态。

## 5. 权限审批与用户问题

| 能力 | 当前接口 | 请求/响应 |
| --- | --- | --- |
| 权限待办 | `GET /permission?directory=...` | `PermissionRequest[]`，含 `id`、`sessionID`、`permission`、`patterns`、`metadata`、`always`、可选 `tool`。 |
| 权限响应 | `POST /permission/{requestID}/reply?directory=...` | `{ "reply": "once", "message"?: "..." }`，reply 可选 `once`、`always`、`reject`；成功为 `true`，过期待办可 404。 |
| 问题待办 | `GET /question?directory=...` | `QuestionRequest[]`，含 `id`、`sessionID`、有序 `questions`、可选 `tool`。 |
| 回答问题 | `POST /question/{requestID}/reply?directory=...` | `{ "answers": [["选项标签"]] }`；二维字符串数组，外层按问题顺序，内层是该题的回答。 |
| 拒绝问题 | `POST /question/{requestID}/reject?directory=...` | 成功为 `true`；过期待办可 404。 |

路由与错误声明见[权限路由][permission-group]、[问题路由][question-group]。官方 Server 说明表仍列 `/session/{id}/permissions/{permissionID}` 与 `response, remember?`；源码确实保留该旧路径，但当前 payload 仅声明 `{ response }`，最终调用同一 reply。**不能按旧文档虚构 `remember` 的持久化语义。** 首期使用 `/permission/{requestID}/reply` 和 `reply` 字段，不做未经验证的静默 fallback。

`always` 也不能翻译为“仅本轮允许”或“永久允许”：在此固定版本[权限实现][permission-state]中，批准规则存于 `InstanceState` 的内存 `approved` 数组；匹配 `request.always` patterns，可影响同一实例后续请求，不是写入项目配置。当前已排队的同 session 匹配请求也会一并放行。应在 UI 显示其实际扩大授权范围，默认选项为单次允许。

`reject` 还会拒绝同 session 其它待审批请求。其他客户端可能先答复，404 应刷新待办，不能自动改为允许或创建第二个请求。权限与问题均等待内存 Deferred；缺少问题处理会使会话停在等待用户输入的工具调用。见[问题生命周期][question-state]。因此问题回答/拒绝是完成继续对话闭环的必要范围，不能仅实现 permission 而遗漏 question。

历史消息中的审批或问题文本不代表当前待办；只有实时事件和当前待办接口可生成可操作控件。需要把待办 ID 绑定 session、服务来源、目录与连接代次。

## 6. 外部进程托管

真实入口为 `opencode serve`，参数见[serve 实现][serve]与[网络参数][network]：`--port`、`--hostname`、`--mdns`、`--mdns-domain`、`--cors`。没有在此 serve command 中声明 `--directory`；工作目录通过子进程 cwd 和逐请求目录选择指定。

文档列默认端口 4096，但源码参数默认是 0，监听逻辑的 **0 是先尝试 4096，失败后分配随机端口**，不能把 `--port 0` 当作一开始必定随机端口。见[端口选择][port-selection]。托管须从子进程实际启动结果获取 endpoint 并核对健康；不能健康检查到用户已有 4096 服务后就把其认领为自己启动的进程。显式非零端口被占用时，不应替换或终止占用者。

建议显式 `--hostname 127.0.0.1 --no-mdns`，通过子进程环境变量传随机服务密码；密码不出现在 argv、日志、relay 服务 key、导出配置或诊断中。连接已有服务标为 attached，只断开客户端连接；仅保留了可靠进程所有权证据的 owned 子进程可由 Pocket 停止。`abort` 是停止 session，`/instance/dispose` 和 `/global/dispose` 会影响共享实例，不能拿它们充当断开按钮。以上所有权策略属于 Pocket 设计要求，不是上游替客户端提供的保证。

## 7. 核查限制与实施验收

本次已只读取得官方发布元信息、官方文档和上述固定 SHA 的真实源码；未启动 OpenCode，未接触用户服务、登录或模型凭据，未发送 prompt、审批、abort 或 dispose 请求。GitHub clone/raw/tarball 部分网络请求失败，已改用 GitHub Contents API 获取所引用文件；不能据此宣称完成实际进程验证、模型生成测试或旧版本兼容测试。

实现后至少需要：固定 schema 契约测试；HTTP/SSE 模拟测试（分帧、UTF-8、未知事件、断线、快照与增量交错）；历史分页与大正文边界；Basic 和跨 origin 重定向不泄露；审批/问题被其他客户端处理；prompt 接受结果不明时不自动重发；attached 服务断开/退出仍存活；owned 子进程身份核验与端口冲突；实际 v1.18.32 独立临时目录端到端验证。真实模型测试单列凭据/费用/网络限制，不能用模拟测试代替声称已验证。保留全部原有 Codex 回归门禁。

[server-doc]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/web/src/content/docs/server.mdx#L9
[server-entry]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/server/server.ts#L56
[health]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/server/routes/instance/httpapi/handlers/global.ts#L66
[auth]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/server/auth.ts#L17
[authorization]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/server/routes/instance/httpapi/middleware/authorization.ts#L73
[routing]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/server/routes/instance/httpapi/middleware/workspace-routing.ts#L86
[session-group]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/server/routes/instance/httpapi/groups/session.ts#L30
[session-handler]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/server/routes/instance/httpapi/handlers/session.ts#L64
[session-list]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/session/session.ts#L980
[history-handler]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/server/routes/instance/httpapi/handlers/session.ts#L106
[message-page]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/session/message-v2.ts#L429
[prompt-schema]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/session/prompt.ts#L1499
[prompt-handler]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/server/routes/instance/httpapi/handlers/session.ts#L295
[events]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/server/routes/instance/httpapi/handlers/event.ts#L12
[global-events]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/server/routes/instance/httpapi/handlers/global.ts#L16
[message-events]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/schema/src/v1/session.ts#L596
[permission-schema]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/schema/src/v1/permission.ts#L27
[question-schema]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/schema/src/v1/question.ts#L27
[permission-group]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/server/routes/instance/httpapi/groups/permission.ts#L11
[question-group]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/server/routes/instance/httpapi/groups/question.ts#L11
[permission-state]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/permission/index.ts#L46
[question-state]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/question/index.ts#L87
[serve]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/cli/cmd/serve.ts#L6
[network]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/cli/network.ts#L6
[port-selection]: https://github.com/anomalyco/opencode/blob/545f51d26cc39a907d2867492d498d9607ea5fa4/packages/opencode/src/server/server.ts#L117
