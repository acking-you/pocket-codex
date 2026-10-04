import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/widgets/draggable_navigation.dart';

void main() {
  testWidgets('drag stays inside viewport across rebuilds and resize', (
    t,
  ) async {
    var taps = 0;
    Widget host(double height) => MaterialApp(
      home: Align(
        alignment: Alignment.topLeft,
        child: SizedBox(
          width: 390,
          height: height,
          child: DraggableNavigation(
            label: 'Move navigation',
            child: SizedBox(
              width: 48,
              height: 100,
              child: Material(
                child: IconButton(
                  icon: const Icon(Icons.arrow_upward),
                  onPressed: () => taps++,
                ),
              ),
            ),
          ),
        ),
      ),
    );
    final control = find.byKey(const Key('draggable-turn-navigation'));
    await t.pumpWidget(host(600));
    final before = t.getTopLeft(control);
    await t.drag(control, const Offset(-140, -180));
    await t.pump();
    final moved = t.getTopLeft(control);
    expect(moved.dx, lessThan(before.dx - 100));
    expect(moved.dy, lessThan(before.dy - 140));
    expect(taps, 0);
    await t.pumpWidget(host(600));
    await t.drag(control, const Offset(50, 50));
    await t.pump();
    expect(t.getTopLeft(control).dx, greaterThan(moved.dx));
    await t.pumpWidget(host(180));
    expect(t.getRect(control).bottom, lessThanOrEqualTo(168));
    await t.tap(find.byType(IconButton));
    expect(taps, 1);
    expect(t.takeException(), isNull);
  });
}
