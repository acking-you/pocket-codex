// Smoke test for the Pocket-Codex placeholder UI.
//
// We render the root widget with a stub `home` so we don't have to
// initialise the Rust bridge in pure-Dart unit tests; the real
// Rust↔Dart round-trip is exercised in the integration test under
// `integration_test/`.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:pocket_codex/main.dart';

void main() {
  testWidgets('Pocket-Codex placeholder UI builds', (
    WidgetTester tester,
  ) async {
    await tester.pumpWidget(
      const PocketCodexApp(
        home: Scaffold(body: Center(child: Text('stub'))),
      ),
    );
    expect(find.byType(MaterialApp), findsOneWidget);
    expect(find.text('stub'), findsOneWidget);
  });
}
