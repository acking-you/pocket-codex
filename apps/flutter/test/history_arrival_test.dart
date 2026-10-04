import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/widgets/history_arrival.dart';

void main() {
  testWidgets('history feedback preserves row state on replay and removal', (
    t,
  ) async {
    Widget host(int? revision) => MaterialApp(
      home: Scaffold(
        body: HistoryArrival(revision: revision, child: const TextField()),
      ),
    );
    await t.pumpWidget(host(null));
    await t.enterText(find.byType(TextField), 'retained row state');
    for (final revision in [1, null, 2]) {
      await t.pumpWidget(host(revision));
      await t.pump(const Duration(milliseconds: 1500));
      expect(find.text('retained row state'), findsOneWidget);
    }
    expect(t.takeException(), isNull);
  });
}
