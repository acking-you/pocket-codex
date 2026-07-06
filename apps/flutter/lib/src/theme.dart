import 'package:flutter/material.dart';
import 'package:pocket_codex/src/fonts.dart';

/// Brand seed colour for both schemes.
const _seed = Color(0xFF4C8DF6);

/// A thin, rounded scrollbar shared by both themes — closer to a modern web
/// chat than the default chunky Material scrollbar. Combined with full-width
/// scroll areas it sits flush at the window edge.
final _scrollbarTheme = ScrollbarThemeData(
  thickness: WidgetStateProperty.all(6.0),
  radius: const Radius.circular(3),
);

/// A flat app bar that blends into the content: same surface colour, no
/// Material-3 scroll tint, no elevation/shadow. This is what makes the top bar
/// stop looking like a separate raised strip — with the native title bar hidden
/// on desktop, the app bar becomes a seamless part of the window.
AppBarTheme _appBarTheme(ColorScheme scheme) => AppBarTheme(
  backgroundColor: scheme.surface,
  surfaceTintColor: Colors.transparent,
  elevation: 0,
  scrolledUnderElevation: 0,
);

/// Light Material 3 theme.
ThemeData lightTheme() {
  final scheme = ColorScheme.fromSeed(
    seedColor: _seed,
    brightness: Brightness.light,
  );
  return ThemeData(
    colorScheme: scheme,
    useMaterial3: true,
    fontFamily: appFontFamily,
    fontFamilyFallback: cjkFontFallback,
    scrollbarTheme: _scrollbarTheme,
    appBarTheme: _appBarTheme(scheme),
  );
}

/// Dark Material 3 theme.
ThemeData darkTheme() {
  final scheme = ColorScheme.fromSeed(
    seedColor: _seed,
    brightness: Brightness.dark,
  );
  return ThemeData(
    colorScheme: scheme,
    useMaterial3: true,
    fontFamily: appFontFamily,
    fontFamilyFallback: cjkFontFallback,
    scrollbarTheme: _scrollbarTheme,
    appBarTheme: _appBarTheme(scheme),
  );
}
