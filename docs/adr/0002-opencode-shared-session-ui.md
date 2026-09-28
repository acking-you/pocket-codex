---
status: accepted
supersedes: 分支 feat/opencode-session-hosting 中的 ADR-0001（独立 OpenCode 协议通路与独立页面）
---

# OpenCode 复用同一套托管与会话界面，协议适配放在桥接层

用户要求托管时选择 Codex 或 OpenCode，之后的托管和对话交互完全相同，只有底层协议不同。因此不再采用旧分支的独立 `/opencode` 页面：它的功能是 Codex 会话界面的子集，而且两套界面会逐渐分叉。

协议翻译放在 Rust 桥接层：桥接层新增 OpenCode 引擎，现有 `app_*` 接口按服务类型分发，把 OpenCode 的会话、消息、工具调用和待办翻译成界面已有的数据形态，并附带一份提供方能力描述。界面只按能力隐藏或显示控件。

主机侧只做透明的 OpenCode 中转网关：把 `/api/*` 限定在回环地址并注入 Basic 凭据。它不把 OpenCode 伪装成 Codex app-server，因为 sandbox、审批策略等 Codex 语义无法忠实映射，这样做也违背「忠于上游、不叠加兼容垫片」的原则。

我们没有选择在 Flutter 层做双实现：那需要重构约 9,400 行会话界面，风险最大。

代价：
- 桥接层要维护第二个协议引擎；
- 界面需要引入能力开关；
- 一部分 Codex 专有控件在 OpenCode 下不显示。

仍然保留旧决定中的两点：
- 附接托管：停止托管时不关闭 OpenCode，只在找不到服务时请 OpenCode 自行启动后台服务。
- 不读取提供方的登录凭据。
