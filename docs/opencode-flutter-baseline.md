# Flutter 环境与施工前基线

日期：2026-09-27。工作树：`feat/opencode-session-hosting`，上游基线
`7d64e9db1cb786b3bb29902a6eeacd23f9e44bfe`。本记录不代表 OpenCode 功能验收。

## 工具来源

- 主机：macOS arm64。
- Flutter：官方 `flutter/flutter` 的 `3.44.0` tag，SHA
  `559ffa3f75e7402d65a8def9c28389a9b2e6fe42`。安装在
  `/Users/wangdejiang6/.local/share/pocket-opencode-tools/flutter-3.44.0`。
- Dart：该 Flutter 版本自带 `3.12.0`；engine revision
  `4c525dac5ebe5971c5708ef73558ed8edcf4a362`。
- FVM：通过该 Dart 的 `dart pub global activate fvm` 安装 `4.3.1`；缓存目录
  `/Users/wangdejiang6/.local/share/pocket-opencode-tools/fvm` 的 `versions/3.44.0`
  链接到上述官方 tag checkout。
- 未修改仓库版本 pin、用户全局 shell 配置或系统安装目录。

官方 Google Storage release manifest 和版本 archive 地址返回 404；同名国内镜像也返回
404。改用官方 Git tag checkout 后，固定 engine 对应的 Dart SDK 和 Flutter artifacts
可成功下载。`flutter --version` 确认 Flutter `3.44.0`、Dart `3.12.0`。

直连 `pub.dev` 超时；命令级使用本机已有 HTTP 代理 `127.0.0.1:7897` 后可解析下载。
中止的仅为本次启动且卡在依赖下载的 Flutter 引导命令，不涉及任何 OpenCode 服务。

## 复用环境

下面只在当前 shell 设置，不需要修改 shell 配置：

```sh
export PATH="/Users/wangdejiang6/.local/share/pocket-opencode-tools/flutter-3.44.0/bin:/Users/wangdejiang6/.pub-cache/bin:$PATH"
export FVM_CACHE_PATH="/Users/wangdejiang6/.local/share/pocket-opencode-tools/fvm"
export FLUTTER_SUPPRESS_ANALYTICS=true
export https_proxy=http://127.0.0.1:7897
export http_proxy=http://127.0.0.1:7897
cd /Users/wangdejiang6/Downloads/pocket-opencode/apps/flutter
```

## 检查记录

| 命令 | 退出码 | 结果 |
| --- | --- | --- |
| `fvm flutter pub get` | 0 | 依赖解析下载成功；锁文件与其他 tracked 文件无变更 |
| `dart format --output=none --set-exit-if-changed lib test integration_test` | 0 | 152 个文件，0 个变更 |
| `fvm flutter analyze` | 0 | `No issues found!`，10.6 秒 |
| `fvm flutter test --reporter expanded` | 0 | 584 通过，3 个图标再生成测试按既有条件跳过；耗时 1 分 37 秒 |

`pub get` 提示现有 native plugins 尚不支持 Swift Package Manager；当前不是依赖解析
错误，未为消除该提示改动插件或升级依赖。

上述检查时 `apps/flutter` 没有 tracked 文件差异。测试没有设置 `REGEN_ICONS=1`，
因此未再生成任何品牌资产。尚未运行需要真实 host/设备的 `integration_test`。

## 平台限制

`xcodebuild -version` 退出码 1：当前开发目录为
`/Library/Developer/CommandLineTools`，没有完整 Xcode。`pod` 未发现。
因此 macOS/iOS 原生构建及真机 UI 验收尚不可执行；纯 Dart/Flutter widget tests
是否通过另列记录，不能代替原生构建或 FRB 动态库验证。
