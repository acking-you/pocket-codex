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

  testWidgets('the control follows the pointer exactly, without rebuilding', (
    t,
  ) async {
    var builds = 0;
    await t.pumpWidget(
      MaterialApp(
        home: Align(
          alignment: Alignment.topLeft,
          child: SizedBox(
            width: 390,
            height: 600,
            child: DraggableNavigation(
              label: 'Move navigation',
              child: Builder(
                builder: (_) {
                  builds++;
                  return const SizedBox(width: 48, height: 100);
                },
              ),
            ),
          ),
        ),
      ),
    );
    final control = find.byKey(const Key('draggable-turn-navigation'));
    final start = t.getTopLeft(control);
    final built = builds;
    final gesture = await t.startGesture(t.getCenter(control));
    // Past the touch slop first, then measured steps.
    await gesture.moveBy(const Offset(0, -30));
    await t.pump();
    final afterSlop = t.getTopLeft(control);
    for (var i = 0; i < 10; i++) {
      await gesture.moveBy(const Offset(-7, -9));
      await t.pump();
    }
    final end = t.getTopLeft(control);
    await gesture.up();
    // 1:1 with the pointer once dragging, and every frame was layout-only.
    expect(end - afterSlop, const Offset(-70, -90));
    expect(end.dy, lessThan(start.dy));
    expect(builds, built);
  });
}
