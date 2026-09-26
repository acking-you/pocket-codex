import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/local_session_view_screen.dart';
import 'package:pocket_codex/src/screens/app_session/generated_image_card.dart';
import 'package:pocket_codex/src/widgets/message_images.dart';

import 'fake_bridge_api.dart';

final _png = base64Decode(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=',
);

class ImageApi extends FakeBridgeApi {
  final reads = <String>[];
  Completer<Uint8List>? pending;
  @override
  Future<Uint8List> metaReadThreadImage(
    String serviceKey,
    String threadId,
    String path,
  ) async {
    reads.add('$serviceKey/$threadId/$path');
    return pending?.future ?? Future.value(_png);
  }
}

Widget host(FakeBridgeApi api, {String? service = 'pcx:remote:app:test'}) =>
    ProviderScope(
      overrides: [bridgeApiProvider.overrideWithValue(api)],
      child: MaterialApp(
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: LocalSessionViewScreen(
          threadId: 'thread',
          serviceKey: service,
          cwd: '/host',
        ),
      ),
    );

ThreadItem imageItem(String path) => ThreadItem(
  id: 'image',
  itemType: 'imageGeneration',
  title: 'Blue circle',
  text: '{"status":"completed"}',
  images: [path],
);

void main() {
  testWidgets('read-only remote files and numeric localhost use host routing', (
    t,
  ) async {
    final api = FakeBridgeApi();
    api.transcripts['thread'] = const [
      ThreadItem(
        id: 'message',
        itemType: 'agentMessage',
        title: '',
        text: '[report](report.md)\n\n[web](http://2130706433:3000)',
      ),
    ];
    api.fileBytes['report.md'] = Uint8List.fromList(utf8.encode('Host report'));
    await t.pumpWidget(host(api));
    await t.pumpAndSettle();
    await t.tap(find.text('report', findRichText: true));
    await t.pumpAndSettle();
    expect(find.text('/host/report.md'), findsOneWidget);
    await t.tap(find.text('Preview'));
    await t.pumpAndSettle();
    expect(find.text('Host report'), findsOneWidget);
    expect(api.filePreviewRequests, ['pcx:remote:app:test/thread/report.md']);
    expect(find.text('Download'), findsOneWidget);
    await t.tap(find.text('Close'));
    await t.pumpAndSettle();
    await t.tap(find.text('web', findRichText: true));
    await t.pumpAndSettle();
    expect(find.textContaining('pb-mapper'), findsOneWidget);
    expect(t.takeException(), isNull);
  });

  testWidgets('read-only local files preview directly without meta service', (
    t,
  ) async {
    late Directory dir;
    late File file;
    await t.runAsync(() async {
      dir = await Directory.systemTemp.createTemp('pcx-readonly-');
      file = await File('${dir.path}/report.txt').writeAsString('Local report');
    });
    addTearDown(() => dir.delete(recursive: true));
    final api = FakeBridgeApi();
    api.transcripts['thread'] = [
      ThreadItem(
        id: 'message',
        itemType: 'agentMessage',
        title: '',
        text: '[report](${file.path})',
      ),
    ];
    await t.pumpWidget(host(api, service: null));
    await t.pumpAndSettle();
    await t.tap(find.text('report', findRichText: true));
    await t.pumpAndSettle();
    await t.runAsync(() async {
      await t.tap(find.text('Preview'));
      await Future<void>.delayed(const Duration(milliseconds: 80));
    });
    await t.pumpAndSettle();
    expect(find.text('Local report'), findsOneWidget);
    expect(api.filePreviewRequests, isEmpty);
  });

  testWidgets(
    'read-only generated images use the selected host and refresh changed references',
    (t) async {
      final api = ImageApi();
      api.transcripts['thread'] = [imageItem('/host/first.png')];
      await t.pumpWidget(host(api));
      await t.pumpAndSettle();
      expect(find.byType(GeneratedImageCard), findsOneWidget);
      expect(find.byKey(const Key('msg-image-0')), findsOneWidget);
      expect(api.reads, ['pcx:remote:app:test/thread//host/first.png']);
      api.transcripts['thread'] = [imageItem('/host/final.png')];
      await t.tap(find.byKey(const Key('local-view-refresh')));
      await t.pumpAndSettle();
      expect(api.reads.last, endsWith('/host/final.png'));
      await t.tap(find.byKey(const Key('msg-image-0')));
      await t.pumpAndSettle();
      expect(find.byType(ImageViewerPage), findsOneWidget);
      expect(t.takeException(), isNull);
    },
  );

  testWidgets('changing read-only hosts discards pending image results', (
    t,
  ) async {
    final api = ImageApi()..pending = Completer<Uint8List>();
    api.transcripts['thread'] = [imageItem('/same.png')];
    await t.pumpWidget(host(api));
    await t.pump();
    await t.pump(const Duration(milliseconds: 100));
    final old = api.pending!;
    api.pending = Completer<Uint8List>();
    await t.pumpWidget(host(api, service: 'pcx:other:app:test'));
    await t.pump();
    await t.pump(const Duration(milliseconds: 100));
    old.complete(_png);
    await t.pump();
    expect(find.byKey(const Key('msg-image-0')), findsNothing);
    api.pending!.complete(_png);
    await t.pumpAndSettle();
    expect(api.reads, [
      'pcx:remote:app:test/thread//same.png',
      'pcx:other:app:test/thread//same.png',
    ]);
    expect(find.byKey(const Key('msg-image-0')), findsOneWidget);
    expect(t.takeException(), isNull);
  });
}
