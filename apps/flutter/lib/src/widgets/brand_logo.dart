import 'package:flutter/material.dart';

/// The Pocket-Codex brand mark: the bare "cloud terminal" glyph.
///
/// Two theme-matched variants ship in the bundle (extracted from the tile
/// masters in `icon/` by `test/gen_icon_test.dart`): `glyph_light.png` (ink
/// cloud, violet arcs) for light themes and `glyph_dark.png` (white cloud,
/// mint arcs) for dark themes. The glyph carries no tile — a rounded launcher
/// tile dropped into a page reads as a pasted app icon, while the bare glyph
/// sits on any surface like an ordinary illustration.
class BrandLogo extends StatelessWidget {
  /// Default constructor.
  const BrandLogo({super.key, this.size = 96});

  /// Edge length of the (square) widget.
  final double size;

  @override
  Widget build(BuildContext context) {
    final dark = Theme.of(context).brightness == Brightness.dark;
    return Image.asset(
      dark ? 'assets/logo/glyph_dark.png' : 'assets/logo/glyph_light.png',
      width: size,
      height: size,
      filterQuality: FilterQuality.medium,
    );
  }
}
