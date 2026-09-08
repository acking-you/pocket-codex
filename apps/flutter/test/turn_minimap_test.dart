// The turn rail: tick geometry, the hover preview, click-to-jump, keyboard
// navigation, and the gutter rules that decide whether it renders at all.

import 'package:flutter/gestures.dart' show PointerDeviceKind;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show LogicalKeyboardKey;
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/widgets/turn_minimap.dart';

List<TurnMinimapItem> _items(int count) => [
  for (var i = 0; i < count; i++)
    TurnMinimapItem(
      rowIndex: i * 3,
      turnId: 'turn-$i',
      userText: 'question $i',
      assistantText: 'answer $i',
    ),
];

/// Mount the rail at a fixed size, with [gutter] standing in for the space
/// beside the conversation column.
Future<List<TurnMinimapItem>> _pump(
  WidgetTester t, {
  required List<TurnMinimapItem> items,
  double gutter = 120,
  (int, int)? visible,
  double height = 600,
}) async {
  final selected = <TurnMinimapItem>[];
  await t.pumpWidget(
    MaterialApp(
      home: Scaffold(
        body: SizedBox(
          width: 400,
          height: height,
          child: TurnMinimap(
            items: items,
            visibleRange: ValueNotifier<(int, int)?>(visible),
            gutterWidth: gutter,
            onSelect: selected.add,
          ),
        ),
      ),
    ),
  );
  await t.pumpAndSettle();
  return selected;
}

/// The rail's tick widgets, in transcript order.
List<Container> _ticks(WidgetTester t) => t
    .widgetList<Container>(
      find.descendant(
        of: find.byType(TurnMinimap),
        matching: find.byWidgetPredicate(
          (widget) =>
              widget is Container &&
              widget.key is ValueKey<String> &&
              (widget.key! as ValueKey<String>).value.startsWith(
                'turn-minimap-tick-',
              ),
        ),
      ),
    )
    .toList();

double _tickWidthAt(WidgetTester t, int index) =>
    t.getSize(find.byKey(ValueKey('turn-minimap-tick-$index'))).width;

/// Hover the rail at [fraction] of its height, which is how the widget resolves
/// which tick the pointer is on.
Future<TestGesture> _hoverTick(WidgetTester t, double fraction) async {
  final rail = t.getRect(find.byKey(const Key('turn-minimap-rail')));
  final gesture = await t.createGesture(kind: PointerDeviceKind.mouse);
  await gesture.addPointer(location: Offset.zero);
  addTearDown(gesture.removePointer);
  await gesture.moveTo(
    Offset(rail.left + 4, rail.top + rail.height * fraction),
  );
  await t.pumpAndSettle();
  return gesture;
}

void main() {
  testWidgets('renders one tick per turn', (t) async {
    await _pump(t, items: _items(5));
    expect(_ticks(t), hasLength(5));
  });

  testWidgets('a single turn is not worth a rail', (t) async {
    // One tick would say nothing the scrollbar doesn't, and a rail with nothing
    // to choose between is just decoration.
    await _pump(t, items: _items(1));
    expect(_ticks(t), isEmpty);
  });

  testWidgets('no gutter to live in means no rail', (t) async {
    // Rather than reach over the centred column — where it would swallow text
    // selection — the rail stands down entirely.
    await _pump(t, items: _items(6), gutter: 0);
    expect(_ticks(t), isEmpty);
  });

  testWidgets('hovering a tick previews that turn and widens it', (t) async {
    await _pump(t, items: _items(6));
    expect(find.text('question 0'), findsNothing);

    await _hoverTick(t, 0);

    // The preview names the turn AND how it was answered — the point of the
    // card over a bare tooltip.
    expect(find.text('question 0'), findsOneWidget);
    expect(find.text('answer 0'), findsOneWidget);
    // The pointed-at tick is the widest, its neighbour narrower: the falloff is
    // what makes the rail read as one object tracking the cursor.
    expect(_tickWidthAt(t, 0), greaterThan(_tickWidthAt(t, 1)));
    expect(_tickWidthAt(t, 1), greaterThan(_tickWidthAt(t, 2)));
  });

  testWidgets('a turn with no reply previews only the question', (t) async {
    await _pump(
      t,
      items: const [
        TurnMinimapItem(rowIndex: 0, userText: 'unanswered'),
        TurnMinimapItem(rowIndex: 4, userText: 'second', assistantText: 'ok'),
        TurnMinimapItem(rowIndex: 8, userText: 'third', assistantText: 'ok'),
        TurnMinimapItem(rowIndex: 12, userText: 'fourth', assistantText: 'ok'),
      ],
    );
    await _hoverTick(t, 0);
    expect(find.text('unanswered'), findsOneWidget);
  });

  testWidgets('an attachment-only turn leads with the reply, not a label', (
    t,
  ) async {
    // The sidebar renders such a message as "[file]" because a row must say
    // something. Heading a preview card with the word "file" names the
    // attachment rather than the turn, so the caller passes no placeholder and
    // the reply carries the card alone.
    await _pump(
      t,
      items: const [
        TurnMinimapItem(rowIndex: 0, userText: '', assistantText: 'read it'),
        TurnMinimapItem(rowIndex: 4, userText: 'next', assistantText: 'ok'),
        TurnMinimapItem(rowIndex: 8, userText: 'third', assistantText: 'ok'),
        TurnMinimapItem(rowIndex: 12, userText: 'fourth', assistantText: 'ok'),
      ],
    );
    await _hoverTick(t, 0);
    expect(find.text('read it'), findsOneWidget);
    expect(find.text('[文件]'), findsNothing);
  });

  testWidgets('a turn with nothing to show gets no card at all', (t) async {
    // An empty card would only occlude the conversation it exists to help you
    // search. The tick stays — it is still a turn you can jump to.
    await _pump(
      t,
      items: const [
        TurnMinimapItem(rowIndex: 0, userText: ''),
        TurnMinimapItem(rowIndex: 4, userText: 'next', assistantText: 'ok'),
        TurnMinimapItem(rowIndex: 8, userText: 'third', assistantText: 'ok'),
        TurnMinimapItem(rowIndex: 12, userText: 'fourth', assistantText: 'ok'),
      ],
    );
    expect(_ticks(t), hasLength(4));
    await _hoverTick(t, 0);
    expect(find.byKey(const Key('turn-minimap-preview')), findsNothing);
    expect(find.text('next'), findsNothing);
  });

  testWidgets('a tight gutter narrows the card instead of burying the text', (
    t,
  ) async {
    // Some overhang is unavoidable — a readable card does not fit a 60 px
    // margin — but it is budgeted, so the card shrinks rather than covering the
    // column it floats beside.
    await _pump(t, items: _items(4), gutter: 60);
    await _hoverTick(t, 0);

    final card = t.widget<Container>(
      find.byKey(const Key('turn-minimap-preview')),
    );
    expect(card.constraints!.maxWidth, lessThan(300));
  });

  testWidgets('clicking a tick jumps to that turn by row index', (t) async {
    final items = _items(6);
    final selected = await _pump(t, items: items);

    final rail = t.getRect(find.byKey(const Key('turn-minimap-rail')));
    // Tap the far end of the rail — the last turn.
    await t.tapAt(Offset(rail.left + 4, rail.bottom - 1));
    await t.pumpAndSettle();

    expect(selected, hasLength(1));
    // Row index, not tick index: a turn's user message sits several rows apart
    // from the previous one's, and the transcript scrolls by row.
    expect(selected.single.rowIndex, items.last.rowIndex);
  });

  testWidgets('the preview leaves on pointer exit', (t) async {
    await _pump(t, items: _items(4));
    final rail = t.getRect(find.byKey(const Key('turn-minimap-rail')));
    final gesture = await t.createGesture(kind: PointerDeviceKind.mouse);
    await gesture.addPointer(location: Offset.zero);
    addTearDown(gesture.removePointer);

    await gesture.moveTo(Offset(rail.left + 4, rail.top));
    await t.pumpAndSettle();
    expect(find.text('question 0'), findsOneWidget);

    // Away from the rail entirely — the card must not linger over the text it
    // was floating above.
    await gesture.moveTo(const Offset(399, 599));
    await t.pumpAndSettle();
    expect(find.text('question 0'), findsNothing);
    expect(_tickWidthAt(t, 0), 6);
    expect(t.getSize(find.byKey(const Key('turn-minimap-rail'))).width, 40);
  });

  testWidgets('leaving for empty space beside the preview restores the rail', (
    t,
  ) async {
    await _pump(t, items: _items(50));
    final rail = t.getRect(find.byKey(const Key('turn-minimap-rail')));
    final gesture = await _hoverTick(t, 0);
    expect(find.byKey(const Key('turn-minimap-preview')), findsOneWidget);

    // Inside the old expanded hit box, but far below the actual preview card.
    await gesture.moveTo(Offset(rail.left + 200, rail.bottom - 2));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('turn-minimap-preview')), findsNothing);
    expect(_tickWidthAt(t, 0), 6);
    expect(t.getSize(find.byKey(const Key('turn-minimap-rail'))).width, 40);
  });

  testWidgets('moving into the actual preview keeps it open', (t) async {
    await _pump(t, items: _items(50));
    final gesture = await _hoverTick(t, 0);
    await gesture.moveTo(
      t.getCenter(find.byKey(const Key('turn-minimap-preview'))),
    );
    await t.pumpAndSettle();
    expect(find.byKey(const Key('turn-minimap-preview')), findsOneWidget);
    expect(_tickWidthAt(t, 0), 26);
  });

  for (final count in [4, 12, 400]) {
    testWidgets('$count turns: padded rail ends remain clickable', (t) async {
      final items = _items(count);
      final selected = await _pump(t, items: items, height: 300);
      final rail = t.getRect(find.byKey(const Key('turn-minimap-rail')));
      final mouse = await _hoverTick(t, 0);
      for (final (point, index) in [
        (Offset(rail.left - 8, rail.top - 8), 0),
        (Offset(rail.left - 8, rail.bottom + 8), count - 1),
      ]) {
        await mouse.moveTo(point);
        await t.pumpAndSettle();
        expect(find.text('question $index'), findsOneWidget);
        await mouse.down(point);
        await mouse.up();
        await t.pumpAndSettle();
        expect(selected.last, same(items[index]));
      }
      expect(selected, hasLength(2));
    });

    testWidgets('$count turns: crossing the gap keeps the preview target', (
      t,
    ) async {
      final items = _items(count);
      final selected = await _pump(t, items: items, height: 300);
      final index = count ~/ 2;
      final mouse = await _hoverTick(t, index / (count - 1));
      final tick = t.getRect(find.byKey(ValueKey('turn-minimap-tick-$index')));
      final gap = Offset(tick.right + 5, tick.center.dy + 8);
      await mouse.moveTo(gap);
      await t.pumpAndSettle();
      expect(find.text('question $index'), findsOneWidget);
      expect(_tickWidthAt(t, index), 26);
      await mouse.down(gap);
      await mouse.up();
      await t.pumpAndSettle();
      expect(selected.single, same(items[index]));
      expect(find.byKey(const Key('turn-minimap-preview')), findsNothing);
    });

    for (final index in [0, count - 1]) {
      testWidgets(
        '$count turns: end preview $index accepts an imprecise click',
        (t) async {
          final items = _items(count);
          final selected = await _pump(t, items: items, height: 300);
          final rail = t.getRect(find.byKey(const Key('turn-minimap-rail')));
          final mouse = await _hoverTick(t, index / (count - 1));
          final card = t.getRect(find.byKey(const Key('turn-minimap-preview')));
          final point = index == 0
              ? Offset(card.right + 4, card.bottom + 4)
              : Offset(card.right + 4, card.top - 4);
          if (count == 4) {
            expect(rail.contains(point), isFalse);
          }
          await mouse.moveTo(point);
          await t.pumpAndSettle();
          expect(find.text('question $index'), findsOneWidget);
          await mouse.down(point);
          await mouse.up();
          await t.pumpAndSettle();
          expect(selected.single, same(items[index]));
        },
      );
    }
  }

  testWidgets('a preview keeps its turn when loading changes rows and order', (
    t,
  ) async {
    var items = _items(12);
    late StateSetter rebuild;
    final selected = <TurnMinimapItem>[];
    final visible = ValueNotifier<(int, int)?>(null);
    addTearDown(visible.dispose);
    await t.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SizedBox(
            width: 400,
            height: 300,
            child: StatefulBuilder(
              builder: (context, setState) {
                rebuild = setState;
                return TurnMinimap(
                  items: items,
                  visibleRange: visible,
                  gutterWidth: 120,
                  onSelect: selected.add,
                );
              },
            ),
          ),
        ),
      ),
    );
    final mouse = await _hoverTick(t, 5 / 11);
    final card = t.getCenter(find.byKey(const Key('turn-minimap-preview')));
    await mouse.moveTo(card);
    await t.pumpAndSettle();
    await mouse.down(card);
    await t.pump(const Duration(milliseconds: 150));
    rebuild(() {
      items = [
        const TurnMinimapItem(rowIndex: 0, userText: 'older', turnId: 'older'),
        for (final item in items)
          TurnMinimapItem(
            rowIndex: item.rowIndex + 20,
            userText: item.userText,
            assistantText: item.assistantText,
            turnId: item.turnId,
          ),
      ];
    });
    await t.pumpAndSettle();
    expect(find.text('question 5'), findsOneWidget);
    await mouse.up();
    await t.pumpAndSettle();
    expect(selected.single.turnId, 'turn-5');
    expect(selected.single.rowIndex, 35);
  });

  testWidgets('a cancelled press does not jump', (t) async {
    final selected = await _pump(t, items: _items(400), height: 300);
    final mouse = await _hoverTick(t, 0.5);
    final card = t.getCenter(find.byKey(const Key('turn-minimap-preview')));
    await mouse.down(card);
    await t.pump(const Duration(milliseconds: 150));
    expect(selected, isEmpty);
    await mouse.moveBy(const Offset(60, 0));
    await mouse.up();
    await t.pumpAndSettle();
    expect(selected, isEmpty);
  });

  testWidgets('empty space beside a preview passes clicks to the transcript', (
    t,
  ) async {
    var backgroundClicks = 0;
    final selected = <TurnMinimapItem>[];
    final visible = ValueNotifier<(int, int)?>(null);
    addTearDown(visible.dispose);
    await t.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SizedBox(
            width: 400,
            height: 300,
            child: Stack(
              children: [
                Positioned.fill(
                  child: GestureDetector(
                    behavior: HitTestBehavior.opaque,
                    onTap: () => backgroundClicks++,
                  ),
                ),
                Positioned.fill(
                  child: TurnMinimap(
                    items: _items(400),
                    visibleRange: visible,
                    gutterWidth: 120,
                    onSelect: selected.add,
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
    await _hoverTick(t, 0);
    final rail = t.getRect(find.byKey(const Key('turn-minimap-rail')));
    await t.tapAt(Offset(rail.left + 200, rail.bottom - 2));
    await t.pumpAndSettle();
    expect(backgroundClicks, 1);
    expect(selected, isEmpty);
  });

  testWidgets('hover takes over one position highlight and exit restores it', (
    t,
  ) async {
    final items = _items(8);
    await _pump(
      t,
      items: items,
      visible: (items[2].rowIndex, items[6].rowIndex),
    );

    List<double> widths() => [
      for (var i = 0; i < items.length; i++) _tickWidthAt(t, i),
    ];
    List<int> darkTicks() {
      final ticks = _ticks(t);
      final alphas = [
        for (final tick in ticks) (tick.decoration! as BoxDecoration).color!.a,
      ];
      final strongest = alphas.reduce((a, b) => a > b ? a : b);
      return [
        for (var i = 0; i < alphas.length; i++)
          if (alphas[i] == strongest) i,
      ];
    }

    final resting = widths();
    expect(resting, [6, 6, 13, 6, 6, 6, 6, 6]);
    expect(darkTicks(), [2]);
    final rail = t.getRect(find.byKey(const Key('turn-minimap-rail')));
    final gesture = await _hoverTick(t, 1 / 7);
    expect(widths(), [20, 26, 20, 14, 10, 6, 6, 6]);
    expect(darkTicks(), [1]);

    // Inspect rendered geometry before a transition could settle: the new
    // pointed-at tick must already be the sole longest/darkest one.
    await gesture.moveTo(Offset(rail.left + 4, rail.top + rail.height * 5 / 7));
    await t.pump();
    await t.pump(const Duration(milliseconds: 30));
    expect(widths(), [6, 6, 10, 14, 20, 26, 20, 14]);
    expect(darkTicks(), [5]);

    await gesture.moveTo(const Offset(2000, 2000));
    await t.pump();
    expect(widths(), resting);
    expect(darkTicks(), [2]);
    expect(find.byKey(const Key('turn-minimap-preview')), findsNothing);
  });

  testWidgets('the rail reshapes as the transcript scrolls', (t) async {
    final items = _items(5);
    final visible = ValueNotifier<(int, int)?>((
      items[1].rowIndex,
      items[1].rowIndex,
    ));
    addTearDown(visible.dispose);
    await t.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SizedBox(
            width: 400,
            height: 600,
            child: TurnMinimap(
              items: items,
              visibleRange: visible,
              gutterWidth: 120,
              onSelect: (_) {},
            ),
          ),
        ),
      ),
    );
    await t.pumpAndSettle();
    expect(_tickWidthAt(t, 1), greaterThan(_tickWidthAt(t, 4)));

    // The user message can leave the viewport while its reply is still being
    // read. Other visible turns must not take over that position marker.
    visible.value = (items[1].rowIndex + 1, items[3].rowIndex);
    await t.pumpAndSettle();
    expect(_tickWidthAt(t, 1), greaterThan(_tickWidthAt(t, 3)));

    // Scroll to the end of the conversation.
    visible.value = (items[4].rowIndex, items[4].rowIndex);
    await t.pumpAndSettle();
    expect(
      _tickWidthAt(t, 4),
      greaterThan(_tickWidthAt(t, 1)),
      reason: 'the single position marker follows the viewport',
    );
  });

  testWidgets('keyboard walks the turns and enter jumps', (t) async {
    final items = _items(5);
    final selected = await _pump(t, items: items);

    // Focus the rail: the first turn becomes active, so tabbing in shows where
    // the keyboard will act rather than nothing at all. Reached through the
    // rail's own Focus widget — an ancestor scope would take the key events.
    final focus = t
        .widget<Focus>(
          find.descendant(
            of: find.byType(TurnMinimap),
            matching: find.byType(Focus),
          ),
        )
        .focusNode!;
    focus.requestFocus();
    await t.pumpAndSettle();
    expect(find.text('question 0'), findsOneWidget);

    await t.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    await t.pumpAndSettle();
    expect(find.text('question 1'), findsOneWidget);

    await t.sendKeyEvent(LogicalKeyboardKey.end);
    await t.pumpAndSettle();
    expect(find.text('question 4'), findsOneWidget);

    await t.sendKeyEvent(LogicalKeyboardKey.enter);
    await t.pumpAndSettle();
    expect(selected.single.rowIndex, items.last.rowIndex);
  });

  testWidgets('many turns compress instead of overflowing the window', (
    t,
  ) async {
    // 200 turns at the nominal 10 px spacing would be a 2000 px rail in a 300 px
    // window. The rail caps and the ticks pack tighter.
    await _pump(t, items: _items(200), height: 300);
    final rail = t.getRect(find.byKey(const Key('turn-minimap-rail')));
    expect(rail.height, lessThanOrEqualTo(300));
    expect(_ticks(t), hasLength(200));
  });

  testWidgets('the rail rests at the window edge, not against the column', (
    t,
  ) async {
    // It used to be placed relative to the conversation column, so it drifted
    // outward with the gutter and floated in the middle of empty margin on a wide
    // window. An index of the whole conversation belongs where a scrollbar is.
    // Asserted across two gutter widths, since the bug was precisely that the
    // position tracked the gutter.
    await _pump(t, items: _items(5), gutter: 60);
    final narrow = t.getTopLeft(find.byKey(const Key('turn-minimap-rail'))).dx;
    await _pump(t, items: _items(5), gutter: 300);
    final wide = t.getTopLeft(find.byKey(const Key('turn-minimap-rail'))).dx;

    expect(narrow, wide, reason: 'the rail must not move with the gutter');
    expect(
      wide,
      lessThanOrEqualTo(kTurnMinimapRailInset),
      reason: 'it sits at the frame, not out beside the prose',
    );
  });

  testWidgets('a clicked tick loses its hover width when the pointer leaves', (
    t,
  ) async {
    final items = _items(5);
    await _pump(
      t,
      items: items,
      visible: (items.last.rowIndex, items.last.rowIndex),
    );
    final restingWidth = _tickWidthAt(t, items.length - 1);
    final rail = t.getRect(find.byKey(const Key('turn-minimap-rail')));
    final gesture = await t.createGesture(kind: PointerDeviceKind.mouse);
    await gesture.addPointer(location: Offset.zero);
    addTearDown(gesture.removePointer);

    // Click the last tick, then take the pointer away entirely.
    await gesture.moveTo(Offset(rail.left + 4, rail.bottom - 1));
    await t.pumpAndSettle();
    await t.tapAt(Offset(rail.left + 4, rail.bottom - 1));
    await t.pumpAndSettle();
    await gesture.moveTo(const Offset(2000, 2000));
    await t.pumpAndSettle();

    expect(
      _tickWidthAt(t, items.length - 1),
      restingWidth,
      reason: 'only the viewport highlight remains after pointer exit',
    );
    expect(t.getSize(find.byKey(const Key('turn-minimap-rail'))).width, 40);
    // The preview card is the one thing that must NOT survive the jump: it would
    // hang over the turn the user just navigated to.
    expect(find.byKey(const Key('turn-minimap-preview')), findsNothing);
  });

  testWidgets('three turns is below the rail, four is not', (t) async {
    // A rail earns its place by showing a conversation's shape. Two or three
    // ticks show none, and what is left is a hover-only target that steps one
    // turn at a time — which is what the corner arrows already do, with a label
    // and a touch-sized target.
    await _pump(t, items: _items(3));
    expect(_ticks(t), isEmpty);
    await _pump(t, items: _items(kTurnMinimapMinItems));
    expect(_ticks(t), hasLength(kTurnMinimapMinItems));
  });
}
