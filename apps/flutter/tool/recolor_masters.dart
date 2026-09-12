// Re-colours the two brand masters onto the design system's palette, writing
// icon/logo_light.png and icon/logo_dark.png in place.
//
// The masters are AI-generated bitmaps, so this works by hue rather than by
// redrawing: the signal arcs are the only saturated pixels in either image, so
// they can be identified by saturation and remapped onto the blue accent while
// the cloud, the prompt glyph and the tile keep their luminance. The tile
// grounds are near-neutral and are matched by how close they sit to the known
// source colour.
//
// Run: dart run tool/recolor_masters.dart
// Then regenerate every derived asset:
//   REGEN_ICONS=1 fvm flutter test test/gen_icon_test.dart

import 'dart:io';
import 'dart:math' as math;

import 'package:image/image.dart' as img;

/// The light theme accent from theme.dart.
const _accentR = 0x35, _accentG = 0x5A, _accentB = 0xCF;

/// The deep end of the accent's hue, for the gradient's far stop.
const _deepR = 0x23, _deepG = 0x43, _deepB = 0xA1;

/// The dark theme's raised-card ground, replacing the masters' near-black blue.
const _darkTileR = 0x1A, _darkTileG = 0x1D, _darkTileB = 0x24;

({double h, double s, double l}) _toHsl(num r, num g, num b) {
  final rf = r / 255, gf = g / 255, bf = b / 255;
  final max = math.max(rf, math.max(gf, bf));
  final min = math.min(rf, math.min(gf, bf));
  final l = (max + min) / 2;
  if (max == min) return (h: 0, s: 0, l: l);
  final d = max - min;
  final s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  double h;
  if (max == rf) {
    h = ((gf - bf) / d + (gf < bf ? 6 : 0)) / 6;
  } else if (max == gf) {
    h = ((bf - rf) / d + 2) / 6;
  } else {
    h = ((rf - gf) / d + 4) / 6;
  }
  return (h: h, s: s, l: l);
}

void main() {
  for (final entry in {
    'icon/logo_light.png': false,
    'icon/logo_dark.png': true,
  }.entries) {
    final path = entry.key;
    final isDark = entry.value;
    final src = img.decodePng(File(path).readAsBytesSync())!;
    var arcs = 0, tiles = 0, glyphs = 0;

    for (final p in src) {
      final hsl = _toHsl(p.r, p.g, p.b);
      // Already migrated blue pixels must not drift on a repeated run.
      if (hsl.h > 0.52 && hsl.h < 0.72 && hsl.s > 0.2) continue;

      // The arcs: the only saturated pixels in either master. Map each onto the
      // accent ramp by how far along the original gradient it sat (cyan→violet
      // in light, mint→teal in dark both run "cool to cooler", so hue position
      // stands in for gradient position), keeping the pixel's own lightness so
      // the anti-aliased edges stay smooth.
      if (hsl.s > 0.25 && hsl.l > 0.18 && hsl.l < 0.95) {
        final t = ((hsl.h - 0.4) / 0.4).clamp(0.0, 1.0);
        final scale = hsl.l / 0.6;
        p
          ..r =
              ((isDark ? 0xA5 - 24 * t : _accentR + (_deepR - _accentR) * t) *
                      scale)
                  .clamp(0, 255)
          ..g =
              ((isDark ? 0xB8 - 24 * t : _accentG + (_deepG - _accentG) * t) *
                      scale)
                  .clamp(0, 255)
          ..b =
              ((isDark ? 0xFF - 15 * t : _accentB + (_deepB - _accentB) * t) *
                      scale)
                  .clamp(0, 255);
        arcs++;
        continue;
      }

      // The light master's cloud + prompt glyph: a blue-black that predates the
      // palette. Neutralise it onto the design's warm ink so the mark and the
      // app's own text are the same colour. Dark keeps its white glyph.
      if (!isDark && hsl.l < 0.35 && p.b - p.r >= 6) {
        final t = (hsl.l / 0.35).clamp(0.0, 1.0);
        // Toward the ink at the glyph's own lightness, so anti-aliased edges
        // still ramp smoothly into the tile.
        p
          ..r = (0x1A + (0xF9 - 0x1A) * t * 0.06).clamp(0, 255)
          ..g = (0x1A + (0xF9 - 0x1A) * t * 0.06).clamp(0, 255)
          ..b = (0x19 + (0xF7 - 0x19) * t * 0.06).clamp(0, 255);
        glyphs++;
        continue;
      }

      // The dark tile's near-black blue → the design's neutral raised card.
      // Matched by distance so the tile's own subtle shading is preserved as a
      // delta rather than flattened.
      //
      // Only the original neutral tile is migrated; already-blue pixels keep
      // their colour if this tool is run again.
      if (isDark) {
        final dr = p.r - 0x1E, dg = p.g - 0x1E, db = p.b - 0x1E;
        if (hsl.s < 0.05 && hsl.l > 0.06 && hsl.l < 0.2) {
          p
            ..r = (_darkTileR + dr).clamp(0, 255)
            ..g = (_darkTileG + dg).clamp(0, 255)
            ..b = (_darkTileB + db).clamp(0, 255);
          tiles++;
        }
      }
    }

    File(path).writeAsBytesSync(img.encodePng(src));
    stdout.writeln(
      '$path: recoloured $arcs arc px, $glyphs glyph px, $tiles tile px',
    );
  }
}
