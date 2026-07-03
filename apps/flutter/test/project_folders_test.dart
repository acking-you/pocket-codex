// Project-folder feature: the host directory-tree picker (mobile) and the
// desktop project-folders editor.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/widgets/folder_tree_picker.dart';
import 'package:pocket_codex/src/widgets/project_folders_editor.dart';

import 'fake_bridge_api.dart';

const _svc = 'pcx:devbox:app:alpha';

Widget _host(Widget child, FakeBridgeApi api) => ProviderScope(
  overrides: [bridgeApiProvider.overrideWithValue(api)],
  child: MaterialApp(
    locale: const Locale('zh'),
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: child,
  ),
);

void main() {
  group('folder tree picker', () {
    testWidgets('single root auto-enters and lists its sub-folders', (t) async {
      final api = FakeBridgeApi();
      api.projectConfigs[_svc] = const ProjectConfig(
        projectRoots: [r'D:\proj'],
      );
      api.dirTree[r'D:\proj'] = const [
        HostDirEntry(name: 'app', path: r'D:\proj\app', isGitRepo: true),
        HostDirEntry(name: 'docs', path: r'D:\proj\docs'),
      ];
      String? picked;
      await t.pumpWidget(
        _host(
          Builder(
            builder: (c) => Scaffold(
              body: Center(
                child: ElevatedButton(
                  onPressed: () async {
                    picked = await showFolderPicker(c, serviceKey: _svc);
                  },
                  child: const Text('open'),
                ),
              ),
            ),
          ),
          api,
        ),
      );
      await t.tap(find.text('open'));
      await t.pumpAndSettle();

      // Auto-entered the single root; both sub-folders listed, git badge shown.
      expect(find.byKey(const Key(r'folder-row-D:\proj\app')), findsOneWidget);
      expect(find.byKey(const Key(r'folder-row-D:\proj\docs')), findsOneWidget);
      expect(find.text('Git 仓库'), findsOneWidget);

      // Drill into "app", then use it.
      await t.tap(find.byKey(const Key(r'folder-row-D:\proj\app')));
      await t.pumpAndSettle();
      await t.tap(find.byKey(const Key('folder-use-btn')));
      await t.pumpAndSettle();
      expect(picked, r'D:\proj\app');
    });

    testWidgets('opens at initialPath when it is inside a root', (t) async {
      final api = FakeBridgeApi();
      api.projectConfigs[_svc] = const ProjectConfig(
        projectRoots: [r'D:\proj'],
      );
      api.dirTree[r'D:\proj\app\lib'] = const [
        HostDirEntry(name: 'src', path: r'D:\proj\app\lib\src'),
      ];
      await t.pumpWidget(
        _host(
          Builder(
            builder: (c) => Scaffold(
              body: Center(
                child: ElevatedButton(
                  onPressed: () => showFolderPicker(
                    c,
                    serviceKey: _svc,
                    initialPath: r'D:\proj\app\lib',
                  ),
                  child: const Text('open'),
                ),
              ),
            ),
          ),
          api,
        ),
      );
      await t.tap(find.text('open'));
      await t.pumpAndSettle();

      // Opened straight at the initial folder: its child is listed, "use this
      // folder" is enabled, and the up button walks back out.
      expect(
        find.byKey(const Key(r'folder-row-D:\proj\app\lib\src')),
        findsOneWidget,
      );
      final useBtn = t.widget<FilledButton>(
        find.byKey(const Key('folder-use-btn')),
      );
      expect(useBtn.onPressed, isNotNull);
      expect(find.byKey(const Key('folder-up-btn')), findsOneWidget);
    });

    testWidgets('no configured roots shows the guidance message', (t) async {
      final api = FakeBridgeApi();
      api.projectConfigs[_svc] = const ProjectConfig();
      await t.pumpWidget(
        _host(
          Builder(
            builder: (c) => Scaffold(
              body: Center(
                child: ElevatedButton(
                  onPressed: () => showFolderPicker(c, serviceKey: _svc),
                  child: const Text('open'),
                ),
              ),
            ),
          ),
          api,
        ),
      );
      await t.tap(find.text('open'));
      await t.pumpAndSettle();
      expect(find.byKey(const Key('folder-picker-no-roots')), findsOneWidget);
    });

    testWidgets('multiple roots list at the top, then drill in', (t) async {
      final api = FakeBridgeApi();
      api.projectConfigs[_svc] = const ProjectConfig(
        projectRoots: [r'D:\a', r'D:\b'],
      );
      api.dirTree[r'D:\a'] = const [
        HostDirEntry(name: 'sub', path: r'D:\a\sub'),
      ];
      await t.pumpWidget(
        _host(
          Builder(
            builder: (c) => Scaffold(
              body: Center(
                child: ElevatedButton(
                  onPressed: () => showFolderPicker(c, serviceKey: _svc),
                  child: const Text('open'),
                ),
              ),
            ),
          ),
          api,
        ),
      );
      await t.tap(find.text('open'));
      await t.pumpAndSettle();
      // Both roots shown; "use this folder" disabled at the roots list.
      expect(find.byKey(const Key(r'folder-row-D:\a')), findsOneWidget);
      expect(find.byKey(const Key(r'folder-row-D:\b')), findsOneWidget);
      final useBtn = t.widget<FilledButton>(
        find.byKey(const Key('folder-use-btn')),
      );
      expect(useBtn.onPressed, isNull);

      await t.tap(find.byKey(const Key(r'folder-row-D:\a')));
      await t.pumpAndSettle();
      expect(find.byKey(const Key(r'folder-row-D:\a\sub')), findsOneWidget);
    });
  });

  group('project folders editor', () {
    testWidgets('lists roots, marks the default, and removes one', (t) async {
      final api = FakeBridgeApi();
      api.projectConfigs[_svc] = const ProjectConfig(
        projectRoots: [r'D:\work', r'D:\play'],
        defaultProject: r'D:\work',
      );
      await t.pumpWidget(
        _host(
          const Scaffold(body: ProjectFoldersEditor(serviceKey: _svc)),
          api,
        ),
      );
      await t.pumpAndSettle();

      expect(find.byKey(const Key(r'project-root-D:\work')), findsOneWidget);
      expect(find.byKey(const Key(r'project-root-D:\play')), findsOneWidget);

      // Promote play to default, then verify it persisted on the fake host.
      await t.tap(find.byKey(const Key(r'default-project-D:\play')));
      await t.pumpAndSettle();
      expect(api.projectConfigs[_svc]?.defaultProject, r'D:\play');

      // Remove work; the fake host reflects a single remaining root.
      await t.tap(find.byKey(const Key(r'remove-project-D:\work')));
      await t.pumpAndSettle();
      expect(api.projectConfigs[_svc]?.projectRoots, [r'D:\play']);
      expect(find.byKey(const Key(r'project-root-D:\work')), findsNothing);
    });

    testWidgets('empty state shows the no-folders hint', (t) async {
      final api = FakeBridgeApi();
      api.projectConfigs[_svc] = const ProjectConfig();
      await t.pumpWidget(
        _host(
          const Scaffold(body: ProjectFoldersEditor(serviceKey: _svc)),
          api,
        ),
      );
      await t.pumpAndSettle();
      expect(find.text('还没有项目文件夹。'), findsOneWidget);
    });
  });
}
