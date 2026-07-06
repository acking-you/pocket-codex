import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:image/image.dart' as img;
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/widgets/message_images.dart';

Uint8List _png({int w = 8, int h = 8}) =>
    img.encodePng(img.Image(width: w, height: h, numChannels: 3));

String _dataUrl() => 'data:image/png;base64,${base64Encode(_png())}';

Widget _wrap(Widget child) => MaterialApp(
  localizationsDelegates: AppLocalizations.localizationsDelegates,
  supportedLocales: AppLocalizations.supportedLocales,
  home: Scaffold(body: child),
);

Future<void> _openViewer(WidgetTester tester, Uint8List bytes) async {
  await tester.pumpWidget(
    _wrap(
      Builder(
        builder: (context) => ElevatedButton(
          onPressed: () => ImageViewerPage.show(context, [bytes], 0),
          child: const Text('open'),
        ),
      ),
    ),
  );
  await tester.tap(find.text('open'));
  await tester.pumpAndSettle();
}

void main() {
  test(
    'resolveImageUrls keeps a placeholder for an undecodable data image',
    () {
      final out = resolveImageUrls([
        _dataUrl(), // good
        'data:image/png;base64,!!!', // corrupt payload
        '/host/path/pic.png', // host reference
      ]);
      expect(out.length, 3, reason: 'the broken image is kept, not dropped');
      expect(out[0].bytes, isNotNull);
      expect(out[0].broken, isFalse);
      expect(out[1].bytes, isNull);
      expect(out[1].broken, isTrue);
      expect(out[2].hostPath, '/host/path/pic.png');
      expect(out[2].broken, isFalse);
    },
  );

  testWidgets(
    'MessageImagesView shows a broken placeholder for a failed image',
    (tester) async {
      final images = resolveImageUrls(['data:image/png;base64,!!!']);
      await tester.pumpWidget(_wrap(MessageImagesView(images: images)));
      expect(find.byIcon(Icons.broken_image_outlined), findsOneWidget);
    },
  );

  testWidgets('ImageViewerPage closes when tapping the backdrop', (
    tester,
  ) async {
    await _openViewer(tester, _png(w: 20, h: 20));
    expect(find.byType(ImageViewerPage), findsOneWidget);
    // Tap the backdrop (bottom-centre, away from the centred image + AppBar).
    await tester.tapAt(const Offset(400, 550));
    await tester.pumpAndSettle();
    expect(find.byType(ImageViewerPage), findsNothing);
  });

  testWidgets('ImageViewerPage closes on a tap over the image too', (
    tester,
  ) async {
    await _openViewer(tester, _png(w: 400, h: 400));
    // The tap lands on the opaque backdrop layer over the image, not the
    // Image RenderBox itself — that's the point, so silence the miss warning.
    await tester.tap(find.byType(Image), warnIfMissed: false);
    await tester.pumpAndSettle();
    expect(find.byType(ImageViewerPage), findsNothing);
  });

  testWidgets('thumbnail reveals a save button on hover (desktop)', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    try {
      final images = resolveImageUrls([_dataUrl()]);
      await tester.pumpWidget(_wrap(MessageImagesView(images: images)));
      await tester.pumpAndSettle();

      // No save affordance until the pointer hovers the thumbnail.
      expect(find.byIcon(Icons.download_outlined), findsNothing);

      final gesture = await tester.createGesture(kind: PointerDeviceKind.mouse);
      await gesture.addPointer(location: Offset.zero);
      addTearDown(gesture.removePointer);
      await gesture.moveTo(tester.getCenter(find.byType(Image)));
      await tester.pumpAndSettle();

      expect(find.byIcon(Icons.download_outlined), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });
}
