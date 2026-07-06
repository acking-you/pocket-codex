import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/widgets/file_browser_panel.dart';

import 'fake_bridge_api.dart';

Widget _app(FakeBridgeApi fake) => ProviderScope(
  overrides: [bridgeApiProvider.overrideWithValue(fake)],
  child: MaterialApp(
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: Builder(
      builder: (context) => Scaffold(
        body: Center(
          child: ElevatedButton(
            onPressed: () =>
                showFileBrowser(context, serviceKey: 'pcx:d:app:x'),
            child: const Text('open'),
          ),
        ),
      ),
    ),
  ),
);

void main() {
  testWidgets('lists a chosen dir\'s sub-folders + files with a download', (
    tester,
  ) async {
    final fake = FakeBridgeApi();
    fake.projectConfigs['pcx:d:app:x'] = const ProjectConfig(
      projectRoots: ['/root'],
    );
    fake.dirTree['/root'] = const [
      HostDirEntry(name: 'sub', path: '/root/sub'),
    ];
    fake.fileTree['/root'] = const [
      HostFileEntry(
        name: 'report.pdf',
        path: '/root/report.pdf',
        size: 2048,
        mtime: 0,
      ),
    ];

    await tester.pumpWidget(_app(fake));
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();

    // A single root drops straight in: the sub-folder and the file both show.
    expect(find.text('sub'), findsOneWidget);
    expect(find.text('report.pdf'), findsOneWidget);
    expect(find.text('2.0 KB'), findsOneWidget);
    // The file carries a per-file download button.
    expect(
      find.byKey(const Key('file-download-/root/report.pdf')),
      findsOneWidget,
    );
    // Upload targets the current folder, so it is enabled.
    final upload = tester.widget<FilledButton>(
      find.byKey(const Key('file-upload-btn')),
    );
    expect(upload.onPressed, isNotNull);
  });

  testWidgets('empty folder shows the empty state', (tester) async {
    final fake = FakeBridgeApi();
    fake.projectConfigs['pcx:d:app:x'] = const ProjectConfig(
      projectRoots: ['/root'],
    );
    fake.dirTree['/root'] = const [];
    fake.fileTree['/root'] = const [];

    await tester.pumpWidget(_app(fake));
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();

    // Default test locale is en.
    expect(find.text('This folder is empty'), findsOneWidget);
  });
}
