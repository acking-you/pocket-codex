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
List<AnimatedContainer> _ticks(WidgetTester t) => t
    .widgetList<AnimatedContainer>(
      find.descendant(
        of: find.byType(TurnMinimap),
        matching: find.byType(AnimatedContainer),
      ),
    )
    .toList();

double _tickWidthAt(WidgetTester t, int index) =>
    _ticks(t)[index].constraints!.maxWidth;

/// Hover the rail at [fraction] of its height, which is how the widget resolves
/// which tick the pointer is on.
Future<void> _hoverTick(WidgetTester t, double fraction) async {
  final rail = t.getRect(find.byKey(const Key('turn-minimap-rail')));
  final gesture = await t.createGesture(kind: PointerDeviceKind.mouse);
  await gesture.addPointer(location: Offset.zero);
  addTearDown(gesture.removePointer);
  await gesture.moveTo(
    Offset(rail.left + 4, rail.top + rail.height * fraction),
  );
  await t.pumpAndSettle();
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
  });

  testWidgets('an on-screen turn is marked whatever the pointer is doing', (
    t,
  ) async {
    // Two independent cues: colour says where you ARE, width says what you are
    // pointing at. Reading them off the same state would lose one of them.
    final items = _items(4);
    await _pump(
      t,
      items: items,
      visible: (items[2].rowIndex, items[2].rowIndex),
    );

    final ticks = t
        .widgetList<AnimatedContainer>(
          find.descendant(
            of: find.byType(TurnMinimap),
            matching: find.byType(AnimatedContainer),
          ),
        )
        .toList();
    Color colorOf(int i) => (ticks[i].decoration! as BoxDecoration).color!;
    // The visible turn is the strong mark; the others are quiet.
    expect(colorOf(2).a, greaterThan(colorOf(0).a));
    // …and it has NOT been widened, because nothing is hovered.
    expect(_tickWidthAt(t, 2), _tickWidthAt(t, 0));
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
    // 200 turns at the nominal 8 px spacing would be a 1600 px rail in a 300 px
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

  testWidgets('a click leaves the rail open with the landed tick marked', (
    t,
  ) async {
    // The jump used to clear the active tick, collapsing the rail to its resting
    // width at the very moment the user arrived — so it stopped saying where in
    // the conversation they now were.
    final items = _items(5);
    await _pump(t, items: items);
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
      greaterThan(_tickWidthAt(t, 0)),
      reason: 'the tick just jumped to stays the widest',
    );
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
