# 设计:Pocket-Codex 移动端 UI(响应式 / Material 3)

- 日期:2026-06-01
- 状态:已通过设计评审,待 spec 复核
- 参考:用户提供的 ChatGPT/Codex App 截图(img1 会话列表、img2 会话详情)

## 背景与动机

`apps/flutter` 目前只是一个 FRB 骨架(仅 `greet`/`bridge_version`),没有真正
的 UI,也没有把 Rust 侧已有的能力(`init` 配置持久化、relay 服务发现、
pb-mapper 订阅、Responses API 代理)接到前端。

目标是做一个**移动端优先、响应式**的界面,让用户:

1. 首次填入 pb-mapper 连接地址 + 共享 key(支持 `pcx1:` base64 一键导入/
   导出),自动发现该 relay 上的 api / app-server 服务;填一次后持久化,后续
   自动复用。
2. 选中 **API 服务** → 订阅 → 在本地选一个端口跑出一个可用的 OpenAI 兼容
   `/v1` 端点。
3. 选中 **App-server 服务** → 进入类 Codex 手机端体验:查看远端最近 session
   及其实时进展(P2)。
4. 随时在设置里查看服务状态、变更 pb-mapper 地址。

## 目标

- 一套响应式布局(<600 手机单栏 / ≥600 平板·桌面双栏 master-detail),用
  Flutter 原生 **Material 3** 组件,跟随系统 light/dark。
- Rust bridge 成为引擎:配置、发现、订阅、(P2)app-server JSON-RPC 全在
  Rust;Dart 只做 UI。契合 roadmap #6「强类型 JSON-RPC 客户端」。
- 复用项目既有 Rust 实现,核心 crate 尽量零改动。
- 用上项目 logo(launcher icon / splash / 引导页)。

## 非目标(YAGNI)

- P1 不做 app-server 会话;P2 只做**只读监看**(发 prompt / 打断 / 转向留作
  P3,但导航与布局预留输入框位置)。
- 不做 Android 后台前台服务(订阅仅前台存活,见「平台现实」)。
- 不引入会取代 Material 3 的整套替代 UI 框架——Material 3 始终是骨架与设计
  语言。但**聚焦、维护良好的第三方功能型包是欢迎的**(如 markdown 渲染、代码
  高亮),用于 stock 控件做不好/做起来不划算的具体能力,详见 §8。

## 已确认的设计决策

| # | 决策点 | 选择 |
|---|---|---|
| 1 | JSON-RPC 协议层位置 | Rust 强类型核心(契合 roadmap #6) |
| 2 | 交付范围 | 整体 spec,分阶段实现(P1 外壳+API 流;P2 会话) |
| 3 | 导航骨架 | 推栈式(无底部 Tab,⚙ 在右上;宽屏 master-detail) |
| 4 | 视觉风格 | Material 3 + 跟随系统 light/dark 双主题 |
| 5 | P2 交互边界 | 只读监看(list + read + 实时事件流) |
| 6 | 状态/路由 | Riverpod + go_router(默认,标准选型) |
| 7 | 导入/导出格式 | `pcx1:` + base64url(JSON `{relay,key}`) |
| 8 | logo | launcher icon / splash / 引导页 hero |
| 9 | 第三方控件 | 允许聚焦的功能型包(markdown 渲染等);M3 仍是骨架,见 §8 |

<!--APPEND-->

## 详细设计

### 1. 架构与分层

```
Dart UI  (go_router + Riverpod)
   │  FRB 调用 / StreamSink<SessionEvent>
   ▼
Rust bridge (pocket_codex_bridge) ── 全局 tokio runtime + 订阅注册表
   ├─ config:   Config (toml @ supportDir/config.toml, unix 0600)
   ├─ discover: pb::keys(relay) → ServiceId 列表 (5s 连接超时已在 pb 层)
   ├─ api 订阅:  pb::subscribe(api key)  → 本地端口            [P1]
   └─ app 订阅:  subscribe(app key) → 本地 ws → JSON-RPC client
                 → typed SessionEvent 流                      [P2]
        ▼  relay (pb-mapper) → 宿主 pocket-codex serve / api serve
```

**与 CLI 的关键差异**:CLI 通过 spawn `__worker` 子进程 + 写 PID 到
`state.toml` 来托管订阅;移动端没有进程模型,改为在 **App 进程内**跑 tokio
任务。bridge 持有一个全局 `tokio::runtime::Runtime` 和一个「活跃订阅注册表」
(`HashMap<ServiceKey, JoinHandle + CancellationToken>`),`*_subscribe` 起任务、
`*_unsubscribe` 取消、`subscriptions()` 汇报状态。bridge 不读写 `state.toml`
(那是 CLI 的契约),自己用内存注册表管理生命周期。

**持久化路径**:`directories::ProjectDirs` 在移动端沙箱不可靠。改由 Dart 用
`path_provider` 取平台正确的 app-support 目录,经 `init_bridge(support_dir)`
传入;bridge 用核心的 `Config` serde 结构自行 `toml` 读写到
`<support_dir>/config.toml`(unix 设 0600)。**核心 crate 零改动**——`Config`
的字段与 `relay()/relay_key()/set_relay()/set_relay_key()` 访问器已是 public。

### 2. Bridge FRB 接口面(Dart ↔ Rust)

**P1**:
- `init_bridge(support_dir: String)` — 设定配置目录 + 初始化 runtime/日志。
- `import_config(text: String) -> ImportedConfig` — 认 `pcx1:` base64 或裸
  `host:port`/`key`;非法 base64 / key≠32 字节返回 typed 错误。
- `export_config() -> String` — 当前 relay+key 编码成 `pcx1:` base64url。
- `get_config() -> ConfigView` / `set_relay(addr)` / `set_key(k)`(32 字节校验)。
- `discover_services() -> Vec<ServiceId>` — 复用 `pb::keys` + `ServiceId::parse_key`。
- `api_subscribe(service_key, local_port)` — 起订阅任务,绑定本地端口后返回。
- `api_unsubscribe(service_key)` / `subscriptions() -> Vec<SubStatus>`。

**P2**:
- `app_subscribe(service_key, local_port)` → 本地 ws → JSON-RPC 客户端握手。
- `thread_list() -> Vec<ThreadSummary>` / `thread_read(thread_id) -> ThreadDetail`。
- `session_events(thread_id) -> Stream<SessionEvent>`(StreamSink):typed 变体
  覆盖 agent 消息增量、命令执行 + 输出、计划更新、turn 起止、状态变更。

类型(`ServiceId{device,kind,name}`、`SubStatus{key,local_addr,state}`、
`SessionEvent` enum 等)定义在 bridge 的 `api/` 模块,由 FRB 生成 Dart 镜像。

### 3. 界面与响应式

- 路由 `go_router`;状态 Riverpod;断点 **<600 手机单栏 / ≥600 双栏
  master-detail**(同一套路由,`LayoutBuilder` 切布局)。
- 导航:推栈式,无底部 Tab,⚙ 在 AppBar 右上(贴合参考图)。

**P1 屏**:
- `OnboardingScreen` — 顶部 logo hero(`poster.png`);粘贴 relay+key 或
  `pcx1:` 一键导入;校验通过 → 保存 → 跳发现。
- `ServicesScreen`(首页) — 头部:设备名 + relay + 状态点(绿/红);分组列出
  API / App-server 服务;右上 ⚙;空态显示 logo mark + 引导文案。
- `ApiServiceScreen` — 选本地端口(默认 18180,可改)→ 启动订阅 → 显示
  `base_url`(复制按钮)+ `[model_providers.pocket-codex-api]` 配置片段 +
  运行状态 + 停止;无鉴权安全提示。
- `SettingsScreen` — 改 relay、重导 key(掩码显示)、各服务状态、导出
  `pcx1:`、关于(版本 = `bridge_version`)。

**P2 屏**:
- `SessionListScreen` — `thread/list` 最近会话(相对时间 + 状态),下拉刷新,
  流式更新;布局对齐 img1。
- `SessionDetailScreen` — `thread/read` 渲染 + 实时事件追加;agent 消息走
  第三方 markdown 渲染包(候选见 §8,含代码块高亮),命令/输出走等宽块;
  active 时头部转圈;**底部输入框占位但 disabled**(P3 预留);布局对齐 img2。

### 4. Logo 使用

- 源:仓库根 `assets/logo/{logo.png 1254² , poster.png 1672×941}`。
- **资源位置约束**:Flutter 不能打包包目录之外的 asset,故把两图复制进
  `apps/flutter/assets/logo/` 并在 `pubspec.yaml` 的 `assets:` 声明;根
  `assets/logo/` 仍是源真相。
- launcher icon:`flutter_launcher_icons` ← `logo.png`(方形,全平台)。
- splash:`flutter_native_splash`,居中 logo,背景跟随 light/dark。
- 引导页 hero:`poster.png`;空态/AppBar:小号 `logo.png` mark。

### 5. 导入 / 导出格式

`pcx1:` + base64url(JSON `{"relay":"host:port","key":"<32B>"}`)。一键复制/
粘贴跨设备搬运;非法 base64 / key≠32 字节 → 内联报错。版本前缀 `pcx1`
为将来格式演进留空间。

### 6. 错误处理与平台现实

- relay 不可达 → 5s 超时即清晰报错;Settings 红状态点。
- 订阅断开 → 状态「重连中」(pb-mapper 自带重连)。
- **本地端点/订阅仅在 App 前台存活**(移动端无后台进程)。Android 后台长连
  需前台服务(带常驻通知),列为 P1 之后增强,不在 P1。spec 与 UI 都明示。
- 本地 API 端点在 `127.0.0.1` **无鉴权**——UI 明确提示(同 CLI 安全约定)。

### 7. 测试

- **Rust**:`pcx1:` base64 往返、config 读写 + 0600、`ServiceId` 分组/解析、
  订阅注册表 start/stop/status(纯逻辑单测;联网发现不强测)。
- **Dart**:FRB 调用包一层 `BridgeApi` Dart 接口,Riverpod provider 依赖该接口
  → widget/provider 测试用 fake,不依赖原生库。覆盖 onboarding 校验、服务
  分组、设置页、(P2)会话列表/详情渲染与事件追加。

### 8. 第三方依赖策略

Material 3 是骨架与设计语言;第三方包只用在 stock 控件做不好或不划算的**具体
能力**上,且必须:活跃维护、pub.dev 评分/likes 健康、与 Flutter 3.44 / M3
兼容、纯 Dart 或主流插件(避免冷门原生通道)。版本在 `pubspec.yaml` **精确
锁定**(脱字号收窄到次版本),并在 `pubspec.lock` 提交。最终选型与当时的维护
状态在 writing-plans 阶段核实后定稿。

**P1 已确定要引入的**(均为标准、广泛使用):
- `flutter_riverpod`(状态)、`go_router`(路由)——§决策 #6。
- `path_provider`(取 app-support 目录传给 `init_bridge`)。
- `flutter_launcher_icons`(dev_dependency,生成 launcher icon ← `logo.png`)。
- `flutter_native_splash`(dev_dependency,生成 splash ← logo)。
- `flutter_rust_bridge`(运行时;codegen 为 dev 工具)。

**P2 markdown / 代码高亮(候选,planning 阶段定一个)**:
- `gpt_markdown` — 面向 LLM 输出、流式友好、内置代码块/LaTeX;最贴 img2 场景。
- `markdown_widget` — 功能全(代码高亮、目录),通用。
- (`flutter_markdown` 曾是官方包,近期维护状态有变动,planning 时确认后再决定
  是否纳入候选。)
  选型准则:优先「流式追加 + 代码高亮」开箱即用、依赖树浅的那个。

**明确不引入**:整套替代 UI 框架(GetX UI、自绘设计系统等);仅为省几行就拉进
来的微依赖(自己写更可控的小工具函数不外包)。

## 分阶段交付

- **P1(独立可用 App)**:bridge 基础(init/config/import-export/discover/
  api 订阅)+ logo 接入 + Onboarding + Services + ApiService + Settings +
  响应式骨架 + 测试。
- **P2**:app-server 会话只读监看(`app_subscribe` + JSON-RPC 客户端 +
  thread list/read + 实时事件流渲染 + SessionList/SessionDetail)。
- spec 写完整愿景以保证导航/视觉/布局一致;writing-plans 阶段先出 **P1 计划**,
  P2 作为后续独立计划。

## 验收标准(P1)

- 冷启动 → Onboarding;`pcx1:` 导入或手填 relay+key → 保存 → 自动发现并在
  Services 列出该 relay 的 api / app 服务。
- 重启 App → 跳过 Onboarding,直接 Services(配置已持久化)。
- 选 API 服务 → 选端口 → 启动 → 本地 `http://127.0.0.1:<port>/v1` 可用(可被
  同机其它 app 当作 OpenAI 兼容端点),显示配置片段 + 停止。
- Settings 改 relay 即时生效;导出 `pcx1:` 可在另一台导入还原。
- 手机(<600)单栏、平板/桌面(≥600)双栏;light/dark 跟随系统;launcher
  icon / splash / 引导页均显示项目 logo。
- `flutter analyze` 干净、`flutter test` 通过、Rust bridge 单测通过。

## 风险与回滚

- **前台限制**是移动端固有,非缺陷;P1 接受此边界,文档明示。
- FRB 代码生成需 `flutter_rust_bridge_codegen`;接口面变动要重生成 + 重测,
  P1 接口刻意收敛以减少 churn。
- 改动集中在 `apps/flutter` 与 `crates/pocket-codex-bridge`;核心 crate 零改动;
  不动 CLI / `state.toml` / `deps/`。回滚即 revert 分支。

