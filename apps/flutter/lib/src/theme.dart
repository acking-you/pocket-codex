import 'package:flutter/material.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/fonts.dart';

/// The brand accent, at the same value in both themes: the design system
/// carries a single accent rather than one per brightness.
const _accent = Color(0xFFE071A7);

/// The ink — the colour every foreground and every container wash is derived
/// from. Warm near-black on paper in light, pure white in dark.
const _inkLight = Color(0xFF1A1A19);
const _inkDark = Color(0xFFFFFFFF);

/// The design's two rules for deriving a scheme from the ink, and the reason
/// the ladders below are alphas rather than resolved colours:
///
/// * **The ink ramp is translucent.** `onSurfaceVariant` (75%), the muted level
///   (50%) and the outlines (14% light / 20% dark) are the foreground ink at an
///   alpha. The same label and the same hairline are drawn on the page *and* on
///   a raised card, which only works if they composite.
/// * **The container ladder is translucent too** — `surfaceContainerLowest`
///   through `Highest` are ink washes at rising alphas, so one fill reads
///   correctly on both grounds. Only `surface` and `surfaceBright` are opaque,
///   because they are the grounds everything else sits on.
///
/// Dark hairlines need the extra step (20% vs 14%) to hold an edge — the one
/// value that isn't the same alpha in both themes.
ColorScheme _flowScheme(Brightness brightness) {
  final light = brightness == Brightness.light;
  final ink = light ? _inkLight : _inkDark;
  Color wash(double alpha) => ink.withValues(alpha: alpha);

  return ColorScheme(
    brightness: brightness,
    primary: _accent,
    onPrimary: light ? const Color(0xFFFFFFFF) : const Color(0xFF1E1E1E),
    primaryContainer: light ? const Color(0xFFF6E9ED) : const Color(0xFF432B37),
    onPrimaryContainer: light
        ? const Color(0xFF8C3A67)
        : const Color(0xFFF5CFE1),
    secondary: light ? const Color(0xFF525251) : const Color(0xFFC5C5C5),
    onSecondary: light ? const Color(0xFFFFFFFF) : const Color(0xFF1E1E1E),
    secondaryContainer: light
        ? const Color(0xFFF0F0EE)
        : const Color(0xFF202020),
    onSecondaryContainer: light
        ? const Color(0xFF1A1A19)
        : const Color(0xFFFFFFFF),
    tertiary: light ? const Color(0xFFA8497B) : const Color(0xFFDE9CC0),
    onTertiary: light ? const Color(0xFFFFFFFF) : const Color(0xFF3D1F2E),
    tertiaryContainer: light
        ? const Color(0xFFF3E4EC)
        : const Color(0xFF4E3040),
    onTertiaryContainer: light
        ? const Color(0xFF5C2743)
        : const Color(0xFFF6DCE9),
    error: light ? const Color(0xFFDC2626) : const Color(0xFFEF4444),
    onError: const Color(0xFFFFFFFF),
    errorContainer: light ? const Color(0xFFFEE2E2) : const Color(0xFF7F1D1D),
    onErrorContainer: light ? const Color(0xFF7F1D1D) : const Color(0xFFFECACA),
    // The page, and the raised card that lifts off it — the only opaque pair.
    surface: light ? const Color(0xFFF9F9F7) : const Color(0xFF171717),
    surfaceBright: light ? const Color(0xFFFFFFFF) : const Color(0xFF1E1E1E),
    onSurface: ink,
    onSurfaceVariant: wash(0.75),
    surfaceContainerLowest: wash(0.02),
    surfaceContainerLow: wash(0.04),
    surfaceContainer: wash(0.06),
    surfaceContainerHigh: wash(0.08),
    surfaceContainerHighest: wash(0.10),
    outline: wash(light ? 0.14 : 0.20),
    outlineVariant: wash(0.06),
    inverseSurface: light ? const Color(0xFF1E1E1E) : const Color(0xFFF9F9F7),
    onInverseSurface: light ? const Color(0xFFF9F9F7) : const Color(0xFF1A1A19),
    inversePrimary: light ? const Color(0xFFE071A7) : const Color(0xFF8C3A67),
  );
}

/// Ink at 50% — muted chrome: placeholders, carets, secondary glyphs. The
/// design draws content at three levels and `ColorScheme` names only two, so
/// this third one is a helper rather than a role.
Color onSurfaceMuted(ColorScheme scheme) =>
    scheme.onSurface.withValues(alpha: 0.5);

/// Ink at 30% — disabled content, like a send button that can't send.
Color onSurfaceDisabled(ColorScheme scheme) =>
    scheme.onSurface.withValues(alpha: 0.3);

/// Additions, in a diff or a change count. The design names no success role,
/// but `+`/`−` is a convention older than any palette — a diff whose additions
/// aren't green reads wrong. Pitched deeper and warmer than Material's stock
/// green so it sits with the warm neutrals instead of glowing against them; the
/// counterpart is `ColorScheme.error`.
Color additionColor(ColorScheme scheme) => scheme.brightness == Brightness.light
    ? const Color(0xFF0E7264)
    : const Color(0xFF5BC4AF);

/// Caution — degraded but not failed: reconnecting, plan mode, a risky
/// permission, a quota running low. The design names no warning role either;
/// this is the syntax palette's `type` amber, which is already tuned to sit
/// with the warm neutrals. Genuine failure uses `ColorScheme.error`.
Color cautionColor(ColorScheme scheme) => scheme.brightness == Brightness.light
    ? const Color(0xFF96540A)
    : const Color(0xFFD9A054);

/// Healthy — a service online, a subscription alive, a host running. The design
/// names no success role, so this follows [additionColor] and takes the syntax
/// palette's `string` green, at the same value: both mean "good" and the hue is
/// already tuned to the warm neutrals. Kept a separate function rather than
/// calling [additionColor] at the status sites, because a diff's additions and a
/// service's health are different meanings — retuning one must not move the
/// other.
Color successColor(ColorScheme scheme) => additionColor(scheme);

/// Informational — a log line that is neither a problem nor a result. The syntax
/// palette's `number` blue; the only status level that isn't already named.
Color infoColor(ColorScheme scheme) => scheme.brightness == Brightness.light
    ? const Color(0xFF1F63BC)
    : const Color(0xFF74A9EC);

/// A thin, rounded scrollbar shared by both themes — closer to a modern web
/// chat than the default chunky Material scrollbar. Combined with full-width
/// scroll areas it sits flush at the window edge.
final _scrollbarTheme = ScrollbarThemeData(
  thickness: WidgetStateProperty.all(6.0),
  radius: const Radius.circular(3),
);

/// The window/scaffold background: the page itself, opaque in both themes.
Color surfaceBackground(ColorScheme scheme) => scheme.surface;

/// A raised content panel (card, sheet, menu, composer) sitting on
/// [surfaceBackground] — white in light, #1E1E1E in dark. Opaque in both, so a
/// panel reads as lifting *off* the page rather than tinting it; the translucent
/// container ladder is for washes drawn on top of either ground.
Color surfacePanel(ColorScheme scheme) => scheme.surfaceBright;

/// The design's type scale, mapped onto Material's roles.
///
/// Two habits of the design carry through: no letter-spacing, and one of two
/// line heights — 1.5 where text wraps into paragraphs, 1.3 where it sits on a
/// single line. Display and headline roles are tighter still (1.15 / 1.2)
/// because they never wrap far.
///
/// The `label` roles are deliberately left at Material's defaults. In the design
/// they are 16/14/12 at w400, but there control text is baked per widget; here
/// `labelLarge` is what every Material button renders its text in, and widening
/// it to 16 lays out buttons the button themes were never measured for.
TextTheme _textTheme(ColorScheme scheme) {
  TextStyle t(double size, double height) =>
      TextStyle(fontSize: size, height: height, fontWeight: FontWeight.w400);

  return TextTheme(
    displayLarge: t(52, 1.15),
    displayMedium: t(46, 1.15),
    displaySmall: t(40, 1.15),
    headlineLarge: t(36, 1.2),
    headlineMedium: t(32, 1.2),
    headlineSmall: t(28, 1.2),
    titleLarge: t(24, 1.3),
    titleMedium: t(21, 1.3),
    titleSmall: t(18, 1.3),
    bodyLarge: t(16, 1.5),
    bodyMedium: t(14, 1.5),
    bodySmall: t(12, 1.5),
  ).apply(
    fontFamily: appFontFamily,
    fontFamilyFallback: cjkFontFallback,
    bodyColor: scheme.onSurface,
    displayColor: scheme.onSurface,
  );
}

/// Base Material 3 theme for [scheme] — shared by mobile and desktop; the
/// desktop layer re-tunes density/hover on top (see `desktop_theme.dart`).
ThemeData _base(ColorScheme scheme) {
  final background = surfaceBackground(scheme);
  final panel = surfacePanel(scheme);
  final radius = BorderRadius.circular(kPanelRadius);
  TextStyle railLabel(Color color, FontWeight weight) => TextStyle(
    fontSize: 12,
    fontWeight: weight,
    color: color,
    fontFamily: appFontFamily,
    fontFamilyFallback: cjkFontFallback,
  );
  return ThemeData(
    colorScheme: scheme,
    useMaterial3: true,
    fontFamily: appFontFamily,
    fontFamilyFallback: cjkFontFallback,
    textTheme: _textTheme(scheme),
    scaffoldBackgroundColor: background,
    scrollbarTheme: _scrollbarTheme,
    // A flat app bar that blends into the content: same colour as the
    // scaffold, no Material-3 scroll tint, no elevation. With the native title
    // bar hidden on desktop, the app bar reads as part of the window itself.
    appBarTheme: AppBarTheme(
      backgroundColor: background,
      surfaceTintColor: Colors.transparent,
      elevation: 0,
      scrolledUnderElevation: 0,
    ),
    // Panels over tone: cards are a lighter layer on the background with a
    // barely-there outline, not a border-drawn box.
    cardTheme: CardThemeData(
      elevation: 0,
      color: panel,
      surfaceTintColor: Colors.transparent,
      margin: EdgeInsets.zero,
      shape: RoundedRectangleBorder(
        borderRadius: radius,
        side: BorderSide(color: scheme.outlineVariant),
      ),
    ),
    navigationRailTheme: NavigationRailThemeData(
      backgroundColor: Colors.transparent,
      indicatorColor: scheme.secondaryContainer,
      selectedIconTheme: IconThemeData(
        color: scheme.onSecondaryContainer,
        size: 22,
      ),
      unselectedIconTheme: IconThemeData(
        color: scheme.onSurfaceVariant,
        size: 22,
      ),
      selectedLabelTextStyle: railLabel(scheme.onSurface, FontWeight.w600),
      unselectedLabelTextStyle: railLabel(
        scheme.onSurfaceVariant,
        FontWeight.w400,
      ),
    ),
    navigationBarTheme: NavigationBarThemeData(
      backgroundColor: panel,
      surfaceTintColor: Colors.transparent,
      indicatorColor: scheme.secondaryContainer,
      elevation: 0,
    ),
    // A fallback for any snack bar raised outside `app_toast.dart`, which draws
    // its own surface and ignores this.
    snackBarTheme: SnackBarThemeData(
      behavior: SnackBarBehavior.floating,
      shape: RoundedRectangleBorder(borderRadius: radius),
    ),
  );
}

/// Wrap [base] in the desktop design layer on desktop; leave mobile on stock
/// Material. See `desktop_theme.dart`.
ThemeData _forPlatform(ThemeData base) => isDesktop ? desktopize(base) : base;

/// Light theme (desktop-tuned on desktop).
ThemeData lightTheme() => _forPlatform(_base(_flowScheme(Brightness.light)));

/// Dark theme (desktop-tuned on desktop).
ThemeData darkTheme() => _forPlatform(_base(_flowScheme(Brightness.dark)));
