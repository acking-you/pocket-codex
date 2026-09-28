# PRD：Pocket-Codex 统一托管 Codex / OpenCode（分支 `fix-opencode`）

状态：v1.0，已通过 grill 评审（两轮，Q1–Q14 均按推荐确认）。
术语见 [`CONTEXT.md`](../../CONTEXT.md)，架构决策见 [ADR-0002](../adr/0002-opencode-shared-session-ui.md)。

## 1. 背景

- 当前 `main` 只支持 Codex。桌面 App 托管外部 `codex app-server` 后，在中转上发布 `app / api / meta` 三个服务。控制器的会话界面按工作目录列出项目和会话，支持流式对话、审批、附件、模型选择、历史分页和磁盘缓存。
- `~/Downloads/pocket-opencode`（分支 `feat/opencode-session-hosting`）已经实现过 OpenCode 2.0.18 的协议客户端、SSE、表单和中转网关，这些代码可以复用。但它的产品形态不符合本次要求：用的是独立的 `/opencode` 页面，而且只能从 CLI 发布。ADR-0002 取代了它的 ADR-0001。
- 本机 OpenCode 为 v2.0.18，装在 `~/.opencode/bin/opencode`。它的后台服务连接信息记在 `~/.local/state/opencode/service.json`，使用 HTTP Basic 认证（用户名 `opencode`），v2 API 全部位于 `/api/*` 下。

## 2. 目标

1. 托管时选择**服务提供方**：Codex 或 OpenCode。之后的托管流程、状态展示、停止、注销 / 重新注册、自动恢复完全一致，只有底层协议不同。
2. 控制器复用**同一套会话界面**。每个托管实例只展示它所属提供方已有的项目和会话。
3. OpenCode 以本机 v2.0.18 的实际协议为基准，并通过契约检查兼容后续版本。

## 3. 非目标

- 不内嵌、不打包 OpenCode 运行时。
- 不读取提供方的登录凭据（`/api/credential`、`/api/integration`）。
- 不支持 OpenCode v1.x 路由。
- 不做 CLI 托管 OpenCode（`pocket-codex opencode serve` 留待后续）。
- 不把 Codex 专有能力伪装到 OpenCode 上：sandbox / 审批预设、Fast、Guardian 审查历史、用量 / 限额、接管 / 强制恢复、本机会话浏览。
- 首版不提供 OpenCode 的 PTY / shell / worktree / revert / fork 界面，也不提供完整的 agent 选择器。

## 4. 用户场景

- **S1 托管**：桌面用户打开「托管」，选择 OpenCode，填写实例名后点击开始。App 附接到本机 OpenCode 后台服务；如果服务没有运行，就请 OpenCode 自行启动后台服务。然后在中转上发布这个实例。之后每次启动 App 都会自动恢复。
- **S2 远程查看**：另一台设备打开首页，发现该主机的 OpenCode 实例并自动连接。侧边栏按目录列出项目和会话，并打开上次使用的会话。
- **S3 对话**：在会话中发送消息，看到流式的文本、推理和工具调用，可以随时停止执行。会话运行中再发消息时，默认排队；也可以选择“补充”，并入当前执行。
- **S4 待办**：OpenCode 发起权限请求或表单时，在远程设备上以和 Codex 相同的卡片完成回复。
- **S5 新建**：在某个项目目录下新建会话，并发送第一条消息。
- **S6 双托管**：同一台主机同时托管 Codex 和 OpenCode，界面用标识区分两者，并可以切换。
- **S7 离线**：主机不可达时，控制器先显示缓存的历史，此时只读，并明确标出已过期。

## 5. 功能需求

### 5.1 托管（主机侧，仅桌面 App，需要 GitHub 账号登录，与 Codex 相同）

| ID | 需求 |
|---|---|
| H1 | 托管对话框新增「服务提供方」选项：Codex / OpenCode。「内置引擎（暂未实现）」只在选择 Codex 时出现。 |
| H2 | OpenCode 采用附接托管。App 读取 `service.json`，要求文件只有属主可读、地址为回环、pid 与版本和 `/api/info` 一致。停止托管时不关闭 OpenCode。 |
| H3 | 找不到可用服务时，调用 `opencode service start`（由 OpenCode 自己管理这个进程），然后重新探测。启动失败时给出可操作的错误提示。 |
| H4 | OpenCode 二进制按「显式路径 → 已保存配置 → PATH → `~/.opencode/bin/opencode`」的顺序解析；找不到时给出安装或路径错误。 |
| H5 | 版本契约检查：拉取 `/openapi.json`，确认用到的全部接口和字段都存在。2.0.18 视为已验证；契约满足的其他版本放行，但提示“未验证版本”；契约不满足则拒绝，并列出缺失项。 |
| H6 | 发布服务：会话服务 `pcxu:<user>:<device>:opencode:<name>`，以及 meta 服务 `…:meta:<name>`，提供文件、上传和 OpenCode 历史来源。OpenCode 实例不发布 `api`（Responses 代理）服务。 |
| H7 | 托管状态、停止、注销 / 重新注册、`ui_state.json` 自动恢复都按提供方区分，交互保持一致。Codex 与 OpenCode 可以同时托管。 |
| H8 | Basic 密码只保存在主机进程内存中，不经过中转，不落盘，也不传给控制器。 |

### 5.2 控制器（会话界面，桌面与移动端）

| ID | 需求 |
|---|---|
| C1 | 首页和服务页能识别 OpenCode 实例，并显示提供方标识。首页按「上次使用 → 本机托管 → 第一个可达」的顺序选择实例，规则与 Codex 相同，两种提供方一起参与排序。 |
| C2 | 侧边栏：项目为 `location.directory`，会话只列根会话（`parentID` 为空）。 |
| C3 | 子会话：在父会话中显示为工具卡片，可以点击进入只读查看。 |
| C4 | 运行中会话来自 `/api/session/active`，进入和离开 Active 分组的行为与 Codex 一致。 |
| C5 | 打开会话时先显示有界的尾部窗口，向上翻页加载更早的内容；分页失败时给出显式重试。 |
| C6 | 流式输出：SSE 事件映射为文本、推理、工具调用的增量更新。断线后自动重连，并通过尾部刷新补齐漏掉的内容。 |
| C7 | 发送：默认 `delivery=queue`，“补充”发送 `delivery=steer`。无法确认服务是否已接收时，进入「提交结果未知」状态，不自动重发。 |
| C8 | 停止执行：`POST /api/session/{id}/interrupt`。 |
| C9 | 权限待办：提供 once / always / reject。always 在 OpenCode 中按项目持久生效，选择前需要再次确认。 |
| C10 | 问题待办：表单支持 string / number / integer / boolean / multiselect 字段，复用提问卡片。遇到不支持的字段时，提示用户到 OpenCode 中完成；也可以取消这个表单。 |
| C11 | 模型：来自 `/api/model`，只列出已启用的模型，切换时调用 `/api/session/{id}/model`。推理强度选择器显示模型的 `variants`，没有 variants 时隐藏。 |
| C12 | Plan 模式映射为 `plan` agent，默认使用 `build`。会话当前如果是自定义 agent，只读显示它的名字，发送时不修改。 |
| C13 | 新建会话时不传权限规则，沿用 OpenCode 自己的配置。 |
| C14 | 重命名、压缩上下文、Git diff 与 Codex 对齐。 |
| C15 | 附件：图片和文件通过 prompt 的 `files[].uri` 发送。 |
| C16 | 磁盘历史缓存（与 Codex 共用 512 MB 配额）：显示缓存时处于只读状态，并明确标出已过期。 |
| C17 | 会话文件链接的预览和下载，以及主机文件浏览，复用 meta 服务的文件接口。 |
| C18 | 界面按提供方的能力显示或隐藏控件，不按提供方名称判断。共用区域的文案改为中性表述，并加上提供方标识。 |

## 6. 非功能需求

- **兼容性**：Codex 的现有行为、服务键、FRB 函数签名、磁盘格式和 `ui_state.json` 字段保持不变，只做新增。旧版控制器会忽略 `opencode` 类型的服务（`ServiceKind::Unknown`）。
- **安全**：中转网关只监听回环地址，信任边界与 Codex app-server 相同（中转凭据 / 账号命名空间）。网关只转发白名单内的 `/api/*` 路由，不开放凭据、集成、PTY、配置写入等接口。
- **性能**：
  - 每个历史窗口不超过 200 条消息，单次响应不超过 32 MiB；
  - SSE 更新按 50 ms 合并推送；
  - 长会话不做全量回放。
- **可验证性**：
  - AGENTS.md §7 的全部命令通过；
  - 协议契约测试基于实测的 2.0.18 OpenAPI；
  - 对本机服务做只读实测，写入类实测只在隔离实例上做。

## 7. 验收标准

1. 桌面 App 选择 OpenCode 托管后，另一台设备在首页能看到这个实例，并按目录打开 OpenCode 的会话列表。
2. 远程发送消息能看到流式回复；排队发送、补充、停止执行都有效。
3. 远程设备能完成 OpenCode 的权限请求和表单回复。
4. 同一台主机同时托管 Codex 和 OpenCode 时，两者都可用、能区分，也能切换。
5. 停止托管后，OpenCode 后台服务仍在运行，本机 TUI 会话不受影响。
6. Codex 的全部回归测试通过，界面行为没有变化。

## 8. 迭代与合并

- 按 TRD 里程碑分批提交；每个里程碑在本地全量验证通过后，推送到 `origin/fix-opencode`。
- 全部完成后，从 `fix-opencode` 向 `origin/main` 开 PR，由用户合并。不推送 main，也不 force push。
