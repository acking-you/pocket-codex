import 'package:flutter/material.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/motion.dart';

/// Graphite neutrals, monochrome controls and one blue "signal".
///
/// The neutrals follow the macOS system greys (window, source list, control
/// background) without a blue cast, so the app sits beside native windows on
/// every desktop rather than reading as a tinted web page. Controls are drawn
/// in ink — a filled button, the send action — the way current desktop apps
/// do. Colour is reserved for meaning: the blue signal (taken from the
/// logo's relay arcs) marks what is live, and success / caution / error / info
/// keep their own hues, so a running turn never looks like a healthy service
/// or a diff addition.
///
/// Text roles are tuned for contrast on every ground they sit on:
/// `onSurfaceVariant` stays above 4.5:1 for small secondary text on the page,
/// the sidebar and raised panels, while `outline` is a border colour only and
/// must not carry text.
ColorScheme _flowScheme(Brightness brightness) {
  final light = brightness == Brightness.light;
  return ColorScheme.fromSeed(
    seedColor: const Color(0xFF2548AE),
    brightness: brightness,
  ).copyWith(
    primary: light ? const Color(0xFF1D1D1F) : const Color(0xFFEDEDEF),
    onPrimary: light ? Colors.white : const Color(0xFF1D1D1F),
    primaryContainer: light ? const Color(0xFFE6E6E4) : const Color(0xFF36363B),
    onPrimaryContainer: light
        ? const Color(0xFF1D1D1F)
        : const Color(0xFFEDEDEF),
    secondary: light ? const Color(0xFF4A4A50) : const Color(0xFFB8B8C0),
    onSecondary: light ? Colors.white : const Color(0xFF1D1D1F),
    secondaryContainer: light
        ? const Color(0xFFE6E6E4)
        : const Color(0xFF36363B),
    onSecondaryContainer: light
        ? const Color(0xFF1D1D1F)
        : const Color(0xFFEDEDEF),
    tertiary: light ? const Color(0xFF2A52C4) : const Color(0xFF93A6F5),
    onTertiary: light ? Colors.white : const Color(0xFF14245C),
    tertiaryContainer: light
        ? const Color(0xFFE7EDFC)
        : const Color(0xFF212B4C),
    onTertiaryContainer: light
        ? const Color(0xFF1D3B94)
        : const Color(0xFFD5DEFC),
    surface: light ? const Color(0xFFFBFBFA) : const Color(0xFF19191B),
    surfaceBright: light ? Colors.white : const Color(0xFF27272A),
    surfaceDim: light ? const Color(0xFFEDEDEB) : const Color(0xFF141416),
    onSurface: light ? const Color(0xFF1D1D1F) : const Color(0xFFEDEDEF),
    onSurfaceVariant: light ? const Color(0xFF4A4A50) : const Color(0xFFB8B8C0),
    surfaceContainerLowest: light ? Colors.white : const Color(0xFF141416),
    surfaceContainerLow: light
        ? const Color(0xFFF3F3F1)
        : const Color(0xFF202023),
    surfaceContainer: light ? const Color(0xFFEDEDEB) : const Color(0xFF27272A),
    surfaceContainerHigh: light
        ? const Color(0xFFE6E6E4)
        : const Color(0xFF2F2F33),
    surfaceContainerHighest: light
        ? const Color(0xFFDEDEDC)
        : const Color(0xFF39393E),
    outline: light ? const Color(0xFFC8C8CB) : const Color(0xFF4B4B51),
    outlineVariant: light ? const Color(0xFFE4E4E2) : const Color(0xFF323236),
    inverseSurface: light ? const Color(0xFF2B2B2E) : const Color(0xFFE6E6E8),
    onInverseSurface: light ? const Color(0xFFF2F2F2) : const Color(0xFF1D1D1F),
    error: light ? const Color(0xFFC4323F) : const Color(0xFFFF8A8F),
    onError: light ? Colors.white : const Color(0xFF4A0D12),
    errorContainer: light ? const Color(0xFFFCE9EA) : const Color(0xFF47201F),
    onErrorContainer: light ? const Color(0xFF8C1F2A) : const Color(0xFFFFD7D8),
  );
}

/// Ink for secondary chrome glyphs: placeholders, carets, quiet icons. Text
/// that must be read uses `onSurfaceVariant` instead, which keeps contrast on
/// the raised panels too.
Color onSurfaceMuted(ColorScheme scheme) =>
    scheme.onSurface.withValues(alpha: 0.55);

/// Ink at 30% — disabled content, like a send button that can't send.
Color onSurfaceDisabled(ColorScheme scheme) =>
    scheme.onSurface.withValues(alpha: 0.3);

/// The live signal: a running turn, a conversation working in the background,
/// the composer while the agent answers. Nothing else is drawn in it, which is
/// what lets one blue dot in a sidebar of grey rows say "this is working".
Color signalColor(ColorScheme scheme) => scheme.tertiary;

/// Dictation: the microphone turning speech into the draft. Teal, apart from
/// the blue of a live voice call, so the two never read as the same thing
/// when both are on screen.
Color dictationColor(ColorScheme scheme) =>
    scheme.brightness == Brightness.light
    ? const Color(0xFF0B7A83)
    : const Color(0xFF52C6CE);

/// Ink on [dictationColor].
Color onDictationColor(ColorScheme scheme) =>
    scheme.brightness == Brightness.light
    ? Colors.white
    : const Color(0xFF00363A);

/// Additions, in a diff or a change count. `+`/`−` is a convention older than
/// any palette — a diff whose additions aren't green reads wrong. Its
/// counterpart is `ColorScheme.error`.
Color additionColor(ColorScheme scheme) => scheme.brightness == Brightness.light
    ? const Color(0xFF1A7F37)
    : const Color(0xFF57C27A);

/// Caution — degraded but not failed: reconnecting, plan mode, a risky
/// permission, a quota running low. Genuine failure uses `ColorScheme.error`.
Color cautionColor(ColorScheme scheme) => scheme.brightness == Brightness.light
    ? const Color(0xFF9A5B00)
    : const Color(0xFFE0A647);

/// Healthy — a service online, a subscription alive, a host running. Kept a
/// separate function rather than calling [additionColor] at the status sites,
/// because a diff's additions and a service's health are different meanings —
/// retuning one must not move the other.
Color successColor(ColorScheme scheme) => additionColor(scheme);

/// Informational — a log line that is neither a problem nor a result.
Color infoColor(ColorScheme scheme) => scheme.brightness == Brightness.light
    ? const Color(0xFF1F63BC)
    : const Color(0xFF74A9EC);

/// A thin, rounded scrollbar shared by both themes — the macOS overlay style
/// rather than Material's chunky default.
final _scrollbarTheme = ScrollbarThemeData(
  thickness: WidgetStateProperty.resolveWith(
    (states) => states.contains(WidgetState.hovered) ? 8.0 : 6.0,
  ),
  radius: const Radius.circular(4),
  crossAxisMargin: 2,
  mainAxisMargin: 2,
);

/// The window/scaffold background: the page itself, opaque in both themes.
Color surfaceBackground(ColorScheme scheme) => scheme.surface;

/// An opaque raised panel shared by cards, sheets, menus and the composer.
Color surfacePanel(ColorScheme scheme) => scheme.surfaceBright;

/// The sidebar ground: a step off the page, like a macOS source list.
Color surfaceSidebar(ColorScheme scheme) => scheme.surfaceContainerLow;

/// Selected row in a list: a neutral plate, not a coloured one — selection is
/// position, not status, and colour stays free for status.
Color surfaceSelection(ColorScheme scheme) => scheme.surfaceContainerHigh;

/// The open conversation in the sidebar: a wash of the accent, so the one row
/// that says "you are here" can be found in a long list without reading it.
/// Plain list selection elsewhere stays the neutral [surfaceSelection].
Color selectedRowColor(ColorScheme scheme) => Color.alphaBlend(
  scheme.tertiary.withValues(
    alpha: scheme.brightness == Brightness.light ? 0.11 : 0.20,
  ),
  surfaceSidebar(scheme),
);

/// A faint accent wash behind the user's own messages and inline code, so the
/// two voices in a transcript, and code inside prose, separate at a glance.
Color accentWash(ColorScheme scheme, {double strength = 1}) =>
    scheme.tertiary.withValues(
      alpha: (scheme.brightness == Brightness.light ? 0.08 : 0.16) * strength,
    );

/// Ink for inline code in prose: the accent, deepened so it holds body-text
/// contrast on its wash (light ≈ 6:1, dark ≈ 9.5:1).
Color inlineCodeColor(ColorScheme scheme) =>
    scheme.brightness == Brightness.light
    ? const Color(0xFF2446A8)
    : const Color(0xFFC2CEFA);

/// Selected rows carry no outline; kept as a function so call sites that draw
/// a border can resolve it from one place.
Color selectionBorder(ColorScheme scheme) => Colors.transparent;

/// Floating previews need a stronger elevation step over the transcript.
Color surfacePreview(ColorScheme scheme) =>
    scheme.brightness == Brightness.light
    ? scheme.surfaceBright
    : scheme.surfaceContainer;

/// The type scale, mapped onto Material's roles.
///
/// Five steps do nearly all the work — 11 (captions), 12 (secondary), 13
/// (controls and rows), 14 (reading), 15 (the composer) — with titles above.
/// Paragraphs use generous line height; single-line controls stay compact.
///
/// The `label` roles are deliberately left at Material's defaults: `labelLarge`
/// is what every Material button renders its text in, and the button themes
/// are measured for it.
TextTheme _textTheme(ColorScheme scheme) {
  TextStyle t(double size, double height, [FontWeight w = FontWeight.w400]) =>
      TextStyle(fontSize: size, height: height, fontWeight: w);

  return TextTheme(
    displayLarge: t(48, 1.12),
    displayMedium: t(40, 1.15),
    displaySmall: t(34, 1.18),
    headlineLarge: t(30, 1.2, FontWeight.w600),
    headlineMedium: t(26, 1.22, FontWeight.w600),
    headlineSmall: t(22, 1.25, FontWeight.w600),
    titleLarge: t(19, 1.3, FontWeight.w600),
    titleMedium: t(16, 1.35, FontWeight.w600),
    titleSmall: t(13, 1.4, FontWeight.w600),
    bodyLarge: t(15, 1.6),
    bodyMedium: t(14, 1.55),
    bodySmall: t(12, 1.45),
  ).apply(
    fontFamily: uiFontFamily,
    fontFamilyFallback: uiCjkFallback,
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
    fontFamily: uiFontFamily,
    fontFamilyFallback: uiCjkFallback,
  );
  return ThemeData(
    colorScheme: scheme,
    useMaterial3: true,
    fontFamily: uiFontFamily,
    fontFamilyFallback: uiCjkFallback,
    textTheme: _textTheme(scheme),
    scaffoldBackgroundColor: background,
    canvasColor: background,
    pageTransitionsTheme: appPageTransitions,
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
        foregroundColor: scheme.onSurface,
        side: BorderSide(color: scheme.outline),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(kControlRadius),
        ),
      ),
    ),
    textButtonTheme: TextButtonThemeData(
      style: TextButton.styleFrom(
        minimumSize: const Size(48, 44),
        foregroundColor: scheme.onSurface,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(kControlRadius),
        ),
      ),
    ),
    iconButtonTheme: IconButtonThemeData(
      style: IconButton.styleFrom(
        minimumSize: const Size(44, 44),
        foregroundColor: scheme.onSurfaceVariant,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(kControlRadius),
        ),
      ),
    ),
    chipTheme: ChipThemeData(
      backgroundColor: Colors.transparent,
      selectedColor: scheme.tertiaryContainer,
      checkmarkColor: scheme.onTertiaryContainer,
      side: BorderSide(color: scheme.outlineVariant),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(kControlRadius),
      ),
      labelStyle: TextStyle(fontSize: 12.5, color: scheme.onSurface),
      padding: const EdgeInsets.symmetric(horizontal: 4),
    ),
    inputDecorationTheme: InputDecorationTheme(
      border: OutlineInputBorder(
        borderRadius: BorderRadius.circular(kControlRadius),
        borderSide: BorderSide(color: scheme.outline),
      ),
      // Only `border`: a field that opts out with `InputBorder.none` (the
      // composer, inline renames) must not get an enabled/focused outline
      // back from the theme.
      contentPadding: const EdgeInsets.symmetric(horizontal: 14, vertical: 14),
      hintStyle: TextStyle(color: scheme.onSurfaceVariant, fontSize: 14),
    ),
    progressIndicatorTheme: ProgressIndicatorThemeData(
      color: scheme.onSurfaceVariant,
      linearTrackColor: scheme.surfaceContainerHighest,
    ),
    switchTheme: SwitchThemeData(
      trackOutlineColor: WidgetStatePropertyAll(scheme.outline),
    ),
    dialogTheme: DialogThemeData(
      backgroundColor: panel,
      surfaceTintColor: Colors.transparent,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(kDialogRadius),
      ),
    ),
    bottomSheetTheme: BottomSheetThemeData(
      backgroundColor: panel,
      surfaceTintColor: Colors.transparent,
      showDragHandle: true,
      dragHandleColor: scheme.outline,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(22)),
      ),
    ),
    scrollbarTheme: _scrollbarTheme,
    // A flat app bar that blends into the content: same colour as the
    // scaffold, no Material-3 scroll tint, no elevation. With the native title
    // bar hidden on desktop, the app bar reads as part of the window itself.
    appBarTheme: AppBarTheme(
      backgroundColor: background,
      foregroundColor: scheme.onSurface,
      surfaceTintColor: Colors.transparent,
      elevation: 0,
      scrolledUnderElevation: 0,
      titleTextStyle: TextStyle(
        fontSize: 15,
        fontWeight: FontWeight.w600,
        color: scheme.onSurface,
        fontFamily: uiFontFamily,
        fontFamilyFallback: uiCjkFallback,
      ),
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
    drawerTheme: DrawerThemeData(
      backgroundColor: surfaceSidebar(scheme),
      surfaceTintColor: Colors.transparent,
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
