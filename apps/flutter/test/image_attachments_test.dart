import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:image/image.dart' as img;
import 'package:pocket_codex/src/image_attachments.dart';

Uint8List _png({int w = 6, int h = 4, bool alpha = false}) {
  final im = img.Image(width: w, height: h, numChannels: alpha ? 4 : 3);
  img.fill(
    im,
    color: alpha ? img.ColorRgba8(200, 40, 40, 0) : img.ColorRgb8(200, 40, 40),
  );
  return img.encodePng(im);
}

void main() {
  test('small PNG passes through byte-identical (lossless screenshots)', () {
    final bytes = _png();
    final out = processImageBytes(bytes);
    expect(out.mime, 'image/png');
    expect(out.bytes, bytes);
    expect(out.width, 6);
    expect(out.height, 4);
    expect(out.dataUrl, 'data:image/png;base64,${base64Encode(bytes)}');
  });

  test('oversized image is downscaled to the edge cap and re-encoded JPEG', () {
    final bytes = _png(w: kMaxImageEdge * 2, h: 100);
    final out = processImageBytes(bytes);
    expect(out.mime, 'image/jpeg');
    expect(out.width, kMaxImageEdge);
    // Aspect preserved (2*cap x 100 → cap x 50).
    expect(out.height, 50);
    final decoded = img.decodeJpg(out.bytes);
    expect(decoded, isNotNull);
    expect(decoded!.width, kMaxImageEdge);
  });

  test('transparent pixels composite over white, not black', () {
    // Fully transparent red, oversized so it takes the JPEG path.
    final bytes = _png(w: kMaxImageEdge + 10, h: 40, alpha: true);
    final out = processImageBytes(bytes);
    expect(out.mime, 'image/jpeg');
    final decoded = img.decodeJpg(out.bytes)!;
    final p = decoded.getPixel(0, 0);
    // White-ish (JPEG is lossy; allow a wide margin — black would be ~0).
    expect(p.r, greaterThan(200));
    expect(p.g, greaterThan(200));
    expect(p.b, greaterThan(200));
  });

  test('non-image bytes throw FormatException', () {
    expect(
      () => processImageBytes(Uint8List.fromList(utf8.encode('not an image'))),
      throwsFormatException,
    );
  });

  test('sniffImageExtension names the real container', () {
    expect(sniffImageExtension(_png()), 'png');
    final gif = img.encodeGif(img.Image(width: 4, height: 4));
    expect(sniffImageExtension(gif), 'gif');
    final jpg = img.encodeJpg(img.Image(width: 4, height: 4));
    expect(sniffImageExtension(jpg), 'jpg');
  });

  test('decodeImageDataUrl round-trips and rejects junk', () {
    final bytes = _png();
    final url = 'data:image/png;base64,${base64Encode(bytes)}';
    expect(decodeImageDataUrl(url), bytes);
    // Not an image data URL.
    expect(decodeImageDataUrl('/host/path/pic.png'), isNull);
    expect(decodeImageDataUrl('data:text/plain;base64,aGk='), isNull);
    // Missing the base64 marker.
    expect(decodeImageDataUrl('data:image/png,plain'), isNull);
    // Corrupt payload.
    expect(decodeImageDataUrl('data:image/png;base64,!!!'), isNull);
  });
}
