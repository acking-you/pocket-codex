// Derives every brand asset from ONE source: the vector mark drawn by
// [renderMark] below (cloud, `>_` prompt, relay arcs) in the palettes of
// [MarkStyle]. Nothing is traced or downscaled from a raster any more, so
// launcher, tray, splash, in-app logo and README art cannot drift apart.
//
// Regeneration is opt-in: PNG encoding is not byte-identical across platforms,
// so writing on every `flutter test` run would dirty the checked-in assets.
// Without REGEN_ICONS=1 these tests are skipped.
//
// Run: REGEN_ICONS=1 fvm flutter test test/gen_icon_test.dart
// then: fvm dart run flutter_launcher_icons
//       fvm dart run flutter_native_splash:create
//       REGEN_ICONS=1 fvm flutter test test/gen_icon_test.dart
// (the second run rewrites the files those tools overwrite with worse output:
// the Windows .ico and the web maskable icons).
//
// Outputs (masters — reference renderings at 1254 px):
//   icon/logo_light.png        light rendition (light surface, ink, blue arcs)
//   icon/logo_dark.png         dark rendition (dark surface, white, pale arcs)
// Outputs (launcher — the blue brand tile):
//   icon/icon_glyph.png        rounded tile + margin (desktop / web / Linux)
//   icon/icon_mobile.png       full-bleed opaque square (iOS + Android legacy)
//   icon/icon_adaptive_bg.png  the tile's gradient (Android adaptive background)
//   icon/icon_adaptive_fg.png  the glyph inside the safe zone (adaptive fg)
//   windows/runner/resources/app_icon.ico
//                              multi-size .ico drawn per frame. NOT
//                              flutter_launcher_icons' job: it writes one
//                              frame, and Windows' downscale is blurry.
// Outputs (splash — theme-matched glyph on the splash colour, sized for the
// Android 12 circle crop):
//   assets/logo/mark_light.png / mark_dark.png
// Outputs (web maskable PWA icons — full bleed):
//   web/icons/Icon-maskable-192.png / Icon-maskable-512.png
// Outputs (in-app — the bare glyph, no tile):
//   assets/logo/glyph_light.png / glyph_dark.png
// Outputs (tray):
//   assets/tray/tray.png       macOS / Linux tray (loaded as a PNG)
//   assets/tray/tray.ico       Windows tray (multi-size .ico)
//   assets/tray/tray_template@2x.png  macOS template (black + alpha)
// Outputs (repository art, both copies kept identical):
//   assets/logo/logo.png, apps/flutter/assets/logo/logo.png
//   assets/logo/poster.png, apps/flutter/assets/logo/poster.png
//                              the poster's app tile redrawn in place
//
// Keep `flutter_native_splash.color` / `color_dark` in pubspec.yaml (and
// pubspec-desktop.yaml) equal to [MarkStyle.light] / [MarkStyle.dark]'s tile.
import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:image/image.dart' as img;

/// One palette of the mark.
class MarkStyle {
  const MarkStyle(this.tile, this.ink, this.signal);

  /// The tile's fill: a top-left to bottom-right gradient, or one colour.
  final List<Color> tile;

  /// Cloud and prompt.
  final Color ink;

  /// Relay arcs.
  final Color signal;

  /// The launcher, tray and README mark: the logo's blue.
  static const brand = MarkStyle(
    [Color(0xFF4F7DF2), Color(0xFF2443B0)],
    Colors.white,
    Color(0xFFCFDDFF),
  );

  /// Light surfaces: the theme's surface, ink cloud, accent arcs.
  static const light = MarkStyle(
    [Color(0xFFFBFBFA)],
    Color(0xFF1D1D1F),
    Color(0xFF2A52C4),
  );

  /// Dark surfaces: the theme's surface, white cloud, pale accent arcs.
  static const dark = MarkStyle(
    [Color(0xFF19191B)],
    Color(0xFFEDEDEF),
    Color(0xFF93A6F5),
  );
}

/// Draws the brand mark as vectors at exactly [px] pixels: a tile with the
/// cloud, `>_` prompt and relay arcs, in [style].
///
/// Desktop chrome shows the icon at 16-48 px, where a downscaled raster leaves
/// sub-pixel strokes that blur into a smudge. Drawn per size instead, every
/// stroke can be held at a legible width (optical sizing: thicker, relative to
/// the tile, the smaller the icon), the tile edge lands on whole pixels, and
/// the smallest sizes drop the detail they cannot carry.
///
/// [margin] insets the tile; [radius] is its corner as a fraction of its side
/// (0 for a full-bleed square); [tile] false draws the glyph alone on a
/// transparent canvas and [glyph] false the tile alone; [glyphScale] shrinks
/// the glyph within the tile (Android adaptive icons and the Android 12 splash
/// keep it inside their safe circle).
Future<img.Image> renderMark(
  int px, {
  double margin = 0,
  MarkStyle style = MarkStyle.brand,
  double radius = 0.23,
  bool tile = true,
  bool glyph = true,
  double glyphScale = 1,
}) async {
  final recorder = ui.PictureRecorder();
  final canvas = Canvas(recorder);
  final inset = margin.roundToDouble();
  final side = px - inset * 2;
  final rect = Rect.fromLTWH(inset, inset, side, side);
  if (tile) {
    final fill = Paint();
    if (style.tile.length == 1) {
      fill.color = style.tile.single;
    } else {
      fill.shader = ui.Gradient.linear(
        rect.topLeft,
        rect.bottomRight,
        style.tile,
      );
    }
    canvas.drawRRect(
      RRect.fromRectAndRadius(rect, Radius.circular(side * radius)),
      fill,
    );
  }
  if (glyph) _drawGlyph(canvas, rect, style, glyphScale);
  final image = await recorder.endRecording().toImage(px, px);
  final png = await image.toByteData(format: ui.ImageByteFormat.png);
  return img.decodePng(png!.buffer.asUint8List())!;
}

/// The cloud, prompt and arcs inside [tile]. Geometry is in tile units (0..1).
void _drawGlyph(Canvas canvas, Rect tile, MarkStyle style, double scale) {
  final side = tile.width;
  // The glyph is drawn slightly larger in a small tile, where the margin it
  // gets at full size would cost pixels the strokes need.
  final zoom =
      scale *
      switch (side) {
        <= 18 => 1.30,
        <= 26 => 1.18,
        <= 40 => 1.08,
        _ => 1.0,
      };
  Offset p(double x, double y) {
    const cx = 0.53, cy = 0.51;
    return Offset(
      tile.left + ((x - cx) * zoom + 0.5) * side,
      tile.top + ((y - cy) * zoom + 0.5) * side,
    );
  }

  double len(double v) => v * zoom * side;
  // Never thinner than ~1.45 px, or a stroke turns into a grey haze.
  double stroke(double v) => math.max(len(v), 1.45);

  final ink = Paint()
    ..color = style.ink
    ..style = PaintingStyle.stroke
    ..strokeCap = StrokeCap.round
    ..strokeJoin = StrokeJoin.round
    ..isAntiAlias = true;

  // Cloud: three lobes and the flat base, open at the top right where the
  // signal leaves it.
  final cloud = Path();
  void arc(double cx, double cy, double r, double fromDeg, double sweepDeg) {
    cloud.addArc(
      Rect.fromCircle(center: p(cx, cy), radius: len(r)),
      fromDeg * math.pi / 180,
      sweepDeg * math.pi / 180,
    );
  }

  arc(0.347, 0.613, 0.164, 255, -165); // left lobe, down to the base
  arc(0.466, 0.509, 0.176, 198, 112); // top lobe, up and over
  arc(0.642, 0.613, 0.168, -29, 119); // right lobe, down to the base
  cloud
    ..moveTo(p(0.347, 0.777).dx, p(0.347, 0.777).dy)
    ..lineTo(p(0.642, 0.781).dx, p(0.642, 0.781).dy);
  canvas.drawPath(cloud, ink..strokeWidth = stroke(0.050));

  // The prompt. At 16 px there is room for the caret or the underscore,
  // not both legibly; the caret is the one that says "terminal".
  final prompt = Path()
    ..moveTo(p(0.383, 0.530).dx, p(0.383, 0.530).dy)
    ..lineTo(p(0.452, 0.604).dx, p(0.452, 0.604).dy)
    ..lineTo(p(0.383, 0.678).dx, p(0.383, 0.678).dy);
  if (side > 18) {
    prompt
      ..moveTo(p(0.515, 0.678).dx, p(0.515, 0.678).dy)
      ..lineTo(p(0.612, 0.678).dx, p(0.612, 0.678).dy);
  }
  canvas.drawPath(prompt, ink..strokeWidth = stroke(0.042));

  // Relay arcs, in the signal colour so the cloud stays the subject. One arc
  // below 24 px: two would merge into a single blot.
  final signal = Paint()
    ..color = style.signal
    ..style = PaintingStyle.stroke
    ..strokeCap = StrokeCap.round
    ..strokeWidth = stroke(0.050);
  final centre = p(0.651, 0.452);
  for (final r in side < 24 ? const [0.150] : const [0.114, 0.222]) {
    canvas.drawArc(
      Rect.fromCircle(center: centre, radius: len(r)),
      -math.pi / 2,
      math.pi / 2 * 0.94,
      false,
      signal,
    );
  }
}

/// The glyph alone, trimmed and centred on a transparent square with ~8%
/// margins: the in-app logo and the macOS template tray icon.
Future<img.Image> glyphOnly(int px, MarkStyle style) async {
  // The glyph spans about 0.62 of a tile; a tile 1.4x the canvas leaves the
  // ~8% margin the in-app logo has always had.
  final big = await renderMark(px * 2, style: style, tile: false);
  var minX = big.width, minY = big.height, maxX = -1, maxY = -1;
  for (final q in big) {
    if (q.a > 8) {
      if (q.x < minX) minX = q.x;
      if (q.x > maxX) maxX = q.x;
      if (q.y < minY) minY = q.y;
      if (q.y > maxY) maxY = q.y;
    }
  }
  final trimmed = img.copyCrop(
    big,
    x: minX,
    y: minY,
    width: maxX - minX + 1,
    height: maxY - minY + 1,
  );
  final side = (math.max(trimmed.width, trimmed.height) * 1.16).round();
  final square = img.Image(width: side, height: side, numChannels: 4);
  img.compositeImage(
    square,
    trimmed,
    dstX: (side - trimmed.width) ~/ 2,
    dstY: (side - trimmed.height) ~/ 2,
  );
  return img.copyResize(
    square,
    width: px,
    height: px,
    interpolation: img.Interpolation.cubic,
  );
}

/// Glyph scale for surfaces that show only a centred circle of the image.
///
/// At scale 1 the glyph reaches 0.80 of the half-side from the centre. An
/// Android adaptive icon keeps a 66 dp circle of its 108 dp layers (0.61), and
/// the Android 12+ splash a circle two thirds of the icon (0.67). Scaled to 0.70
/// the glyph reaches 0.56, so no mask shape clips it. `_safeZone` checks this.
const _safeZoneScale = 0.70;

/// The tightest mask circle the scaled glyph must fit in (adaptive icon).
const _safeZone = 33 / 54;

/// Frame sizes for a Windows `.ico`: every size the shell asks for at the
/// common display scales (100/125/150/175/200/250%), so none is a resample.
const _icoSizes = [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 96, 128, 256];

/// Where the poster's app tile sits (measured on the 1536x1024 poster): its
/// bounding box, padded a few pixels to cover the anti-aliased edge.
const _posterTile = (x: 437, y: 64, side: 128);

Future<void> _png(String path, img.Image image) =>
    File(path).writeAsBytes(img.encodePng(image));

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  // Rasterisation output differs per platform, so regenerating unconditionally
  // would leave the working tree dirty after a plain `flutter test`.
  final skip = Platform.environment['REGEN_ICONS'] == '1'
      ? false
      : 'set REGEN_ICONS=1 to regenerate the checked-in icon assets';

  // Runs on every `flutter test`, not only when regenerating: it guards the
  // geometry the Android launcher and splash rely on.
  test('the adaptive and splash glyph fits inside every mask circle', () async {
    double reach(img.Image im) {
      final c = im.width / 2;
      var r = 0.0;
      for (final q in im) {
        if (q.a > 16) {
          r = math.max(
            r,
            math.sqrt(math.pow(q.x + .5 - c, 2) + math.pow(q.y + .5 - c, 2)),
          );
        }
      }
      return r / c;
    }

    final glyph = await renderMark(
      432,
      tile: false,
      glyphScale: _safeZoneScale,
    );
    expect(reach(glyph), lessThan(_safeZone - 0.02));
    // The background layer is the tile alone: nothing drawn twice.
    final bg = await renderMark(432, radius: 0, glyph: false);
    expect(
      bg.where((q) => q.r > 200 && q.g > 200 && q.b > 200).length,
      0,
      reason: 'the adaptive background must not carry the glyph',
    );
  });

  test('derive masters, launcher, splash and in-app assets', () async {
    // Reference renderings of the two surface variants.
    await _png(
      'icon/logo_light.png',
      await renderMark(1254, margin: 24, style: MarkStyle.light),
    );
    await _png(
      'icon/logo_dark.png',
      await renderMark(1254, margin: 24, style: MarkStyle.dark),
    );

    // Launchers: the blue tile everywhere, so every platform shows the same
    // icon as the Windows taskbar. Desktop/web get the usual ~10% margin;
    // mobile is a full-bleed opaque square (the OS masks it, iOS rejects
    // alpha).
    await _png('icon/icon_glyph.png', await renderMark(1024, margin: 51));
    await _png('icon/icon_mobile.png', await renderMark(1024, radius: 0));
    // Android adaptive layers. The launcher stacks BOTH, so the background
    // must be the tile alone: drawing the glyph here as well is what put two
    // overlapping clouds on the home screen. The foreground is the glyph alone,
    // inside the 66/108 safe zone that every mask shape keeps.
    await _png(
      'icon/icon_adaptive_bg.png',
      await renderMark(1024, radius: 0, glyph: false),
    );
    await _png(
      'icon/icon_adaptive_fg.png',
      await renderMark(1024, tile: false, glyphScale: _safeZoneScale),
    );

    // Splash, one per theme. Android 12+ shows the icon cropped to a circle
    // two thirds of its side, on the splash colour, so the mark is the bare
    // glyph sized to sit inside that circle — the same image serves the
    // pre-12 splash, iOS and web, where it is simply centred.
    await _png(
      'assets/logo/mark_light.png',
      await renderMark(
        1024,
        style: MarkStyle.light,
        tile: false,
        glyphScale: _safeZoneScale,
      ),
    );
    await _png(
      'assets/logo/mark_dark.png',
      await renderMark(
        1024,
        style: MarkStyle.dark,
        tile: false,
        glyphScale: _safeZoneScale,
      ),
    );

    // In-app brand marks: the bare theme-matched glyph, no tile — a launcher
    // tile inside a page reads as a pasted app icon.
    await _png(
      'assets/logo/glyph_light.png',
      await glyphOnly(512, MarkStyle.light),
    );
    await _png(
      'assets/logo/glyph_dark.png',
      await glyphOnly(512, MarkStyle.dark),
    );
  }, skip: skip);

  test('derive the web maskable icons (full-bleed)', () async {
    // flutter_launcher_icons writes the maskable PWA icons from the same
    // rounded, margined tile as the plain ones. A maskable icon is masked by the
    // launcher, so its transparent margin and corners showed as a ring around
    // the tile. They must be full bleed, with the glyph inside the 80% safe
    // circle — which the scale-1 glyph already is. Written AFTER
    // `flutter_launcher_icons`, which would otherwise overwrite them.
    for (final s in const [192, 512]) {
      await _png(
        'web/icons/Icon-maskable-$s.png',
        await renderMark(s, radius: 0),
      );
    }
  }, skip: skip);

  test('derive the Windows launcher icon (multi-size ico)', () async {
    // Each frame is DRAWN at its size, not resampled: at 24-32 px a
    // downscaled raster leaves 1 px strokes smeared over two. The margin
    // matches the shell's own icons (≈1/16 of the frame, never under 1 px), so
    // the tile sits the same size as its neighbours. Written AFTER
    // `flutter_launcher_icons` runs, which would otherwise overwrite it.
    final frames = <img.Image>[
      for (final s in _icoSizes)
        await renderMark(s, margin: math.max(1, s / 16)),
    ];
    await File(
      'windows/runner/resources/app_icon.ico',
    ).writeAsBytes(img.IcoEncoder().encodeImages(frames));
  }, skip: skip);

  test('derive tray assets (png + template + multi-size ico)', () async {
    Directory('assets/tray').createSync(recursive: true);

    // Linux loads the icon as a PNG and scales it to the ~22 px the panel
    // shows; 64 px keeps that downscale small and its strokes intact.
    await _png('assets/tray/tray.png', await renderMark(64));

    // macOS wants a TEMPLATE image (black + alpha; the menu bar tints it).
    // tray_manager pins it to 18pt, so ship the @2x file directly.
    await _png(
      'assets/tray/tray_template@2x.png',
      await glyphOnly(
        36,
        const MarkStyle([Colors.black], Colors.black, Colors.black),
      ),
    );

    // Windows: LoadImage(IMAGE_ICON) needs a true .ico. Full-colour rounded
    // tile, edge to edge: the notification area gives 16 px at 100% and every
    // pixel of margin is a pixel the cloud loses.
    final frames = <img.Image>[
      for (final s in _icoSizes.where((s) => s <= 64)) await renderMark(s),
    ];
    await File(
      'assets/tray/tray.ico',
    ).writeAsBytes(img.IcoEncoder().encodeImages(frames));
  }, skip: skip);

  test('derive repository art: logo and poster', () async {
    final logo = await renderMark(1024, margin: 51);
    for (final path in ['../../assets/logo/logo.png', 'assets/logo/logo.png']) {
      await _png(path, logo);
    }

    // The poster keeps its illustration; only its app tile is redrawn, over
    // the poster's own background, so the README matches the launcher.
    final poster = img.decodePng(
      File('../../assets/logo/poster.png').readAsBytesSync(),
    )!;
    final t = _posterTile;
    final under = poster.getPixel(t.x - 4, t.y + t.side ~/ 2);
    img.fillRect(
      poster,
      x1: t.x,
      y1: t.y,
      x2: t.x + t.side - 1,
      y2: t.y + t.side - 1,
      color: img.ColorRgb8(under.r.toInt(), under.g.toInt(), under.b.toInt()),
    );
    img.compositeImage(
      poster,
      await renderMark(t.side, margin: 3, radius: 0.22),
      dstX: t.x,
      dstY: t.y,
    );
    for (final path in [
      '../../assets/logo/poster.png',
      'assets/logo/poster.png',
    ]) {
      await _png(path, poster);
    }
  }, skip: skip);
}
