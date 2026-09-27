# OpenCode 2.0.18 验收记录

日期：2026-09-27。平台：macOS arm64。

本记录区分 Rust 后端、打包原生桥接库和完整桌面交互。通过 HTTP 测试不等于桌面包已经验收，也不代表真实模型执行已经验收。

## 会话打开失败回归

用户在桌面端选择非服务默认目录后，能列出会话，但打开会话显示“请求失败，请重试”。旧包 `447fbde` 的真实桥接库也复现 `OpenCode object is outside the selected project`。

用户随后补充：输入的是 HOME 下的 Downloads 目录，列出会话后点击“新建会话”失败。`OpenCodeController::create` 在上游创建成功后立即调用 `open_session`，因此同一错误也会使新建流程显示失败，不能仅凭提示认定上游没有创建会话。新增打包桥接库测试在隔离的 HOME/Downloads 中执行同一路径，旧包同样在 `RustBridgeApi.create` 返回上述错误；没有在用户目录创建测试会话。

根因是 OpenCode 2.0.18 的 `/api/permission/request` 和 `/api/form` 使用 `location[directory]` 查询参数。旧实现传入 `directory`，被上游忽略并回退到服务工作目录，随后被 Pocket 的项目范围检查拒绝。官方源码依据为 `packages/protocol/src/groups/location.ts` 的 `LocationQuery` 与 `packages/server/src/location.ts` 的 location middleware。会话列表接口的 `directory` 参数仍然正确。

修复只改变这两个请求的查询参数，保留目录范围校验。新增 bridge 回归测试通过公开 `OpenCodeController::open_session` 复现上游回退行为，先失败后通过；HTTP fixture 同步检查真实参数，隔离真实服务的工作目录与会话目录也刻意不同。

## 用户服务只读验收

本机官方注册文件报告 OpenCode `2.0.18`，注册文件为当前用户所有、权限 `0600`，服务 PID 与 `/api/info` 一致。验收使用只读 discovery，凭据只在 Rust 内存中使用；没有调用 ensure、stop、重启、prompt、create、interrupt 或任何审批接口。

运行命令：

```sh
PCX_LIVE_OPENCODE=1 \
PCX_REQUIRE_LIVE_SESSION=1 \
PCX_OPENCODE_DIRECTORY="<existing-project-directory>" \
  cargo test -p pocket-codex-host-svc --test opencode_live_readonly \
  -- --ignored --nocapture
```

初次验收仅选了没有会话的 HOME 目录，且它恰好等于服务工作目录，遗漏了本次故障。这不是用户服务没有历史，而是测试选择了错误的样本范围。

修复后的复测选择用户已有的非默认 OpenCode worktree 目录，并强制要求存在会话和非空历史。结果：`1 passed`。直连和 Pocket loopback gateway 均成功读取真实会话及历史，比较会话元数据和历史正文一致；状态、权限、Forms 和 SSE 连接也通过。测试停止的只有自有 gateway，随后再次读取上游 `/api/info`，版本和 PID 未改变。没有为制造样本写入用户服务。

针对用户补充的 Downloads 目录也执行同一只读验收，结果为 `1 passed`；该次没有开启非空历史要求，不将它视为正文验收的替代。

## 隔离真实服务验收

使用安装的官方 `opencode v2.0.18` 二进制，在临时 HOME、XDG 目录、项目目录和随机服务密码下启动一个测试自有服务。服务默认目录与会话项目目录不同。Rust 测试创建一个空会话，读取直连列表、空历史和权限/Forms，再通过 Pocket gateway 读取并比较，最后只回收测试自有进程。

运行命令：

```sh
PCX_RUN_REAL_OPENCODE=1 \
PCX_TEST_OPENCODE_BINARY="$HOME/Library/Application Support/ai.opencode.desktop/cli/2.0.18/opencode-cli" \
  cargo test -p pocket-codex-host-svc --test opencode_v2_live \
  -- --ignored --nocapture
```

结果：`1 passed`。未发送模型 prompt，所以没有验证供应商调用、真实输出、工具执行或真实审批生命周期。测试完成后只剩用户原有的 OpenCode 进程。

## 自动化结果

- OpenCode v2 合同测试：13 项通过。
- 版本化 Pocket gateway v2：5 项通过，覆盖协商/分页、SSE、prompt/interrupt、权限/typed Forms、目录和 session 越权、停止 gateway 不影响上游。
- v1 HTTP/gateway 回归：13 项 HTTP、4 项 gateway 通过。
- discovery：10 项通过；连接协商：2 项通过。
- bridge Rust 测试：107 项通过，6 项按现有规则忽略。
- `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --locked` 均通过。
- Flutter 依赖解析、格式检查、`flutter analyze` 均通过；普通 `flutter test` 为 638 项通过，依赖真实服务的测试默认跳过并单独显式执行。

## 打包桥接库验收方法

`apps/flutter/test/opencode_live_bridge_test.dart` 使用真实 `RustBridgeApi` 和 DMG 配套 ZIP 中的原生 framework，执行与客户端相同的发现、列出会话、选择会话、加载较早历史和重新打开会话。测试要求存在非空历史，临时 support 目录不读取或覆盖当前客户端配置，也不启动应用的自动托管。

```sh
cd apps/flutter
PCX_LIVE_OPENCODE=1 \
PCX_OPENCODE_DIRECTORY="<existing-project-directory>" \
PCX_OPENCODE_BRIDGE_LIBRARY="<extracted-app>/Contents/Frameworks/pocket_codex_bridge.framework/Versions/A/pocket_codex_bridge" \
RUST_BACKTRACE=0 fvm flutter test --no-pub test/opencode_live_bridge_test.dart
```

旧包 `447fbde` 实际执行结果：失败，错误为 `OpenCode object is outside the selected project`，与桌面点击会话一致。新包必须使用同一测试、同一用户目录验证通过后交付；普通 `flutter test` 默认跳过该测试，不能代替产物验收。

修复产物 `af4177e` 的同一测试实际通过（`1 passed`）：读取已有非空历史、切换会话、加载较早历史并重新打开。实际加载的是 GitHub Actions 生成的发布 framework，不是本机重新编译的替代库。

新建会话另用真实官方服务和真实 framework 验收，临时服务工作目录设为 HOME，选择其 Downloads 子目录。测试从空列表开始，创建并打开会话，核对可写状态和列表身份，再重新打开该会话；不发送模型请求，结束时只清理测试自有服务和目录。

```sh
PCX_RUN_REAL_OPENCODE=1 \
PCX_TEST_OPENCODE_BINARY="$HOME/Library/Application Support/ai.opencode.desktop/cli/2.0.18/opencode-cli" \
PCX_OPENCODE_BRIDGE_LIBRARY="<extracted-app>/Contents/Frameworks/pocket_codex_bridge.framework/Versions/A/pocket_codex_bridge" \
RUST_BACKTRACE=0 fvm flutter test --no-pub test/opencode_create_live_bridge_test.dart
```

结果：旧包 `447fbde` 失败；修复后的本地原生库和新包 `af4177e` 均为 `1 passed`。新包实际完成了创建、自动打开、列表更新和再次选择，能够覆盖用户反馈的“列表成功但新建失败”流程。

## 桌面产物

- 源码提交：`af4177e334f23861a12e30aec22071ad18ed3582`。后续提交仅补充验收测试和文档，不改变这次打包的应用代码。
- [GitHub Actions 构建](https://github.com/WaBranium/pocket-codex/actions/runs/36314028660)：macOS arm64 和 x64 均成功；Android 缺少 fork 签名配置失败，不将整条发布流程描述为成功。
- arm64 DMG SHA-256：`95d34218f6b53bb792a1964e8612afd40ac78ce8286c22a7ae9023b5bfb1ad58`。
- 配套 ZIP SHA-256：`8b1bb654bd0c377ddb092b8147cf093b4193349b2eca86fd02ccd248cce7a6c5`。
- 下载文件与 GitHub 资产摘要一致；DMG 校验、arm64 架构检查和 `codesign --verify --deep --strict` 均通过。桥接库验收使用上述 ZIP 中的 framework，并与只读挂载的 DMG 内 framework 逐字节比较一致；主可执行文件也一致，挂载已解除。

## 当前限制

真实模型输出、执行中权限/Forms、interrupt 和完整桌面 UI 操作尚未在本次修复验收中声明通过。只读验收不发送消息、不处理待办、不停止会话或用户服务。
