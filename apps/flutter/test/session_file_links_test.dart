import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:file_selector_platform_interface/file_selector_platform_interface.dart'
    as fsel;
import 'package:plugin_platform_interface/plugin_platform_interface.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/file_exports.dart';
import 'package:pocket_codex/src/widgets/links.dart';
import 'package:pocket_codex/src/widgets/markdown_view.dart';
import 'package:pocket_codex/src/widgets/session_file_links.dart';

import 'fake_bridge_api.dart';

class DelayedApi extends FakeBridgeApi {
  final result = Completer<FilePreviewData>();
  @override
  Future<FilePreviewData> metaFilePreview(
    String serviceKey,
    String? threadId,
    String href,
  ) => result.future;
}

class DownloadApi extends FakeBridgeApi {
  String? destination;
  @override
  Future<void> metaFileDownload(
    String serviceKey,
    String? threadId,
    String href,
    String destination,
  ) async {
    this.destination = destination;
    await File(
      destination,
    ).writeAsString('Downloaded from $serviceKey/$threadId');
  }
}

class SavePicker extends fsel.FileSelectorPlatform
    with MockPlatformInterfaceMixin {
  String? destination;
  bool fail = false;
  @override
  Future<fsel.FileSaveLocation?> getSaveLocation({
    List<fsel.XTypeGroup>? acceptedTypeGroups,
    fsel.SaveDialogOptions options = const fsel.SaveDialogOptions(),
  }) async {
    if (fail) throw StateError('Destination unavailable');
    return destination == null ? null : fsel.FileSaveLocation(destination!);
  }
}

Future<void> host(
  WidgetTester tester,
  FakeBridgeApi api,
  String href, {
  bool markdown = false,
  Brightness brightness = Brightness.light,
}) async {
  await tester.pumpWidget(
    MaterialApp(
      theme: ThemeData(brightness: brightness),
      locale: const Locale('en'),
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      home: Scaffold(
        body: SessionFileLinks(
          api: api,
          serviceKey: 'pcx:remote:app:test',
          threadId: 'thread',
          cwd: '/host/project',
          child: Builder(
            builder: (context) => markdown
                ? MarkdownView(data: '[report]($href)')
                : TextButton(
                    onPressed: () => openUrl(context, href),
                    child: const Text('report'),
                  ),
          ),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
  await tester.tap(find.text('report', findRichText: true));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('whitespace cannot bypass remote localhost routing', (
    tester,
  ) async {
    await host(tester, FakeBridgeApi(), ' http://localhost:3000/preview ');
    expect(find.textContaining('pb-mapper'), findsOneWidget);
  });

  testWidgets(
    'saving over a local alias preserves the source file',
    (tester) async {
      final original = fsel.FileSelectorPlatform.instance;
      final picker = SavePicker();
      fsel.FileSelectorPlatform.instance = picker;
      addTearDown(() => fsel.FileSelectorPlatform.instance = original);
      await tester.runAsync(() async {
        final dir = await Directory.systemTemp.createTemp('pcx-alias-test-');
        try {
          final source = await File(
            '${dir.path}/source.txt',
          ).writeAsString('Original content');
          picker.destination = '${dir.path}/alias.txt';
          await Link(picker.destination!).create(source.path);
          expect(
            await exportFile(source.path, 'source.txt'),
            picker.destination,
          );
          expect(await source.readAsString(), 'Original content');
        } finally {
          await dir.delete(recursive: true);
        }
      });
    },
    variant: TargetPlatformVariant.only(TargetPlatform.linux),
    skip: Platform.isWindows,
  );

  for (final brightness in Brightness.values) {
    for (final width in [360.0, 800.0, 1280.0]) {
      testWidgets('file preview fits $width $brightness', (tester) async {
        await tester.binding.setSurfaceSize(Size(width, 700));
        addTearDown(() => tester.binding.setSurfaceSize(null));
        final api = FakeBridgeApi()
          ..fileBytes['/host/report.md'] = Uint8List.fromList(
            utf8.encode('Report content\n' * 100),
          );
        await host(tester, api, '/host/report.md', brightness: brightness);
        await tester.tap(find.text('Preview'));
        await tester.pumpAndSettle();
        expect(find.text('Download'), findsOneWidget);
        expect(tester.takeException(), isNull);
        final rect = tester.getRect(find.byType(AlertDialog));
        expect(rect.left, greaterThanOrEqualTo(0));
        expect(rect.right, lessThanOrEqualTo(width));
      });
    }
  }

  for (final outcome in ['save', 'cancel', 'failure']) {
    testWidgets(
      'remote download cleans staging after $outcome',
      (tester) async {
        final original = fsel.FileSelectorPlatform.instance;
        final picker = SavePicker();
        fsel.FileSelectorPlatform.instance = picker;
        addTearDown(() => fsel.FileSelectorPlatform.instance = original);
        late Directory output;
        await tester.runAsync(() async {
          output = await Directory.systemTemp.createTemp('pcx-saved-test-');
        });
        addTearDown(() => output.delete(recursive: true));
        if (outcome == 'save') picker.destination = '${output.path}/report.md';
        picker.fail = outcome == 'failure';
        final api = DownloadApi();
        await host(tester, api, '/host/report.md');
        await tester.runAsync(() async {
          await tester.tap(find.text('Download'));
          await Future<void>.delayed(const Duration(milliseconds: 100));
        });
        await tester.pumpAndSettle();
        expect(api.destination, isNotNull);
        expect(File(api.destination!).existsSync(), isFalse);
        expect(
          Directory(File(api.destination!).parent.path).existsSync(),
          isFalse,
        );
        if (outcome == 'save') {
          expect(
            File(picker.destination!).readAsStringSync(),
            'Downloaded from pcx:remote:app:test/thread',
          );
        } else if (outcome == 'failure') {
          expect(
            find.textContaining('Destination unavailable'),
            findsOneWidget,
          );
          expect(find.text('Retry'), findsOneWidget);
        }
        expect(tester.takeException(), isNull);
      },
      variant: TargetPlatformVariant.only(TargetPlatform.linux),
    );
  }

  testWidgets(
    'markdown file opens a chooser; preview reads selected host only',
    (tester) async {
      final api = FakeBridgeApi()
        ..fileBytes['report.md'] = Uint8List.fromList(
          utf8.encode('From remote host'),
        );
      await host(tester, api, 'report.md', markdown: true);
      expect(find.text('/host/project/report.md'), findsOneWidget);
      expect(api.filePreviewRequests, isEmpty);
      await tester.tap(find.text('Preview'));
      await tester.pumpAndSettle();
      expect(find.text('From remote host'), findsOneWidget);
      expect(api.filePreviewRequests, ['pcx:remote:app:test/thread/report.md']);
      expect(find.text('Download'), findsOneWidget);
    },
  );

  testWidgets('failed preview stays visible and can explicitly retry', (
    tester,
  ) async {
    final api = FakeBridgeApi();
    await host(tester, api, '/tmp/report.md');
    await tester.tap(find.text('Preview'));
    await tester.pumpAndSettle();
    expect(find.text('Retry'), findsOneWidget);
    api.fileBytes['/tmp/report.md'] = Uint8List.fromList(
      utf8.encode('Restored'),
    );
    await tester.tap(find.text('Retry'));
    await tester.pumpAndSettle();
    expect(find.text('Restored'), findsOneWidget);
    expect(api.filePreviewRequests, hasLength(2));
  });

  testWidgets('closing a pending preview ignores the late response', (
    tester,
  ) async {
    final api = DelayedApi();
    await host(tester, api, '/tmp/report.md');
    await tester.tap(find.text('Preview'));
    await tester.pump();
    expect(find.byType(LinearProgressIndicator), findsOneWidget);
    await tester.tap(find.text('Close'));
    await tester.pumpAndSettle();
    api.result.complete(
      FilePreviewData(bytes: Uint8List.fromList([65]), totalSize: 1),
    );
    await tester.pumpAndSettle();
    expect(find.byType(AlertDialog), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('local file preview uses disk without a host read', (
    tester,
  ) async {
    late Directory dir;
    late File file;
    await tester.runAsync(() async {
      dir = await Directory.systemTemp.createTemp('pcx-local-test-');
      file = await File('${dir.path}/report.md').writeAsString('Local content');
    });
    addTearDown(() => dir.delete(recursive: true));
    final api = FakeBridgeApi()..hostIsLocal = true;
    await host(tester, api, file.path);
    await tester.runAsync(() async {
      await tester.tap(find.text('Preview'));
      await Future<void>.delayed(const Duration(milliseconds: 80));
    });
    await tester.pumpAndSettle();
    expect(find.text('Local content'), findsOneWidget);
    expect(api.filePreviewRequests, isEmpty);
  });

  testWidgets(
    'remote localhost explains mapping without browser or favicon reads',
    (tester) async {
      await tester.binding.setSurfaceSize(const Size(360, 640));
      addTearDown(() => tester.binding.setSurfaceSize(null));
      await host(
        tester,
        FakeBridgeApi(),
        'http://localhost:5173/demo?q=x',
        markdown: true,
      );
      expect(find.textContaining('pb-mapper'), findsOneWidget);
      expect(find.text('http://localhost:5173/demo?q=x'), findsOneWidget);
      expect(find.byType(Image), findsNothing);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets('same-device localhost goes to the system browser', (
    tester,
  ) async {
    final launches = <String>[];
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      const MethodChannel('plugins.flutter.io/url_launcher'),
      (call) async {
        if (call.method == 'launch') {
          launches.add((call.arguments as Map)['url'] as String);
        }
        return true;
      },
    );
    addTearDown(
      () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        const MethodChannel('plugins.flutter.io/url_launcher'),
        null,
      ),
    );
    await host(
      tester,
      FakeBridgeApi()..hostIsLocal = true,
      'http://127.0.0.1:3000/demo',
    );
    expect(launches, ['http://127.0.0.1:3000/demo']);
    expect(find.byType(AlertDialog), findsNothing);
  });

  testWidgets(
    'binary and PDF previews offer download instead of executing content',
    (tester) async {
      final api = FakeBridgeApi()
        ..fileBytes['/tmp/report.pdf'] = Uint8List.fromList(
          utf8.encode('%PDF-1.7'),
        );
      await host(tester, api, '/tmp/report.pdf');
      await tester.tap(find.text('Preview'));
      await tester.pumpAndSettle();
      expect(find.textContaining('cannot be previewed'), findsOneWidget);
      expect(find.text('%PDF-1.7'), findsNothing);
    },
  );
}
