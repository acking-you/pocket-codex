// OpenCode hosts next to Codex ones: the home picks and badges them, restores
// OpenCode hosting on a desktop cold start, and the services page lists them.

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

const _codex = ServiceEntry(
  device: 'devbox',
  kind: 'app',
  name: 'alpha',
  key: 'pcx:devbox:app:alpha',
);
const _openCode = ServiceEntry(
  device: 'devbox',
  kind: 'opencode',
  name: 'oc',
  key: 'pcx:devbox:opencode:oc',
);
const _account = ConfigInfo(
  relay: '',
  hasKey: false,
  mode: 'account',
  accountLogin: 'octocat',
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
        locale: const Locale('zh'),
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        routerConfig: router,
      ),
    ),
  );
  return container;
}

Finder _badge(String provider) =>
    find.byWidgetPredicate((w) => w is ProviderBadge && w.provider == provider);

void main() {
  setUp(() {
    AppSessionScreen.debugResetThreadMemory();
    HomeScreen.debugResetAutoHost();
  });

  testWidgets('home opens an OpenCode host and badges it', (t) async {
    final api = FakeBridgeApi(config: _account, services: [_openCode]);
    await _pumpHome(t, api);
    await t.pumpAndSettle();
    expect(api.appIsConnected(_openCode.key), isTrue);
    expect(find.byKey(const Key('send-btn')), findsOneWidget);
    expect(_badge('opencode'), findsWidgets);
  });

  testWidgets('home ranks both providers and badges each in the switcher', (
    t,
  ) async {
    final api = FakeBridgeApi(config: _account, services: [_codex, _openCode]);
    await _pumpHome(
      t,
      api,
      seed: (c) =>
          c.read(uiPrefsProvider.notifier).setLastService(_openCode.key),
    );
    await t.pumpAndSettle();
    // Last used wins across providers.
    expect(api.appIsConnected(_openCode.key), isTrue);
    await t.tap(find.byKey(const Key('sidebar-service-switcher')));
    await t.pumpAndSettle();
    expect(_badge('codex'), findsWidgets);
    expect(_badge('opencode'), findsWidgets);
  });

  testWidgets('desktop cold start restores Codex and OpenCode hosting once', (
    t,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    try {
      final api = FakeBridgeApi(config: _account);
      await _pumpHome(
        t,
        api,
        seed: (c) {
          final prefs = c.read(uiPrefsProvider.notifier);
          prefs.setAutoHost(const AutoHostPrefs(port: 18080, name: 'default'));
          prefs.setAutoHostOpenCode(
            const AutoHostOpenCodePrefs(name: 'oc', binaryOverride: '/x/oc'),
          );
        },
      );
      await t.pumpAndSettle();
      expect(api.lastServePort, 18080);
      expect(api.openCodeServeCalls, [('oc', '/x/oc')]);
      expect(api.serveHosts.map((h) => h.provider).toSet(), {
        'codex',
        'opencode',
      });
      expect(find.byKey(const Key('send-btn')), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('a running OpenCode host is not restarted at boot', (t) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    try {
      final api = FakeBridgeApi(config: _account);
      await api.appServeStartOpencode(name: 'oc');
      api.openCodeServeCalls.clear();
      await _pumpHome(
        t,
        api,
        seed: (c) => c
            .read(uiPrefsProvider.notifier)
            .setAutoHostOpenCode(const AutoHostOpenCodePrefs(name: 'oc')),
      );
      await t.pumpAndSettle();
      expect(api.openCodeServeCalls, isEmpty);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('services lists a remote OpenCode host, probed via appProbe', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'relay:7666', hasKey: true),
      services: const [_codex, _openCode],
    )..reachable[_openCode.key] = false;
    await t.pumpWidget(host(const ServicesScreen(), api));
    await t.pumpAndSettle();
    await openDevice(t);
    final row = find.byKey(Key('device-capability-${_openCode.key}'));
    expect(row, findsOneWidget);
    expect(
      find.descendant(of: row, matching: _badge('opencode')),
      findsOneWidget,
    );
    expect(
      find.descendant(
        of: find.byKey(Key('device-capability-${_codex.key}')),
        matching: _badge('codex'),
      ),
      findsOneWidget,
    );
    expect(api.appProbeCalls, contains(_openCode.key));
    expect(api.apiProbeCalls, isNot(contains(_openCode.key)));
    // Unreachable through appProbe reads as such on the OpenCode row.
    expect(find.descendant(of: row, matching: find.text('不可达')), findsWidgets);
  });

  testWidgets('a local OpenCode host has a badge and no API row', (t) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    try {
      final api = FakeBridgeApi(config: _account, services: const []);
      await api.appServeStartOpencode(name: 'oc');
      await t.pumpWidget(host(const ServicesScreen(), api));
      await t.pumpAndSettle();
      await openDevice(t);
      expect(
        find.byKey(const Key('device-capability-pcx:local:opencode:oc')),
        findsOneWidget,
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
      final card = find.byKey(const Key('local-host-oc'));
      expect(card, findsOneWidget);
      expect(
        find.descendant(of: card, matching: _badge('opencode')),
        findsOneWidget,
      );
      // Health comes from appProbe (dispatching), not the Codex handshake.
      expect(api.appProbeCalls, contains('pcx:local:opencode:oc'));
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });
}
