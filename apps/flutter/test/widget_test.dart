// Smoke test: the root app builds at the services route with a fake bridge,
// so no native library is needed in pure-Dart unit tests.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/main.dart';
import 'package:pocket_codex/src/providers.dart';

import 'fake_bridge_api.dart';

void main() {
  testWidgets('App builds at services route with a fake bridge', (
    tester,
  ) async {
    await tester.pumpWidget(
      ProviderScope(
        overrides: [bridgeApiProvider.overrideWithValue(FakeBridgeApi())],
        child: const PocketCodexApp(initialLocation: '/'),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.byType(MaterialApp), findsOneWidget);
    expect(find.text('Pocket-Codex'), findsOneWidget);
  });
}
