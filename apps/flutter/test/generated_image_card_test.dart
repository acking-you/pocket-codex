import 'dart:convert';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/screens/app_session/generated_image_card.dart';
import 'package:pocket_codex/src/screens/app_session/transcript_model.dart';
import 'package:pocket_codex/src/screens/app_session/transcript_rows.dart';
import 'package:pocket_codex/src/widgets/message_images.dart';

void main() {
  for (final width in [360.0, 800.0, 1280.0]) {
    for (final brightness in Brightness.values) {
      testWidgets('generation lifecycle at $width in $brightness', (t) async {
        t.view.devicePixelRatio = 1;
        t.view.physicalSize = Size(width, 800);
        addTearDown(t.view.reset);
        final item = TranscriptItem(
          id: 'image',
          type: 'imageGeneration',
          title: 'A blue circle on a white background',
          text: '{"status":"in_progress"}',
          streaming: true,
        );
        Future<void> show({bool reducedMotion = false}) => t.pumpWidget(
          MaterialApp(
            theme: brightness == Brightness.dark ? darkTheme() : lightTheme(),
            localizationsDelegates: AppLocalizations.localizationsDelegates,
            supportedLocales: AppLocalizations.supportedLocales,
            home: MediaQuery(
              data: MediaQueryData(
                size: Size(width, 800),
                disableAnimations: reducedMotion,
              ),
              child: Scaffold(
                body: Padding(
                  padding: const EdgeInsets.all(20),
                  child: GeneratedImageCard(item: item),
                ),
              ),
            ),
          ),
        );
        await show();
        await t.pump(const Duration(milliseconds: 200));
        expect(find.byType(ImageLoadingPlaceholder), findsOneWidget);
        expect(find.byType(CircularProgressIndicator), findsOneWidget);
        expect(buildTranscriptRows([item]).single, same(item));
        expect(t.takeException(), isNull);
        await show(reducedMotion: true);
        expect(find.byType(CircularProgressIndicator), findsNothing);
        expect(find.text('Generating image…'), findsOneWidget);
        item.streaming = false;
        await show();
        expect(find.text('Image generation incomplete'), findsOneWidget);
        expect(find.byType(ImageLoadingPlaceholder), findsNothing);
        item.text =
            '{"status":"failed","failure":{"type":"usageLimitExceeded"}}';
        await show();
        expect(
          find.text('Image generation usage limit reached'),
          findsOneWidget,
        );
        expect(find.byType(CircularProgressIndicator), findsNothing);
        expect(t.takeException(), isNull);
      });
    }
  }

  // Opt-in visual proof using the output of an isolated, real Codex generation.
  const nativePath = String.fromEnvironment('PCX_IMAGE_OUTPUT');
  const captureDirectory = String.fromEnvironment('PCX_IMAGE_SCREENSHOTS');
  if (nativePath.isNotEmpty) {
    for (final brightness in Brightness.values) {
      testWidgets('real native image decodes in $brightness', (t) async {
        t.view.devicePixelRatio = 1;
        t.view.physicalSize = const Size(360, 800);
        addTearDown(t.view.reset);
        final bytes = await t.runAsync(() => File(nativePath).readAsBytes());
        final boundary = GlobalKey();
        await t.pumpWidget(
          MaterialApp(
            theme: brightness == Brightness.dark ? darkTheme() : lightTheme(),
            localizationsDelegates: AppLocalizations.localizationsDelegates,
            supportedLocales: AppLocalizations.supportedLocales,
            home: RepaintBoundary(
              key: boundary,
              child: Scaffold(
                body: SafeArea(
                  child: Padding(
                    padding: const EdgeInsets.all(20),
                    child: GeneratedImageCard(
                      item: TranscriptItem(
                        id: 'native',
                        type: 'imageGeneration',
                        title: 'A blue circle on a white background',
                        text: '{"status":"completed"}',
                        images: resolveImageUrls([
                          'data:image/png;base64,${base64Encode(bytes!)}',
                        ]),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ),
        );
        await t.pumpAndSettle();
        await t.runAsync(() async {
          await Future<void>.delayed(const Duration(milliseconds: 200));
        });
        await t.pump();
        expect(find.byKey(const Key('msg-image-0')), findsOneWidget);
        expect(find.byIcon(Icons.broken_image_outlined), findsNothing);
        expect(t.takeException(), isNull);
        if (captureDirectory.isNotEmpty) {
          await t.runAsync(() async {
            final render =
                boundary.currentContext!.findRenderObject()!
                    as RenderRepaintBoundary;
            final image = await render.toImage(pixelRatio: 1);
            final data = await image.toByteData(format: ui.ImageByteFormat.png);
            await File(
              '$captureDirectory/native-${brightness.name}.png',
            ).writeAsBytes(data!.buffer.asUint8List());
            image.dispose();
          });
        }
      });
    }
  }
}
