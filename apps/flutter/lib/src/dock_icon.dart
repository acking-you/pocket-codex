import 'package:flutter/foundation.dart'
    show defaultTargetPlatform, kIsWeb, TargetPlatform, visibleForTesting;
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

/// Keeps the macOS Dock icon in step with the app's light/dark appearance.
///
/// macOS ships ONE `AppIcon` per bundle — there is no asset-catalog appearance
/// variant the way iOS has — so the only way to show a theme-matched launcher
/// icon is to assign it at runtime from native code. That assignment is
/// process-scoped and vanishes on quit, which is fine: the app re-applies it on
/// the next launch from its own prefs.
///
/// Call [apply] with the brightness actually in effect (resolved, not the
/// preference — "follow system" has to become a concrete light or dark), and
/// only the changes are forwarded to the platform.
class DockIcon {
  DockIcon._();

  static const _channel = MethodChannel('pocket_codex/dock_icon');

  /// Whether this platform has a Dock icon worth swapping.
  static bool get supported =>
      !kIsWeb && defaultTargetPlatform == TargetPlatform.macOS;

  static String? _applied;

  /// Match the Dock icon to [brightness]. Cheap to call on every build: the
  /// platform call is skipped unless the resolved appearance actually changed.
  static void apply(Brightness brightness) {
    if (!supported) return;
    final mode = brightness == Brightness.dark ? 'dark' : 'light';
    if (_applied == mode) return;
    _applied = mode;
    // Fire and forget: a failed icon swap is cosmetic, and must never take a
    // frame (or the app) down with it.
    _channel.invokeMethod<void>('setAppearance', {'mode': mode}).catchError((
      _,
    ) {
      // Leave `_applied` set: retrying every frame on a platform that can't do
      // this would be worse than keeping the bundle's icon.
    });
  }

  /// Test seam: forget which appearance was pushed.
  @visibleForTesting
  static void reset() => _applied = null;
}
