import 'package:flutter/foundation.dart';

/// Font configuration. Chinese rendered with Flutter's default (no declared
/// CJK font) looks thin and unevenly weighted on Windows, because the OS picks
/// a fallback face per glyph-run. To fix it we bundle Noto Sans SC — but ONLY
/// in the desktop builds (Windows/macOS/Linux), which is where the problem is.
/// Mobile (Android/iOS) keeps the OS CJK fonts (PingFang SC / Noto Sans CJK SC),
/// which already render Chinese well, so those artifacts don't carry the ~17 MB.
///
/// Desktop bundling is done by swapping `pubspec-desktop.yaml` (which adds a
/// `fonts:` block for the family below) over `pubspec.yaml` before the build —
/// see the desktop jobs in .github/workflows/release.yml and the note in
/// AGENTS.md. The default `pubspec.yaml` omits the font, so mobile/test never
/// bundle it.

/// Family registered by the `fonts:` block in pubspec-desktop.yaml. Must match
/// that block byte-for-byte, and is only actually registered on desktop builds.
const desktopFontFamily = 'Noto Sans SC';

/// True on desktop OSes, where Noto Sans SC is bundled. Gated on
/// [defaultTargetPlatform] (NOT `dart:io` Platform) on purpose: `flutter test`
/// forces the platform to android, so tests take the mobile branch and never
/// set the primary family to a face that isn't registered in the test harness.
bool get isDesktop =>
    !kIsWeb &&
    (defaultTargetPlatform == TargetPlatform.windows ||
        defaultTargetPlatform == TargetPlatform.macOS ||
        defaultTargetPlatform == TargetPlatform.linux);

/// The app's primary font: the bundled Noto Sans SC on desktop, and the system
/// default (null) on mobile/web so Latin keeps the native UI font and CJK falls
/// to the OS face. Latin glyphs on desktop also come from Noto Sans SC, which
/// the user asked for (one uniform typeface for 中英文).
String? get appFontFamily => isDesktop ? desktopFontFamily : null;

/// CJK fallback for the primary text theme. On mobile (primary = system font)
/// these resolve Chinese to the OS CJK face; on desktop they are extra
/// insurance behind the bundled family. Harmless where a name isn't installed.
const cjkFontFallback = ['PingFang SC', 'Noto Sans CJK SC', 'Microsoft YaHei'];

/// CJK fallback for `fontFamily: 'monospace'` styles (command output, diffs,
/// file paths, code blocks). 'monospace' has no Han glyphs, so without this
/// Chinese-in-code re-falls-back to an ugly OS face. The bundled family is
/// listed first (used on desktop; silently skipped where unregistered, e.g.
/// mobile, falling through to the OS CJK names). Kept const so the many
/// `const TextStyle(...)` code sites stay const.
const monoCjkFallback = [
  desktopFontFamily,
  'PingFang SC',
  'Noto Sans CJK SC',
  'Microsoft YaHei',
];
