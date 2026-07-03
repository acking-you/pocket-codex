// First-run welcome guide: desktop one-click hosting setup, mobile
// point-to-PC steps with live host detection, and the seen-once marking.

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/welcome_guide_screen.dart';
import 'package:pocket_codex/src/ui_prefs.dart';

import 'fake_bridge_api.dart';

const _accountConfig = ConfigInfo(
  relay: null,
  hasKey: false,
  mode: 'account',
  accountLogin: 'octocat',
);

/// Mount the guide under a GoRouter (it navigates to '/') with a warmed
/// provider container.
Future<ProviderContainer> _pump(WidgetTester t, FakeBridgeApi api) async {
  final container = ProviderContainer(
    overrides: [bridgeApiProvider.overrideWithValue(api)],
  );
  addTearDown(container.dispose);
  final router = GoRouter(
    initialLocation: '/welcome',
    routes: [
      GoRoute(path: '/welcome', builder: (c, s) => const WelcomeGuideScreen()),
      GoRoute(
        path: '/',
        builder: (c, s) => const Scaffold(body: Text('HOME-ROUTE')),
      ),
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

/// Bounded settle: the mobile guide's waiting spinner animates forever, so
/// pumpAndSettle would never return while it is on screen.
Future<void> _settle(WidgetTester t, [int frames = 10]) async {
  for (var i = 0; i < frames; i++) {
    await t.pump(const Duration(milliseconds: 50));
  }
}

void main() {
  testWidgets('desktop guide: one-click hosting unlocks project folders', (
    t,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    // Tall surface so the whole guide (both step cards + footer) is hittable
    // without scrolling once the folder editor unlocks.
    t.view.physicalSize = const Size(900, 1200);
    t.view.devicePixelRatio = 1;
    addTearDown(t.view.reset);
    try {
      final api = FakeBridgeApi(config: _accountConfig);
      final container = await _pump(t, api);
      await t.pumpAndSettle();

      // The focused guide is on screen, marked as seen for this device.
      expect(find.byKey(const Key('welcome-title')), findsOneWidget);
      expect(container.read(uiPrefsProvider).valueOrNull?.guideSeen, isTrue);
      // Step 2 is locked until hosting is up.
      expect(find.byKey(const Key('welcome-folders-locked')), findsOneWidget);

      // One-click: the guide opens the prefilled hosting dialog…
      await t.tap(find.byKey(const Key('welcome-start-hosting-btn')));
      await t.pumpAndSettle();
      expect(find.byKey(const Key('start-hosting-btn')), findsOneWidget);
      // …and a single confirm starts the host with the defaults.
      await t.tap(find.byKey(const Key('start-hosting-btn')));
      await t.pumpAndSettle();
      expect(api.lastServePort, 18080);

      // Step 1 flips to "hosting is up"; step 2 unlocks the folder editor.
      expect(find.text('托管已开启'), findsOneWidget);
      expect(find.byKey(const Key('welcome-folders-locked')), findsNothing);
      expect(find.byKey(const Key('add-project-folder-btn')), findsOneWidget);

      // Enter the chat.
      await t.tap(find.byKey(const Key('welcome-enter-btn')));
      await t.pumpAndSettle();
      expect(find.text('HOME-ROUTE'), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('mobile guide: steps shown, live detect flips when a host '
      'appears', (t) async {
    // Phone-ish tall surface so the footer buttons are hittable directly.
    t.view.physicalSize = const Size(420, 1000);
    t.view.devicePixelRatio = 1;
    addTearDown(t.view.reset);
    final api = FakeBridgeApi(config: _accountConfig);
    await _pump(t, api);
    await _settle(t); // bounded: the waiting spinner animates forever

    // The three point-to-PC steps + the waiting indicator; no hosting button
    // on a phone.
    expect(find.text('在电脑上安装并打开 Pocket-Codex'), findsOneWidget);
    expect(find.text('登录同一个 GitHub 账号'), findsOneWidget);
    expect(find.byKey(const Key('welcome-start-hosting-btn')), findsNothing);
    expect(find.byKey(const Key('welcome-waiting-host')), findsOneWidget);

    // The desktop comes online → the periodic re-discovery finds it.
    api.services.add(
      const ServiceEntry(
        device: 'devbox',
        kind: 'app',
        name: 'alpha',
        key: 'pcx:devbox:app:alpha',
      ),
    );
    await t.pump(const Duration(seconds: 6));
    await _settle(t);
    expect(find.textContaining('已发现主机'), findsOneWidget);
    expect(find.byKey(const Key('welcome-waiting-host')), findsNothing);

    // Skip also lands in the chat.
    await t.tap(find.byKey(const Key('welcome-skip-btn')));
    await t.pumpAndSettle();
    expect(find.text('HOME-ROUTE'), findsOneWidget);
  });
}
