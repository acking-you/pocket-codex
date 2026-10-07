/// Renders the main screens to PNGs with the real fonts, for design review.
///
// ignore_for_file: invalid_use_of_visible_for_testing_member

/// Not part of the test suite: it asserts nothing and lives outside `test/`.
/// Run it from `apps/flutter`:
///
///   fvm flutter test tool/screenshots/ui_shots_test.dart --update-goldens
///
/// The images land in `tool/screenshots/out/` (git-ignored). Each scene is
/// rendered as macOS desktop, Windows desktop and a phone, light and dark.
library;

import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/screens/services_screen.dart';
import 'package:pocket_codex/src/screens/settings_screen.dart';
import 'package:pocket_codex/src/theme.dart';

import '../../test/fake_bridge_api.dart';

const _service = 'pcx:lb7666:app:default';

Future<void> _loadFont(String family, List<String> paths) async {
  final loader = FontLoader(family);
  for (final p in paths) {
    loader.addFont(
      Future.value(ByteData.sublistView(File(p).readAsBytesSync())),
    );
  }
  await loader.load();
}

Future<void> _loadFonts() async {
  const fonts = 'assets/fonts';
  await _loadFont('Figtree', [
    for (final w in ['Light', 'Regular', 'Medium', 'SemiBold', 'Bold'])
      '$fonts/Figtree-$w.ttf',
  ]);
  await _loadFont('GeistMono', [
    for (final w in ['Regular', 'Medium', 'SemiBold'])
      '$fonts/GeistMono-$w.ttf',
  ]);
  await _loadFont('Noto Sans SC', ['$fonts/NotoSansSC-VariableFont_wght.ttf']);
  // The engine resolves the Apple system face only on a real Mac. Stand in
  // for SF Pro / PingFang with an installed SF-like face when there is one,
  // else the bundled faces, so mac shots show metrics rather than tofu.
  final sfLike = [
    'C:/Windows/Fonts/segoeui.ttf',
    'C:/Windows/Fonts/segoeuib.ttf',
    'C:/Windows/Fonts/seguisb.ttf',
  ].where((p) => File(p).existsSync()).toList();
  await _loadFont(
    'CupertinoSystemText',
    sfLike.isEmpty
        ? [
            for (final w in ['Regular', 'SemiBold']) '$fonts/Figtree-$w.ttf',
          ]
        : sfLike,
  );
  await _loadFont('PingFang SC', ['$fonts/NotoSansSC-VariableFont_wght.ttf']);
  // The icon font ships with the SDK, not the app.
  final sdk =
      Platform.environment['FLUTTER_ROOT'] ??
      File(Platform.resolvedExecutable).parent.parent.parent.parent.parent.path;
  await _loadFont('MaterialIcons', [
    '$sdk/bin/cache/artifacts/material_fonts/materialicons-regular.otf',
  ]);
}

FakeBridgeApi _api() {
  final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
  final api = FakeBridgeApi(
    config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
  );
  api.appThreads.addAll([
    ThreadMeta(
      id: 't1',
      preview: '把首页的加载状态改成骨架屏',
      cwd: '/Users/me/work/pocket-codex',
      updatedAt: now - 120,
    ),
    ThreadMeta(
      id: 't2',
      preview: 'Fix the flaky relay reconnect test',
      cwd: '/Users/me/work/pocket-codex',
      updatedAt: now - 3600,
    ),
    ThreadMeta(
      id: 't3',
      preview: '整理 CHANGELOG 并准备 0.2.9',
      cwd: '/Users/me/work/pocket-codex',
      updatedAt: now - 86400 * 2,
    ),
    ThreadMeta(
      id: 't4',
      preview: 'Add dark mode to the landing page',
      cwd: '/Users/me/work/website',
      updatedAt: now - 7200,
    ),
    ThreadMeta(
      id: 't5',
      preview: 'Investigate memory spike in the indexer',
      cwd: '/Users/me/work/indexer',
      updatedAt: now - 86400 * 6,
    ),
  ]);
  api.readResult = const ThreadHistory(
    branch: 'feat/skeleton-loading',
    cwd: '/Users/me/work/pocket-codex',
    tokensUsed: 48000,
    contextWindow: 258000,
    model: 'gpt-5.5',
    items: [
      ThreadItem(
        id: 'u1',
        itemType: 'userMessage',
        title: '',
        text: '把首页的加载状态改成骨架屏，不要再用转圈的 spinner。',
        turnId: 'turn1',
      ),
      ThreadItem(
        id: 'r1',
        itemType: 'reasoning',
        title: '',
        text: '**Locating the home loading state**',
        turnId: 'turn1',
      ),
      ThreadItem(
        id: 'c1',
        itemType: 'commandExecution',
        title: 'rg -n "CircularProgressIndicator" lib/src/screens',
        text: 'lib/src/screens/home_screen.dart:603',
        turnId: 'turn1',
      ),
      ThreadItem(
        id: 'a1',
        itemType: 'agentMessage',
        title: '',
        text:
            '已经把首页的加载态换成了骨架屏：\n\n'
            '- `HomeScreen` 的 splash 改用 `ListLoadingSkeleton`\n'
            '- 保留了 250 ms 的延迟显示，缓存命中时不会闪一下\n\n'
            '```dart\n'
            'return const ListLoadingSkeleton(rows: 4);\n'
            '```\n\n'
            '要不要顺便把会话列表的加载态也统一？',
        turnId: 'turn1',
        turnCompletedAt: 1,
        turnDurationMs: 48000,
      ),
    ],
    running: false,
  );
  return api;
}

Widget _app(Widget child, BridgeApi api, Brightness brightness) =>
    ProviderScope(
      overrides: [bridgeApiProvider.overrideWithValue(api)],
      child: MaterialApp(
        debugShowCheckedModeBanner: false,
        locale: const Locale('zh'),
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        theme: lightTheme(),
        darkTheme: darkTheme(),
        themeMode: brightness == Brightness.dark
            ? ThemeMode.dark
            : ThemeMode.light,
        home: child,
      ),
    );

typedef _Device = ({String name, TargetPlatform platform, Size size});

const List<_Device> _devices = [
  (name: 'mac', platform: TargetPlatform.macOS, size: Size(1280, 800)),
  (name: 'win', platform: TargetPlatform.windows, size: Size(1280, 800)),
  (name: 'phone', platform: TargetPlatform.iOS, size: Size(393, 852)),
];

/// Screens to capture, by file prefix.
final Map<String, Widget> _scenes = {
  'session': const AppSessionScreen(
    serviceKey: _service,
    threadId: 't1',
    home: true,
  ),
  'new': const AppSessionScreen(serviceKey: _service, home: true),
  'settings': const SettingsScreen(),
  'services': const ServicesScreen(),
};

Future<void> _shoot(
  WidgetTester t,
  _Device d,
  Brightness b,
  String name,
  Widget scene,
) async {
  debugDefaultTargetPlatformOverride = d.platform;
  try {
    AppSessionScreen.debugResetThreadMemory();
    t.view.devicePixelRatio = 2.0;
    t.view.physicalSize = d.size * 2.0;
    addTearDown(t.view.reset);
    final api = _api();
    await api.appConnect(_service, 28080);
    await t.pumpWidget(_app(scene, api, b));
    await t.pumpAndSettle();
    await expectLater(
      find.byType(MaterialApp),
      matchesGoldenFile('out/$name-${d.name}-${b.name}.png'),
    );
    // Unmount while the override is still in effect, so platform-dependent
    // dispose paths see the same platform their build did.
    await t.pumpWidget(const SizedBox());
  } finally {
    debugDefaultTargetPlatformOverride = null;
  }
}

void main() {
  setUpAll(_loadFonts);
  drawerShots();

  for (final MapEntry(key: name, value: scene) in _scenes.entries) {
    for (final d in _devices) {
      for (final b in Brightness.values) {
        testWidgets(
          '$name ${d.name}-${b.name}',
          (t) => _shoot(t, d, b, name, scene),
        );
      }
    }
  }
}

/// The phone's conversation drawer, opened over the session.
void drawerShots() {
  for (final b in Brightness.values) {
    testWidgets('drawer phone-${b.name}', (t) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.iOS;
      try {
        AppSessionScreen.debugResetThreadMemory();
        t.view.devicePixelRatio = 2.0;
        t.view.physicalSize = _devices.last.size * 2.0;
        addTearDown(t.view.reset);
        final api = _api();
        await api.appConnect(_service, 28080);
        await t.pumpWidget(_app(_scenes['session']!, api, b));
        await t.pumpAndSettle();
        final state = t.state<ScaffoldState>(find.byType(Scaffold).first);
        state.openDrawer();
        await t.pumpAndSettle();
        await expectLater(
          find.byType(MaterialApp),
          matchesGoldenFile('out/drawer-phone-${b.name}.png'),
        );
        await t.pumpWidget(const SizedBox());
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    });
  }
}
