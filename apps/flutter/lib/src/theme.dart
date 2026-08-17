import 'package:flutter/material.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/fonts.dart';

/// Brand seed colours, one per brightness, each taken from that theme's logo
/// variant so app accents and the visible logo always share a family:
/// light mode follows `icon/logo_light.png`'s indigo-violet signal arcs,
/// dark mode follows `icon/logo_dark.png`'s mint ones.
const _seedLight = Color(0xFF6B5CE7);
const _seedDark = Color(0xFF3EDDA4);

/// A thin, rounded scrollbar shared by both themes — closer to a modern web
/// chat than the default chunky Material scrollbar. Combined with full-width
/// scroll areas it sits flush at the window edge.
final _scrollbarTheme = ScrollbarThemeData(
  thickness: WidgetStateProperty.all(6.0),
  radius: const Radius.circular(3),
);

/// The colour scheme for [brightness].
///
/// Light keeps the stock tonal-spot scheme (strict Material 3: muted accent,
/// faintly tinted neutrals). Dark keeps the seeded accent roles but swaps the
/// neutral ladder for the brand INK (the dark logo tile, #10121C — a
/// near-black blue), so the app and its icon read as one object.
ColorScheme _scheme(Brightness brightness) {
  final seeded = ColorScheme.fromSeed(
    seedColor: brightness == Brightness.light ? _seedLight : _seedDark,
    brightness: brightness,
  );
  if (brightness == Brightness.light) return seeded;
  return seeded.copyWith(
    surface: const Color(0xFF10121C),
    onSurface: const Color(0xFFE4E7F0),
    surfaceContainerLowest: const Color(0xFF0B0D14),
    surfaceContainerLow: const Color(0xFF151825),
    surfaceContainer: const Color(0xFF1A1E2E),
    surfaceContainerHigh: const Color(0xFF212639),
    surfaceContainerHighest: const Color(0xFF282E44),
    onSurfaceVariant: const Color(0xFF9AA3BB),
    outline: const Color(0xFF5B647C),
    outlineVariant: const Color(0xFF2C3147),
    inverseSurface: const Color(0xFFE4E7F0),
    onInverseSurface: const Color(0xFF191D2B),
  );
}

/// The window/scaffold background: a step *below* the panels, so depth comes
/// from tone instead of hairline borders. Light: softly tinted paper under
/// white cards. Dark: the deepest surface under tonally-raised cards.
Color surfaceBackground(ColorScheme scheme) =>
    scheme.brightness == Brightness.light
    ? scheme.surfaceContainerLow
    : scheme.surface;

/// A raised content panel (card, sheet, list row) sitting on
/// [surfaceBackground]: white in light mode, a tonally lighter container in
/// dark mode — the Material-3 "elevation is tone" model.
Color surfacePanel(ColorScheme scheme) => scheme.brightness == Brightness.light
    ? scheme.surfaceContainerLowest
    : scheme.surfaceContainer;

/// Base Material 3 theme for [scheme] — shared by mobile and desktop; the
/// desktop layer re-tunes density/hover on top (see `desktop_theme.dart`).
ThemeData _base(ColorScheme scheme) {
  final background = surfaceBackground(scheme);
  final panel = surfacePanel(scheme);
  final radius = BorderRadius.circular(14);
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
        side: BorderSide(color: scheme.outlineVariant.withValues(alpha: 0.5)),
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
    snackBarTheme: SnackBarThemeData(
      behavior: SnackBarBehavior.floating,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10)),
    ),
  );
}

/// Wrap [base] in the desktop design layer on desktop; leave mobile on stock
/// Material. See `desktop_theme.dart`.
ThemeData _forPlatform(ThemeData base) => isDesktop ? desktopize(base) : base;

/// Light theme (desktop-tuned on desktop).
ThemeData lightTheme() => _forPlatform(_base(_scheme(Brightness.light)));

/// Dark theme (desktop-tuned on desktop).
ThemeData darkTheme() => _forPlatform(_base(_scheme(Brightness.dark)));
