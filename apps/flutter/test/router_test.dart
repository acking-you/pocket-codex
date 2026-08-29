// Tray navigation: opening Settings from outside the widget tree must not leave
// two instances of another utility route on the stack.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/router.dart';

import 'fake_bridge_api.dart';

/// A router over stub pages, so this exercises the tray's navigation decision
/// rather than the real screens' own behaviour. Registered through
/// [buildRouter]'s side effect by pumping it first, then swapped for these
/// routes — `openSettingsFromTray` reads the module-level router either way.
GoRouter _stubRouter(String initial) => GoRouter(
  initialLocation: initial,
  routes: [
    for (final path in ['/', '/logs', '/settings', '/manage'])
      GoRoute(
        path: path,
        builder: (_, _) => Scaffold(body: Text('page:$path')),
      ),
  ],
);

/// Mount [router] and expose it to `openSettingsFromTray` the way the app does.
Future<void> _pump(WidgetTester t, GoRouter router) async {
  final api = FakeBridgeApi(
    config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
  );
  await t.pumpWidget(
    ProviderScope(
      overrides: [bridgeApiProvider.overrideWithValue(api)],
      child: MaterialApp.router(
        locale: const Locale('zh'),
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        routerConfig: router,
      ),
    ),
  );
  await t.pumpAndSettle();
}

/// The route stack's paths, outermost last.
List<String> _paths(GoRouter router) => [
  for (final match in router.routerDelegate.currentConfiguration.matches)
    match.matchedLocation,
];

void main() {
  testWidgets('over the conversation the tray pushes, keeping a back path', (
    t,
  ) async {
    final router = _stubRouter('/');
    debugSetAppRouterForTesting(router);
    addTearDown(debugSetAppRouterForTesting);
    await _pump(t, router);

    openSettingsFromTray();
    await t.pumpAndSettle();

    // Chat underneath, Settings above it — so compact layouts have their
    // conventional back button out of Settings.
    expect(_paths(router), ['/', '/settings']);
  });

  testWidgets('over another utility page the tray replaces it', (t) async {
    // Pushing here stacked Settings on a PEER, and the page menu's next swap
    // would replace only the top one: Logs → tray Settings → Logs left two
    // LogViewScreens alive, each holding its own log-stream subscription.
    final router = _stubRouter('/');
    debugSetAppRouterForTesting(router);
    addTearDown(debugSetAppRouterForTesting);
    await _pump(t, router);
    router.push('/logs');
    await t.pumpAndSettle();
    expect(_paths(router), ['/', '/logs']);

    openSettingsFromTray();
    await t.pumpAndSettle();

    // Settings took the utility slot rather than stacking on Logs, so returning
    // to Logs cannot produce a second one.
    expect(_paths(router), ['/', '/settings']);
  });

  testWidgets('a repeated tray click does not stack Settings', (t) async {
    final router = _stubRouter('/');
    debugSetAppRouterForTesting(router);
    addTearDown(debugSetAppRouterForTesting);
    await _pump(t, router);

    openSettingsFromTray();
    await t.pumpAndSettle();
    openSettingsFromTray();
    await t.pumpAndSettle();

    expect(_paths(router), ['/', '/settings']);
  });
}
