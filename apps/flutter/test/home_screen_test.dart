// Chat-first home: service resolution, latest-conversation restore, fallback
// heroes, auto-retry, service switching, and the sidebar's home-mode extras.

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/screens/home_screen.dart';
import 'package:pocket_codex/src/ui_prefs.dart';

import 'fake_bridge_api.dart';

const _app1 = ServiceEntry(
  device: 'devbox',
  kind: 'app',
  name: 'alpha',
  key: 'pcx:devbox:app:alpha',
);
const _app2 = ServiceEntry(
  device: 'laptop',
  kind: 'app',
  name: 'beta',
  key: 'pcx:laptop:app:beta',
);
const _accountConfig = ConfigInfo(
  relay: null,
  hasKey: false,
  mode: 'account',
  accountLogin: 'octocat',
);

/// Mount [HomeScreen] under a MaterialApp (zh locale, matching the app
/// default) inside an [UncontrolledProviderScope] so tests can seed prefs on
/// the container before the first frame.
Future<ProviderContainer> _pumpHome(
  WidgetTester t,
  FakeBridgeApi api, {
  void Function(ProviderContainer c)? seed,
  List<GoRoute> extraRoutes = const [],
}) async {
  final container = ProviderContainer(
    overrides: [bridgeApiProvider.overrideWithValue(api)],
  );
  addTearDown(container.dispose);
  seed?.call(container);
  final router = GoRouter(
    initialLocation: '/',
    routes: [
      GoRoute(path: '/', builder: (c, s) => const HomeScreen()),
      ...extraRoutes,
    ],
  );
  await t.pumpWidget(
    UncontrolledProviderScope(
      container: container,
      child: MaterialApp.router(
        locale: const Locale('zh'),
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        routerConfig: router,
      ),
    ),
  );
  return container;
}

GoRoute _stub(String path, String label) => GoRoute(
  path: path,
  builder: (c, s) => Scaffold(
    // AppBar so `tester.pageBack()` has a back button to press.
    appBar: AppBar(title: Text(label)),
    body: Text('stub:$label ${s.uri.query}'),
  ),
);

void main() {
  setUp(() {
    AppSessionScreen.debugResetThreadMemory();
    HomeScreen.debugResetAutoHost();
  });

  testWidgets('Home opens straight into the most recent conversation', (
    t,
  ) async {
    final api = FakeBridgeApi(config: _accountConfig, services: [_app1]);
    api.appThreads.addAll(const [
      ThreadMeta(
        id: 't-old',
        preview: 'old topic',
        cwd: r'D:\proj\alpha',
        updatedAt: 1000,
      ),
      ThreadMeta(
        id: 't-new',
        preview: 'newest topic',
        cwd: r'D:\proj\beta',
        updatedAt: 2000,
      ),
    ]);
    await _pumpHome(t, api);
    await t.pumpAndSettle();

    // Connected + resumed the newest thread; the chat composer is on screen.
    expect(api.appIsConnected(_app1.key), isTrue);
    expect(api.lastResumed, 't-new');
    expect(find.byKey(const Key('send-btn')), findsOneWidget);
    // The sidebar (inline at 800px) lists sessions from BOTH projects, each
    // annotated with its project name. 'beta' can only come from the second
    // project's row annotation (the single-service switcher label reads
    // 'devbox · alpha'), so this pins the per-row project tag specifically.
    expect(find.text('old topic'), findsOneWidget);
    expect(find.text('newest topic'), findsOneWidget);
    expect(find.textContaining('beta'), findsWidgets);
  });

  testWidgets('Home restores the conversation the user last had open', (
    t,
  ) async {
    final api = FakeBridgeApi(config: _accountConfig, services: [_app1]);
    api.appThreads.addAll(const [
      ThreadMeta(id: 't-old', preview: 'old', cwd: '', updatedAt: 1000),
      ThreadMeta(id: 't-new', preview: 'new', cwd: '', updatedAt: 2000),
    ]);
    await _pumpHome(
      t,
      api,
      seed: (c) =>
          c.read(uiPrefsProvider.notifier).setLastThread(_app1.key, 't-old'),
    );
    await t.pumpAndSettle();
    expect(api.lastResumed, 't-old');
  });

  testWidgets('Idle desktop hero offers one-tap hosting (account mode)', (
    t,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    try {
      final api = FakeBridgeApi(config: _accountConfig);
      await _pumpHome(t, api);
      await t.pumpAndSettle();
      expect(find.byKey(const Key('home-hero-title')), findsOneWidget);
      expect(find.byKey(const Key('home-start-hosting-btn')), findsOneWidget);
      expect(find.byKey(const Key('home-retry-btn')), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('Idle phone hero points at the desktop, no hosting button', (
    t,
  ) async {
    final api = FakeBridgeApi(config: _accountConfig);
    await _pumpHome(t, api);
    await t.pumpAndSettle();
    expect(find.byKey(const Key('home-hero-title')), findsOneWidget);
    expect(find.byKey(const Key('home-start-hosting-btn')), findsNothing);
    expect(
      find.text('在电脑上打开 PocketCodex 并开启托管（app-server），这里就会自动进入对话。'),
      findsOneWidget,
    );
  });

  testWidgets('Home enters the chat by itself once a host appears', (t) async {
    final api = FakeBridgeApi(config: _accountConfig);
    await _pumpHome(t, api);
    await t.pumpAndSettle();
    expect(find.byKey(const Key('home-hero-title')), findsOneWidget);

    // The desktop host comes online; the periodic re-check finds it. The
    // background re-resolve doesn't animate the hero, so advance fake time
    // explicitly past the bounded prefs/dismissed waits before settling.
    api.services.add(_app1);
    await t.pump(const Duration(seconds: 16));
    await t.pump(const Duration(seconds: 3));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('send-btn')), findsOneWidget);
    expect(api.appIsConnected(_app1.key), isTrue);
  });

  testWidgets('Cold start restores hosting even when a remote host exists', (
    t,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    try {
      // A reachable REMOTE host is discoverable, but this machine held a
      // hosting record — both must come up: hosting restored AND the chat
      // opens (preferring the local host in the ranking).
      final api = FakeBridgeApi(config: _accountConfig, services: [_app2]);
      await _pumpHome(
        t,
        api,
        seed: (c) => c
            .read(uiPrefsProvider.notifier)
            .setAutoHost(const AutoHostPrefs(port: 18080, name: 'default')),
      );
      await t.pumpAndSettle();

      expect(api.lastServePort, 18080);
      expect(find.byKey(const Key('send-btn')), findsOneWidget);
      expect(api.appIsConnected('pcx:local:app:default'), isTrue);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('Switching to an unreachable host keeps the current chat', (
    t,
  ) async {
    final api = FakeBridgeApi(config: _accountConfig, services: [_app1, _app2]);
    api.reachable[_app2.key] = false;
    await _pumpHome(t, api);
    await t.pumpAndSettle();
    expect(api.appIsConnected(_app1.key), isTrue);

    await t.tap(find.byKey(const Key('sidebar-service-switcher')));
    await t.pumpAndSettle();
    await t.tap(find.text('laptop · beta').last);
    await t.pumpAndSettle();

    // Probe failed → snackbar, no teardown: the chat stays on the first host.
    expect(find.text('无法连接该主机，已保持当前主机不变。'), findsOneWidget);
    expect(find.byKey(const Key('send-btn')), findsOneWidget);
    expect(api.appIsConnected(_app2.key), isFalse);
    expect(api.appIsConnected(_app1.key), isTrue);
  });

  testWidgets('Sidebar switcher moves the chat to another host', (t) async {
    final api = FakeBridgeApi(config: _accountConfig, services: [_app1, _app2]);
    final container = await _pumpHome(t, api);
    await t.pumpAndSettle();
    expect(api.appIsConnected(_app1.key), isTrue);

    await t.tap(find.byKey(const Key('sidebar-service-switcher')));
    await t.pumpAndSettle();
    await t.tap(find.text('laptop · beta').last);
    await t.pumpAndSettle();

    expect(api.appIsConnected(_app2.key), isTrue);
    expect(find.byKey(const Key('send-btn')), findsOneWidget);
    expect(
      container.read(uiPrefsProvider).valueOrNull?.lastServiceKey,
      _app2.key,
    );
  });

  testWidgets(
    'Sidebar footer reaches manage / host history / logs / settings',
    (t) async {
      final api = FakeBridgeApi(config: _accountConfig, services: [_app1]);
      await _pumpHome(
        t,
        api,
        extraRoutes: [
          _stub('/manage', 'manage'),
          _stub('/sessions', 'sessions'),
          _stub('/logs', 'logs'),
          _stub('/settings', 'settings'),
        ],
      );
      await t.pumpAndSettle();

      await t.tap(find.byKey(const Key('sidebar-manage-btn')));
      await t.pumpAndSettle();
      expect(find.textContaining('stub:manage'), findsOneWidget);
      // pageBack() looks for the English 'Back' tooltip; tap the button itself
      // (the app runs under the zh locale here).
      await t.tap(find.byType(BackButton));
      await t.pumpAndSettle();

      // The host-history shortcut carries the service key so the browser reads
      // THIS host's sessions over its meta tunnel.
      await t.tap(find.byKey(const Key('sidebar-history-btn')));
      await t.pumpAndSettle();
      expect(
        find.textContaining('svc=pcx%3Adevbox%3Aapp%3Aalpha'),
        findsOneWidget,
      );
    },
  );

  testWidgets('Desktop cold start restores the hosting the user left running', (
    t,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    try {
      final api = FakeBridgeApi(config: _accountConfig);
      await _pumpHome(
        t,
        api,
        seed: (c) => c
            .read(uiPrefsProvider.notifier)
            .setAutoHost(
              const AutoHostPrefs(
                port: 18080,
                name: 'default',
                proxy: 'http://127.0.0.1:11111',
                embedded: true,
              ),
            ),
      );
      await t.pumpAndSettle();

      // Hosting was re-run with the persisted params and the chat opened on
      // the freshly published local service.
      expect(api.lastServePort, 18080);
      expect(api.lastServeEmbedded, isTrue);
      expect(api.lastServeProxy, 'http://127.0.0.1:11111');
      expect(find.byKey(const Key('send-btn')), findsOneWidget);
      expect(api.appIsConnected('pcx:local:app:default'), isTrue);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('Home drawer on a phone: no back tile, footer shortcuts shown', (
    t,
  ) async {
    t.view.physicalSize = const Size(400, 800);
    t.view.devicePixelRatio = 1;
    addTearDown(t.view.reset);
    final api = FakeBridgeApi(config: _accountConfig, services: [_app1]);
    await _pumpHome(t, api);
    await t.pumpAndSettle();

    // Narrow layout: sessions live in the drawer behind the hamburger.
    expect(find.byKey(const Key('sidebar-manage-btn')), findsNothing);
    await t.tap(find.byIcon(Icons.menu));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('drawer-back-to-projects')), findsNothing);
    expect(find.byKey(const Key('sidebar-manage-btn')), findsOneWidget);
    expect(find.byKey(const Key('sidebar-settings-btn')), findsOneWidget);
  });

  testWidgets('Discovery failure shows the retry hero, then recovers', (
    t,
  ) async {
    final api = FakeBridgeApi(config: _accountConfig, services: [_app1]);
    api.discoverError = StateError('relay down');
    await _pumpHome(t, api);
    await t.pumpAndSettle();
    expect(find.byKey(const Key('home-hero-title')), findsOneWidget);

    api.discoverError = null;
    await t.tap(find.byKey(const Key('home-retry-btn')));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('send-btn')), findsOneWidget);
  });
}
