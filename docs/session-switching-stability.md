# Session 切换断线排查与验证（2026-09-08）

## 已确认的主因

UI 打开 session 时先调用 `thread/resume`，再通过 `thread/read`、
`thread/turns/list`、`thread/items/list` 读取历史。resume 没有设置
`excludeTurns`，所以服务器还会额外恢复并返回一次完整历史。

真实桌面宿主的两个历史样本分别返回 25,820,325 和 36,948,729 字节。
客户端原先使用 tungstenite 默认配置，单帧上限为 16 MiB，消息上限却为
64 MiB。服务器以一个 WebSocket 帧发送 JSON-RPC 响应，因此这些合法历史
会触发客户端协议层关闭连接。读循环原先丢弃底层错误，最终只剩
`app-server connection closed`。

用旧上限再次复现时，实际错误为 WebSocket 1009：37,976,717 字节的帧
超过 16,777,216 字节上限。同一历史以 `excludeTurns: true` 返回 2,251 字节。
样本会随实时会话变化，以上大小来自各次独立采样。

## 修复

- Bridge 和 host meta service 的 resume 请求都设置 `excludeTurns: true`。
  历史仍通过现有分页接口读取，保留完整历史导航和运行时配置。
- AppClient 的帧上限与原来的 64 MiB 消息上限对齐，兼容较大的旧版历史。
  保留底层读错误、关闭帧、写失败和存活检测失败的原因。
- 已发送 RPC 的单次响应超时只结束该请求。迟到响应按 id 丢弃，不再关闭
  其他 session 共用的连接；写失败和真实连接故障仍关闭连接。
- UI 每次加载使用递增代次。切走后旧 resume 不再追加 history/config 请求，
  A→B→A 时第一次 A 的结果也不能覆盖第二次 A。
- 每个 service 的连接建立和断开串行化，避免并发重连互相移除刚建立的连接。
- Backend 续期失败后先查询 relay：只有确认旧 key 已不存在才重新签发，
  避免一次网络失败把同一账号的设备分到不同命名空间。缓存锁改为每账号一把，
  一个账号的网络操作不再阻塞其他账号读取缓存。
- `../pb-mapper`：订阅端连接 relay、注册端连接本地服务都具有超时；
  转发的两个本地 TCP 端也关闭 Nagle。补充真实大流量与小消息连续传输测试，
  并从订阅端 tracing span 中排除 credential。

不改变 CLI、持久化格式或 relay 协议。`excludeTurns` 是当前 Codex 已有字段；
没有改动 `deps/codex`。

## 验证结果

- Pocket-Codex 完整 workspace：309 passed、7 ignored；ignored 项为手工测试。
- Flutter：457 passed、3 skipped；analyze 无问题；Rust/Dart 格式检查通过。
- 两个仓库的 Clippy 通过。
- pb-mapper 完整 workspace：207 passed、3 ignored；随后新增的本地 TCP
  `TCP_NODELAY` 属性测试也通过。
- Backend 使用隔离的真实 relay 额外运行 13 项测试，覆盖签发、复用和失效。
  故障注入主动丢弃续期连接，确认不会再签发第二个命名空间；跨账号锁测试通过。
- WebSocket 回归覆盖 17 MiB 单帧、同连接 100 次请求、单次超时后其他请求继续、
  迟到响应、协议错误原因保留、peer close 和写超时。
- Flutter 回归覆盖 A→B→A 的迟到 resume 和迟到 history。
- pb-mapper 对明文/codec 两种模式分别传输 20 MiB 后，在同一连接继续进行
  200 次小消息往返，并验证半关闭后的尾部完整性。小消息 P95 均约 2.1 ms。

### 实际 Rust bridge 的完整历史加载

使用真实历史数据库的隔离副本，包含上述大历史及其 fork 祖先；不启动模型 turn。
每次切换执行实际 `thread_resume` + `thread_read`，包括历史分页和摘要读取，
在同一 WebSocket 上轮换三个 session。数据为本机开发构建，测试时还在编译其他目标。

| 路径 | 连续切换 | 中位数 | P95 | 最大值 | 断线 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 直连独立 app-server | 100 | 49.5 ms | 155.1 ms | 636.3 ms | 0 |
| 本地真实 pb-mapper relay | 100 | 45.0 ms | 129.1 ms | 166.5 ms | 0 |
| 修复后的 bridge/pb-mapper 联编，经本地 relay | 300 | 13.8 ms | 45.5 ms | 63.3 ms | 0 |

联编组单独运行，缓存也已预热，不能把两组差值全归因于 TCP_NODELAY。

单独比较 resume：两个真实样本从 1.44 s / 5.94 s 降到 9 ms / 25 ms，
响应缩小到约 2.2 KiB。该比较包括冷热状态差异，不能直接当作端到端 UI 加速倍数。

### 复跑

先准备独立 app-server（推荐复制历史数据库和相关 rollout，避免与正在使用的
session 争抢 writer），然后运行：

```sh
PCX_SOAK_WS=127.0.0.1:18880 \
PCX_SOAK_THREAD_IDS='<thread-a>,<thread-b>,<thread-c>' \
PCX_SOAK_ROUNDS=100 \
cargo test -p pocket_codex_bridge real_session_switch_soak -- --ignored --nocapture
```

要覆盖 relay，将 `PCX_SOAK_WS` 改为本地订阅端口。测试要求样本有历史内容。
它会 resume 指定 session，因此应使用独立副本或确认没有其他 writer 的会话。

## 接入与验证边界

Pocket-Codex 仍保留仓库原有的 registry 依赖 `pb-mapper = "0.5.0"`。
pb-mapper 的源码修复在独立仓库；正式接入应先发布修复版本，再更新这里的依赖和
Cargo.lock，不能提交只在当前机器成立的绝对路径依赖。本地联编可通过 Cargo 的
`patch.crates-io.pb-mapper-client.path` 和 `patch.crates-io.pb-mapper-protocol.path`
指向旁边的仓库；完成后恢复 registry lockfile。本轮已按此方式联编并通过上述 300 次切换验证。

从 Pocket-Codex 根目录执行本地联编（需要相邻的 `../pb-mapper` checkout）：

```sh
cargo \
  --config 'patch.crates-io.pb-mapper-client.path="../pb-mapper/crates/pb-mapper-client"' \
  --config 'patch.crates-io.pb-mapper-protocol.path="../pb-mapper/crates/pb-mapper-protocol"' \
  test --workspace --no-run
```

该命令会将 lockfile 的两个 registry 包临时解析到本地。运行前备份自己的
Cargo.lock，测试完成后恢复备份；本轮工作树没有保留本地路径 lockfile。

上述结果验证本地链路和隔离历史上的稳定性，不是公网延迟或长期在线 SLA。
当前运行的旧桌面进程在调查中也出现过 `/readyz` 无响应；线程采样已保存，
本轮没有把该进程的停顿归因于未经确认的 App Nap 或 backend。
源码修复需要重新构建/启动应用才会用于现有桌面会话；本轮未部署生产 backend/relay。
