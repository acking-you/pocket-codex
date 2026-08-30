// Derives every launcher / splash / tray / in-app logo asset from the two
// checked-in brand masters:
//
//   icon/logo_light.png  white tile, ink cloud->_ glyph, cyan→violet arcs
//   icon/logo_dark.png   ink tile, white cloud->_ glyph, mint arcs
//
// The masters are AI-generated opaque PNGs (no alpha, black surround, tile
// inset varies), so this pipeline normalises them: it detects the tile's
// bounding box against the surround, samples the tile's interior colour, and
// re-composites with a clean 22% rounded-rect clip (the macOS squircle
// proportion) at whatever inset each consumer needs.
//
// Regeneration is opt-in: PNG encoding is not byte-identical across platforms,
// so writing on every `flutter test` run would dirty the checked-in assets.
// Without REGEN_ICONS=1 these tests are skipped.
//
// Run: REGEN_ICONS=1 fvm flutter test test/gen_icon_test.dart
// Outputs (launcher — from logo_dark.png, works on light AND dark docks):
//   icon/icon_glyph.png        rounded tile + margin (desktop / web)
//   icon/icon_mobile.png       full-bleed opaque tile (iOS + Android legacy)
//   icon/icon_adaptive_bg.png  solid tile colour (Android adaptive background)
//   icon/icon_adaptive_fg.png  tile scaled into the safe zone (adaptive fg)
// Outputs (Windows launcher — from logo_dark.png):
//   windows/runner/resources/app_icon.ico
//                              multi-size .ico. NOT flutter_launcher_icons'
//                              job: it writes one frame, and Windows' own
//                              downscale to 16/32 px is blurry.
// Outputs (in-app + splash — theme-matched):
//   assets/logo/mark_light.png rounded light tile (light theme / light splash)
//   assets/logo/mark_dark.png  rounded dark tile  (dark theme / dark splash)
// Outputs (tray — from logo_dark.png):
//   assets/tray/tray.png       macOS / Linux tray (loaded as a PNG)
//   assets/tray/tray.ico       Windows tray (multi-size .ico; tray_manager
//                              feeds it to LoadImage, which needs a real ICO)
//
// It also prints the sampled tile interior colours: keep
// `flutter_native_splash.color` / `color_dark` in pubspec.yaml in sync with
// them so the splash tile melts seamlessly into the splash background.
import 'dart:io';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:image/image.dart' as img;

/// A decoded master plus the geometry/colour facts the compositor needs.
class Master {
  Master(this.image, this.pixels, this.tile, this.interior);

  /// The decoded PNG.
  final ui.Image image;

  /// Raw RGBA bytes of [image].
  final Uint8List pixels;

  /// Bounding box of the tile (everything that differs from the surround).
  final Rect tile;

  /// The tile's flat interior colour (sampled inside the tile, off-glyph).
  final Color interior;
}

Color _pixel(Uint8List rgba, int width, int x, int y) {
  final i = (y * width + x) * 4;
  return Color.fromARGB(rgba[i + 3], rgba[i], rgba[i + 1], rgba[i + 2]);
}

int _diff(Color a, Color b) {
  return ((a.r - b.r).abs() + (a.g - b.g).abs() + (a.b - b.b).abs() * 255)
      .round();
}

/// Loads a master PNG and derives its tile bbox + interior colour.
Future<Master> loadMaster(String path) async {
  final bytes = await File(path).readAsBytes();
  final codec = await ui.instantiateImageCodec(bytes);
  final image = (await codec.getNextFrame()).image;
  final data = await image.toByteData(format: ui.ImageByteFormat.rawRgba);
  final rgba = data!.buffer.asUint8List();
  final w = image.width, h = image.height;

  // Everything sufficiently different from the corner pixel is "tile".
  final surround = _pixel(rgba, w, 0, 0);
  var minX = w, minY = h, maxX = -1, maxY = -1;
  for (var y = 0; y < h; y++) {
    for (var x = 0; x < w; x++) {
      if (_diff(_pixel(rgba, w, x, y), surround) > 30) {
        if (x < minX) minX = x;
        if (x > maxX) maxX = x;
        if (y < minY) minY = y;
        if (y > maxY) maxY = y;
      }
    }
  }
  expect(maxX, greaterThan(minX), reason: 'no tile found in $path');
  final tile = Rect.fromLTRB(
    minX.toDouble(),
    minY.toDouble(),
    (maxX + 1).toDouble(),
    (maxY + 1).toDouble(),
  );

  // Sample the flat tile colour near the top-left inside the tile — clear of
  // the centred glyph and of the arcs in the top-right corner.
  final interior = _pixel(
    rgba,
    w,
    (tile.left + tile.width * 0.14).round(),
    (tile.top + tile.height * 0.14).round(),
  );
  return Master(image, rgba, tile, interior);
}

/// Draws [m]'s tile centred at [fraction] of a [size] canvas, clipped to a
/// 22% rounded rect. With a [background] the canvas is opaque (full-bleed
/// consumers); without one the surround stays transparent.
///
/// [zoom] enlarges the tile's ARTWORK within that rounded rect, cropping the
/// master's own padding rather than the tile itself: the rounded silhouette stays
/// exactly [fraction] of the canvas, while the mark inside grows. The tray needs
/// this — the glyph is only ~72% of the tile, so at a 16 px frame it lands on
/// ~10 px and the cloud outline plus the `>_` prompt dissolve into noise. Zooming
/// spends those pixels on the mark instead of on margin.
Future<Uint8List> compose(
  Master m, {
  double size = 1024,
  double fraction = 1.0,
  Color? background,
  double zoom = 1.0,
}) async {
  final recorder = ui.PictureRecorder();
  final canvas = Canvas(recorder, Rect.fromLTWH(0, 0, size, size));
  if (background != null) {
    canvas.drawRect(
      Rect.fromLTWH(0, 0, size, size),
      Paint()..color = background,
    );
  }
  final draw = size * fraction;
  final offset = (size - draw) / 2;
  final dst = Rect.fromLTWH(offset, offset, draw, draw);
  canvas.save();
  canvas.clipRRect(RRect.fromRectAndRadius(dst, Radius.circular(draw * 0.22)));
  // Zoom crops the SOURCE rect, so the destination — and therefore the rounded
  // silhouette — is unchanged; only how much of the master fills it varies.
  final src = zoom == 1.0
      ? m.tile
      : Rect.fromCenter(
          center: m.tile.center,
          width: m.tile.width / zoom,
          height: m.tile.height / zoom,
        );
  canvas.drawImageRect(
    m.image,
    src,
    dst,
    Paint()..filterQuality = FilterQuality.high,
  );
  canvas.restore();
  final out = await recorder.endRecording().toImage(size.toInt(), size.toInt());
  final png = await out.toByteData(format: ui.ImageByteFormat.png);
  return png!.buffer.asUint8List();
}

Future<void> writePng(String path, Uint8List bytes) =>
    File(path).writeAsBytes(bytes);

/// Extracts the transparent GLYPH (cloud + prompt + arcs, no tile) from a
/// master by chroma-keying out the tile's flat interior colour, trimming to
/// content and centring on a square transparent canvas with ~8% margins.
///
/// In-app brand marks use this: a rounded launcher tile dropped into a page
/// reads as a pasted app icon, while the bare glyph sits on any surface like
/// an ordinary illustration. With [recolor] every kept pixel is repainted
/// (preserving the keyed alpha) — used for the macOS template tray icon,
/// which must be black + alpha so the menu bar can tint it.
img.Image glyphFromMaster(String path, {img.ColorRgb8? recolor}) {
  final src = img.decodePng(File(path).readAsBytesSync())!;
  int maxDiff(img.Pixel p, num r, num g, num b) => [
    (p.r - r).abs(),
    (p.g - g).abs(),
    (p.b - b).abs(),
  ].reduce((a, c) => a > c ? a : c).round();

  // Tile bbox: everything sufficiently different from the corner surround.
  final corner = src.getPixel(0, 0);
  var minX = src.width, minY = src.height, maxX = -1, maxY = -1;
  for (final p in src) {
    if (maxDiff(p, corner.r, corner.g, corner.b) > 30) {
      if (p.x < minX) minX = p.x;
      if (p.x > maxX) maxX = p.x;
      if (p.y < minY) minY = p.y;
      if (p.y > maxY) maxY = p.y;
    }
  }
  // Inset the crop well past the tile boundary: the tile's rounded corners,
  // hairline border and the anti-aliased tile→surround transition all differ
  // from the interior colour and would survive the key as a ghost ring. The
  // glyph sits centred with generous margins, so 8% per side is safe.
  final inset = ((maxX - minX + 1) * 0.08).round();
  final tile = img.copyCrop(
    src,
    x: minX + inset,
    y: minY + inset,
    width: maxX - minX + 1 - inset * 2,
    height: maxY - minY + 1 - inset * 2,
  );

  // Key out the flat tile colour (sampled off-glyph, clear of the arcs); the
  // 10..60 ramp keeps anti-aliased glyph edges as partial alpha.
  final interior = tile.getPixel(
    (tile.width * 0.14).round(),
    (tile.height * 0.14).round(),
  );
  final keyed = img.Image(
    width: tile.width,
    height: tile.height,
    numChannels: 4,
  );
  var gMinX = tile.width, gMinY = tile.height, gMaxX = -1, gMaxY = -1;
  for (final p in tile) {
    final d = maxDiff(p, interior.r, interior.g, interior.b);
    final a = (((d - 10) * 255) / 50).clamp(0, 255).round();
    if (a == 0) continue;
    keyed.setPixelRgba(
      p.x,
      p.y,
      recolor?.r ?? p.r,
      recolor?.g ?? p.g,
      recolor?.b ?? p.b,
      a,
    );
    if (a > 8) {
      if (p.x < gMinX) gMinX = p.x;
      if (p.x > gMaxX) gMaxX = p.x;
      if (p.y < gMinY) gMinY = p.y;
      if (p.y > gMaxY) gMaxY = p.y;
    }
  }
  final trimmed = img.copyCrop(
    keyed,
    x: gMinX,
    y: gMinY,
    width: gMaxX - gMinX + 1,
    height: gMaxY - gMinY + 1,
  );
  final side =
      (trimmed.width > trimmed.height
              ? trimmed.width * 1.16
              : trimmed.height * 1.16)
          .round();
  final canvas = img.Image(width: side, height: side, numChannels: 4);
  img.compositeImage(
    canvas,
    trimmed,
    dstX: (side - trimmed.width) ~/ 2,
    dstY: (side - trimmed.height) ~/ 2,
  );
  return canvas;
}

/// Encodes [image] resized to [size]x[size] (cubic) as PNG bytes.
Uint8List glyphPng(img.Image image, int size) => img.encodePng(
  img.copyResize(
    image,
    width: size,
    height: size,
    interpolation: img.Interpolation.cubic,
  ),
);

String _hex(Color c) =>
    '#${(c.r * 255).round().toRadixString(16).padLeft(2, '0')}'
            '${(c.g * 255).round().toRadixString(16).padLeft(2, '0')}'
            '${(c.b * 255).round().toRadixString(16).padLeft(2, '0')}'
        .toUpperCase();

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  // Rasterisation output differs per platform, so regenerating unconditionally
  // would leave the working tree dirty after a plain `flutter test`.
  final skip = Platform.environment['REGEN_ICONS'] == '1'
      ? false
      : 'set REGEN_ICONS=1 to regenerate the checked-in icon assets';

  test('derive launcher + in-app assets from the brand masters', () async {
    final light = await loadMaster('icon/logo_light.png');
    final dark = await loadMaster('icon/logo_dark.png');
    // Keep flutter_native_splash color / color_dark in sync with these.
    debugPrint(
      'tile interior light=${_hex(light.interior)} '
      'dark=${_hex(dark.interior)}',
    );

    // Splash images, one per theme, transparent corners (the splash background
    // colour equals the tile interior, so only the glyph shows).
    await writePng('assets/logo/mark_light.png', await compose(light));
    await writePng('assets/logo/mark_dark.png', await compose(dark));

    // In-app brand marks: the bare theme-matched glyph, no tile — a launcher
    // tile inside a page reads as a pasted app icon.
    await File(
      'assets/logo/glyph_light.png',
    ).writeAsBytes(glyphPng(glyphFromMaster('icon/logo_light.png'), 512));
    await File(
      'assets/logo/glyph_dark.png',
    ).writeAsBytes(glyphPng(glyphFromMaster('icon/logo_dark.png'), 512));

    // Launcher icons all come from the dark master: the ink tile reads on
    // light AND dark docks/launchers, and matches the app's brand surface.
    // Desktop/web get the usual ~10% margin; mobile is full-bleed opaque
    // (the OS applies its own mask, and iOS rejects alpha).
    await writePng('icon/icon_glyph.png', await compose(dark, fraction: 0.9));
    await writePng(
      'icon/icon_mobile.png',
      await compose(dark, background: dark.interior),
    );
    // Android adaptive layers: solid tile colour behind, the tile shrunk into
    // the safe zone in front (masks show only the centre ~66%).
    await writePng(
      'icon/icon_adaptive_bg.png',
      await compose(dark, fraction: 0, background: dark.interior),
    );
    await writePng(
      'icon/icon_adaptive_fg.png',
      await compose(dark, fraction: 0.72, background: dark.interior),
    );

    for (final f in [
      'assets/logo/mark_light.png',
      'assets/logo/mark_dark.png',
      'assets/logo/glyph_light.png',
      'assets/logo/glyph_dark.png',
      'icon/icon_glyph.png',
      'icon/icon_mobile.png',
      'icon/icon_adaptive_bg.png',
      'icon/icon_adaptive_fg.png',
    ]) {
      expect(File(f).existsSync(), isTrue, reason: '$f not written');
    }
  }, skip: skip);

  test('derive the Windows launcher icon (multi-size ico)', () async {
    // `flutter_launcher_icons` writes this file too, but with ONE 256 px frame
    // (`icon_size: 256` in pubspec.yaml is all it can express). Windows then
    // downscales that single raster everywhere it needs a smaller icon — the
    // taskbar at 32 px, the title bar and Alt-Tab at 16 px — and a detailed mark
    // reduced 8:1 in one step comes out visibly blurry. macOS never showed the
    // problem because its `.appiconset` ships each size as its own file.
    //
    // So the launcher icon is rendered per size and packed like the tray icon
    // below, which has always done this. Written AFTER `flutter_launcher_icons`
    // runs, since that tool would otherwise overwrite it.
    final dark = await loadMaster('icon/logo_dark.png');
    final frames = <img.Image>[];
    // The sizes Windows actually asks for: 16 title bar / Alt-Tab, 20 and 24 at
    // fractional DPI scaling, 32 taskbar, 48 large icons, 64 for 150% desktop,
    // 128 and 256 for the extra-large views and Explorer's tile mode.
    for (final s in [16, 20, 24, 32, 48, 64, 128, 256]) {
      final png = await compose(dark, size: s.toDouble(), fraction: 0.9);
      frames.add(img.decodePng(png)!);
    }
    await File(
      'windows/runner/resources/app_icon.ico',
    ).writeAsBytes(img.IcoEncoder().encodeImages(frames));

    expect(File('windows/runner/resources/app_icon.ico').existsSync(), isTrue);
  }, skip: skip);

  test('derive tray assets (png + template + multi-size ico)', () async {
    final dark = await loadMaster('icon/logo_dark.png');
    Directory('assets/tray').createSync(recursive: true);

    // Linux loads the icon as a PNG; a 256 source scales down cleanly to the
    // ~18-22 px the tray actually shows.
    await writePng('assets/tray/tray.png', await compose(dark, size: 256));

    // macOS wants a TEMPLATE image (black + alpha; the menu bar tints it to
    // match light/dark mode and highlight state). tray_manager loads the
    // exact asset key and pins it to 18pt, so ship the @2x file directly —
    // Retina fills those points, 1x displays scale it back down cleanly.
    await File('assets/tray/tray_template@2x.png').writeAsBytes(
      glyphPng(
        glyphFromMaster('icon/logo_dark.png', recolor: img.ColorRgb8(0, 0, 0)),
        36,
      ),
    );

    // Windows: tray_manager hands the path to LoadImage(IMAGE_ICON,
    // LR_LOADFROMFILE), which needs a true .ico — a .png silently fails to
    // load. Render each frame size directly (crisper than downscaling one big
    // raster) and pack them into a PNG-framed ICO (Vista+). LoadImage asks for
    // SM_CXSMICON (16-24 px), so the small frames carry the on-screen look.
    //
    // Stays a full-colour rounded tile: that is what a Windows tray icon is, and
    // what every neighbour in the flyout looks like. (macOS is the odd one — its
    // tray wants a monochrome TEMPLATE it tints itself, which is why the file
    // above is black-on-alpha. Do not carry that convention over here.)
    //
    // The small frames zoom the artwork instead. The glyph is only ~72% of the
    // tile, so a 16 px frame gave it ~10 px — the cloud outline and the `>_`
    // prompt landed on too few pixels and read as a smudge. Zooming trades the
    // master's own padding for glyph pixels, most aggressively where pixels are
    // scarcest; past 48 px there are enough to render the mark as drawn.
    // Bounded at 1.25: past that the glyph starts touching the tile's rounded
    // corners, which looks cramped rather than crisp.
    double zoomFor(int px) => switch (px) {
      <= 24 => 1.25,
      <= 32 => 1.18,
      <= 48 => 1.10,
      _ => 1.0,
    };
    final frames = <img.Image>[];
    for (final s in [16, 20, 24, 32, 48, 64, 256]) {
      final png = await compose(dark, size: s.toDouble(), zoom: zoomFor(s));
      frames.add(img.decodePng(png)!);
    }
    final ico = img.IcoEncoder().encodeImages(frames);
    await File('assets/tray/tray.ico').writeAsBytes(ico);

    expect(File('assets/tray/tray.png').existsSync(), isTrue);
    expect(File('assets/tray/tray_template@2x.png').existsSync(), isTrue);
    expect(File('assets/tray/tray.ico').existsSync(), isTrue);
  }, skip: skip);
}
