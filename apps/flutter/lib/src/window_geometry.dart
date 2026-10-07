import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:ui';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';
import 'package:window_manager/window_manager.dart';

/// Remembers the desktop window's size and position across launches, and
/// gives it a minimum size the two-pane layout still fits in.
///
/// Stored as a small JSON file next to `ui_state.json`. Restoring is
/// best-effort: an unreadable file, or a saved frame that no longer lands on
/// any screen (a monitor was unplugged), falls back to the platform default
/// placement rather than opening the window off-screen.
class WindowGeometry with WindowListener {
  WindowGeometry._();

  /// Process-wide singleton (there is one main window).
  static final WindowGeometry instance = WindowGeometry._();

  /// Below this the sidebar and the transcript fight for width.
  static const minimumSize = Size(720, 480);

  static const _chrome = MethodChannel('pocket_codex/window_chrome');

  Timer? _saveDebounce;
  bool _attached = false;

  Future<File> _file() async {
    final dir = await getApplicationSupportDirectory();
    return File('${dir.path}/window_state.json');
  }

  /// Apply the minimum size and the saved frame, then start tracking changes.
  /// Call once, after `windowManager.ensureInitialized()`.
  Future<void> attach() async {
    if (_attached) return;
    _attached = true;
    try {
      await windowManager.setMinimumSize(minimumSize);
      final saved = await _load();
      if (saved != null && await _onSomeScreen(saved.bounds)) {
        await windowManager.setBounds(saved.bounds);
        if (saved.maximized) await windowManager.maximize();
      }
    } catch (_) {
      // Geometry is cosmetic; a failure must not block startup.
    }
    windowManager.addListener(this);
    await layoutTrafficLights();
  }

  /// Ask the macOS runner to re-centre the traffic lights in the app's own
  /// title strip. Needed after anything that changes the title bar style.
  static Future<void> layoutTrafficLights() async {
    if (kIsWeb || defaultTargetPlatform != TargetPlatform.macOS) return;
    try {
      await _chrome.invokeMethod<void>('layoutTrafficLights');
    } catch (_) {
      // Older runner without the channel: the buttons keep AppKit's spot.
    }
  }

  Future<({Rect bounds, bool maximized})?> _load() async {
    try {
      final file = await _file();
      if (!await file.exists()) return null;
      final json = jsonDecode(await file.readAsString());
      if (json is! Map) return null;
      double? n(String k) => switch (json[k]) {
        num v when v.isFinite => v.toDouble(),
        _ => null,
      };
      final x = n('x'), y = n('y'), w = n('width'), h = n('height');
      if (x == null || y == null || w == null || h == null) return null;
      if (w < minimumSize.width || h < minimumSize.height) return null;
      return (
        bounds: Rect.fromLTWH(x, y, w, h),
        maximized: json['maximized'] == true,
      );
    } catch (_) {
      return null;
    }
  }

  /// Whether a saved frame can still be grabbed: its title strip lies within
  /// the combined span of the current displays.
  ///
  /// Flutter reports display sizes but not their arrangement, so the check is
  /// against the widest plausible desktop — every display side by side — and
  /// only rejects frames that are clearly off it, such as one left on a
  /// monitor that has since been unplugged. The OS clamps the rest.
  Future<bool> _onSomeScreen(Rect r) async {
    final displays = PlatformDispatcher.instance.displays;
    if (displays.isEmpty) return true;
    var width = 0.0, height = 0.0;
    for (final d in displays) {
      final s = d.size / d.devicePixelRatio;
      width += s.width;
      if (s.height > height) height = s.height;
    }
    const grab = 40.0;
    return r.right > grab &&
        r.left < width - grab &&
        r.top > -grab &&
        r.top < height - grab;
  }

  void _scheduleSave() {
    _saveDebounce?.cancel();
    _saveDebounce = Timer(const Duration(milliseconds: 500), _save);
  }

  Future<void> _save() async {
    try {
      final maximized = await windowManager.isMaximized();
      final file = await _file();
      // Keep the last normal frame while maximized, so un-maximizing after a
      // restart returns to it.
      Map<String, dynamic> previous = const {};
      if (maximized && await file.exists()) {
        final raw = jsonDecode(await file.readAsString());
        if (raw is Map<String, dynamic>) previous = raw;
      }
      final b = maximized && previous.isNotEmpty
          ? null
          : await windowManager.getBounds();
      await file.writeAsString(
        jsonEncode({
          if (b == null) ...previous,
          if (b != null) ...{
            'x': b.left,
            'y': b.top,
            'width': b.width,
            'height': b.height,
          },
          'maximized': maximized,
        }),
      );
    } catch (_) {
      // Best-effort.
    }
  }

  @override
  void onWindowResized() => _scheduleSave();

  @override
  void onWindowMoved() => _scheduleSave();

  @override
  void onWindowMaximize() => _scheduleSave();

  @override
  void onWindowUnmaximize() => _scheduleSave();

  @override
  void onWindowLeaveFullScreen() => layoutTrafficLights();
}
