# Pocket-Codex 使用指南（GitHub 账号模式）

> English version: [`docs/usage.md`](usage.md)

使用 Pocket-Codex 最简单的方式：**用 GitHub 账号登录**。一台机器保持登录
Codex，其它所有设备登录**同一个**账号后即可驱动它。无需配置中转地址（relay）
或共享密钥——后端会替你代理这些服务。

> 想自建中转（relay）？见 [README](../README.md) 的 *Self-host (advanced)* 一节。
> 想自己部署后端？见 [`deploy/README.md`](../deploy/README.md)。

---

## 0. 开始之前

- 必须有一个**可访问的 `pocket-codex-backend`**。它由某个人部署一次（你自己，
  或服务器的拥有者）。CLI 和 App 内置了一个默认后端；如需指向你自己的后端，给
  `login` 加上 `--backend https://你的主机`（会被记住），或在 App 的高级设置里
  填写。
- **宿主机**（对外暴露 Codex 的那台）需要在 `PATH` 中有可用的 `codex` 程序。
  客户端设备**不需要**安装 Codex——除非走 CLI 的 `connect` 路径（它会在本地
  运行 `codex --remote`）。

---

## 1. 用 GitHub 登录

### App

启动 App → 点击 **Sign in with GitHub**。它会显示一个短验证码并打开
`github.com/login/device`；输入验证码并授权。随后你会进入主界面，顶部显示当前
登录的账号。

### CLI

```bash
pocket-codex login
```

它会打印一个验证网址和一次性验证码。打开网址、输入验证码、完成授权。用以下命令
确认：

```bash
pocket-codex account     # → 已登录为 @you，模式 = account
```

---

## 2. 暴露你的 Codex（宿主机）

在已安装并登录 `codex` 的机器上：

```bash
pocket-codex serve
```

它会启动（或复用）本地的 `codex app-server`，并以你的账号注册它。让它保持运行
——这就是其它设备要连接的目标。

常用选项：

- `--name work`——同时跑多个（每个服务用不同的名字区分）。
- `--proxy http://…`——通过上游代理访问 `chatgpt.com`。

---

## 3. 从任意设备驱动

在另一台设备上登录**同一个** GitHub 账号，然后：

### App（推荐）

主界面会列出你的 app-server，并带一个实时健康状态点。点进去后：

- **新建对话**（**＋** 按钮）；选择一个工作目录。
- 输入提示词并发送。**思考**与**工具调用**会带着计时实时显示。
- 输入框上的控件：**模型**、**审批模式**、**工作目录**、**计划模式**、
  **思考强度**。
- **审批**——当 Codex 请求执行命令或应用补丁时，在此就地批准或拒绝。
- **停止**可中断正在进行的回合。
- 随时重新打开一个对话即可查看历史——若某个回合仍在进行，还会显示实时进度。

### CLI

```bash
pocket-codex services list      # 查看你注册的服务
pocket-codex connect            # 订阅；打印出对应的 codex 命令
codex --remote ws://127.0.0.1:28080
```

`connect` 会把远端的 app-server 暴露到本地端口，并打印出用于连接它的
`codex --remote …` 命令。当你有多个时，用 `--device <id>` / `--name <name>`
指定具体的宿主/实例。

---

## 4. 作为 OpenAI 兼容 API 使用（可选）

把你的 Codex 登录当作标准的 Responses API，从任意设备访问。

在宿主机上：

```bash
pocket-codex api serve
```

在任意设备上：

```bash
pocket-codex api connect        # 打印一段本地 model_providers 配置片段
```

把任意 OpenAI 兼容工具指向打印出的本地地址即可（它提供 `/v1/responses`）。
在 App 中，API 服务会出现在主界面，并带有自己的健康指示。

---

## 5. 状态与登出

```bash
pocket-codex account            # 你是谁 + 传输模式
pocket-codex status             # 本地 serve / api / codex 的状态
pocket-codex stop               # 停止本地的 serve / api
pocket-codex logout             # 吊销会话并清除本地令牌
```

在 App 中：**设置 → 账号 → 退出登录**。

---

## 常见问题

- **App 里显示「app-server 不可达」**——宿主机上的 `pocket-codex serve` 没在
  运行，或连接已断开。在宿主机上（重新）启动它。App 会周期性重新探测；用刷新
  控件可立即检查。
- **登录过期**——再次运行 `pocket-codex login`。会话有效期内会自动刷新，所以
  每台设备通常只需登录一次。
- **多个宿主或服务**——分别给每个起一个 `--name`，然后在 CLI 用 `--name` /
  `--device` 选择，或在 App 中点击对应的服务。
