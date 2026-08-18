import 'package:flutter/foundation.dart';
import 'package:pocket_codex/src/bridge_api_rust.dart';
import 'package:tray_manager/tray_manager.dart';
import 'package:window_manager/window_manager.dart';

/// Localized labels for the tray context menu, injected by the app layer.
///
/// Keeping the strings out of this module lets it stay free of the l10n/router
/// imports, and lets the menu rebuild when the user switches UI language (the
/// app re-calls [DesktopTray.setMenu] with fresh labels). Value type so the
/// controller can skip the platform round-trip when nothing changed.
@immutable
class TrayMenuLabels {
  /// All three menu entries.
  const TrayMenuLabels({
    required this.show,
    required this.settings,
    required this.quit,
  });

  /// "Show window" — reveals and focuses the main window.
  final String show;

  /// "Settings" — shows the window and navigates to the settings screen.
  final String settings;

  /// "Quit" — the only path that actually terminates the process.
  final String quit;

  @override
  bool operator ==(Object other) =>
      other is TrayMenuLabels &&
      other.show == show &&
      other.settings == settings &&
      other.quit == quit;

  @override
  int get hashCode => Object.hash(show, settings, quit);
}

/// Owns the desktop system tray and the close-to-tray window behaviour.
///
/// Cross-platform via `tray_manager` + `window_manager`. [init] is called once
/// from `main()` on desktop (a no-op everywhere else). Closing the window then
/// hides it to the tray instead of quitting; the tray menu and — on Windows — a
/// left-click on the icon bring it back. Quit is the only path that terminates
/// the process.
class DesktopTray with TrayListener, WindowListener {
  DesktopTray._();

  /// Process-wide singleton (the tray is a single OS resource).
  static final DesktopTray instance = DesktopTray._();

  bool _initialised = false;
  TrayMenuLabels? _labels;
  VoidCallback? _onOpenSettings;

  /// Whether the current platform has a desktop system tray.
  ///
  /// Gated on [defaultTargetPlatform] (NOT `dart:io` Platform) like
  /// `fonts.dart`: `flutter test` forces the android target, so the tray is
  /// never initialised there and the native channels stay untouched in tests.
  static bool get supported =>
      !kIsWeb &&
      (defaultTargetPlatform == TargetPlatform.windows ||
          defaultTargetPlatform == TargetPlatform.macOS ||
          defaultTargetPlatform == TargetPlatform.linux);

  // Platform checks via defaultTargetPlatform (NOT `dart:io` Platform), so this
  // file imports no dart:io and main.dart can import it unconditionally without
  // breaking the web compile. Only read on desktop (init/handlers run only when
  // [supported]).
  static bool get _isWindows => defaultTargetPlatform == TargetPlatform.windows;
  static bool get _isMacOS => defaultTargetPlatform == TargetPlatform.macOS;
  static bool get _isLinux => defaultTargetPlatform == TargetPlatform.linux;

  /// Brings up the tray icon and arms close-to-tray. Idempotent.
  ///
  /// [onOpenSettings] is invoked by the "Settings" menu item (after the window
  /// is shown), so this module needn't import the router.
  Future<void> init({required VoidCallback onOpenSettings}) async {
    if (!supported || _initialised) return;
    _initialised = true;
    _onOpenSettings = onOpenSettings;

    await windowManager.ensureInitialized();
    // Frameless on macOS + Windows: hide the native title bar so the app's own
    // bar sits flush at the top edge (WindowTitleBar handles the button
    // avoidance + window drag). Keep the native window buttons visible. Linux
    // keeps its native decorations for now. Best-effort — a title-bar-style
    // failure must not abort tray setup / close-to-tray.
    if (_isMacOS || _isWindows) {
      try {
        await windowManager.setTitleBarStyle(
          TitleBarStyle.hidden,
          windowButtonVisibility: true,
        );
      } catch (_) {}
    }
    // Intercept the window close button: hide to tray instead of terminating.
    // onWindowClose (below) then hides the window; only the tray "Quit" exits.
    await windowManager.setPreventClose(true);
    windowManager.addListener(this);

    trayManager.addListener(this);
    // Per-platform icon: Windows' Shell_NotifyIcon needs a real .ico
    // (tray_manager feeds the path to LoadImage); Linux takes a PNG. macOS
    // menu-bar items are TEMPLATE images — black + alpha that the system
    // tints to match the bar (light/dark mode, highlight) — so it gets the
    // monochrome glyph. The plugin loads the exact asset key and pins it to
    // 18pt, so ship the @2x file directly: Retina fills those points and 1x
    // displays scale it back down cleanly.
    if (_isMacOS) {
      await trayManager.setIcon(
        'assets/tray/tray_template@2x.png',
        isTemplate: true,
      );
    } else {
      await trayManager.setIcon(
        _isWindows ? 'assets/tray/tray.ico' : 'assets/tray/tray.png',
      );
    }
    // appindicator (Linux) has no hover tooltip; setting one is a harmless
    // no-op, but skip it to keep the platform log clean.
    if (!_isLinux) {
      await trayManager.setToolTip('PocketCodex');
    }
  }

  /// (Re)builds the tray context menu with localized [labels]. Called by the
  /// app layer on first build and whenever the UI language changes; skips the
  /// platform round-trip when the labels are unchanged.
  Future<void> setMenu(TrayMenuLabels labels) async {
    if (!_initialised || labels == _labels) return;
    _labels = labels;
    await trayManager.setContextMenu(
      Menu(
        items: [
          MenuItem(key: 'show', label: labels.show, onClick: (_) => _show()),
          MenuItem(
            key: 'settings',
            label: labels.settings,
            onClick: (_) => _openSettings(),
          ),
          MenuItem.separator(),
          MenuItem(key: 'quit', label: labels.quit, onClick: (_) => _quit()),
        ],
      ),
    );
  }

  Future<void> _show() async {
    if (await windowManager.isMinimized()) {
      await windowManager.restore();
    }
    await windowManager.show();
    await windowManager.focus();
    // macOS hides by ordering the window out, which also drops the app from
    // the foreground. A single show/focus pair sometimes lands while the shell
    // still owns activation — the click on the tray just gave it away — and
    // the window stays gone. Confirm it came back and ask once more if not.
    if (!_isMacOS) return;
    await Future<void>.delayed(const Duration(milliseconds: 120));
    if (await windowManager.isVisible()) return;
    await windowManager.show();
    await windowManager.focus();
  }

  Future<void> _openSettings() async {
    await _show();
    _onOpenSettings?.call();
  }

  Future<void> _quit() async {
    // Stop any locally-hosted codex app-servers first so a real quit leaves no
    // orphan processes (closing to the tray, by contrast, keeps the app alive
    // and therefore keeps hosting). Best-effort + time-bounded so a wedged stop
    // can never block shutdown; a no-op when nothing is hosting.
    try {
      await const RustBridgeApi().appServeStopAll().timeout(
        const Duration(seconds: 3),
      );
    } catch (_) {}
    // Remove the tray icon, lift the close guard so destroy() isn't intercepted
    // again, then tear the window down. window_manager.destroy() ends the app —
    // Windows posts WM_QUIT to break the runner's message loop, macOS/Linux
    // close the last window — so the process exits cleanly.
    await trayManager.destroy();
    await windowManager.setPreventClose(false);
    await windowManager.destroy();
  }

  // --- WindowListener -------------------------------------------------------

  @override
  void onWindowClose() {
    // setPreventClose(true) stopped the real close, so the window is still
    // alive — hide it to the tray.
    windowManager.hide();
  }

  // --- TrayListener ---------------------------------------------------------

  @override
  void onTrayIconMouseDown() {
    // Windows: primary click reveals the window (the menu is on right-click).
    // macOS: the menu-bar convention is that a LEFT click opens the menu, and
    // it is also the only click the plugin reports reliably there (the status
    // item's right-click arrives through an NSView subview that doesn't
    // always receive the event). Linux/appindicator opens its menu natively
    // and sends no click events, so it needs nothing.
    if (_isWindows) {
      _show();
    } else if (_isMacOS) {
      _popUpMenu();
    }
  }

  @override
  void onTrayIconRightMouseDown() {
    // Windows and macOS pop the context menu on (right-)click; Linux's
    // appindicator does it natively.
    if (_isWindows || _isMacOS) {
      _popUpMenu();
    }
  }

  void _popUpMenu() {
    // Win32 requires the menu's owner to be the foreground window before
    // TrackPopupMenu, or the menu stays on screen when the user clicks away.
    // bringAppToFront is the plugin's only route to SetForegroundWindow; it is
    // deprecated upstream but still the fix. Harmless elsewhere.
    // ignore: deprecated_member_use
    trayManager.popUpContextMenu(bringAppToFront: _isWindows);
  }
}
