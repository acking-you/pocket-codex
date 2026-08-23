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
    // Bottom-LEFT, not bottom-centre: away from the centred image, the AppBar,
    // and the zoom pill that now floats at the bottom centre.
    await tester.tapAt(const Offset(80, 550));
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

  testWidgets('the viewer zoom bar steps, clamps and resets', (tester) async {
    await _openViewer(tester, _png(w: 40, h: 40));
    expect(find.byKey(const Key('image-zoom-bar')), findsOneWidget);
    expect(find.text('100%'), findsOneWidget);

    // Zooming out is disabled only at the floor, so it works from 100%.
    await tester.tap(find.byKey(const Key('image-zoom-out')));
    await tester.pumpAndSettle();
    expect(find.text('80%'), findsOneWidget);

    // In twice: 80 → 100 → 125.
    await tester.tap(find.byKey(const Key('image-zoom-in')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('image-zoom-in')));
    await tester.pumpAndSettle();
    expect(find.text('125%'), findsOneWidget);

    // Tapping the readout snaps back to 100% — the fast way out of a
    // deep zoom.
    await tester.tap(find.byKey(const Key('image-zoom-reset')));
    await tester.pumpAndSettle();
    expect(find.text('100%'), findsOneWidget);
  });

  testWidgets('the zoom bar disables its buttons at the limits', (
    tester,
  ) async {
    await _openViewer(tester, _png(w: 40, h: 40));
    // Walk to the ceiling; the step is multiplicative so this terminates well
    // before the loop bound.
    for (var i = 0; i < 12; i++) {
      final btn = tester.widget<IconButton>(
        find.byKey(const Key('image-zoom-in')),
      );
      if (btn.onPressed == null) break;
      await tester.tap(find.byKey(const Key('image-zoom-in')));
      await tester.pumpAndSettle();
    }
    expect(find.text('600%'), findsOneWidget);
    expect(
      tester
          .widget<IconButton>(find.byKey(const Key('image-zoom-in')))
          .onPressed,
      isNull,
      reason: 'at max zoom the + button must say so, not silently no-op',
    );
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

  testWidgets('the viewer keeps the Windows caption buttons', (tester) async {
    // The viewer fills the window, so it replaces the title bar. On Windows the
    // native caption buttons are gone (TitleBarStyle.hidden), so a viewer that
    // doesn't redraw them leaves the window with no way to be closed, minimised
    // or maximised for as long as an image is open.
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    try {
      await _openViewer(tester, _png());
      // Asserted by tooltip, not by icon: the zoom bar's own −/+ controls reuse
      // Icons.remove, so an icon finder can't tell them from a caption button.
      final l10n = await AppLocalizations.delegate.load(const Locale('en'));
      expect(find.byTooltip(l10n.windowMinimize), findsOneWidget);
      expect(find.byTooltip(l10n.windowMaximize), findsOneWidget);
      expect(find.byTooltip(l10n.windowClose), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });
}
