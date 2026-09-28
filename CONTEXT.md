# Pocket-Codex 会话控制

本词汇表描述 Codex 与 OpenCode 共存时的托管与会话控制概念。

## Language

**服务提供方（Provider）**：实际运行会话的产品，目前为 Codex 或 OpenCode。托管时由用户选择；每个托管实例只属于一个提供方。
_Avoid_: 把所有提供方都称为 Codex、引擎。

**主机（Host）**：能访问会话服务、代表用户发布远程访问能力的设备。
_Avoid_: 将中转服务器称为主机。

**控制器（Controller）**：用户查看历史、发送消息、处理待办、停止执行的设备端应用。同一套会话界面服务所有提供方。
_Avoid_: 将控制器本地目录当作远程会话目录。

**托管实例（Hosted Instance）**：主机上以「提供方 + 实例名」标识、发布到中转的一组服务。
_Avoid_: 服务、端口。

**附接托管（Attached Hosting）**：发布用户已有的提供方服务的访问能力，不取得该服务进程的所有权。OpenCode 托管采用这种方式；必要时可以请提供方自行启动它的后台服务，但启动后进程仍归提供方管理。
_Avoid_: 接管进程、托管即拥有进程。

**项目（Project）**：会话所属的工作目录。Codex 取会话的 cwd，OpenCode 取会话的 location.directory。
_Avoid_: 仓库、工作区（在不引起歧义时可作口语）。

**会话（Session）**：提供方中的一条对话。Codex 称 thread；OpenCode 称 session。
_Avoid_: 线程（面向用户的文案中）。

**子会话（Child Session）**：由另一会话派生的会话（例如 OpenCode subagent）。不单独列在侧边栏，而是在父会话中呈现。

**断开连接（Disconnect）**：控制器停止观察和访问服务，不改变会话的执行状态。
_Avoid_: 停止执行、停止托管。

**停止托管（Stop Hosting）**：主机撤销自己发布的访问能力。附接托管下，提供方服务继续运行。
_Avoid_: 关闭 OpenCode 服务。

**停止执行（Abort / Interrupt）**：请求会话停止当前执行，服务本身继续可用。
_Avoid_: 杀进程、退出服务。

**排队发送（Queue）/ 补充（Steer）**：会话运行中发送消息时，排队发送等当前执行结束后作为新的一轮处理；补充则并入当前执行。

**权限待办（Pending Permission）**：服务当前等待用户授权的请求。历史中出现过的授权文字不属于待办。
_Avoid_: 把历史审查结果当作可执行审批。

**问题待办（Pending Question）**：服务当前等待用户回答的问题（OpenCode 称表单 form），与权限待办分开处理。
_Avoid_: 将回答问题等同于批准权限。

**提交结果未知（Submission Unknown）**：控制器无法确认服务是否已接收一条消息时所处的状态。
_Avoid_: 发送失败、可以安全重发。

**能力（Capability）**：提供方是否支持某个界面功能，例如 Fast、sandbox 预设、推理强度。界面按能力决定显示哪些控件，不按提供方名称判断。
