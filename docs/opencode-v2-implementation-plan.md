# OpenCode 2.0.18 施工与验收记录

日期：2026-09-27。用户已批准按 [源码研究](opencode-2.0.18-source-research.md) 的顺序实施。

## 输出契约

- 保留原 v1 HTTP 客户端、Codex 功能和首页；新增独立的原生 v2 类型和客户端，不把 v2 消息伪装为 v1 parts。
- 连接层识别原生 v1/v2；附接网关采用显式版本化的 Pocket 协议，不能伪报上游接口版本。
- 只读本机发现读取官方注册文件并核对服务身份；不调用 ensure/stop，不启动或终止用户服务，不传出凭据。
- Flutter 在已有独立入口呈现两类原生消息、审批和 Forms。v2 持久授权需准确说明；未知或不安全表单明确拒绝操作。
- 真实模型请求、授权扩大和用户服务写操作不由只读验收代替；未经明确许可不对现有会话执行这些操作。

## 已批准的测试接口

1. 公开 HTTP 客户端与实际 loopback fixture：契约、身份、分页、原生消息、SSE、权限和 Forms。
2. 公开只读 discovery 与临时注册文件：文件安全、身份一致、无副作用、凭据脱敏。
3. 公开 bridge controller：选择、窗口保留、事件刷新、重连和异步竞争。
4. Flutter OpenCodeApi 和 widget：双版本消息、本机连接、当前待办、错误呈现；不 mock 内部实现。
5. 网关实际 HTTP：版本协商、允许路由、目录和 session 范围、停止网关不影响上游。

## 实施顺序

1. `host-svc/src/opencode/v2/`：固定 2.0.18 原生类型、身份和规范核验，然后逐片实现读、事件、写操作。
2. `host-svc/src/opencode/discovery.rs`：只读注册文件发现、安全校验和实际身份核对。
3. `host-svc/src/opencode/connection.rs` 与版本化 gateway：隔离 v1/v2 差异，原生消息跨层传递，明确网关契约。
4. `bridge/src/engine/opencode/`、`api/bridge.rs`：接入双版本，保留较早历史窗口，新增 Forms 回复与安全错误码；生成 FRB。
5. Flutter OpenCode 文件：本机显式连接、原生 v2 渲染、权限语义、Forms；首页不变。
6. 定向测试、全套 Rust/Flutter 门禁、真实隔离 2.0.18、用户服务只读验证、GitHub macOS 构建与桌面验收。

每片先验证 RED，再以最小实现转 GREEN。共享 Rust 构建串行，避免重复消耗磁盘。此文件记录施工，不表示上述步骤已完成。
