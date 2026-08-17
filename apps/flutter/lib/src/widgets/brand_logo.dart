import 'package:flutter/material.dart';

/// The Pocket-Codex brand mark: the rounded "cloud terminal" tile.
///
/// Two theme-matched variants ship in the bundle (generated from the masters
/// in `icon/` by `test/gen_icon_test.dart`): `mark_light.png` (white tile,
/// ink glyph) for light themes and `mark_dark.png` (ink tile, white glyph)
/// for dark themes, so the logo always reads against the current surface.
/// Corners are transparent — the rounding is baked into the PNGs.
class BrandLogo extends StatelessWidget {
  /// Default constructor.
  const BrandLogo({super.key, this.size = 96});

  /// Edge length of the (square) widget.
  final double size;

  @override
  Widget build(BuildContext context) {
    final dark = Theme.of(context).brightness == Brightness.dark;
    return Image.asset(
      dark ? 'assets/logo/mark_dark.png' : 'assets/logo/mark_light.png',
      width: size,
      height: size,
      filterQuality: FilterQuality.medium,
    );
  }
}
