# OpenCode 施工评估与 TDD 任务清单

日期：2026-09-27。设计依据：[集成设计](opencode-integration-design.md)、[官方源码核查](opencode-api-research.md)。用户已接受建议的首版范围并授权建分支、评估、拆分任务及通过后施工。

方法：按 `/grill-with-docs` 使用 grilling 与 domain-modeling，按 `/tdd` 逐个行为执行 RED → GREEN。本文列的是待验证的行为清单，不代表测试已编写或运行。

## 1. 施工门结论

**架构方向可行；用户已确认按设计和任务清单施工，S1–S5 测试接口确认门通过。** 当前正在准备环境基线。用户已接受的范围不重复询问：首版附接已有服务、支持直连/relay、保留 Codex、只在主机使用上游密码、不会停止用户进程。

| 评估问题 | 结论及处理 | 是否阻挡业务施工 |
| --- | --- | --- |
| 现有 Codex 能否无感保留？ | 独立服务类型/会话控制器；Codex 默认和旧文件保持原状；既有回归必须通过 | 设计已解决，T1/T8/T12 验证 |
| 托管停止会不会杀用户进程？ | Attached 不持有上游 PID，不调用 dispose/kill；关闭网关后的上游健康验证是验收依据 | 设计已解决，T6 验证 |
| 服务目录是否能当权限边界？ | 不能；session 自带目录/workspace 可覆盖请求上下文，逐请求核验对象归属 | 设计已解决，T5 验证 |
| 是否已具备完整对话闭环？ | permission 与 question 都要覆盖；204 只算受理；busy 保留草稿 | 设计已解决，T4/T9 验证 |
| SSE 能否恢复、不重复文字？ | 无 replay；完整对象替换、dirty 消息校准、队列溢出重同步，不能盲追加缓冲 delta | 设计可实现，T3/T7 验证 |
| 超时发送是否能自动重试？ | 无已证实幂等保证；保持 unknown，按消息 ID 查询，不自动重发 | 设计已解决，T4 验证 |
| 会话列表能否保证全量？ | 官方稳定接口无旧页游标，最近 100 条加搜索并标示范围；消息历史另有 before | 范围已写入设计，T2/T8 验证 |
| 凭据是否可自动恢复？ | 首版内存凭据，重启重新输入；无密码不自动托管，不增加平台密钥库工程 | 需要落实到 UI/CLI 契约，T1/T6/T8 验证 |
| 历史跨重启如何检测替换？ | 没有可靠源 revision 时使用连接代次失效，不把更新时间当强版本 | 需施工细化，T7/T10 验证 |
| 上游契约是否真实？ | 固定 v1.18.32/tag SHA，文档与源码差异已列明；实际进程待验证 | T0 固定运行版本，T12 实测 |
| 实现能否成为独立 PR？ | 当前 origin 非 fork；必须使用上游 main 分支基线，不携带 Initial commit | G0 分支准备 |
| 本地能运行红绿测试吗？ | Rust/Flutter/FVM/FRB/OpenCode 未在常见路径发现，需要引导安装 | 是，T0 |
| 哪些公共接口值得测试？ | 第 3 节明确 S1–S5，用户已确认按本清单施工 | 已通过 |

## 2. 决策树与已发现的具体场景

已确定根节点：保留 Codex 通路；新增附接 OpenCode；公开 PR 为独立功能变更。

| 分支 | 反例场景 | 采用的规则 |
| --- | --- | --- |
| 所有权 | OpenCode 已在 4096 运行，Pocket 发布失败或用户退出 | 只回收 Pocket 资源；不认领 4096，不尝试重新启动 |
| 授权 | 用户在 A 会话选择 always，B 会话属于同一 instance | 展示 instance 范围，不声称仅 A 有效；不写入 OpenCode 配置 |
| 来源隔离 | 请求指定目录 A，但 session ID 实际属于 B | 取得 session 身份后拒绝；pending ID 同样校验；拒绝远程 workspace |
| 重复提交 | prompt 已被接收，204 返回前网络中断 | 显示 unknown，保留草稿和 message ID，核对后再允许明确重试 |
| 快照竞态 | GET 历史已包含某段文字，等待期间 SSE 也收到相同 delta | 将该消息标 dirty，用权威全量校准，不二次追加 |
| 用户切换 | A 会话历史很慢，用户已打开 B，随后 A 返回 | generation/session 校验，B 的视口、草稿、权限卡不被覆盖 |
| 多控制器 | 另一设备已回答 permission/question | 刷新待办，移除已消费项，不将 404 当作再次授权理由 |
| 共享代理 | 本机 HTTP_PROXY 指向系统代理 | loopback/relay 客户端显式 no_proxy；直连默认也不继承不透明系统代理 |
| 状态落盘 | 新应用保存 opencode 默认选择，用户降级旧应用 | OpenCode 独立文件，不写旧 strict TOML 与 Codex UI 默认项 |
| 超大事件 | 单个工具结果超过事件/响应预算 | 显式报告受限及校准失败，绝不静默截断后标完整 |

术语已记录到根目录 [CONTEXT.md](../CONTEXT.md)，关键架构权衡见 [ADR-0001](adr/0001-opencode-attached-hosting.md)。

## 3. 已确认的测试接口（Seams）

测试穿过下列公共接口，不测试私有函数调用次数、内部字段排布或整个生成文件快照。接口草案方法详见设计 §4.3，名称可按仓库规范调整，行为契约保持一致。

| ID | 公共接口 | 使用真实模块验证的行为 | 外部替身 |
| --- | --- | --- | --- |
| S1 | core 的 ServiceId 与 OpenCode profile/default store | 键解析、新旧配置共存、保存后读取、无秘密、旧默认不被改写 | 临时目录/时间 |
| S2 | OpenCodeClient 与对外 HTTP 网关 | 官方请求/响应、分页、认证、SSE、目录/对象白名单、响应预算与错误脱敏 | loopback 官方接口 fixture server，仅模拟外部 OpenCode |
| S3 | OpenCode 会话控制器及事件订阅接口 | 历史/增量合并、generation、重连、unknown 发送、审批/问题/abort、公共缓存读取 | 同一外部 HTTP fixture，可控时钟 |
| S4 | CLI 命令与 bridge 托管的公共 lifecycle | 启动发布/停止发布、断开、失败回滚、外部服务持续健康、状态文件隔离 | 本测试创建的外部服务进程、隔离 relay；真实 relay 单列集成测试 |
| S5 | Flutter OpenCodeApi 与用户可见控件 | 服务切换、历史锚点、草稿、权限范围、问题回答、停止与错误状态、Codex 默认回归 | 仅在 FRB 进程边界替换 OpenCodeApi/既有 BridgeApi |

测试中验证外部服务仍存活，使用独立 HTTP 健康访问，不依赖被测对象内部 PID。凭据泄漏检查是可观察输出契约，覆盖 debug/error/log/export/cache；不要求阅读真实用户秘密文件。

2026-09-27 用户要求根据任务清单和设计文档开始施工，并授权子智能体施工及验收；本表与第 1/2 节的施工共识已确认。

## 4. 环境基线 T0

调查事实：当前 PATH 与常见安装目录未发现 cargo/rustup、Flutter/Dart/FVM、FRB codegen、OpenCode。可用 brew、git、gh、Node/npm、Python 和 CommandLineTools；磁盘剩余约 154 GiB。Codex submodule 未初始化，不代表当前 first-party 编译一定依赖它。

- [x] T0.1 用户级安装固定 Rust `nightly-2026-06-01`，含 rustfmt/clippy/rust-src/wasm 目标；rustc `1.98.0-nightly (14210df0e 2026-05-31)`，未改 shell 配置与仓库 pin。
- [x] T0.2 准备 Flutter `3.44.0`/Dart、依赖并刷新 FRB 绑定；本机未单独安装全局 codegen CLI，生成文件由仓库锁定的 FRB 工具链核对。
- [x] T0.3 隔离工具目录安装 OpenCode `1.18.32` 并完成无模型接口核验；见 [运行核验](opencode-runtime-verification.md)，未读取用户凭据或附接用户会话。
- [x] T0.4 Rust fmt、workspace clippy（`-D warnings`）、workspace locked test 均退出 0；Flutter pub get、format、analyze、test 均退出 0（605 通过、3 跳过）。既有真实模型/原生集成测试的 ignored 不计作通过。
- [ ] T0.5 核实桌面构建依赖及 relay 测试环境；工具下载失败、无法编译、模型/平台缺失分别记录。

安装属于已授权施工的准备工作，可自主执行；不需让用户手工调查工具。真实模型凭据缺失不是纯协议/fixture 测试的阻塞，但真实模型验收必须标为未验证，不能因此宣称功能已全面通过。

## 5. 垂直切片任务

每一行是一个用户能力切片；行内从第一条行为开始，每次只写一个失败测试，然后实现最小通过代码并运行相关回归。不能先把下表所有测试批量写完。

| ID | 切片与接口 | 首个 RED 行为及后续关键反例 | GREEN 输出及验收 | 依赖 |
| --- | --- | --- | --- | --- |
| T1 | 来源身份与 profile（S1） | opencode service key 可以往返且 Codex key 不变；新增 profile 不污染旧配置；密码不出现在读取视图 | core 服务类型/独立 store；临时目录保存读取；旧 default fixture 兼容 | T0、接口确认 |
| T2 | 直连并查看历史（S2） | 通过 Basic 连接 fixture，按明确目录列会话并按 limit=20 读尾页；正数 limit、before、401、Link 外源 | host-svc `opencode/client,protocol`；真实 HTTP 测试证明字段、认证和分页信号 | T1 |
| T3 | SSE 实时接收（S2） | UTF-8 多字节跨网络块仍得到同一正文；snapshot/delta、心跳、截断帧、预算、取消 | 标准 SSE parser 与类型事件；unknown 事件不中断连接，坏帧不静默通过 | T2 |
| T4 | 发送、审批、问题、abort（S2） | prompt 返回204后仍须等待状态；逐一覆盖 permission/question；断线提交不自动重发 | typed 操作请求；当前 pending 匹配；abort 后上游健康；供应商配置默认沿用 | T2/T3 |
| T5 | 有限主机网关（S2） | 同目录历史通过，但跨目录 session/任意路由被拒绝；Authorization 注入不回流；恶意 header/query/path | `gateway` router、session/requestID 校验；流不缓冲；客户端 only 使用安全 origin/cursor | T2/T4 |
| T6 | 附接并发布（S4） | stop hosting 后本测试创建的既有服务仍健康；Ctrl-C/退出/relay失败/同名冲突回滚 | CLI `opencode serve/connect/status/stop` 与 bridge hosting；自建/账户传输；无上游 PID 所有权 | T1/T5 |
| T7 | 控制器状态一致性（S3） | 快照与 delta 交错不重复正文；旧 generation 返回不污染新会话；重连恢复 pending | bridge `engine/opencode`；ready/synchronizing/stale/unknown 状态明确，事件队列有界 | T3/T4 |
| T8 | UI 直连与浏览（S5） | 未配置 Codex/relay 的用户可直连并读历史；最近100条/搜索范围可见；旧首页默认不变 | 专用 OpenCodeApi/controller/page，FRB API 与生成绑定；首页/管理/引导分派 | T1/T2/T7 |
| T9 | UI 对话闭环（S5） | 发送后流式更新，busy留草稿；权限一次/实例范围/拒绝，question回答/拒绝，abort失败保持真实状态 | 可操作控件和本地化；离线/缓存无活审批；跨设备已回复刷新 | T4/T7/T8 |
| T10 | 缓存与历史续读（S3/S5） | 重启先只读显示缓存，新源不能继承旧审批/游标；分页失败显式重试；额度被Codex/OpenCode共享 | OpenCode只读历史适配器/投影；来源代次、原子保存、锚点/淘汰与命名空间隔离 | T7/T8 |
| T11 | 多设备可发现与账户兼容（S1/S4/S5） | 旧客户端列表不收到新种类，新客户端 opt-in 可见；不同账户同名仍隔离 | backend query协商、CLI选择、Flutter service_key/status、同机loopback优先 | T6/T8 |
| T12 | 整体回归与真实接口（S1–S5） | 先运行固定OpenCode隔离实测，再核对Codex与Responses HTTP/WS；失败分别记录 | 全套格式/clippy/test/analyze；桌面构建与手机/平板/桌面明暗UI；安全日志扫描 | T1–T11 |
| T13 | 文档与独立中文PR | 最终差异只含本功能；身份/fork/base正确；验证说明不虚报 | README/AGENTS里程碑、使用说明、测试事实矩阵；中文PR标题正文 | T12 |

### 文件定位

T1：`crates/pocket-codex-core/src/service.rs`、新增 `src/opencode/`。T2–T5：`crates/pocket-codex-host-svc/src/opencode/`、manifest。T6：CLI `cli.rs`/`commands/opencode/`/status/stop，bridge `engine/opencode/hosting.rs`。T7：bridge `engine/opencode/`、`api/opencode.rs`。

T8–T9：Flutter 新增 OpenCode API/控制器/页面和 tests；修改 `router.dart`、首页、管理页、引导、本地化；生成 FRB 文件。T10：host-svc 只读 adapter、bridge provider 独立缓存投影、Flutter 历史测试。T11：backend `api.rs`、core/account-proto 的 namespace 测试、CLI/Flutter 服务选择。T12–T13：已有测试与 docs，README、AGENTS。只有真实新增依赖才修改 Cargo.lock 或同步两份 pubspec。

### RED / GREEN 记录

每一切片记录：测试名、失败命令和退出码、失败是否命中预期行为、最小实现、通过命令/退出码、相关回归、剩余限制。依赖未安装、下载失败、语法错误或 unrelated baseline failure 不记为功能 RED。

- T1 已完成：服务键往返、独立 profile 保存读取/旧配置不变、写入与读取凭据 URL 拒绝、并发更新、未来版本、无效默认引用及 Debug 脱敏，8 个行为逐项退出 101 后修复为 0。最终 core 39、account-proto 17、CLI 44 个测试通过；三包格式和 clippy 通过。尚未验证 Windows 文件锁及跨进程并发。
- T10 来源身份前置切片：`direct_opencode_cache_identity_separates_profiles_origins_and_directories` 先因缺少公共 namespace 接口编译失败（E0425，退出 101；不是已运行的行为失败），增加 profile/origin/directory 独立哈希后通过（退出 0）。此项不代表缓存与历史续读已经实现。

## 6. 分支与发布

已创建分支：`feat/opencode-session-hosting`，基于目标上游 main `7d64e9db1cb786b3bb29902a6eeacd23f9e44bfe`。隔离 worktree 位于 `/Users/wangdejiang6/Downloads/pocket-opencode`；原目录和既有未跟踪设计文档保留，不重置 Initial commit。为避免下载无关历史，仅浅取上游当前提交；PR 分支的父提交真实属于目标仓库，需要比较更早历史时再定向 deepen。

比较原目录与上游发现原目录另有 5 个 macOS shell 环境相关文件差异，合计 192 行。本功能分支未迁入这些差异，保持独立 PR 范围。上游工作树的 AGENTS 与本次读取版本相同。

本次 `gh api user` 返回 `WaBranium`；不是未来发布的永久保证。创建 PR 前重新 `gh auth status`/`gh api user`，核对同一 fork 网络的 head、目标 base 及 merge-base diff。当前 origin `WaBranium/pocket-iteration` 非 fork，不直接把它作为跨仓 PR head。

业务施工结束前顺序运行 AGENTS §7 全套检查。真实模型未测则单列限制；必须通过的本地构建/协议验收未完成时不得宣称完成或创建可合并 PR，必要时仅提交明确说明限制的草稿供检查。

## 7. 当前执行账本

- [x] 读取用户指定 `/grill-with-docs`、其依赖 grilling/domain-modeling 和 `/tdd`。
- [x] 阅读现有源码、设计、官方研究和本机工具链事实。
- [x] 用户已接受的首版范围沉淀为词汇表及 ADR。
- [x] 整理施工风险、S1–S5 公共测试接口与 T0–T13 垂直任务。
- [x] 完成上游独立分支创建及文档迁入。
- [x] 用户确认本轮评估共识与 S1–S5 测试接口（2026-09-27：“根据任务清单和设计文档，开始施工”，并授权子智能体施工及验收）。
- [x] T0 Rust/Flutter 自动化环境基线通过；FRB 生成器准备中，缺完整 Xcode/CocoaPods 的原生 UI 验收限制单列。
- [x] T1 第一个行为的 RED → GREEN 以及来源身份/profile 切片完成。
- [x] T2–T12 已完成实现及自动化验收；T12 的真实模型 token、真实用户服务、真实 relay 部署、Windows/Linux/TLS 和原生桌面构建仍列为未验证限制。
- [ ] T13 文档最终复核、独立中文 PR。

评估不等于实现完成。Rust/Flutter 自动化基线已通过，可以进入 T1 行为测试。真实模型、relay 和原生 UI 验收仍须在对应切片执行或准确列为限制。
