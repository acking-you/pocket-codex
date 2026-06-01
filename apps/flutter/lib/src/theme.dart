import 'package:flutter/material.dart';

/// Brand seed colour for both schemes.
const _seed = Color(0xFF4C8DF6);

/// Light Material 3 theme.
ThemeData lightTheme() => ThemeData(
  colorScheme: ColorScheme.fromSeed(
    seedColor: _seed,
    brightness: Brightness.light,
  ),
  useMaterial3: true,
);

/// Dark Material 3 theme.
ThemeData darkTheme() => ThemeData(
  colorScheme: ColorScheme.fromSeed(
    seedColor: _seed,
    brightness: Brightness.dark,
  ),
  useMaterial3: true,
);
