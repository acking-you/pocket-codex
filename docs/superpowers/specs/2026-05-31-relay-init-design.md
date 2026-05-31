# 设计:`pocket-codex init` + relay 配置持久化 + connect 超时修复

- 日期:2026-05-31
- 分支:`feature/relay-init`
- 状态:已通过用户设计评审,待 spec 复核

## 背景与动机

`pocket-codex services list`（以及任何走 relay 发现/状态查询的命令）在某些
环境下会“卡死”。实测定位到两个互相叠加的根因:

1. **默认 relay 指向死地址。** `PbRelayArgs.relay` 当前是 required，通过
   clap 的 `env = "PB_MAPPER_SERVER"` 回落。用户 shell 里
   `PB_MAPPER_SERVER=8.138.13.158:7666` 是一台**不可达**的 relay；活着的是
   `lb7666.top:7666`。不带 `--relay` 时就落到死地址。
2. **代码缺连接超时。** `pocket-codex-pb::query_status` 里的
   `TcpStream::connect` 没有超时包裹。连不可达主机时卡满内核 TCP connect
   超时（`tcp_syn_retries=6` ≈ **123 秒**），对用户就是“卡死”。

A/B 实测（同机）:

| relay | `services list` 表现 |
|---|---|
| `lb7666.top:7666`（活） | 0 秒返回，正常列出 2 个服务 |
| `8.138.13.158:7666`（死，当前默认 env） | 内核 TCP connect 123.1s 才超时 |

此外，`config.pb_mapper.relay` 字段在 schema 里**已存在但从未被读取**（死
字段），CLI 完全依赖 `--relay`/env。

## 目标

提供一个交互式的一次性初始化命令，持久化 relay URL 与共享密钥
（`MSG_HEADER_KEY`），让后续命令在不显式 `--relay` 时默认走这份配置；并修
复 connect 无超时导致的“卡死”体验。

## 非目标（YAGNI）

- 不改 `state.toml` 磁盘格式（userspace 契约，保持不动）。
- 不修 `pb status`（走上游 `handle_status_cli` 透传）在死 relay 上的久挂
  ——它是低层直通，本次不封装。
- 不做 key 轮换、多 relay profile、relay 增删子命令——无现实需求。

## 已确认的设计决策

| # | 决策点 | 选择 |
|---|---|---|
| 1 | relay 解析优先级 | `flag > config > env`（init 治本，不被陈旧 env 遮蔽） |
| 2 | key 存储 | 写入 `config.toml` + `save()` 时 `chmod 0600`（unix） |
| 3 | 是否修 connect 超时 | 修：给发现/状态查询加有界连接超时 |
| 4 | 命令形态 | 顶层 `pocket-codex init` |
| 5 | 可脚本化 | 交互优先 + `--relay`/`--key` flag 可跳过 |
| 6 | init 校验 | 默认连通校验 + `--no-verify` 跳过 |
| 7 | key 解析优先级 | `config > env`，与 relay 对称，init 一锤定音 |

## 详细设计

### 1. 命令面（userspace 契约）

新增顶层子命令 `pocket-codex init`，`InitArgs`:

- `--relay <url>`（可选）— 省略且 stdin 为 TTY 时交互提示，默认值预填现有
  config 的 relay。
- `--key <32字节>`（可选）— 同上；校验恰好 32 字节，否则报 pb-mapper 同款
  长度错误（`MSG_HEADER_KEY must have 256 bit(32 byte)`）。
- `--no-verify`（flag）— 跳过连通校验，仅写盘。

交互/非交互行为:

- `--relay` / `--key` 缺失 **且 stdin 是 TTY** → 逐项提示输入。
- 缺失 **且非 TTY**（CI/管道/无 TTY 会话）→ 直接报错
  `non-interactive environment: pass --relay and --key`，**不挂起**。

### 2. 配置存储

`PbMapperConfig` 在现有 `relay: Option<String>`（当前死字段）旁新增
`key: Option<String>`:

```rust
pub struct PbMapperConfig {
    /// Bare `host:port` of the upstream pb-mapper relay.
    pub relay: Option<String>,
    /// Shared 32-byte MSG_HEADER_KEY for relay control messages.
    pub key: Option<String>,
}
```

- `relay` 规范化为裸 `host:port`:剥掉可选 `tcp://` 前缀，校验同时含 host
  与 port。
- `Config::save()` 在 unix 下把 config.toml 权限收紧到 `0o600`
  （`#[cfg(unix)]` + `PermissionsExt`）——保护 key 这个 secret，也顺带收紧
  现有文件（行为变更但更安全、非破坏）。
- 新增访问器:`relay()`、`relay_key()`、`set_relay(...)`、
  `set_relay_key(...)`。

**向前兼容注记**:`Config` 上有 `#[serde(deny_unknown_fields)]`，因此旧二进制
读到带 `key` 字段的新 config 会反序列化失败。同代/新二进制不受影响。此为已知
限制，写入文档。

### 3. 解析与 key 应用（核心机制）

**relay 解析。** `PbRelayArgs.relay` 由 required 改为 `Option<String>`，并
**去掉** clap 的 `env = "PB_MAPPER_SERVER"`（改为我们自己读 env，以掌控优先
级）。新增解析函数:

```text
resolve_relay(flag: Option<&str>, config: &Config) -> Result<String>
  = flag(非空)
  > config.pb_mapper.relay
  > $PB_MAPPER_SERVER env
  > Err("no relay configured; run `pocket-codex init` or pass --relay")
```

7 个命令文件、共 19 处 `args.relay.relay` 引用统一改调 `resolve_relay`
（`services.rs`、`pb.rs`、`serve.rs`、`connect.rs`、`worker.rs`、
`remote_hint.rs`、`api.rs`）。worker 子命令由父进程在 argv 里显式带上 relay，
照样命中 flag 分支。

**key 应用钩子。** `commands::dispatch` 开头（任何 worker spawn / relay 查询
之前）加载 config:

- `config.pb_mapper.key == Some(k)` → 调
  `pocket_codex_pb::set_process_msg_header_key(Some(k))`。该上游函数同时:
  ① `set_var(MSG_HEADER_KEY)`（供 spawn 的 worker 子进程继承——已确认
  `spawn_worker` 用 `Command::new(exe).args(...)` 且**无** `.env_clear()`，
  默认继承父 env）；② 更新本进程运行时 key（供进程内发现查询）。
- `config.pb_mapper.key == None` → 不动，沿用现有 `$MSG_HEADER_KEY` env
  （向后兼容）。

即 key 优先级 **config > env**，与 relay 对称，init 一锤定音。
`set_process_msg_header_key` 经 `pocket_codex_pb` 薄封装暴露，使
`pocket-codex-pb` 仍是与 pb-mapper 交互的唯一边界；其内部 `set_var`/`unsafe`
封装在上游 crate，本仓库各 crate 的 `#![forbid(unsafe_code)]` 不受影响。

### 4. connect 无超时修复

`pocket-codex-pb::query_status` 里的 `TcpStream::connect` 用
`tokio::time::timeout` 包裹（默认 5s，时长可注入以便测试），超时给清晰错误
`connecting to pb-mapper relay {addr} timed out after {d:?}`。覆盖
`keys()` → `discover_services` → `services list`，以及
`service_connections()`。

`get_status` 内部对 write/read 两段本就有 30s `control_io_timeout`（上游
已实现），本次只补缺失的 **connect** 段。

### 5. init 执行流

1. 载入现有 config。
2. 取 relay:`--relay` 否则交互提示（默认=现有值）。
3. 取 key:`--key` 否则交互提示（默认=保留现有）；校验 32 字节。
4. 规范化 relay → 裸 `host:port`。
5. 非 `--no-verify`:先 `set_process_msg_header_key(Some(key))`，再
   `discover_services(relay)`（已被 5s 连接超时兜底）。
   - ✓ 打印 `reached relay, N service(s)`。
   - ✗ **不存盘**，报错并提示:relay 临时不可达可加 `--no-verify` 跳过。
6. 存盘:写 relay + key 到 config.toml（unix 下 0600）。
7. 摘要输出，**key 脱敏**（只显 `len=32`，不回显明文）。

### 6. 错误处理与测试

- **core**:config 带 key 往返序列化；`save` 在 unix 设 0600；
  `normalize_relay`（含 `tcp://` 剥离、缺 port 报错）、`resolve_relay` 优先级
  与无配置报错——均为纯函数，单测。
- **pb**:连接超时——对真实死地址计时易 flaky，故将超时时长做成可注入参数，
  单测验证错误映射；死 relay 123s → 有界已**实测**（见背景表）。
- **cli/init**:I/O 壳尽量薄，把 `normalize_relay` / `validate_key` / 源选择
  抽成纯函数单测；完整交互流走手动验证。

## 验收标准

- `pocket-codex init --relay lb7666.top:7666 --key <32B>` 写盘成功（0600），
  连通校验打印服务数。
- 之后 `pocket-codex services list`（不带 `--relay`）走 config，秒回。
- shell 里残留 `PB_MAPPER_SERVER=<死地址>` 时，config 仍优先，不再卡死。
- 指向死 relay 时，命令在 ~5s 内报超时错误，而非挂 123s。
- 既有 `--relay` 显式用法、`$PB_MAPPER_SERVER` 回落（无 config 时）保持可用。

## 风险与回滚

- **行为变更**:relay/key 现在 config 优先于 env。对依赖 env 覆盖的用户是语义
  变化，但正是本次要的修复方向；写入 README/文档。
- **0600 收紧**:对现有 config.toml 是权限变化，更安全、非破坏。
- 改动集中在 CLI + core + pb 三 crate；不动 `state.toml`、不动 Flutter、
  不动 `deps/`。回滚即 revert 本分支。

