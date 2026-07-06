# PocketCodex UI 优化：frameless 顶栏、图片预览、host 文件传输

- 日期：2026-07-06
- 范围：`apps/flutter`（UI + macOS/Windows 原生 runner）+ Phase 3 涉及 `crates/pocket-codex-host-svc`、`crates/pocket-codex-bridge`
- 平台重点：macOS + Windows 桌面（frameless 两端一致）；移动端保持现状

## 背景

用户对 PocketCodex 桌面 app 提出三点体验反馈：

1. **消除默认顶栏**：当前"系统原生标题栏 + 厚 Material AppBar"两层顶栏突兀。目标是隐藏系统标题栏、把标题区做成与内容融为一体的一条 frameless 顶条，**macOS 与 Windows 一致**，更符合现代桌面审美。
2. 图片放大预览后，点击图片边缘/背景区域也应能退出预览（当前仅左上角按钮可关）。
3. **host 目录双向文件传输**：能从 host 目录树里**选一个目录**，浏览目录内的文件、把 host（远程）文件**下载**到本地，也能把本地文件**上传**到该目录；读写均报错可见。（这扩展了最初"给附件加下载按钮"的诉求。）

## 现状与问题（含代码位置）

- **窗口层**：完全标准的原生窗口。macOS 普通 `NSWindow`（`MainFlutterWindow.swift:4-15`）；Windows `WS_OVERLAPPEDWINDOW` 含 `WS_CAPTION`（`win32_window.cpp:137-142`）。Flutter 侧只用 `window_manager` 0.5.1 做关闭到托盘（`main.dart:48-51`、`desktop_tray.dart:81-103`），无 frameless/自定义标题栏/`DragToMoveArea`。
- **AppBar 层**：11+ 屏幕都用 Material `AppBar`。主屏 `app_session_screen.dart:2367` 最复杂：leading（汉堡/返回）、title（`BrandLogo` + "PocketCodex" + 项目名）、bottom（`CodexSetupBar` 警告条）、actions（`_ContextGauge` token 表 + popup）。右上角红色 `DEBUG` 斜角是 Flutter debug banner，release 自动消失。
- **图片预览**：`ImageViewerPage` 是 `MaterialPageRoute(fullscreenDialog: true)`（`message_images.dart:221`），含 `InteractiveViewer` 缩放、`PageView` 翻页、AppBar 关闭按钮；**背景不可点击关闭**。图片内嵌 base64，全屏预览**已有"保存图片"按钮**（`:310`），但**加载/解码失败静默跳过或空白**（`:40`、`:324`）。
- **host 文件浏览 + roots 安全模型（复用基础）**：
  - `folder_tree_picker.dart`：bottom-sheet（`DraggableScrollableSheet`），调 `metaListDir(serviceKey, path)` 列**子目录**（`showFolderPicker()` 公开函数）；起点是 project roots。
  - **project roots**：host 配置的允许根目录，是"远程浏览被限制的边界"，持久化在 `HostStore`（`$CODEX_HOME/pocket-codex-host.json`），经 `meta_project_config`/`meta_set_project_config` 读写。
  - `/fs/list?path=`（`lib.rs:248`）：`within_roots()`（`fs.rs:47`，canonicalize + 防 `..`/符号链接穿越 + `starts_with` root）校验，越界 403；`list_subdirs` **只返回目录**，`DirEntry{name,path,is_git_repo}`，**无文件、无 size/mtime**。
  - 调用链：Flutter `metaListDir` → FRB → `bridge.rs:1375` → `meta.rs:218` → HTTP GET `/fs/list` → host-svc（loopback 或 relay tunnel）。
  - **上传落点**：`meta_upload_file`（`bridge.rs:1301`）→ `POST /uploads/<name>` → 落到 `$CODEX_HOME/pocket-codex-uploads/`（**在 roots 之外**的专用附件目录，唯一子目录防覆盖）。

## 设计

分三阶段，各自独立 PR。三块相对独立，顺序可调；建议先用 Phase 1 小改验证流程，再啃 Phase 2/3。

- **Phase 1（纯 Flutter，低风险）**：单元 B + C。
- **Phase 2（frameless 顶栏，跨原生 mac+win + Flutter，最大）**：单元 A。
- **Phase 3（host 文件传输面板，跨 Rust + Flutter，需 codegen + 安全把关）**：单元 D。

### 单元 A — Frameless 融合顶栏（Phase 2）

目标：隐藏系统原生标题栏，做一条**与内容同色、可拖拽、延伸到顶**的自定义顶条，macOS 与 Windows 一致。分三层：

**A1 原生窗口（隐藏系统标题栏）**
- **macOS**（`macos/Runner/MainFlutterWindow.swift`）：`styleMask` 加 `.fullSizeContentView`；`titleVisibility = .hidden`；`titlebarAppearsTransparent = true`。交通灯保留在左上约 70px，Flutter 顶条左侧留避让 inset。
- **Windows**（`windows/runner/win32_window.cpp`）：隐藏系统标题栏同时保留可用窗口控制。两条路线，**实现时优先评估 (b)**：
  - (a) 去 `WS_CAPTION` + 自绘最小化/最大化/关闭按钮 + `WM_NCHITTEST`/`WM_NCCALCSIZE`（视觉最统一，成本最高）。
  - (b) `DwmExtendFrameIntoClientArea` 延伸玻璃帧、内容延顶、**保留系统 caption 按钮**（自绘最少，风险更低）。
- **不破坏 close-to-tray**：`window_manager` 的 `setPreventClose(true)`/`onWindowClose→hide()`/托盘 `show()` 必须继续工作；确认 0.5.1 API 与所选原生方案兼容。

**A2 Flutter 自定义顶条**
- 新增 `lib/src/widgets/window_title_bar.dart`：`DragToMoveArea` 包裹的 slim 顶条，与内容同 `surface` 色、无分界；含交通灯/caption 避让 inset。双击最大化、拖拽移动接上。
- 平台门控（`defaultTargetPlatform`）：桌面走 frameless 顶条；**移动端保留原生 Material AppBar**。`flutter test`（强制 android）走移动分支，不受影响。

**A3 控件重新安置**
- 主屏 AppBar 的 `BrandLogo`/标题/项目名/汉堡-返回/`_ContextGauge`/`CodexSetupBar` 迁入新顶条/侧栏，保持 `_ContextGauge` onTap+tooltip 与 setup 警告逻辑。
- 其余屏幕（home/services/settings/api_service/codex_setup/app_service/local_sessions/log_view/local_session_view/account_onboarding）title/actions 统一改用新顶条或极简化；`welcome_guide_screen` 已无 AppBar，作模板。

**涉及文件**：`MainFlutterWindow.swift`、`win32_window.cpp`(/.h)、`main.dart`、新增顶条 widget、各 screen、`theme.dart`。

### 单元 B — 图片预览点背景关闭（Phase 1）

- `ImageViewerPage`（`message_images.dart:221`）黑色背景层外包 `GestureDetector(onTap: () => Navigator.pop())`；点图片外空白关闭。保留 `InteractiveViewer` 缩放、`PageView` 翻页、X 按钮（靠控件层级区分"点空白"与"操作图片"）。

### 单元 C — 图片加载明确报错 + 保存更顺手（Phase 1）

- **报错**：全屏 `Image.memory` 加 `errorBuilder`（`:324`）；`resolveImageUrls` 中 decode 失败的项不再静默丢弃（`:40`），保留"损坏/无法加载"占位。
- **保存更顺手**：缩略图悬停/右键快捷保存，复用现有 `getSaveLocation` + `writeAsBytes` 与 l10n（`imageSave`/`imageSaved`/`imageSaveFailed`）；加载失败占位上也给保存入口。
- **文件**：`message_images.dart`；新增 l10n。

### 单元 D — host 目录双向文件传输面板（Phase 3）

**入口**：聊天界面附件区（📎 旁）一个"浏览/传输 host 文件"入口，复用当前会话的 `serviceKey`（无需先选 host）。

**面板形态**：复用 `folder_tree_picker` 的 bottom-sheet 选目录 UI —— 用户在 host 目录树里选一个目录（限 project roots 内）→ 面板列出该目录内的**文件**（名称/大小/修改时间）→ 每个文件"下载"到本地；顶部"上传"按钮把本地文件传到当前目录。

**host-svc**（`crates/pocket-codex-host-svc`）：
- **列文件**：新增 `GET /fs/files?path=<dir>` + `fs::list_files()`（镜像 `list_subdirs` 逻辑，返回文件而非目录，带 size/mtime/可读性），新 `FileEntry` 结构。`within_roots` 校验。
- **下载（读字节）**：新增 `GET /fs/read?path=<file>&offset=&limit=` + `fs::read_file()`（**range 分段读**避免大文件内存尖峰）。`within_roots` 校验，只读普通文件。
- **上传到选定目录（写字节）**：现有 `/uploads` 写死落 `CODEX_HOME`（roots 外），不满足。新增 `POST /fs/write?path=<dir>`（body=bytes）落到 roots 内选定目录。`within_roots(dir)` 校验 + 文件名 `sanitize` + **不静默覆盖**（同名唯一命名或提示）。

**bridge**（`crates/pocket-codex-bridge`）：新增 `meta_list_files`、`meta_read_file(offset,limit)`、`meta_write_file(dir, name, bytes)`（走 meta tunnel，复用 `meta_list_dir`/`meta_upload_file` 模式）；重跑 `flutter_rust_bridge_codegen generate`。

**Flutter**：新增 `lib/src/widgets/file_browser_panel.dart`（复用 picker 的 bottom-sheet 模式）：选目录 → `metaListFiles` 列文件 → 下载按钮（`metaReadFile` → `getSaveLocation` + `writeAsBytes`）→ 上传按钮（`openFiles` 选本地文件 → `metaWriteFile`）；成功/失败 snackbar。桌面门控（沿用 `_canSave`）。聊天附件区加入口。

**涉及文件**：`fs.rs`、`lib.rs`、`meta.rs`、`api/bridge.rs`、FRB 生成物、新增 `file_browser_panel.dart`、`app_session_screen.dart`（附件区入口）、l10n。

## 安全考量（Phase 3）

- **读（`/fs/files`、`/fs/read`）与写（`/fs/write`）都必须 `within_roots` 限制在 project roots 内**：规范化路径（解析 `..`、符号链接）后校验位于某 root 之内，否则 403。
- 写入面风险更高：额外 `sanitize` 文件名到单一路径分量、拒绝目录穿越、**不静默覆盖**同名文件、只写普通文件。
- 读大文件用 range 分段，避免一次性载入内存。
- 复用 host-svc 现有隧道/鉴权边界，不新增裸端口。
- 单测覆盖：允许 roots 内文件读/写、拒绝 `..` 穿越、拒绝 roots 外绝对路径、拒绝符号链接逃逸、写入不覆盖同名。

## 非目标（YAGNI）

- **Linux frameless 本次不做**（GTK 装饰另议；本轮仅 macOS + Windows）。
- **移动端不改顶栏**（保留原生 Material AppBar）。
- Phase 3 **不开放 project roots 之外**的目录（含 `CODEX_HOME` uploads 目录本身不在本面板范围）。
- 不实现 PDF/文档的应用内预览（下载后用系统程序打开）；不做本地↔host 的自动同步语义（仅手动下载/上传）。
- 不支持移动端保存/传输（维持桌面门控）。
- 不移除/改动 Flutter debug banner。

## 验证

- **Phase 1**：`dart format --set-exit-if-changed`、`fvm flutter analyze`、`fvm flutter test`（`message_images` widget 测试：背景 tap 关闭、errorBuilder 占位、损坏项保留、快捷保存）；手动确认点背景可退出、坏图有占位。
- **Phase 2**：跨 **macOS + Windows** 手动——窗口拖拽、双击最大化、caption/交通灯可用、深浅色、多 DPI、close-to-tray 到托盘再恢复不受影响；Flutter widget 测试新顶条 + 移动端仍走 AppBar 分支；`fvm flutter analyze`/`test`；macOS `cargo check -p pocket_codex_bridge`。
- **Phase 3**：`cargo test -p pocket-codex-host-svc`（列文件/读/写 + 路径安全用例）、`cargo clippy --workspace -- -D warnings`、macOS `cargo check -p pocket_codex_bridge`、`fvm flutter test`；手动：选目录→列文件→下载一个文件、上传一个本地文件成功、构造穿越路径被拒。

Rust 全量验证（Phase 2/3 触及 Rust 时）：
```
cargo fmt -p pocket-codex-core -p pocket-codex-codex -p pocket-codex-pb -p pocket-codex-cli -p pocket_codex_bridge -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
```
