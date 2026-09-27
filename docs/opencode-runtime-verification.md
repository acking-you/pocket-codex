# OpenCode 1.18.32 隔离运行核验

日期：2026-09-27。平台：macOS arm64，Node.js 22.20.0。

本次运行的是官方 npm 发布的 `opencode-darwin-arm64@1.18.32`，不是 HTTP mock。
仅核验上游接口；这不是 Pocket-Codex 网关、Flutter 或 relay 已通过验收的声明。
源码依据见 [官方接口研究](opencode-api-research.md)。

## 环境与所有权

安装命令：

```sh
npm install --prefix "$HOME/.local/share/pocket-opencode-test-tools" \
  --ignore-scripts --no-audit --no-fund opencode-darwin-arm64@1.18.32
```

- 二进制：`~/.local/share/pocket-opencode-test-tools/node_modules/opencode-darwin-arm64/bin/opencode`。
- 二进制 SHA-256：`a3c45d4e1d6620b436851f1ef6b25c71befcf06a382e279a1eb1c2196424395e`。
- npm tarball integrity：`sha512-sDsHk/A6FNqeN5NuAiII6AAhi4pJIFcIY0S2yjG9gToIIVaqrQRaHQ6g0kpoVqTiMci8tvaND6tWD4eDT3ahFw==`。
- `spawn` 使用新建环境对象，未继承进程中的模型密钥、代理凭据或 OpenCode 配置变量。
- `HOME`、`XDG_CONFIG_HOME`、`XDG_DATA_HOME`、`XDG_CACHE_HOME`、`XDG_STATE_HOME`、`OPENCODE_CONFIG_DIR`、`TMPDIR` 均指向同一个临时夹具下的独立目录。
- 工作目录是临时创建的 `project space 中文`，另建 `other` 目录验证作用域。
- 设置 `OPENCODE_DISABLE_AUTOUPDATE=true`、`OPENCODE_DISABLE_MODELS_FETCH=true`、`OPENCODE_DISABLE_PROJECT_CONFIG=true`、`OPENCODE_EXPERIMENTAL_DISABLE_FILEWATCHER=true`，配置不含插件。
- 32 字节随机服务密码只通过子进程环境和内存 Authorization header 使用，未放入 argv、URL、结果文件或本文。
- 在本进程临时绑定回环端口取得可用非零端口后释放，再启动 `serve --hostname 127.0.0.1 --port <port> --no-mdns`。没有使用可能先占用 4096 的 `--port 0`。
- 最终仓库脚本通过运行的子进程 PID 为 `65606`，由脚本持有的 child handle 发送 `SIGTERM` 回收。脚本不按进程名、端口或持久化 PID 查杀。中断路径验证的 PID `66852` 也由脚本自身回收。
- 未读取用户的 OpenCode 配置、模型凭据和现有会话，未连接用户启动的服务。

## 实际结果

最终仓库脚本完整运行退出码为 0，结果 `passed:true`，覆盖下表断言。首次临时夹具在消息刚出现、parts 尚未写入时提前断言失败，修正为等待消息及正文同时出现后通过。这是运行中观察到的真实写入竞态。

移植脚本时还修复了 SSE 取消等待挂起：改用直接持有的 AbortController、显式 reader.cancel，并在 finally 中先回收自己的服务再等待读任务。该次挂起运行仅人工回收了明确由本次夹具创建的 PID `62232`，不计为通过；未触及用户进程。最终脚本的正常退出和 SIGTERM 中断均无需该手动处理。

| 操作 | 观察结果 |
| --- | --- |
| 正确 Basic 的 `GET /global/health` | 200，`{"healthy":true,"version":"1.18.32"}` |
| 缺失或错误 Basic 的健康请求 | 均 401，`WWW-Authenticate: Basic realm="Secure Area"` |
| `GET /doc` | 200，OpenAPI `3.1.0`；设计所列健康、会话、消息、prompt_async、abort、event、permission、question 路由均存在 |
| 新目录 `GET /session?directory=...&limit=100` | 200，`[]` |
| `POST /session?directory=...` | 200，会话含 `id`、`slug`、`projectID`、`directory`、`path`、`title`、`time`、`version`、`tokens`、`cost`；本次没有 `workspaceID` |
| 新会话 `GET /session/{id}/message?limit=20` | 200，`[]`，无 `X-Next-Cursor` |
| 三次 `POST /session/{id}/prompt_async` | 均 204；使用 `noReply:true` 和显式 `fixture/unused` model，只保存用户消息，不调用模型 |
| 保存完三条后 `message?limit=2` | 200，返回第 2、3 条，页内从旧到新；有 `X-Next-Cursor` 和 `Link` |
| 同源 `message?limit=2&before=<cursor>` | 200，返回第 1 条，无下一游标 |
| `GET /session/{id}/message/{messageID}` | 200，对象与对应已稳定的历史条目一致 |
| `GET /permission`、`GET /question` | 均 200，`[]` |
| 无任务时 `GET /session/status` | 200，`{}`；不是每个 idle 会话都有显式条目 |
| 回复不存在的 permission | 404，`PermissionNotFoundError` |
| 回复或拒绝不存在的 question | 均 404，`QuestionNotFoundError` |
| `GET /event` | 200，`Content-Type: text/event-stream` |
| SSE 初始和心跳 | 收到 `server.connected`、`server.heartbeat`；JSON keys 为 `id,type,properties`，所有已收集帧均无 SSE `id:` 行 |
| 创建会话和保存消息产生的事件 | 收到 `session.created`、`session.updated`、`message.updated`、`message.part.updated` |
| 对 idle 会话 `POST /session/{id}/abort` | 200，`true`；随后健康仍为 200，并收到 `session.status` 的 idle 和 `session.idle` |
| 切到另一个空目录列会话 | 200，`[]` |
| 以另一目录 query 读取原会话 ID | 仍为 200，返回原目录的会话；实证 directory 不是授权边界 |
| 夹具 stdout/stderr 扫描 | 未出现随机服务密码及其 Basic 编码；仅涉及此夹具输出，不是 Pocket 全链路泄漏验收 |

## 对实现的影响

1. **目录需要规范化。** 本机 `mkdtemp` 返回 `/var/folders/...`，OpenCode 的 session.directory 返回 `/private/var/folders/...`。网关不能直接对用户输入路径做字符串比较，否则拒绝合法会话；也不能只因路径不同就放宽校验。主机侧应先 canonicalize 到真实目录，固定该身份后核对 session/workspace。
2. **消息与 parts 不是一次原子快照。** 本次首次运行看到了已存在消息但 `parts:[]`，稍后才收到 parts。不能把消息 ID 已出现视为全部正文已写入，也不能仅凭一次空 parts 就永久缓存完整历史。SSE 触及消息后按设计标脏校准。
3. **idle 状态列表可以为空。** 此次全部会话 idle 时状态接口为 `{}`；连接失败仍不能冒充 idle。
4. **当前无前缀路由、游标、认证、pending 404 和 SSE 无 replay ID 与研究吻合。** 另观察到 `session.idle` 事件；可作为兼容信号，主状态仍以 `session.status` 与权威刷新为准。

## 可重复请求顺序

仓库夹具为 [opencode_runtime_check.mjs](../crates/pocket-codex-host-svc/tests/fixtures/opencode_runtime_check.mjs)。在仓库根目录运行以下命令，会生成新隔离目录和自己拥有的服务，结束后自动回收：

```sh
PCX_TEST_OPENCODE_BINARY="$HOME/.local/share/pocket-opencode-test-tools/node_modules/opencode-darwin-arm64/bin/opencode" \
  node crates/pocket-codex-host-svc/tests/fixtures/opencode_runtime_check.mjs
```

`PCX_TEST_OPENCODE_BINARY` 必须是显式绝对路径，脚本不搜索 PATH，也不安装或启动用户已有配置。其他平台需自行取得同版本的官方对应二进制；Windows 环境仅额外继承 SystemRoot，并重建必要系统 PATH，HOME、USERPROFILE、APPDATA 等仍指向隔离目录。脚本使用 Node 内置模块，不需要额外 npm 测试依赖。

该脚本属于 S2 外部协议 characterization，不替代业务实现的 RED/GREEN 测试。所有表中关键结果都执行 assert，包括 `/doc` 中 14 个路径的 15 个 method 声明，而非仅打印是否存在。UTF-8 SSE 使用流式 TextDecoder；失败只显示预定阶段名称，不输出异常消息或上游日志。输出工件还会替换本次随机密码和 Basic 编码，原始日志与 SSE 集合有大小上限。

请求体如下；目录统一编码，认证由内存中的随机密码构造，不把密码写进命令参数：

```json
{"title":"Pocket isolated API fixture"}
```

```json
{
  "noReply": true,
  "model": { "providerID": "fixture", "modelID": "unused" },
  "parts": [{ "type": "text", "text": "Fixture message 1" }]
}
```

后续两条仅改变 text。每次 204 后有界轮询 `message?limit=20`，等待对应消息和 text part 同时出现；不能只等消息数增加。随后 `limit=2`，读取 `X-Next-Cursor` 并构造同源 `before` 请求。不要跟随 `Link` 中任意 origin。

过期待办请求分别为 `POST /permission/per_missing/reply` 的 `{"reply":"once"}`、`POST /question/que_missing/reply` 的 `{"answers":[["yes"]]}` 和 `POST /question/que_missing/reject`。这些请求只涉及本次隔离服务中不存在的请求 ID。

最终通过运行的本机工件目录：

```text
/var/folders/sw/vhnrdlcj12n49hgxzsmx9cdh0000gp/T/pocket-opencode-fixture-uoXNz9/
  results.json   # 结构化断言结果及收集的 SSE JSON
  openapi.json   # 该二进制运行时的 /doc
  server.log    # 夹具进程 stdout/stderr
```

文件仅包含临时夹具生成的数据；未作为用户项目数据或模型生成样本提交。

额外验证了脚本本身的失败行为：相对 binary 路径立即退出 1；用 `/usr/bin/false` 模拟服务启动失败，记录 `passed:false` 并退出 1；运行真实隔离服务后向夹具发 SIGTERM，夹具退出 1、记录 `passed:false` 并回收自己的服务 PID `66852`。这些失败路径不能计为上游协议通过。`node --check` 与 `git diff --check` 通过。

## 未验证范围

- 未调用模型，因而**未验证真实 token 增量输出、工具执行、真实 permission/question 生命周期、执行中 abort、模型费用或供应商连接**。`noReply:true` 的 204 和 idle abort 不能替代这些结果。
- 未验证 Pocket-Codex 附加托管退出、初始化失败、relay 失联和 Ctrl-C 后上游仍存活；需使用此类拥有的服务作为外部上游再运行产品生命周期测试。
- 未验证 Pocket 的凭据导出、缓存、日志、错误脱敏及跨源重定向。这些应由业务测试和端到端验收分别覆盖。
- 未验证 Windows/Linux、其他 OpenCode 版本、TLS、远程 workspace、异常大正文和断网恢复。上述版本及平台外没有兼容性保证。
- 本文不替代 Rust/Flutter 测试、FRB 生成检查、桌面构建或 UI 验收。
