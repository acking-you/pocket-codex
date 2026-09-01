# Handoff: `thread/list` 无响应导致连接反复重建

**日期**：2026-09-01
**状态**：根因未确认，卡点已收窄到被复用的 app-server 进程
**分支**：`chore/codex-upstream-sync`（有大量未提交改动，见文末）

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

即：**进程收下了请求却不处理**。这解释了为什么重启 app 无效 —— 问题跟着这个被复用的
进程走，而不是跟着 app。

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

1. **排除实验（最优先）**：`kill 96166`，让 app 拉起一个干净的 app-server 再复现。
   - 恢复 → 确认是长期存活进程劣化。接着查：那个进程为何僵（对它做 `sample` 时抓
     `thread/list` 处理路径）、以及 adopt 是否该加健康探测
   - 不恢复 → 卡点在客户端请求路径，回到 `client.rs` 与隧道层继续查

2. 若需在服务端复现，`thread/list` 的处理入口在
   `deps/codex/codex-rs/app-server/src/request_processors/thread_processor.rs`，
   注意它会取 `acquire_thread_list_state_permit()` —— 那是一个全局
   **`Semaphore::new(1)`**（`app-server/src/message_processor.rs:382`）。
   若某个先前的持有者未释放，`thread/list` 会永久阻塞，与观察到的现象吻合。
   **这条尚未验证**，是最值得先查的一条。

3. 顺带一个独立缺陷（与本问题无关，但值得修）：socket 健康标志
   `crates/pocket-codex-codex/src/client.rs:242` 在整个 bridge 里**零调用者**。
   看门狗（15 秒 ping / 20 秒判死，`client.rs:196-216`）判定 socket 已死，结论却从未
   传到 UI，所以状态栏在连接已死时仍显示"就绪"。

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

## 七、未提交的改动（21 个文件，全部门禁已过）

分支 `chore/codex-upstream-sync`，已有提交 `3106f22`（codex 子模块升级 +318 commits，
已推送）。工作区还有未提交内容，分三部分：

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
