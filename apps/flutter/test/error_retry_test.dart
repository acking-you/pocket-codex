// The shared failure surface: one card for "it broke, try again", and the one
// place an automatic retry becomes visible instead of looking like a freeze.
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/widgets/error_retry.dart';

import 'fake_bridge_api.dart';

Widget _host(BridgeApi api, Widget child) => ProviderScope(
  overrides: [bridgeApiProvider.overrideWithValue(api)],
  child: MaterialApp(
    locale: const Locale('zh'),
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: Scaffold(body: child),
  ),
);

void main() {
  testWidgets('shows the failure and calls back on retry', (t) async {
    final api = FakeBridgeApi();
    var taps = 0;
    await t.pumpWidget(
      _host(
        api,
        ErrorRetry(
          message: 'meta GET failed',
          errorKey: const Key('probe-error'),
          onRetry: () => taps++,
        ),
      ),
    );
    await t.pumpAndSettle();

    expect(find.byKey(const Key('probe-error')), findsOneWidget);
    expect(find.text('meta GET failed'), findsOneWidget);
    // Nothing is retrying yet, so no progress line.
    expect(find.byKey(const Key('retry-progress')), findsNothing);

    await t.tap(find.text('重试')); // retry (zh)
    await t.pumpAndSettle();
    expect(taps, 1);
  });

  testWidgets('surfaces an automatic retry with its attempt count', (t) async {
    final api = FakeBridgeApi();
    await t.pumpWidget(_host(api, ErrorRetry(message: 'boom', onRetry: () {})));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('retry-progress')), findsNothing);

    // The bridge reports a transient failure it is about to re-attempt. The
    // whole point of the requirement: the user must see this, not a frozen card.
    api.pushRetry(2);
    // pump, not pumpAndSettle: the progress spinner animates forever, so
    // "settled" never arrives.
    await t.pump();
    await t.pump();
    expect(find.byKey(const Key('retry-progress')), findsOneWidget);
    expect(find.textContaining('2/10'), findsOneWidget);

    // Manual retry stays reachable — a user who won't wait out the backoff
    // shouldn't have to.
    expect(find.text('重试'), findsOneWidget);

    // A later attempt supersedes the count.
    api.pushRetry(7);
    await t.pump();
    await t.pump();
    expect(find.textContaining('7/10'), findsOneWidget);
  });

  testWidgets('the retry line clears instead of sticking forever', (t) async {
    final api = FakeBridgeApi();
    await t.pumpWidget(_host(api, ErrorRetry(message: 'boom', onRetry: () {})));
    await t.pumpAndSettle();
    api.pushRetry(1);
    await t.pump();
    await t.pump();
    expect(find.byKey(const Key('retry-progress')), findsOneWidget);

    // The bridge only reports FAILED attempts — a success arrives as the
    // request's own result — so the line has to time out on its own or it would
    // claim "retrying" forever after the request resolved.
    await t.pump(const Duration(seconds: 5));
    await t.pump();
    expect(find.byKey(const Key('retry-progress')), findsNothing);
  });

  testWidgets('a busy retry swaps the button for a spinner', (t) async {
    final api = FakeBridgeApi();
    await t.pumpWidget(
      _host(api, ErrorRetry(message: 'boom', onRetry: () {}, busy: true)),
    );
    await t.pump();
    // Tapping again while the first attempt runs would just queue duplicate
    // work, so the affordance is replaced rather than merely disabled.
    expect(find.text('重试'), findsNothing);
    expect(find.byType(CircularProgressIndicator), findsOneWidget);
  });
}
