import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/widgets/middle_click_scroll.dart';

const _anchor = Offset(250, 250);
final _indicator = find.byKey(const Key('middle-click-scroll-anchor'));

Future<ScrollController> _pump(WidgetTester t, {int count = 100}) async {
  final controller = ScrollController(
    initialScrollOffset: count > 10 ? 500 : 0,
  );
  addTearDown(controller.dispose);
  await t.pumpWidget(
    MaterialApp(
      home: Scaffold(
        body: MiddleClickScroll(
          controller: controller,
          child: SelectionArea(
            child: ListView.builder(
              controller: controller,
              itemExtent: 60,
              itemCount: count,
              itemBuilder: (_, i) => Text('message $i'),
            ),
          ),
        ),
      ),
    ),
  );
  await t.pumpAndSettle();
  return controller;
}

Future<TestGesture> _middle(WidgetTester t, {bool hold = false}) async {
  final mouse = await t.createGesture(
    kind: PointerDeviceKind.mouse,
    buttons: kMiddleMouseButton,
  );
  await mouse.addPointer(location: _anchor);
  addTearDown(mouse.removePointer);
  await mouse.down(_anchor);
  await t.pump();
  if (!hold) {
    await mouse.up();
    await t.pump();
  }
  return mouse;
}

Future<void> _frames(WidgetTester t) async {
  for (var i = 0; i < 12; i++) {
    await t.pump(const Duration(milliseconds: 16));
  }
}

void main() {
  testWidgets('middle click scrolls in either direction and has a dead zone', (
    t,
  ) async {
    final controller = await _pump(t);
    final mouse = await _middle(t);
    expect(_indicator, findsOneWidget);
    await _frames(t);
    expect(controller.offset, 500);

    await mouse.moveTo(_anchor + const Offset(0, 80));
    await _frames(t);
    final lower = controller.offset;
    expect(lower, greaterThan(500));

    await mouse.moveTo(_anchor - const Offset(0, 80));
    await _frames(t);
    expect(controller.offset, lessThan(lower));

    await mouse.moveTo(_anchor + const Offset(0, 5));
    await _frames(t);
    final resting = controller.offset;
    await _frames(t);
    expect(controller.offset, resting);

    await mouse.down(_anchor);
    await mouse.up();
    await t.pumpAndSettle();
    expect(_indicator, findsNothing);
  });

  testWidgets('holding middle scrolls and releasing after movement stops', (
    t,
  ) async {
    final controller = await _pump(t);
    final mouse = await _middle(t, hold: true);
    await mouse.moveTo(_anchor + const Offset(0, 80));
    await _frames(t);
    expect(controller.offset, greaterThan(500));
    await mouse.up();
    await t.pumpAndSettle();
    expect(_indicator, findsNothing);
    final stopped = controller.offset;
    await _frames(t);
    expect(controller.offset, stopped);
  });

  testWidgets('Escape and leaving the viewport cancel auto-scroll', (t) async {
    final controller = await _pump(t);
    final mouse = await _middle(t);
    await mouse.moveTo(_anchor + const Offset(0, 80));
    await _frames(t);
    await t.sendKeyEvent(LogicalKeyboardKey.escape);
    await t.pumpAndSettle();
    expect(_indicator, findsNothing);
    final stopped = controller.offset;
    await _frames(t);
    expect(controller.offset, stopped);

    await mouse.down(_anchor);
    await mouse.up();
    await t.pump();
    expect(_indicator, findsOneWidget);
    await mouse.moveTo(const Offset(2000, 2000));
    await t.pumpAndSettle();
    expect(_indicator, findsNothing);
  });

  testWidgets('the wheel exits auto-scroll without losing its movement', (
    t,
  ) async {
    final controller = await _pump(t);
    await _middle(t);
    await t.sendEventToBinding(
      const PointerScrollEvent(
        kind: PointerDeviceKind.mouse,
        position: _anchor,
        scrollDelta: Offset(0, 120),
      ),
    );
    await t.pumpAndSettle();
    expect(_indicator, findsNothing);
    expect(controller.offset, 620);
  });

  testWidgets(
    'window focus loss stops scrolling and disposal removes handlers',
    (t) async {
      final controller = await _pump(t);
      final mouse = await _middle(t);
      await mouse.moveTo(_anchor + const Offset(0, 80));
      await _frames(t);
      t.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
      await t.pumpAndSettle();
      expect(_indicator, findsNothing);
      final stopped = controller.offset;
      await _frames(t);
      expect(controller.offset, stopped);
      t.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);

      await mouse.down(_anchor);
      await mouse.up();
      await t.pump();
      expect(_indicator, findsOneWidget);
      await t.pumpWidget(const SizedBox.shrink());
      await t.sendKeyEvent(LogicalKeyboardKey.escape);
      await t.pumpAndSettle();
      expect(t.takeException(), isNull);
    },
  );

  testWidgets('short content and primary clicks do not start auto-scroll', (
    t,
  ) async {
    await _pump(t, count: 2);
    await _middle(t);
    await t.pumpAndSettle();
    expect(_indicator, findsNothing);
    await _pump(t);
    await t.tapAt(_anchor, kind: PointerDeviceKind.mouse);
    await t.pumpAndSettle();
    expect(_indicator, findsNothing);
  });

  testWidgets('auto-scroll stays within the transcript boundaries', (t) async {
    final controller = await _pump(t);
    controller.jumpTo(controller.position.maxScrollExtent - 1);
    final mouse = await _middle(t);
    await mouse.moveTo(_anchor + const Offset(0, 80));
    await _frames(t);
    expect(controller.offset, controller.position.maxScrollExtent);
    controller.jumpTo(1);
    await mouse.moveTo(_anchor - const Offset(0, 80));
    await _frames(t);
    expect(controller.offset, controller.position.minScrollExtent);
    await t.sendKeyEvent(LogicalKeyboardKey.escape);
    await t.pumpAndSettle();
  });
}
