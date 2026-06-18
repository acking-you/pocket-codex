// Rasterises the vector brand mark (icon/pocket_mark.svg) into the transparent
// PNGs the launcher-icon / splash pipeline consumes, so the single SVG stays
// the source of truth and every platform render is pixel-identical to it.
//
// Run: fvm flutter test test/gen_icon_test.dart
// Outputs:
//   icon/icon_glyph.png          big, bare mark (iOS / desktop / web / legacy)
//   icon/adaptive_foreground.png mark inside the Android adaptive safe zone
//   assets/logo/mark.png         in-app BrandLogo + splash glyph
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:flutter_test/flutter_test.dart';

/// Renders [svg] centred at [markFraction] of a [size]x[size] transparent
/// canvas and writes it to [path] as a PNG.
Future<void> renderSvg(
  String svg,
  String path,
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

  final img = await recorder.endRecording().toImage(size.toInt(), size.toInt());
  final bytes = await img.toByteData(format: ui.ImageByteFormat.png);
  await File(path).writeAsBytes(bytes!.buffer.asUint8List());
  info.picture.dispose();
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  test('rasterise pocket mark into icon assets', () async {
    final svg = File('icon/pocket_mark.svg').readAsStringSync();
    await renderSvg(svg, 'icon/icon_glyph.png', 0.82);
    await renderSvg(svg, 'icon/adaptive_foreground.png', 0.58);
    await renderSvg(svg, 'assets/logo/mark.png', 0.82);
    expect(File('icon/icon_glyph.png').existsSync(), isTrue);
    expect(File('icon/adaptive_foreground.png').existsSync(), isTrue);
    expect(File('assets/logo/mark.png').existsSync(), isTrue);
  });
}
