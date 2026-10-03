// ACP hosts next to Codex and OpenCode ones (TRD §4.7.1, §4.7.4): the home
// and services page list and badge them, a desktop cold start restores
// `autoHostAcp`, and a remote device offers ACP agent management.

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
import 'package:pocket_codex/src/screens/services_screen.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/widgets/provider_badge.dart';

import 'fake_bridge_api.dart';
import 'support/screen_harness.dart';

const _acp = ServiceEntry(
  device: 'devbox',
  kind: 'acp',
  name: 'claude',
  key: 'pcx:devbox:acp:claude',
);
const _codex = ServiceEntry(
  device: 'devbox',
  kind: 'app',
  name: 'alpha',
  key: 'pcx:devbox:app:alpha',
);
const _account = ConfigInfo(
  relay: '',
  hasKey: false,
  mode: 'account',
  accountLogin: 'octocat',
);

Finder _acpBadge([String? label]) => find.byWidgetPredicate(
  (w) =>
      w is ProviderBadge &&
      w.provider == 'acp' &&
      (label == null || w.label == label),
);

Future<ProviderContainer> _pumpHome(
  WidgetTester t,
  FakeBridgeApi api, {
  void Function(ProviderContainer c)? seed,
}) async {
  final container = ProviderContainer(
    overrides: [bridgeApiProvider.overrideWithValue(api)],
  );
  addTearDown(container.dispose);
  seed?.call(container);
  final router = GoRouter(
    routes: [GoRoute(path: '/', builder: (c, s) => const HomeScreen())],
  );
  await t.pumpWidget(
    UncontrolledProviderScope(
      container: container,
      child: MaterialApp.router(
        locale: const Locale('en'),
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        routerConfig: router,
      ),
    ),
  );
  return container;
}

void main() {
  setUp(() {
    AppSessionScreen.debugResetThreadMemory();
    HomeScreen.debugResetAutoHost();
  });

  testWidgets('home opens an ACP host', (t) async {
    final api = FakeBridgeApi(config: _account, services: [_acp]);
    await _pumpHome(t, api);
    await t.pumpAndSettle();
    expect(api.appIsConnected(_acp.key), isTrue);
    expect(find.byKey(const Key('send-btn')), findsOneWidget);
  });

  testWidgets('desktop cold start restores every remembered ACP host once', (
    t,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    try {
      final api = FakeBridgeApi(config: _account);
      // `work` is already running: only `oc` is started.
      await api.appServeStartAcp(name: 'work', agentId: 'claude-acp');
      api.acpServeCalls.clear();
      await _pumpHome(
        t,
        api,
        seed: (c) {
          final prefs = c.read(uiPrefsProvider.notifier);
          prefs.setAutoHostAcp(
            const AutoHostAcpPrefs(name: 'work', agentId: 'claude-acp'),
          );
          prefs.setAutoHostAcp(
            const AutoHostAcpPrefs(name: 'oc', agentId: 'opencode-acp'),
          );
        },
      );
      await t.pumpAndSettle();
      expect(api.acpServeCalls, [('oc', 'opencode-acp')]);
      expect(api.serveHosts.where((h) => h.isAcp), hasLength(2));
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  test('autoHostAcp round-trips and ignores entries without an agent', () {
    final prefs = UiPrefs.fromJson({
      'autoHostAcp': [
        {'name': 'work', 'agentId': 'claude-acp'},
        {'name': 'broken'},
        'garbage',
      ],
    });
    expect(prefs.autoHostAcp.map((p) => (p.name, p.agentId)), [
      ('work', 'claude-acp'),
    ]);
    expect(UiPrefs.fromJson(prefs.toJson()).autoHostAcp.single.name, 'work');
    expect(const UiPrefs().toJson().containsKey('autoHostAcp'), isFalse);
    final merged = mergeAutoHostAcp(
      const [
        AutoHostAcpPrefs(name: 'a', agentId: 'x'),
        AutoHostAcpPrefs(name: 'b', agentId: 'y'),
      ],
      const [
        AutoHostAcpPrefs(name: 'b', agentId: 'z'),
        AutoHostAcpPrefs(name: 'c', agentId: 'w'),
      ],
    );
    expect(merged.map((p) => '${p.name}:${p.agentId}'), ['a:x', 'b:z', 'c:w']);
  });

  testWidgets('services lists a local ACP host with its agent, no API row', (
    t,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    try {
      final api = FakeBridgeApi(config: _account, services: const []);
      await api.appServeStartAcp(name: 'work', agentId: 'claude-acp');
      await t.pumpWidget(host(const ServicesScreen(), api));
      await t.pumpAndSettle();
      await openDevice(t);
      final row = find.byKey(const Key('device-capability-pcx:local:acp:work'));
      expect(row, findsOneWidget);
      expect(
        find.descendant(of: row, matching: _acpBadge('Claude Code')),
        findsOneWidget,
      );
      expect(
        find.descendant(of: row, matching: find.textContaining('ACP')),
        findsWidgets,
      );
      expect(
        find.byWidgetPredicate(
          (w) =>
              w.key is ValueKey<String> &&
              (w.key! as ValueKey<String>).value.startsWith(
                'device-capability-pcx:local:api:',
              ),
        ),
        findsNothing,
      );
      final card = find.byKey(const Key('local-host-work'));
      expect(
        find.descendant(of: card, matching: _acpBadge('Claude Code')),
        findsOneWidget,
      );
      // An ACP host is probed through appProbe, not the Codex handshake.
      expect(api.appProbeCalls, contains('pcx:local:acp:work'));
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('a remote ACP row takes its badge label from capabilities', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'relay:7666', hasKey: true),
      services: const [_codex, _acp],
    );
    await t.pumpWidget(host(const ServicesScreen(), api));
    await t.pumpAndSettle();
    await openDevice(t);
    final row = find.byKey(Key('device-capability-${_acp.key}'));
    expect(
      find.descendant(of: row, matching: _acpBadge('Claude Code')),
      findsOneWidget,
    );
    expect(api.appProbeCalls, contains(_acp.key));
  });

  testWidgets('a remote device offers ACP agent management', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'relay:7666', hasKey: true),
      services: const [_codex, _acp],
    );
    await t.pumpWidget(
      routerHost(
        api,
        initial: '/manage',
        routes: [
          GoRoute(path: '/manage', builder: (_, _) => const ServicesScreen()),
          GoRoute(
            path: '/settings/acp',
            builder: (_, s) =>
                Scaffold(body: Text('acp:${s.uri.queryParameters['svc']}')),
          ),
        ],
      ),
    );
    await t.pumpAndSettle();
    await openDevice(t);
    await t.tap(find.byKey(const Key('host-menu')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('host-acp-manage')));
    await t.pumpAndSettle();
    expect(find.textContaining('acp:pcx:devbox:'), findsOneWidget);
  });
}
