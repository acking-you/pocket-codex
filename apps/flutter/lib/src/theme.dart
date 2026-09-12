import 'package:flutter/material.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/fonts.dart';

/// Shared neutral surfaces and a restrained, accessible blue accent.
ColorScheme _flowScheme(Brightness brightness) {
  final light = brightness == Brightness.light;
  return ColorScheme.fromSeed(
    seedColor: const Color(0xFF355ACF),
    brightness: brightness,
  ).copyWith(
    primary: light ? const Color(0xFF355ACF) : const Color(0xFFA5B8FF),
    onPrimary: light ? Colors.white : const Color(0xFF162352),
    primaryContainer: light ? const Color(0xFFE8EDFF) : const Color(0xFF263459),
    onPrimaryContainer: light
        ? const Color(0xFF2343A1)
        : const Color(0xFFDCE4FF),
    secondary: light ? const Color(0xFF586174) : const Color(0xFFB2BCCF),
    secondaryContainer: light
        ? const Color(0xFFEDF0F5)
        : const Color(0xFF292E39),
    onSecondaryContainer: light
        ? const Color(0xFF263044)
        : const Color(0xFFE6EAF2),
    surface: light ? const Color(0xFFF7F8FA) : const Color(0xFF111318),
    surfaceBright: light ? Colors.white : const Color(0xFF1A1D24),
    onSurface: light ? const Color(0xFF202634) : const Color(0xFFEBEEF5),
    onSurfaceVariant: light ? const Color(0xFF626C7E) : const Color(0xFFA4AEC1),
    surfaceContainerLowest: light ? Colors.white : const Color(0xFF14171D),
    surfaceContainerLow: light
        ? const Color(0xFFF1F3F7)
        : const Color(0xFF1B1F28),
    surfaceContainer: light ? const Color(0xFFEBEEF3) : const Color(0xFF222732),
    surfaceContainerHigh: light
        ? const Color(0xFFE4E8F0)
        : const Color(0xFF2B3140),
    surfaceContainerHighest: light
        ? const Color(0xFFDDE3ED)
        : const Color(0xFF353D4D),
    outline: light ? const Color(0xFFCBD2DF) : const Color(0xFF465063),
    outlineVariant: light ? const Color(0xFFE3E7EF) : const Color(0xFF2C3341),
    error: light ? const Color(0xFFBE3245) : const Color(0xFFFF8B97),
    errorContainer: light ? const Color(0xFFFFEDEF) : const Color(0xFF48232C),
    onErrorContainer: light ? const Color(0xFF8D2030) : const Color(0xFFFFD9DE),
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

/// An opaque raised panel shared by cards, sheets, menus and the composer.
Color surfacePanel(ColorScheme scheme) => scheme.surfaceBright;

/// Opaque selection tint, shared by both sidebar views without dimming text.
Color surfaceSelection(ColorScheme scheme) => scheme.primaryContainer;

/// A stable outline separates selection from transient hover highlights.
Color selectionBorder(ColorScheme scheme) => Color.alphaBlend(
  scheme.primary.withValues(alpha: 0.26),
  surfaceSelection(scheme),
);

/// Floating previews need a stronger elevation step over the transcript.
Color surfacePreview(ColorScheme scheme) =>
    scheme.brightness == Brightness.light
    ? scheme.surfaceBright
    : scheme.surfaceContainer;

/// The design's type scale, mapped onto Material's roles.
///
/// Paragraphs use generous line height; single-line controls stay compact.
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
    titleLarge: t(22, 1.3),
    titleMedium: t(18, 1.4),
    titleSmall: t(14, 1.4),
    bodyLarge: t(16, 1.6),
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
    dividerTheme: DividerThemeData(
      color: scheme.outlineVariant,
      thickness: 1,
      space: 1,
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: FilledButton.styleFrom(
        minimumSize: const Size(48, 48),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(kControlRadius),
        ),
        textStyle: const TextStyle(fontSize: 14, fontWeight: FontWeight.w600),
      ),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: OutlinedButton.styleFrom(
        minimumSize: const Size(48, 48),
        side: BorderSide(color: scheme.outline),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(kControlRadius),
        ),
      ),
    ),
    textButtonTheme: TextButtonThemeData(
      style: TextButton.styleFrom(
        minimumSize: const Size(48, 44),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(kControlRadius),
        ),
      ),
    ),
    iconButtonTheme: IconButtonThemeData(
      style: IconButton.styleFrom(
        minimumSize: const Size(44, 44),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(kControlRadius),
        ),
      ),
    ),
    inputDecorationTheme: InputDecorationTheme(
      border: OutlineInputBorder(
        borderRadius: BorderRadius.circular(kControlRadius),
      ),
      contentPadding: const EdgeInsets.symmetric(horizontal: 14, vertical: 14),
      hintStyle: TextStyle(color: scheme.onSurfaceVariant, fontSize: 14),
    ),
    dialogTheme: DialogThemeData(
      backgroundColor: panel,
      surfaceTintColor: Colors.transparent,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(20)),
    ),
    bottomSheetTheme: BottomSheetThemeData(
      backgroundColor: panel,
      surfaceTintColor: Colors.transparent,
      showDragHandle: true,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(24)),
      ),
    ),
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

/// Add desktop density and pointer feedback to the shared touch-ready theme.
ThemeData _forPlatform(ThemeData base) => isDesktop ? desktopize(base) : base;

/// Light theme (desktop-tuned on desktop).
ThemeData lightTheme() => _forPlatform(_base(_flowScheme(Brightness.light)));

/// Dark theme (desktop-tuned on desktop).
ThemeData darkTheme() => _forPlatform(_base(_flowScheme(Brightness.dark)));
