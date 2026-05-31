# Pocket-Codex CLI 端到端验证指南

> 适用环境：relay 已部署在 `lb7666.top:7666`（pb-mapper **0.2.14**，systemd 托管，
> 已在线）。本指南让你在**一台机器上**用 loopback-through-relay 的方式跑通
> `pocket-codex` 的全部命令面，无需第二台设备。所有步骤已在 `lb7666` 主机上实测通过。

## 0. 环境现状（已由部署方备妥，你无需再动）

| 项 | 值 |
| --- | --- |
| relay 地址 | `lb7666.top:7666`（公网，`0.0.0.0:7666`） |
| pb-mapper 版本 | `0.2.14`（与本仓库 submodule 协议一致） |
| 启动参数 | `--pb-mapper-port 7666 --use-machine-msg-header-key` |
| 共享密钥文件 | `/var/lib/pb-mapper-server/msg_header_key`（32 字节，世界可读） |
| 旧版本备份 | `/opt/pb-mapper-server/pb-mapper-server.0.2.13.bak.20260530` |

> **为什么必须是 0.2.14**：客户端订阅侧的健康探测用 `PbConnStatusReq::Service{key}`，
> 这是 0.2.14 才引入的控制协议。若 relay 停留在 0.2.13，探测会每秒超时并反复重启
> 订阅 listener（日志刷 `remote key probe timed out after 1s` / `listener will restart`），
> 长连接被周期性打断。升级到 0.2.14 后该问题消失（实测 30s 内 churn=0）。

## 1. 前置条件

- 本机已安装 `codex` 且已 `codex login`（ChatGPT 账号，`~/.codex/auth.json` 存在）。
  `pocket-codex` 不自带模型运行时，宿主的 codex 登录态是唯一真相来源。
- 已 clone 本仓库并初始化 submodule：

  ```bash
  git submodule update --init --recursive
  ```

## 2. 构建 CLI

```bash
cargo build --release -p pocket-codex-cli
# 产物：target/release/pocket-codex
```

下文统一用：

```bash
export PCX=./target/release/pocket-codex
```

## 3. 【最关键】导出共享密钥 MSG_HEADER_KEY

relay 用 `--use-machine-msg-header-key` 启动，对**每一条控制消息**做 32 字节密钥校验。
客户端（含 `pocket-codex` 派生出来的 pb worker，它们继承父进程环境变量）**必须用同一个
key**，否则 relay 静默拒绝：CLI 会照常打印 "started"，但注册/订阅永远连不上 relay。

每开一个新终端，先执行（直接从 relay 拉取，免 sudo，保证一致）：

```bash
export MSG_HEADER_KEY="$(ssh ubuntu@lb7666.top 'cat /var/lib/pb-mapper-server/msg_header_key')"
echo "len=${#MSG_HEADER_KEY}"   # 必须是 32
```

> 这是排查失败的第一嫌疑。忘记 export，或 key 不是 32 字节，是最常见的"命令成功但
> 隧道不通"原因。

（已 `init` 配好 relay 和 key 的话，本节的手动 export 可跳过——`init` 会把 key 写入
config，之后所有命令自动读取，无需每次 export。）

## 3.1. 一次性初始化 relay（推荐先做）

```bash
$PCX init
# 交互填入：relay host:port、32 字节 MSG_HEADER_KEY
# 或非交互：
$PCX init --relay lb7666.top:7666 --key <32B> [--no-verify]
```

写入 `~/.config/pocket-codex/config.toml`（unix 下 0600）。之后所有命令在不带
`--relay` 时默认走这份配置，且会自动应用其中的 key——无需再 `export
MSG_HEADER_KEY`。

**解析优先级**：`--relay` flag > config > `$PB_MAPPER_SERVER` env。key 同理
`config > $MSG_HEADER_KEY`。所以 init 后即便 shell 里残留旧的
`PB_MAPPER_SERVER`，也以 config 为准。

**连接超时**：发现/状态查询现在对 relay 连接有 5s 上限——指向不可达 relay 时
~5s 内报错，不再卡满内核 TCP 超时（~123s）。

`init` 默认存盘前会连一次 relay 校验（✓ 列出服务数 / ✗ 报错不存盘）；relay
临时不可达时可加 `--no-verify` 跳过。

## 4. app-server 流程（serve → connect → codex --remote）

单机演双角色：`serve` 注册本地 app-server 到 relay，`connect` 再从 relay 订阅回来，
数据真实穿一次公网往返。

### 4.1 serve —— 起 codex app-server 并注册到 relay

> **⚠️ 国内必读：app-server 链路同样要走代理才能到 chatgpt.com。**
> spawn 出来的 `codex app-server` 本身就是 codex，它的**模型推理调用**、
> **codex_apps MCP**（连 `chatgpt.com/backend-api/wham/apps`）、插件 / 分析同步
> 都直打 chatgpt.com。这些请求只认**进程环境变量**里的代理，**不读 codex 的
> config.toml**；被墙网络下若没代理，codex_apps bootstrap 会超时
> （`MCP client for codex_apps timed out after 30 seconds`），模型调用一并卡死。
>
> 因此 `serve`（及 `codex start`）支持 `--proxy`，把解析后的代理注入子进程环境
> （`HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` 及其小写变体）。代理来源优先级：
> `--proxy` 显式参数 > `HTTPS_PROXY` > `ALL_PROXY` > `HTTP_PROXY` 环境变量。
>
> **务必用 `http://` 代理。** codex 的 reqwest 不支持 socks——`socks5://` 只能带
> 模型 WebSocket 一条路，codex_apps / 插件等 HTTP 流量仍直连、仍会超时。`https://`
> 代理会被拒绝。未配置任何代理时启动会打 warning。
>
> app-server 已在运行时再传 `--proxy` **不会自动重启**（只打 warning）：换代理需
> 先 `pocket-codex stop`（或 `pocket-codex codex stop`）再 `serve --proxy …`。

```bash
# 方式一：显式指定（推荐，最不易错）
$PCX serve --relay lb7666.top:7666 --proxy http://127.0.0.1:11111

# 方式二：继承环境变量（不传 --proxy）
export https_proxy=http://127.0.0.1:11111
$PCX serve --relay lb7666.top:7666
```

预期输出（`pcx:<本机hostname>:app:default`，下例 hostname=`lb7666`）：

```text
✓ codex app-server
    pid       ...
    listen    ws://127.0.0.1:18080
    log       .../codex-app-server.log
    proxy     http://127.0.0.1:11111
✓ pb register started
    pid       ...
    key       pcx:lb7666:app:default
    relay     lb7666.top:7666
    log       ...
→ client setup
    pocket-codex connect --key pcx:lb7666:app:default --relay lb7666.top:7666
```

`codex app-server` 块里的 `proxy` 字段确认代理已注入子进程；若没传 `--proxy` 也没有
env 代理，该字段不出现，转而向 stderr 打 `⚠ no upstream proxy configured ...`，
子进程会直连 chatgpt.com 而失败。

### 4.2 确认注册真的落到 relay 上

```bash
$PCX pb status --kind keys --relay lb7666.top:7666
```

应能看到自己的 key（`home-ubuntu`/`my-mac` 是 relay 上他人的服务，忽略即可）：

```text
Status:{ "Keys": [ "pcx:lb7666:app:default", "home-ubuntu", "my-mac" ] }
```

### 4.3 connect —— 从 relay 订阅回来，暴露成本地端口

```bash
$PCX connect --relay lb7666.top:7666
```

不带 `--key`/`--device` 时会自动向 relay 发现服务：本机只注册了一个 app 服务，
会被唯一选中。预期：

```text
✓ pb subscribe started
    pid       ...
    key       pcx:lb7666:app:default
    relay     lb7666.top:7666
    log       ...
→ codex remote
    codex --remote ws://127.0.0.1:28080
```

此时 `127.0.0.1:28080` 是「穿 relay 回到本地 app-server」的入口。

### 4.4 status —— 看全局会话

```bash
$PCX status
```

应有 3 条 alive：`codex` + `pb register` + `pb subscribe`，以表格形式输出：

```text
Pocket-Codex runtime status · lb7666.top:7666

┌──────────────┬───────┬───────┬──────────────────────┬────────────────────────┬────────┐
│ COMPONENT    ┆ STATE ┆ PID   ┆ ENDPOINT             ┆ KEY                    ┆ UPTIME │
╞══════════════╪═══════╪═══════╪══════════════════════╪════════════════════════╪════════╡
│ codex        ┆ alive ┆ ...   ┆ ws://127.0.0.1:18080 ┆ —                      ┆ ...    │
│ pb register  ┆ alive ┆ ...   ┆ 127.0.0.1:18080      ┆ pcx:lb7666:app:default ┆ ...    │
│ pb subscribe ┆ alive ┆ ...   ┆ 127.0.0.1:28080      ┆ pcx:lb7666:app:default ┆ ...    │
└──────────────┴───────┴───────┴──────────────────────┴────────────────────────┴────────┘
logs  ~/.local/state/pocket-codex/logs
```

`STATE` 列 alive 为绿、stale 为红；管道重定向或设了 `NO_COLOR` 时自动退化成无边框纯文本，方便脚本解析。

### 4.5 （可选）验证隧道数据面真的通

```bash
# 直连 app-server
curl -s -o /dev/null -w "direct  %{http_code}\n" http://127.0.0.1:18080/readyz
# 穿隧道（28080 -> relay -> 18080）
curl -s -o /dev/null -w "tunnel  %{http_code}\n" http://127.0.0.1:28080/readyz
```

两条都应是 `200`。隧道侧多出的延迟（~0.1s）即一次公网往返，正常。

### 4.6 codex --remote —— 真正驱动远端 app-server

```bash
codex --remote ws://127.0.0.1:28080
```

这会把 codex TUI 接到「穿 relay 的远端 app-server」。WebSocket 升级握手已实测
（直连与隧道都返回 `101 Switching Protocols`），TUI 应能正常进入会话。

> 真实多设备场景：第二台设备只需装 `pocket-codex` + `codex`（无需 codex 登录态），
> 设好同一个 `MSG_HEADER_KEY`，跑 `pocket-codex connect --relay lb7666.top:7666` 再
> `codex --remote ws://127.0.0.1:28080` 即可驱动你这台主机上的 Codex。

## 5. Responses API 代理流程（api serve → api connect）

给「没装 codex」的设备用：把宿主的 codex 登录态暴露成 OpenAI 兼容的 `/v1/responses`。

### 5.1 api serve —— 起本地代理并注册

> **⚠️ 国内必读：API 代理必须走代理才能到 chatgpt.com。**
> `api serve` 的上游是 `chatgpt.com/backend-api/codex`，国内直连必失败。
> （app-server 链路同样直打 chatgpt.com、同样需要代理，见 §4.1 的 `--proxy`，
> 别再以为「app server 不碰 chatgpt.com」。）两条上游链路都已支持代理：
> - **HTTP**（`reqwest`）：默认读 env 代理
> - **WebSocket**（codex 优先走这条）：之前完全不认代理，现已修复，会走同一个代理
>
> 代理来源优先级：`--proxy` 显式参数 > `HTTPS_PROXY` > `ALL_PROXY` > `HTTP_PROXY`
> 环境变量。支持 `http://`、`socks5://`（`https://` 代理会被拒绝——WS 隧道走的是
> 明文 CONNECT，无法对接自带 TLS 的代理）。未配置时启动会打 warning。
>
> 已存活的 worker 若用 `--proxy`/env 换了代理再跑 `api serve`，会自动重启 worker
> 让新代理生效（无需先手动 `stop`）。

```bash
# 方式一：显式指定（推荐，最不易错）
$PCX api serve --relay lb7666.top:7666 --proxy http://127.0.0.1:11111
# socks5 同理：--proxy socks5://127.0.0.1:1080

# 方式二：继承环境变量（不传 --proxy）
export https_proxy=http://127.0.0.1:11111
$PCX api serve --relay lb7666.top:7666
```

预期：

```text
✓ api proxy started
    pid       ...
    listen    127.0.0.1:18180
    log       .../api-proxy-pcx_lb7666_api_default.log
    proxy     http://127.0.0.1:11111
✓ pb register started
    pid       ...
    key       pcx:lb7666:api:default
    relay     lb7666.top:7666
    log       ...
→ client setup
    pocket-codex api connect --key pcx:lb7666:api:default --relay lb7666.top:7666
```

`api proxy` 块里的 `proxy` 字段确认代理已生效；若没传 `--proxy` 也没有 env 代理，
该字段不出现，转而向 stderr 打 `⚠ no upstream proxy configured ...`，上游会直连
chatgpt.com 而失败。

> 代理鉴权来源：先看 `CODEX_ACCESS_TOKEN` 环境变量，否则读 `~/.codex/auth.json`
> （`codex login` 写入的 ChatGPT token）。本机已登录，无需额外配置。

### 5.2 api connect —— 订阅并打印 Codex provider 配置

```bash
$PCX api connect --relay lb7666.top:7666
```

预期会打印一段可直接粘进 `~/.codex/config.toml` 的配置：

```text
✓ pb subscribe started
    pid       ...
    key       pcx:lb7666:api:default
    relay     lb7666.top:7666
    log       ...
→ codex provider config
    paste into ~/.codex/config.toml:
model_provider = "pocket-codex-api"

[model_providers.pocket-codex-api]
name = "Pocket-Codex API"
base_url = "http://127.0.0.1:28180/v1"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = true
```

### 5.3 验证 API 隧道连通

```bash
curl -s -o /dev/null -w "api tunnel %{http_code}\n" http://127.0.0.1:28180/
```

预期 `403`：非 `/v1/responses` 路径会命中代理的 fallback——能拿到 403 本身就证明
请求穿过了 relay 到达本地代理。真正的模型调用走 `/v1/responses`（由 codex 客户端发起）。

## 6. 服务发现与默认目标

```bash
# 列出 relay 上的 pcx:* 服务（裸 key 自动过滤）
$PCX services list --relay lb7666.top:7666
$PCX services list --kind api --relay lb7666.top:7666     # 按 kind 过滤

# 记录本地默认目标，之后 connect 不带 --device/--key 也能直接命中
$PCX services default set --kind app --device lb7666 --name default
$PCX services default set --kind api --device lb7666 --name default
# 写入 ~/.config/pocket-codex/config.toml 的 [services.*] 段
```

## 7. 低层 pb 命令（调试用，前台阻塞）

高层 `serve`/`connect` 把 pb worker 派生到后台、CLI 立即返回；要看实时报错时用低层
命令，它们前台运行、Ctrl-C 退出，错误直接打到终端：

```bash
# 注册任意本地 TCP 服务（先自己起个监听，如 python3 -m http.server 9000）
$PCX pb register --key demo --local-addr 127.0.0.1:9000 --relay lb7666.top:7666
# 另开终端订阅
$PCX pb subscribe --key demo --local-addr 127.0.0.1:9001 --relay lb7666.top:7666
# 查询 relay 状态
$PCX pb status --kind keys      --relay lb7666.top:7666
$PCX pb status --kind remote-id --relay lb7666.top:7666
```

## 8. 停止与清理

```bash
$PCX stop                       # 停掉本机所有受管会话（codex + api + 所有 pb worker）
$PCX stop --role subscribe      # 只停订阅，保留注册
$PCX stop --key pcx:lb7666:app:default   # 只停某个 key
```

日志目录：`~/.local/state/pocket-codex/logs/`
（`codex-app-server.log`、`pb-{register,subscribe}-<key>.log`、`api-proxy-<key>.log`）。

## 9. 排错速查

| 现象 | 原因 / 处理 |
| --- | --- |
| CLI 打印 started，但 `pb status` 看不到自己的 key | 99% 是 `MSG_HEADER_KEY` 没 export 或不是 32 字节。重做第 3 步。 |
| 订阅日志刷 `remote key probe timed out` / `listener will restart` | relay 版本 < 0.2.14。本环境已升到 0.2.14，不应再出现；若出现，确认连的是 `lb7666.top:7666`。 |
| `connect` 报 `multiple/no ... services found` | relay 上同类服务不唯一或没有。用 `--key` 或 `--device` 明确指定。 |
| `codex --remote` 连不上 | 先确认 `$PCX status` 里 codex + 两个 pb 会话都 alive，再确认 28080 在监听（`ss -tln | grep 28080`）。 |
| api 代理 502 / 鉴权失败 | 确认本机 `codex login` 有效（`~/.codex/auth.json`）；或设置 `CODEX_ACCESS_TOKEN`。 |

## 10. relay 升级记录（部署方操作，仅供追溯）

2026-05-30 将 `lb7666.top` 的 pb-mapper-server 从 **0.2.13 → 0.2.14**（与本仓库 submodule
匹配），消除订阅侧健康探测 churn。仅替换二进制，保留 systemd 单元、drop-in 配置与机器密钥：

- 新二进制：v0.2.14 官方 musl 静态包
- 旧二进制备份：`/opt/pb-mapper-server/pb-mapper-server.0.2.13.bak.20260530`
- 机器密钥：升级前后一致（从 hostname+MAC 派生，已持久化，重启不变）
- 回滚：停服 → 用备份覆盖 `/opt/pb-mapper-server/pb-mapper-server` → 启服

