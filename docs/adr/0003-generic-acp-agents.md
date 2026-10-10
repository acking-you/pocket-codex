---
status: accepted
---

# 通用 ACP 智能体由主机持有唯一客户端，经版本化网关接入同一套会话界面

用户希望连接任意 ACP（Agent Client Protocol）智能体，只配置「程序 + 参数列表」，不经过 shell；OpenCode（`opencode acp`）是第一个预设。Codex app-server 仍是独立的原生引擎；OpenCode 与其他智能体共用 ACP 接入路径。

决定：
- **主机持有唯一的 ACP 客户端。** ACP 是单客户端的 stdio 协议，因此由 host-svc 的 `AgentHost` 启动并独占智能体进程，负责生命周期、代次（generation）、事件日志和条目折叠。控制器不接触 ACP 帧。
- **控制器通过版本化的 `/acp/v1` HTTP + SSE 网关访问**，服务键为 `acp:<name>`，meta 服务照旧为 `meta:<name>`。快照带代次和序号水位，断线后从水位续读；位置已被丢弃时网关发送 `reset`，桥接层重新读取快照。
- **桥接层引入 `SessionEngine` 边界**，有 `CodexEngine`、`AcpEngine` 两个实现。协议身份只看服务键的 kind（`app`／`acp`），不看提供方名称；不支持的操作返回稳定的「不支持」错误，而不是发给错误的协议。
- **能力按协商结果、保守地开启。** 智能体完成初始化之前不开放任何可选能力；Flutter 的所有控件都按能力显示，包括语音、听写预热和快捷键。
- **Windows 暂不支持托管。** 进程组清理需要作业对象（job object），在正确实现之前直接关闭这一入口。

我们没有选择让每个控制器各自启动或直连智能体：stdio 只能有一个客户端，多设备也无法共享同一进程。

也没有选择把 ACP 伪装成 Codex app-server：sandbox、审批策略、线程元数据等 Codex 语义无法忠实映射。

Codex 和 ACP 主机共享实例名称表，防止服务键冲突。OpenCode 只是程序及参数预设，不引入专用引擎、服务类型或偏好结构。

代价：
- 主机侧新增一个有状态模块（进程、日志、转录稿），重启后只保留已持有的历史；
- 智能体参数以明文保存在偏好中，界面提示不要在参数里放密钥；
- 较旧的后端会忽略 `include_acp`，账号模式下无法发现远端 ACP 主机；
- 已用模拟智能体覆盖协议及竞态，并在 macOS 上用真实 OpenCode 1.18.35、独立 host/controller 进程经公网 relay 验证流式回答、权限、取消及恢复；这不代表其他智能体、第二台物理设备或其他平台均已验证。

详见 [ACP agents](../acp-agents.md)。
