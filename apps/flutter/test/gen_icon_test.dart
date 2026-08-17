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
Future<Uint8List> compose(
  Master m, {
  double size = 1024,
  double fraction = 1.0,
  Color? background,
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
  canvas.drawImageRect(
    m.image,
    m.tile,
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

    // In-app brand marks + splash images, one per theme, transparent corners.
    await writePng('assets/logo/mark_light.png', await compose(light));
    await writePng('assets/logo/mark_dark.png', await compose(dark));

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
      'icon/icon_glyph.png',
      'icon/icon_mobile.png',
      'icon/icon_adaptive_bg.png',
      'icon/icon_adaptive_fg.png',
    ]) {
      expect(File(f).existsSync(), isTrue, reason: '$f not written');
    }
  }, skip: skip);

  test('derive tray assets (png + multi-size ico)', () async {
    final dark = await loadMaster('icon/logo_dark.png');
    Directory('assets/tray').createSync(recursive: true);

    // macOS / Linux load the icon as a PNG; a 256 source scales down cleanly
    // to the ~18-22 px the tray actually shows.
    await writePng('assets/tray/tray.png', await compose(dark, size: 256));

    // Windows: tray_manager hands the path to LoadImage(IMAGE_ICON,
    // LR_LOADFROMFILE), which needs a true .ico — a .png silently fails to
    // load. Render each frame size directly (crisper than downscaling one big
    // raster) and pack them into a PNG-framed ICO (Vista+). LoadImage asks for
    // SM_CXSMICON (16-24 px), so the small frames carry the on-screen look.
    final frames = <img.Image>[];
    for (final s in [16, 24, 32, 48, 256]) {
      final png = await compose(dark, size: s.toDouble());
      frames.add(img.decodePng(png)!);
    }
    final ico = img.IcoEncoder().encodeImages(frames);
    await File('assets/tray/tray.ico').writeAsBytes(ico);

    expect(File('assets/tray/tray.png').existsSync(), isTrue);
    expect(File('assets/tray/tray.ico').existsSync(), isTrue);
  }, skip: skip);
}
