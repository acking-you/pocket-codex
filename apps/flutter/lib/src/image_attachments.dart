import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:image/image.dart' as img;

/// Longest edge an attachment is downscaled to before sending. Keeps a photo's
/// base64 payload in the hundreds-of-KB range (vs multi-MB camera originals)
/// so it stays cheap through the relay tunnel, in the rollout file, and in the
/// model context — while remaining plenty of resolution for the model to read
/// UI text and diagrams.
const int kMaxImageEdge = 1568;

/// JPEG quality for re-encoded attachments.
const int kJpegQuality = 85;

/// Originals at most this size that also fit [kMaxImageEdge] and carry no
/// EXIF rotation are sent as-is — preserving lossless PNG screenshots instead
/// of fuzzing their text through a JPEG round-trip.
const int kPassthroughMaxBytes = 1024 * 1024;

/// The most images one message may carry (UI guard; the protocol has no cap).
const int kMaxImagesPerMessage = 8;

/// A picked image after client-side processing, ready to send.
class ProcessedImage {
  /// Creates a processed image (bytes + the mime they're encoded in).
  const ProcessedImage({
    required this.bytes,
    required this.mime,
    required this.width,
    required this.height,
  });

  /// Encoded image bytes (original passthrough, or the downscaled JPEG).
  final Uint8List bytes;

  /// Mime type of [bytes] (e.g. `image/jpeg`).
  final String mime;

  /// Pixel width of the encoded image.
  final int width;

  /// Pixel height of the encoded image.
  final int height;

  /// The `data:` URL form sent to the app-server — the only image form that
  /// works for BOTH local and relay-tunneled remote hosts (a filesystem path
  /// would resolve on the wrong machine).
  String get dataUrl => 'data:$mime;base64,${base64Encode(bytes)}';
}

/// Decode → EXIF-bake → downscale → re-encode one picked image.
///
/// Pure and top-level so it runs in a background isolate via [compute] — a
/// 12 MP photo takes noticeable CPU to decode in pure Dart, which must not
/// jank the UI. Small, already-fitting originals pass through byte-identical
/// (see [kPassthroughMaxBytes]); everything else is re-encoded as JPEG
/// [kJpegQuality] with the long edge capped at [kMaxImageEdge] and any alpha
/// composited over white (JPEG has no alpha). Throws [FormatException] for
/// data that isn't a decodable image.
ProcessedImage processImageBytes(Uint8List bytes) {
  final decoder = img.findDecoderForData(bytes);
  // frame: 0 — decoding ALL frames of an animated GIF/WebP would materialize
  // every frame's RGBA in memory (potentially hundreds of MB) when only the
  // first is ever used. A small animated original still passes through
  // byte-identical below, keeping its animation.
  final decoded = decoder?.decode(bytes, frame: 0);
  if (decoder == null || decoded == null) {
    throw const FormatException('not a decodable image');
  }
  final mime = _decoderMime(decoder);
  final rotated =
      decoded.exif.imageIfd.hasOrientation &&
      decoded.exif.imageIfd.orientation != 1;
  final longEdge = decoded.width > decoded.height
      ? decoded.width
      : decoded.height;
  if (!rotated &&
      longEdge <= kMaxImageEdge &&
      bytes.length <= kPassthroughMaxBytes &&
      mime != null) {
    return ProcessedImage(
      bytes: bytes,
      mime: mime,
      width: decoded.width,
      height: decoded.height,
    );
  }
  // Bake EXIF rotation so the model (and our thumbnails) see the photo
  // upright, then downscale to the cap.
  var image = rotated ? img.bakeOrientation(decoded) : decoded;
  final edge = image.width > image.height ? image.width : image.height;
  if (edge > kMaxImageEdge) {
    image = image.width >= image.height
        ? img.copyResize(
            image,
            width: kMaxImageEdge,
            interpolation: img.Interpolation.linear,
          )
        : img.copyResize(
            image,
            height: kMaxImageEdge,
            interpolation: img.Interpolation.linear,
          );
  }
  // JPEG drops alpha; composite transparent images over white first so
  // transparent regions don't come out black. hasAlpha covers RGBA (4ch) AND
  // grayscale+alpha (2ch); the latter converts to RGBA so compositing sees
  // color channels.
  if (image.hasAlpha) {
    final rgba = image.numChannels == 4 ? image : image.convert(numChannels: 4);
    final bg = img.Image(width: rgba.width, height: rgba.height);
    img.fill(bg, color: img.ColorRgb8(255, 255, 255));
    img.compositeImage(bg, rgba);
    image = bg;
  }
  final jpg = img.encodeJpg(image, quality: kJpegQuality);
  return ProcessedImage(
    bytes: jpg,
    mime: 'image/jpeg',
    width: image.width,
    height: image.height,
  );
}

/// Test seam for [processImage]: widget tests swap this for a direct call —
/// [compute] spawns a real isolate, whose completion never lands under
/// flutter_test's fake clock (pumpAndSettle would hang).
@visibleForTesting
Future<ProcessedImage> Function(Uint8List bytes) processImageImpl = (bytes) =>
    compute(processImageBytes, bytes);

/// [processImageBytes] on a background isolate.
Future<ProcessedImage> processImage(Uint8List bytes) => processImageImpl(bytes);

String? _decoderMime(img.Decoder decoder) => switch (decoder) {
  img.JpegDecoder() => 'image/jpeg',
  img.PngDecoder() => 'image/png',
  img.GifDecoder() => 'image/gif',
  img.WebPDecoder() => 'image/webp',
  _ => null,
};

/// File extension for [bytes] sniffed from container magic: `png` / `gif` /
/// `webp`, else `jpg` (the only other format this app produces or passes
/// through). Used to suggest an honest filename when saving a viewed image.
String sniffImageExtension(Uint8List bytes) {
  if (bytes.length > 8 && bytes[0] == 0x89 && bytes[1] == 0x50) return 'png';
  if (bytes.length > 4 && bytes[0] == 0x47 && bytes[1] == 0x49 && bytes[2] == 0x46) {
    return 'gif';
  }
  if (bytes.length > 12 &&
      bytes[0] == 0x52 &&
      bytes[1] == 0x49 &&
      bytes[2] == 0x46 &&
      bytes[3] == 0x46 &&
      bytes[8] == 0x57 &&
      bytes[9] == 0x45 &&
      bytes[10] == 0x42 &&
      bytes[11] == 0x50) {
    return 'webp';
  }
  return 'jpg';
}

/// Decode the base64 payload of a `data:image/...` URL; null for any other
/// URL shape (host paths, http URLs) or a corrupt payload.
Uint8List? decodeImageDataUrl(String url) {
  if (!url.startsWith('data:image/')) return null;
  final comma = url.indexOf(',');
  if (comma < 0) return null;
  if (!url.substring(0, comma).endsWith(';base64')) return null;
  try {
    return base64Decode(url.substring(comma + 1));
  } on FormatException {
    return null;
  }
}
