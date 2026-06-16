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

/// Light Material 3 theme.
ThemeData lightTheme() => ThemeData(
  colorScheme: ColorScheme.fromSeed(
    seedColor: _seed,
    brightness: Brightness.light,
  ),
  useMaterial3: true,
  fontFamily: appFontFamily,
  fontFamilyFallback: cjkFontFallback,
  scrollbarTheme: _scrollbarTheme,
);

/// Dark Material 3 theme.
ThemeData darkTheme() => ThemeData(
  colorScheme: ColorScheme.fromSeed(
    seedColor: _seed,
    brightness: Brightness.dark,
  ),
  useMaterial3: true,
  fontFamily: appFontFamily,
  fontFamilyFallback: cjkFontFallback,
  scrollbarTheme: _scrollbarTheme,
);
