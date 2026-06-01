# 设计:Pocket-Codex 中/英国际化(i18n)

- 日期:2026-06-01
- 分支/PR:`feature/flutter-ui` / PR #8(在现有 PR 内完善,不新开)
- 状态:设计已逐节通过,直接实现

## 背景与动机

`apps/flutter` 全部 UI 文案硬编码简体中文(盘点 ~32 处,分布在
`main.dart` + `screens/{settings,services,api_service,onboarding}.dart`),
没有任何 locale 配置,也未引入 `flutter_localizations` / `intl`。需要做成
中/英双语,支持用户在设置里手动切换并持久化。

## 已确认决策

| # | 决策点 | 选择 |
|---|---|---|
| 1 | 方案 | Flutter 自带 `gen-l10n`(ARB),`flutter_localizations` + `intl` |
| 2 | 语言切换 | 设置内手动切(简中 / English / 跟随系统)+ 持久化 |
| 3 | 持久化落点 | 复用 Rust `config.toml` 的 `[ui] locale` 段(不引入 shared_preferences) |
| 4 | Rust 错误本地化 | 按操作上下文在 Dart 取 l10n 主消息,原始英文串作次要诊断细节附下方 |
| 5 | 默认 | locale 未设 = 跟随系统;supportedLocales = `en` / `zh` |

## 非目标(YAGNI)

- 不做 RTL / 复数规则等高级 i18n(中英不需要)。
- 不本地化 Rust 错误本身(按操作在 Dart 兜底已覆盖体验)。
- 不引入 `shared_preferences`(复用 Rust config 唯一持久化通道)。

## 详细设计

### 1. l10n 基础设施
- `apps/flutter/l10n.yaml`:`arb-dir: lib/l10n`,`template-arb-file: app_en.arb`,
  `output-localization-file: app_localizations.dart`,`synthetic-package: false`
  (Flutter 3.22+ 弃用合成包,生成进源码树 `lib/l10n/`)。
- `pubspec.yaml`:顶层 `flutter:` 加 `generate: true`;deps 加
  `flutter_localizations: { sdk: flutter }` + `intl`(版本由 SDK 约束)。
- `MaterialApp.router` 挂 `AppLocalizations.localizationsDelegates` +
  `supportedLocales` + `locale:`(来自 provider)。

### 2. Locale 状态 + 持久化
- Riverpod `localeProvider`(`StateProvider<Locale?>`,`null` = 跟随系统)。
- boot 时与现有 `getConfig()` 同一次桥调用读出 `locale`,经
  `ProviderScope(overrides:)` 注入初值(零额外延迟、无语言闪烁)。
- `PocketCodexApp` 改 `ConsumerWidget`,`watch(localeProvider)` 驱动
  `MaterialApp.locale`。切换 = 设 provider state + `await bridgeApi.setLocale(code)`。

### 3. Rust + Bridge
- `pocket-codex-core` Config 加 `#[serde(default)] pub ui: UiConfig`,
  `UiConfig { locale: Option<String> }` + `locale()` / `set_locale()` 访问器。
  旧 config 无 `[ui]` 段 → serde default(向后兼容)。
- bridge `ConfigView` 加 `locale: Option<String>`;新增
  `pub fn set_locale(locale: Option<String>)`(load→set→save)。重生成 FRB 绑定。
- `BridgeApi` 接口 + `RustBridgeApi` + `FakeBridgeApi` 加 `setLocale`;
  `ConfigInfo` 加 `locale`。

### 4. 字符串迁移 + 错误本地化
- 5 个文件 ~32 串全部换成 `AppLocalizations.of(context)!`(`l10n`)。
  `_UnsupportedPlatformApp` 也挂 delegates 并本地化(跟随系统,引擎在 web 不可用)。
- Dart 校验错误(`relay 地址不能为空` / `端口必须是 1 到 65535…`):在回调内用
  `l10n` 取串后 `setState` 设本地化消息,不再抛裸 `FormatException`。
- Rust 引擎错误:每个调用点按操作选 l10n 主消息(`discoverServices` 失败 →
  `discoverFailed`;`apiSubscribe` 失败 → `subscribeFailed`),原始 `$e` 作次要
  细节拼在主消息下(`'$main\n$detail'`)。

### 5. ARB key 清单(en 值示意;zh 值 = 现有中文)

```
appTitle               "Pocket-Codex"
webUnsupported         "Pocket-Codex needs local network and file access; Web is not supported.\nUse Android / iOS / desktop."
onboardingTitle        "Connect to a pb-mapper relay"
importFieldLabel       "pcx1: share string (one-tap import)"
importButton           "Import"
relayFieldLabel        "relay host:port"
keyFieldLabel          "MSG_HEADER_KEY (32 bytes)"
save                   "Save"
cancel                 "Cancel"
relayEmpty             "relay address cannot be empty"
apiServicesSection     "API services"
appServerServices      "App-server services"
appServerSubtitle      "{device} · sessions in P2"      (placeholder: device)
selectApiService       "Select an API service"
relayNotConfigured     "(no relay configured)"
noServicesFound        "No services found on this relay"
retry                  "Retry"
discoverFailed         "Couldn't reach the relay"
apiServiceTitle        "API service"
localPortLabel         "Local port"
startSubscription      "Start subscription"
stop                   "Stop"
portRangeError         "Port must be an integer from 1 to 65535"
noAuthWarning          "⚠ The local endpoint has no auth and binds 127.0.0.1 only. Alive only while the app is foregrounded."
subscribeFailed        "Couldn't start the subscription"
settingsTitle          "Settings"
notConfigured          "(not configured)"
keySet                 "•••••••• (set)"
keyNotSet              "(not set)"
activeSubscriptions    "Active subscriptions"
none                   "(none)"
exportShareString      "Export pcx1: share string"
copiedShareString      "Copied pcx1: share string"
language               "Language"
languageSystem         "Follow system"
languageChinese        "简体中文"
languageEnglish        "English"
```

### 6. 测试
- `_host` 测试帮手加 `localizationsDelegates` + `supportedLocales` + 锁定
  `locale: Locale('zh')`,现有中文断言继续成立(zh ARB 值 = 原文)。
- 新增英文渲染测试:`locale: Locale('en')` 挂 `ServicesScreen`,断言英文 key 值。
- Rust:core `ui.locale` 往返 + `set_locale` 单测;bridge `ConfigView.locale` 跟随。

## 验收标准
- 设置页可选 简中 / English / 跟随系统;切换即时改变全 app 文案。
- 选择持久化:重启 app 沿用上次语言(存于 `config.toml [ui] locale`)。
- 全部 ~32 串(含校验错误、Rust 错误主消息)随 locale 切换。
- `flutter analyze` 干净、`flutter test`(含 zh + en 渲染)通过;Rust bridge 测试通过。

## 风险
- `deny_unknown_fields` + 新 `[ui]` 段:旧二进制读新 config 会失败(同 relay-init
  的已知前向兼容限制);同代/新二进制不受影响。
- 改动集中在 `apps/flutter` + `pocket-codex-core` + `pocket-codex-bridge`;
  不动 CLI / `state.toml` / `deps/`。回滚即 revert 这批 commit。
