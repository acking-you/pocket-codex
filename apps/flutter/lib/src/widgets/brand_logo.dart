import 'package:flutter/material.dart';

/// The Pocket-Codex brand mark.
///
/// `assets/logo/mark.png` is the transparent glyph (cropped from the
/// adaptive-icon foreground), so it can sit directly on any surface.
/// With [plated] the mark is centred on a rounded plate tinted from the
/// active [ColorScheme], which keeps the tile in tune with both the
/// light and dark themes instead of baking in a fixed background.
class BrandLogo extends StatelessWidget {
  /// Default constructor.
  const BrandLogo({super.key, this.size = 96, this.plated = true});

  /// Edge length of the (square) widget.
  final double size;

  /// Whether to draw the rounded theme-tinted plate behind the mark.
  final bool plated;

  @override
  Widget build(BuildContext context) {
    final mark = Image.asset(
      'assets/logo/mark.png',
      width: plated ? null : size,
      height: plated ? null : size,
      filterQuality: FilterQuality.medium,
    );
    if (!plated) return mark;
    final scheme = Theme.of(context).colorScheme;
    return Container(
      width: size,
      height: size,
      // ~25% corner radius matches the app-icon plate proportions.
      decoration: BoxDecoration(
        color: scheme.primaryContainer,
        borderRadius: BorderRadius.circular(size * 0.25),
      ),
      padding: EdgeInsets.all(size * 0.16),
      child: mark,
    );
  }
}
