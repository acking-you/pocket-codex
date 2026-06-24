// Rasterises the vector brand marks into the transparent PNGs the
// launcher-icon / splash / tray pipelines consume, so the single SVGs stay the
// source of truth and every platform render is pixel-identical to them.
//
// Run: fvm flutter test test/gen_icon_test.dart
// Outputs (app icon — icon/pocket_mark.svg):
//   icon/icon_glyph.png          big, bare mark (iOS / desktop / web / legacy)
//   icon/adaptive_foreground.png mark inside the Android adaptive safe zone
//   assets/logo/mark.png         in-app BrandLogo + splash glyph
// Outputs (tray icon — icon/tray_mark.svg):
//   assets/tray/tray.png         macOS / Linux tray (loaded as a PNG)
//   assets/tray/tray.ico         Windows tray (multi-size .ico; tray_manager
//                                feeds it to LoadImage, which needs a real ICO)
import 'dart:io';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:image/image.dart' as img;

/// Rasterises [svg] centred at [markFraction] of a [size]x[size] transparent
/// canvas and returns the PNG bytes.
Future<Uint8List> rasteriseSvg(
  String svg,
  double markFraction, {
  double size = 1024,
}) async {
  final info = await vg.loadPicture(SvgStringLoader(svg), null);
  final src = info.size;

  final recorder = ui.PictureRecorder();
  final canvas = Canvas(recorder, Rect.fromLTWH(0, 0, size, size));
  final draw = size * markFraction;
  final scale = draw / src.width;
  final offset = (size - draw) / 2;
  canvas.translate(offset, offset);
  canvas.scale(scale);
  canvas.drawPicture(info.picture);

  final image = await recorder.endRecording().toImage(
    size.toInt(),
    size.toInt(),
  );
  final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
  info.picture.dispose();
  return bytes!.buffer.asUint8List();
}

/// Renders [svg] at [markFraction] of a [size] canvas and writes it to [path]
/// as a PNG.
Future<void> renderSvg(
  String svg,
  String path,
  double markFraction, {
  double size = 1024,
}) async {
  final bytes = await rasteriseSvg(svg, markFraction, size: size);
  await File(path).writeAsBytes(bytes);
}

/// Stacks [layers] (each `(svg, markFraction)`, bottom-first) onto one
/// [size]x[size] transparent canvas and returns the PNG bytes. Each layer is
/// scaled to [markFraction] of the canvas and centred (the SVGs are square), so
/// a full-bleed background (1.0) can carry a smaller centred glyph on top.
Future<Uint8List> compositeSvgs(
  List<(String, double)> layers, {
  double size = 1024,
}) async {
  final recorder = ui.PictureRecorder();
  final canvas = Canvas(recorder, Rect.fromLTWH(0, 0, size, size));
  for (final (svg, markFraction) in layers) {
    final info = await vg.loadPicture(SvgStringLoader(svg), null);
    final draw = size * markFraction;
    final scale = draw / info.size.width;
    final offset = (size - draw) / 2;
    canvas.save();
    canvas.translate(offset, offset);
    canvas.scale(scale);
    canvas.drawPicture(info.picture);
    canvas.restore();
    info.picture.dispose();
  }
  final image = await recorder.endRecording().toImage(
    size.toInt(),
    size.toInt(),
  );
  final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
  return bytes!.buffer.asUint8List();
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('rasterise pocket mark into app-icon assets', () async {
    final svg = File('icon/pocket_mark.svg').readAsStringSync();
    await renderSvg(svg, 'icon/icon_glyph.png', 0.82);
    await renderSvg(svg, 'icon/adaptive_foreground.png', 0.58);
    await renderSvg(svg, 'assets/logo/mark.png', 0.82);
    expect(File('icon/icon_glyph.png').existsSync(), isTrue);
    expect(File('icon/adaptive_foreground.png').existsSync(), isTrue);
    expect(File('assets/logo/mark.png').existsSync(), isTrue);
  });

  test('rasterise mobile icon (brand tile + centred white glyph)', () async {
    final bg = File('icon/mobile_bg.svg').readAsStringSync();
    final glyph = File('icon/mobile_glyph.svg').readAsStringSync();

    // Legacy Android mipmaps + iOS take a single full-bleed image: the brand
    // gradient tile with the white >_ centred and small (the launcher / iOS
    // round the corners). Composited so the glyph sits over the gradient.
    final mobile = await compositeSvgs([(bg, 1.0), (glyph, 0.76)]);
    await File('icon/icon_mobile.png').writeAsBytes(mobile);

    // Android adaptive layers (API 26+): the gradient is the full-bleed
    // background and the white >_ is the foreground. The foreground is rendered
    // a touch smaller (0.66) because the adaptive safe zone shows less of the
    // canvas, so the glyph ends up the same on-screen size as the legacy icon.
    await renderSvg(bg, 'icon/icon_adaptive_bg.png', 1.0);
    await renderSvg(glyph, 'icon/icon_adaptive_fg.png', 0.66);

    expect(File('icon/icon_mobile.png').existsSync(), isTrue);
    expect(File('icon/icon_adaptive_bg.png').existsSync(), isTrue);
    expect(File('icon/icon_adaptive_fg.png').existsSync(), isTrue);
  });

  test('rasterise tray mark into tray assets (png + multi-size ico)', () async {
    final svg = File('icon/tray_mark.svg').readAsStringSync();
    Directory('assets/tray').createSync(recursive: true);

    // macOS / Linux load the icon as a PNG. The SVG already bakes its own
    // padding, so render the full mark (markFraction 1.0); a 256 source scales
    // down cleanly to the ~18-22 px the tray actually shows.
    await renderSvg(svg, 'assets/tray/tray.png', 1.0, size: 256);

    // Windows: tray_manager hands the path to LoadImage(IMAGE_ICON,
    // LR_LOADFROMFILE), which needs a true .ico — a .png silently fails to
    // load. Render each frame size directly from the vector (crisper than
    // downscaling one big raster) and pack them into a PNG-framed ICO
    // (supported by Windows Vista+). LoadImage asks for SM_CXSMICON (16-24 px),
    // so the small frames carry the on-screen look.
    final frames = <img.Image>[];
    for (final s in [16, 24, 32, 48, 256]) {
      final png = await rasteriseSvg(svg, 1.0, size: s.toDouble());
      frames.add(img.decodePng(png)!);
    }
    final ico = img.IcoEncoder().encodeImages(frames);
    await File('assets/tray/tray.ico').writeAsBytes(ico);

    expect(File('assets/tray/tray.png').existsSync(), isTrue);
    expect(File('assets/tray/tray.ico').existsSync(), isTrue);
  });
}
