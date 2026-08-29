import 'package:flutter/material.dart';
import 'package:pocket_codex/src/desktop_theme.dart';

/// A rounded tile carrying one icon — what heads a capability row, a settings
/// row, a panel, a host card.
///
/// This shape was open-coded at half a dozen sites, which is how it came to have
/// five hardcoded sizes and three hardcoded radii for what reads as one
/// primitive. Two variants earn their keep and are named here; both derive the
/// icon size and radius from the tile so a caller never picks them apart.
class IconBadge extends StatelessWidget {
  /// A leading badge for a row: quiet by default, accented when [accent].
  const IconBadge({
    super.key,
    required this.icon,
    this.size = 32,
    this.accent = false,
    this.background,
    this.foreground,
  });

  /// The glyph.
  final IconData icon;

  /// Tile edge length. The icon and the corner radius follow from it.
  final double size;

  /// Whether this badge is the accented kind — the subject of its row rather
  /// than a label on it.
  final bool accent;

  /// Explicit colours, for the rare badge that is neither quiet nor the accent
  /// (a host card's tertiary tint). [accent] is ignored when these are given.
  final Color? background;
  final Color? foreground;

  /// The glyph size for this tile. Whole pixels, tabulated rather than scaled:
  /// a fractional size lands the glyph off the device-pixel grid and softens its
  /// stems, which is plainly visible at 17 px. The sizes are the ones the
  /// open-coded copies already used, so unifying them changes no pixels.
  double get _iconSize => switch (size) {
    <= 32 => 17,
    <= 36 => 18,
    _ => 20,
  };

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final bg =
        background ??
        (accent ? scheme.primaryContainer : scheme.surfaceContainer);
    final fg =
        foreground ??
        (accent ? scheme.onPrimaryContainer : scheme.onSurfaceVariant);
    return Container(
      width: size,
      height: size,
      decoration: BoxDecoration(
        color: bg,
        // Grows a little with the tile — a 40 px square at the control radius
        // reads squarer than a 32 px one — but stays inside the theme's two
        // steps rather than inventing a third.
        borderRadius: BorderRadius.circular(
          size >= 40 ? kPanelRadius : kControlRadius,
        ),
      ),
      child: Icon(icon, size: _iconSize, color: fg),
    );
  }
}
