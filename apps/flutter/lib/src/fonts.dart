import 'package:flutter/foundation.dart';

/// Font configuration.
///
/// The design system is set in Figtree (prose) and Geist Mono (code), both
/// bundled on every platform — see the `fonts:` block in pubspec.yaml. Neither
/// face carries a single CJK glyph, so Chinese never renders *from* them: it
/// resolves down the fallback chains below.
///
/// Chinese rendered with Flutter's default (no declared CJK font) looks thin
/// and unevenly weighted on Windows, because the OS picks a fallback face per
/// glyph-run. To fix it we bundle Noto Sans SC — but ONLY in the desktop builds
/// (Windows/macOS/Linux), which is where the problem is. Mobile (Android/iOS)
/// keeps the OS CJK fonts (PingFang SC / Noto Sans CJK SC), which already
/// render Chinese well, so those artifacts don't carry the ~17 MB.
///
/// Desktop bundling is done by swapping `pubspec-desktop.yaml` (which adds a
/// `fonts:` entry for the family below) over `pubspec.yaml` before the build —
/// see the desktop jobs in .github/workflows/release.yml and the note in
/// AGENTS.md. The default `pubspec.yaml` omits it, so mobile/test never bundle
/// it — which is why [cjkFontFallback] can't name it unconditionally.

/// The design system's prose face, bundled on every platform. Latin only.
const appFontFamily = 'Figtree';

/// The design system's code face, bundled on every platform. Latin only.
const monoFontFamily = 'GeistMono';

/// Family registered by the `fonts:` block in pubspec-desktop.yaml. Must match
/// that block byte-for-byte, and is only actually registered on desktop builds.
const desktopFontFamily = 'Noto Sans SC';

/// True on desktop OSes, where Noto Sans SC is bundled. Gated on
/// [defaultTargetPlatform] (NOT `dart:io` Platform) on purpose: `flutter test`
/// forces the platform to android, so tests take the mobile branch — matching
/// the mobile pubspec, which is what a headless harness actually has registered.
bool get isDesktop =>
    !kIsWeb &&
    (defaultTargetPlatform == TargetPlatform.windows ||
        defaultTargetPlatform == TargetPlatform.macOS ||
        defaultTargetPlatform == TargetPlatform.linux);

/// CJK fallback behind [appFontFamily]. Figtree is Latin-only, so every Han
/// glyph in the app resolves from this chain rather than the primary family.
///
/// The bundled Noto Sans SC leads: on desktop it wins, which is the whole point
/// (a single declared face instead of the per-glyph-run OS guessing that made
/// Chinese look unevenly weighted on Windows). It is silently skipped where
/// unregistered — mobile, web, `flutter test` — falling through to the OS CJK
/// names, which already render Chinese well there. Kept const so the many
/// `const TextStyle(...)` sites stay const.
const cjkFontFallback = [
  desktopFontFamily,
  'PingFang SC',
  'Noto Sans CJK SC',
  'Microsoft YaHei',
];

/// CJK fallback for mono styles (command output, diffs, file paths, code
/// blocks). Geist Mono has no Han glyphs either, so Chinese in code resolves
/// down the same chain — proportional, since no bundled CJK face is monospaced.
const monoCjkFallback = cjkFontFallback;
