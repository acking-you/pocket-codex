# Handoff: `thread/list` 无响应导致连接反复重建

**更新日期**：2026-09-05（下方保留 09-01 的故障现场记录）
**状态**：已修复可确定复现的连接状态、复用探测与分页缺陷；原 Mac 进程内部阻塞的根因仍未确认
**分支**：`chore/codex-upstream-sync`，交接时的分页与日志改动已在 `0a52779` 提交

## 本次续接结果（2026-09-05）

### 已确认并修复

- `AppClient` 原先仅在 ping 看门狗判死时清除健康标志。正常关闭、写入失败、
  RPC 超时后仍可能被 bridge 当作在线；请求超时也没有覆盖写锁等待。
  现在统一关闭状态、终止事件流并唤醒全部等待请求，取消请求时清理 pending。
  普通业务 RPC 错误不关闭连接。见 `crates/pocket-codex-codex/src/client.rs:253`、
  `crates/pocket-codex-codex/src/client.rs:288`、
  `crates/pocket-codex-codex/src/client_tests.rs:1`。
- Flutter 的内容加载函数会保留旧数据并吞掉异常；自动重连因此可能在新连接的
  第一个 `thread/list` 已失败后仍宣告就绪。现在流关闭立即发布离线状态，
  恢复完成前再次检查连接。已先用测试复现误报，再验证修复。
  见 `apps/flutter/lib/src/screens/app_session_screen.dart:1178`、
  `apps/flutter/lib/src/screens/app_session_screen.dart:2778`。
- 复用旧进程必须在一个总超时内通过 WebSocket、`initialize`、`thread/list(limit=1)`；
  HTTP 200 和 pong 不能单独证明请求处理正常。`0.0.0.0` 监听通过回环地址探测。
  运行中 CLI/桌面托管进程的既有看门狗也使用功能探测，连续三次失败进入既有
  重启流程。没有启动模型 turn；启动时拒绝复用不健康的外部进程，仍保留不擅自
  杀掉该进程的原有边界。见 `crates/pocket-codex-codex/src/readiness.rs:244`、
  `crates/pocket-codex-codex/src/readiness_rpc_tests.rs:1`、
  `crates/pocket-codex-bridge/src/engine/serve.rs:1442`。
- 分页历史的 `has_older` 以 item 分页游标为准。原逻辑在只有一个长 turn 时可能
  错误隐藏更早的 item；同时支持上游 user message 的 `content[]` 文本摘要。
  两项均有失败前后的回归证据。见
  `crates/pocket-codex-bridge/src/engine/app_session_pagination_tests.rs:1`。
- 账号中继凭证续期原先先清空缓存，backend 失联后连尚未过期的凭证也无法复用。
  现在续期成功后才替换缓存，网络请求与缓存读取使用不同锁；后台续期卡住时，
  同一账号仍能立即使用有效凭证，过期与账号切换仍拒绝复用。5 个回归测试覆盖
  失联、请求挂起、过期、成功续期与途中切换账号。见
  `crates/pocket-codex-bridge/src/engine/account_relay_tests.rs:1`。

### Windows UI 补充修复

- 会话正文接入中键自动滚动：点按后上下移动鼠标控制方向和速度；按住中键移动
  也可滚动，松开停止。再次点击、Esc、滚轮、离开正文或窗口失焦都会停止。
  沿用现有 ScrollController 和滚动通知，较早历史加载及阅读位置追踪仍生效。
  见 `apps/flutter/lib/src/widgets/middle_click_scroll.dart:13`。
- 分页游标不再把最后点击的 tick 持续当作悬停位置；离开游标与实际预览卡片后
  恢复宽度，保留可视区的位置高亮。展开区域中的透明空白也会结束悬停。
  见 `apps/flutter/lib/src/widgets/turn_minimap.dart:325`。
- 两个游标缺陷先由回归测试确认失败，修复后 19 项游标测试通过；另有 7 项
  中键行为测试和 1 项真实会话页面/虚拟列表测试，覆盖方向、停止条件与边界。
  Windows 正式 Release 构建成功，启动后已显示真实账号的会话正文。自动鼠标
  操作遇到窗口正在被用户操作的提示，未继续争用输入；交互行为由上述测试验证。

### 真实环境证据与限制

- Windows 本机使用固定子模块 `05daa82d` 编译出的嵌入式 app-server，复制当前
  Codex 的 185 个会话（168 legacy、17 paginated）及两个 SQLite 数据库。
  SQLite 使用在线 backup；副本中的 rollout 路径已指向副本。未复制登录凭据，
  未向模型发送 turn，未修改真实会话库。
- 第一轮 5 次新连接、15 次 `thread/list`、45 次 metadata read 全部成功；
  第二轮加入 resume、legacy 正文、paginated turns/items 分页也成功。
  因此本机新进程加真实数据副本**没有复现原始持续卡死**，不能据此确认 Mac
  长期存活进程内部阻塞的原因，更不能声称原故障已得到原位验证。
- 可重跑的独立诊断程序：
  `crates/pocket-codex-codex/examples/app_server_probe.rs:1`。
  使用 `--features embedded-codex`、`--embedded` 启动固定版本；`--resume` 会更新
  被恢复会话的元数据，应搭配独立 `CODEX_HOME` 副本。
- `lb7666.top` 中继经 SSH 只读检查仍正常；本次没有部署或重启生产中继。
  UI 验收使用 WSL pb-mapper 0.5.0 临时中继与 Windows 本地发布端。
- 真实 UI 回归入口：`apps/flutter/integration_test/history_recovery_test.dart:1`。
  配置需给出一个超过 100 items 的 paginated 会话和一个 legacy 会话，覆盖反复
  list、打开 paginated → legacy → paginated、滚动/点击加载更早历史。
  宿主机原生 Windows Profile 实测依次渲染 **100、82、100 items**，随后加载
  **41 个更早 items**，全程保持连接，测试通过。临时中继还需发布对应的真实
  `meta` 服务，否则可选配置的订阅等待会推迟正文显示。
  测试入口只挂会话页面，不初始化正式入口的系统托盘，也不套用完整应用主题；
  它不能替代 `main.dart` + `pubspec-desktop.yaml` 的完整桌面启动验收。
- 随后按桌面 manifest 构建正式 Windows Release：产物包含 Figtree、Geist Mono、
  Noto Sans SC（17,772,300 bytes）及 Windows 托盘 ICO。正式界面明亮/暗黑主题
  切换正常，验收后恢复原来的明亮设置；关闭主窗口后进程保持运行，重新启动
  可唤回同一窗口及 PID。
- 正式账号模式的冷启动另遇到 `/auth/refresh` 的 HTTPS 证书过期，发生在
  获取中继凭证之前，不能归为 `thread/list` 或 pb-mapper 数据通道故障。
  SSH 只读核验：backend `:8443` 使用的独立 PEM 在 **2026-09-02** 到期，
  Caddy `:443` 的证书已续到 **2026-10-31**；两者没有同步。
  已将 backend 的副本更新为 Caddy 当前有效证书，保持 `pcx:pcx`、0640，
  并重启账号服务；启用默认 TLS 校验的 `/healthz` 验证通过。原证书备份于
  服务器 `/etc/pocket-codex/tls-backup-20260905/`。未重启生产中继。
  自动证书同步任务尚未部署，后续续签仍需同步此独立副本。
- 包含缓存修复的正式 Windows Release 重编成功并启动。真实账号日志确认
  已直连 relay、`thread/resume`（2.13s）与 `thread/read`（2.75s）成功。
  Windows 首次网络访问的防火墙弹窗由用户自行处理；后续已观察到正式界面的
  真实会话正文，亮暗主题及关闭到托盘/重新唤回的验证见上文。
- **当前架构边界**：自建 relay 模式直接使用 relay + key；账号模式的 HTTPS
  负责登录、领取与续期凭证，会话数据直接走 pb-mapper。App 凭证缓存仍只在
  内存，冷启动仍需 backend；backend 发放的凭证 TTL 为 24 小时，到期会被
  relay 拒绝并断开隧道。因此当前实现是“backend 不在数据路径上”，并非
  “登录一次后永久不依赖 backend”。本次缓存修复没有改变凭证 TTL 或信任机制。

### 验证记录

- WSL：`cargo clippy --workspace --all-targets --locked -- -D warnings` 通过；
  `cargo test --workspace --locked` **289 passed，6 ignored**（已有手动/在线测试）。
- Windows：`cargo check -p pocket_codex_bridge --locked` 通过，确实编译嵌入式路径；
  Flutter 3.44.0 Windows Release 构建成功。
- Flutter：pub get、全量 dart format、analyze 通过；单元/组件测试
  **425 passed，3 skipped**（包含后续中键滚动与游标修复）。
- Rust 全部第一方包格式检查通过，Windows checkout 使用
  `-- --config newline_style=Auto` 保留已有 CRLF；默认 Unix 换行检查会对未改动文件
  报整文件换行差异，没有批量改写这些文件，也没有格式化 `deps/`。
- 原生 UI 集成测试：`flutter drive --driver=test_driver/integration_test.dart
  --target=integration_test/history_recovery_test.dart -d windows --profile
  --dart-define-from-file=<隔离配置文件>` 通过。驱动使用实时帧，并显示/聚焦 Windows
  窗口，避免逐帧等待因窗口在后台而挂起。配置键见测试文件顶部；不要选子代理或
  归档会话，它们会被上游默认列表过滤。
- 本机原始输出保存在忽略目录 `target/handoff-validation/`，包含真实会话副本，
  **不可提交或上传**。

### 如原症状再次出现

在故障原进程上先运行独立 probe（不加 `--embedded`），记录 initialize/list 是否
分别成功及耗时，再对同一进程抓栈。保留原进程证据后才做旧进程/新进程对照；
09-01 的 PID 96166 只是历史记录，不应按该 PID 直接杀当前机器的进程。

以下为历史现场，时间和环境均指 2026-09-01 的 Mac。

---

## 一、用户观察到的现象

1. 启动后**第一个会话**能正常加载，之后**所有**会话都很卡并自动失败
2. 报错为 `app-server connection closed`，有时是 `request 'thread/resume' timed out`
3. 早期版本静置几分钟能自愈；最新一次构建后**不再自愈**，启动即报错
4. 状态栏同时显示"就绪"，与连接已死矛盾

---

## 二、已确证的硬事实（都有实证，不是推理）

### 2.1 客户端日志的 72 秒周期

日志位置（本次新增的文件落盘，保留 6 小时）：

```
~/Library/Application Support/io.github.ackingyou.pocketCodex/logs/pocket-codex-<date>.log
```

```
11:57:39  thread/list id=2  SLOW 2.11s  ok=true     ← 唯一一次成功（= "第一个会话正常"）
11:58:41  thread/list id=7  TIMED OUT after 60.00s
          随后所有请求 → "Sending after closing is not allowed"
11:59:09  新建隧道重连
12:00:10  thread/list id=4  TIMED OUT after 60.00s  ← 无限循环
```

**关键：整份日志里没有任何一条 `thread_read`。** 卡死发生在会话列表阶段，本次新增的分页历史读取代码从未被调用到。

### 2.2 中继侧完全健康（已 ssh 到 lb7666.top 核实）

- `pb-mapper admin service list --all`：每个服务 `CONNECTIONS = 2`，配额 16，**无饱和**
- 所有连接 `health: Healthy`
- 隧道建立 `setup_elapsed_ms: 46~57ms`
- `journalctl -u pb-mapper-server`：**零 error、零 warn**
- 服务端与客户端同为 pb-mapper **0.5.0**

中继日志里 `client forward finished` 的间隔严格 72 秒，误差毫秒级：

```
12:24:29 / 12:25:41 / 12:26:53 / 12:28:05 / 12:29:17 ...
```

`72 = 60 + 12`，而 60 秒正是我们自己 `crates/pocket-codex-codex/src/client.rs:60` 的
`REQUEST_TIMEOUT`。**是我们的客户端主动断开**，中继只是忠实记录。

### 2.3 每周期只回 573 字节，且每次完全相同

```
forward finish! we send 573 bytes, detail:server->client
```

573 字节装不下 70+ 个会话的 `thread/list` 响应，且数值每次一模一样 —— 像是一个固定的
小响应（疑为 `initialize` 的应答），之后 app-server 再没回过任何东西。

### 2.4 被复用的 app-server 进程是空转的

`adopting codex app-server already on the listen port, pid=96166`

对 PID 96166 采样（`sample 96166 3`）：

- 所有 `tokio-rt-worker` 都停在 `parking_lot::condvar::Condvar::wait`（空闲等活）
- CPU **0%**，RSS 约 430–605MB，已存活 **5.5 小时**
- WebSocket 握手仍返回 `101 Switching Protocols`（传输层正常）

这些采样支持被复用进程可能不再处理请求的假设，但没有记录具体请求进入处理器的
证据；仅凭空闲线程栈不能确认阻塞位置。

---

## 三、当前最强假设（未验证）

**被复用的 app-server 进程内部劣化，能答 `initialize` 但不答 `thread/list`。**

支撑：2.3 的 573 字节 + 2.4 的空转采样 + 重启 app 无效。

### 关键可疑点：adopt 没有功能性健康检查

`crates/pocket-codex-codex/src/process.rs:412-438`

adopt 的判据只有「端口被占用」+「占用者是 codex 进程」，随后直接返回
`listener_confirmed: true`。**没有发一个真实请求验证它还能工作。** 一个僵掉的
app-server 会被无条件复用，且每次启动都复用同一个。

---

## 四、建议的下一步（按顺序）

1. **历史排除实验建议**：先确认仍是当时的故障进程，再停止它，让 app 拉起干净进程复现。
   - 恢复 → 确认是长期存活进程劣化。接着查：那个进程为何僵（对它做 `sample` 时抓
     `thread/list` 处理路径）、以及 adopt 是否该加健康探测
   - 不恢复 → 卡点在客户端请求路径，回到 `client.rs` 与隧道层继续查

2. **09-05 更正**：固定子模块中 `thread/list` 不直接获取此前描述的全局
   `thread_list_state` permit，不能把该信号量作为已定位的根因。
   应沿实际 request dispatcher 和 `thread_list` 调用链结合现场栈继续排查。

3. **09-05 更正**：`AppClient::is_alive()` 在 bridge 有调用者；真正可复现的问题
   是多个关闭路径没有更新标志，以及 UI 重连过程中吞掉加载失败后仍标为就绪。
   修复和测试见本文顶部。

---

## 五、我走过的弯路（避免重复）

三个曾被我当作根因、后被实证推翻的结论：

1. **`itemsView: "full"` 触发服务端嵌套循环** —— 机制真实存在
   （`thread_processor.rs:3213-3217` 逐 turn 串行、内层再按页拉完所有 item），已改为
   `notLoaded` + 独立 item 分页。但日志证明卡死发生在 `thread_read` 之前，**不是本问题的原因**。

2. **FRB 线程池被侧栏摘要占满** —— 机制也真实（`Normal` 模式走
   `thread_pool.execute`，池大小 = CPU 核数 = 12，每个调用 `block_on` 真阻塞），已给
   `threadSummaryProvider` 加了并发上限 3（`apps/flutter/lib/src/providers.dart`）。
   但同样**不是本问题的原因**。

3. **pb-mapper 控制连接泄漏** —— **完全错误，已撤回**。这来自一份 8 天前的记忆，但：
   - `deps/pb-mapper` 早已不是 submodule，现在是 registry 依赖 `pb-mapper = "0.5.0"`
     （`Cargo.toml:49`），`deps/` 下那些目录只是残留
   - 0.5.0 的 client 已用 `JoinSet` 重写控制池
     （`pb-mapper-client-0.5.0/src/server/mod.rs:536`），泄漏已修
   - 服务器实测连接数 2/16，毫无饱和

**教训：不要拿旧记忆当结论，先验证。** 我靠读代码猜了三轮，日志一次就定位了。

### pb-mapper 的续期与临时 key 逻辑已逐条读过，均健康，不必重查

- 控制心跳 2 秒 / 容忍 6 秒 / 租约 15 秒（`pb-mapper-core-0.5.0/src/config.rs:207-215`），
  日志中 `local_server_heartbeat_sent` 正常
- 临时凭据提前 **30 分钟**续期（`crates/pocket-codex-pb/src/keepalive.rs:33`）
- 凭据缓存带余量检查（`crates/pocket-codex-bridge/src/engine/account.rs:477`）
- 订阅侧 `keep_alive: true`（`crates/pocket-codex-pb/src/session.rs:93`）

---

## 六、环境与工具

- 服务器：`ssh ubuntu@lb7666.top`（密码在用户手里；`sshpass` 已装）
  - 中继诊断需要：
    ```bash
    export PB_MAPPER_SERVER=127.0.0.1:7666        # 注意不是 ..._SERVER_ADDR
    export MSG_HEADER_KEY=$(sudo cat /var/lib/pb-mapper-server/msg_header_key | tr -d '\n')
    sudo -E pb-mapper admin service list --all
    ```
- `gh` CLI 在这台 Mac 上不可用（keyring token 失效 + 公司 iOA 拦截 TLS）；推送走 SSH，
  PR 用预填 compare URL
- **磁盘极度紧张**：曾降到 581MB。`cargo test --workspace` 需 20G+，会耗尽磁盘，
  **请按 crate 分批跑**。可回收：`target/debug/{deps,incremental}`。
  **绝不可删** `target/debug/gn_out/obj/librusty_v8.a`（145MB，此网络环境下无法重新下载）
- Release 构建（单架构，照 CI 做法）：
  ```bash
  export RUSTY_V8_ARCHIVE=$PWD/target/debug/gn_out/obj/librusty_v8.a
  export FLUTTER_XCODE_ARCHS=arm64 FLUTTER_XCODE_ONLY_ACTIVE_ARCH=NO
  export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_PROFILE_RELEASE_LTO=false
  (cd apps/flutter && fvm flutter build macos --release)
  ```

---

## 七、历史交接改动（已提交到 `0a52779`）

分支 `chore/codex-upstream-sync`，已有提交 `3106f22`（codex 子模块升级 +318 commits，
已推送）。当时的工作区内容后续已提交到 `0a52779`，分三部分：

**1. 分页历史加载（功能，本次主要工作）**
- `crates/pocket-codex-bridge/src/engine/app_session.rs`：按 `historyMode` 分流，
  分页会话走 `thread/turns/list` + `thread/items/list`；legacy 保持整体读取；
  新增 `thread_older_page` / `thread_turn_items`；照抄上游 `advancing_cursor` 防游标死循环
- `crates/pocket-codex-bridge/src/api/bridge.rs` + FRB 生成物：新增
  `app_thread_older_page` / `app_thread_turn_items`，`ThreadHistoryDto` 加
  `has_older` / `turns`
- Dart：刻度栏改为骨架驱动（打开即显示完整会话长度）、滚动到顶与 hover 刻度按需补正文、
  顶部"更早历史"行（可点击，因内容不足一屏时无法滚动）

**2. 日志基础设施（本次新增，建议保留）**
- `crates/pocket-codex-bridge/src/engine/logging.rs`：日志落盘 + 6 小时保留
- `crates/pocket-codex-codex/src/client.rs`：每个 RPC 的方向/耗时/字节数/in-flight 深度，
  慢于 2 秒告警、超时报 error
- `app_session.rs`：`thread_read` 进出与各阶段耗时、bridge 并发深度

> 正是这套日志一次定位了 `thread/list`，此前三轮读代码推测全错。

**3. 一处防御性改动**
- `apps/flutter/lib/src/providers.dart`：`threadSummaryProvider` 并发上限 3

**门禁状态**：`cargo fmt` / `cargo clippy --workspace -D warnings` / Rust 123 测试 /
`dart format` / `flutter analyze` / Flutter 409 测试（含 6 个新增分页测试）全部通过。

**尚未验证**：分页功能的端到端 UI 行为。因为本问题（`thread/list` 卡死）导致会话
列表都加载不出来，分页路径压根没机会执行。
