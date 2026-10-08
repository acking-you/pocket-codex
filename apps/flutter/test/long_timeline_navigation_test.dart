import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/screens/app_session/activity_cards.dart';
import 'package:pocket_codex/src/screens/app_session/transcript_model.dart';
import 'package:pocket_codex/src/widgets/turn_minimap.dart';
import 'package:pocket_codex/src/widgets/turn_outline.dart';

Widget host(Widget child) => MaterialApp(
  locale: const Locale('en'),
  localizationsDelegates: AppLocalizations.localizationsDelegates,
  supportedLocales: AppLocalizations.supportedLocales,
  home: Scaffold(body: child),
);

void main() {
  for (final desktop in [false, true]) {
    testWidgets('searchable 10000-turn outline stays lazy, desktop=$desktop', (
      t,
    ) async {
      debugDefaultTargetPlatformOverride = desktop
          ? TargetPlatform.macOS
          : TargetPlatform.android;
      t.view.physicalSize = desktop
          ? const Size(1200, 900)
          : const Size(390, 844);
      t.view.devicePixelRatio = 1;
      addTearDown(t.view.resetPhysicalSize);
      addTearDown(t.view.resetDevicePixelRatio);
      final items = List.generate(
        10000,
        (i) => TurnMinimapItem(
          rowIndex: -1,
          turnId: 'turn-$i',
          userText: 'Request ${i + 1}',
          assistantText: 'Response ${i + 1}',
        ),
      );
      TurnMinimapItem? selected;
      await t.pumpWidget(
        host(
          Builder(
            builder: (context) => TextButton(
              onPressed: () async => selected = await showTurnOutline(
                context,
                items: items,
                current: 5000,
              ),
              child: const Text('Open'),
            ),
          ),
        ),
      );
      await t.tap(find.text('Open'));
      await t.pumpAndSettle();
      expect(t.widgetList(find.byType(ListTile)).length, lessThan(20));
      expect(find.text('Request 5001'), findsOneWidget);
      await t.enterText(find.byKey(const Key('turn-outline-search')), '9999');
      await t.pumpAndSettle();
      expect(find.byType(ListTile), findsOneWidget);
      expect(find.text('Request 9999'), findsOneWidget);
      expect(selected, isNull);
      await t.tap(find.text('Request 9999'));
      await t.pumpAndSettle();
      expect(selected?.turnId, 'turn-9998');
      expect(t.takeException(), isNull);
      await t.pumpWidget(const SizedBox());
      debugDefaultTargetPlatformOverride = null;
    });
  }

  testWidgets(
    '1000-step work stays bounded, jumps exactly, and preserves selection on append',
    (t) async {
      t.view.physicalSize = const Size(390, 844);
      t.view.devicePixelRatio = 1;
      addTearDown(t.view.resetPhysicalSize);
      addTearDown(t.view.resetDevicePixelRatio);
      final steps = List.generate(
        1000,
        (i) => TranscriptItem(
          id: 's-$i',
          type: 'commandExecution',
          title: 'pwd $i',
          text: '/project',
          turnId: 'turn',
        ),
      );
      Widget view() => host(
        SingleChildScrollView(child: TurnWorkCard(work: TurnWork(steps))),
      );
      await t.pumpWidget(view());
      await t.tap(find.byKey(const Key('turn-work-toggle')));
      await t.pumpAndSettle();
      expect(find.text('Steps 1–25 of 1000'), findsOneWidget);
      expect(t.widgetList(find.byType(ActivityCard)).length, lessThan(26));
      await t.tap(find.byKey(const Key('work-step-picker')));
      await t.pumpAndSettle();
      await t.enterText(find.byType(TextFormField), '701');
      await t.tap(find.text('OK'));
      await t.pumpAndSettle();
      expect(find.text('Steps 701–725 of 1000'), findsOneWidget);
      expect(find.byKey(const ValueKey('work-step-s-700')), findsOneWidget);
      steps.add(
        TranscriptItem(
          id: 'last',
          type: 'commandExecution',
          title: 'new',
          text: 'new',
        ),
      );
      await t.pumpWidget(view());
      await t.pumpAndSettle();
      expect(find.text('Steps 701–725 of 1001'), findsOneWidget);
      await t.tap(find.text('Latest steps'));
      await t.pumpAndSettle();
      expect(find.text('Steps 977–1001 of 1001'), findsOneWidget);
      expect(t.takeException(), isNull);
    },
  );

  group('a running fold follows its newest step', () {
    const viewHeight = 844.0;
    late List<TranscriptItem> steps;
    late ScrollController scroll;

    TranscriptItem step(int i) => TranscriptItem(
      id: 's-$i',
      type: 'commandExecution',
      title: 'pwd $i',
      text: '/project',
      turnId: 'turn',
    );

    Future<void> render(WidgetTester t, {bool active = true}) async {
      // A fresh list each time, as the transcript rebuilds its rows.
      await t.pumpWidget(
        host(
          SingleChildScrollView(
            controller: scroll,
            child: Column(
              children: [
                const SizedBox(height: 600),
                TurnWorkCard(work: TurnWork([...steps]), active: active),
              ],
            ),
          ),
        ),
      );
      await t.pumpAndSettle();
    }

    Future<void> append(WidgetTester t, {int count = 1, bool active = true}) {
      for (var i = 0; i < count; i++) {
        steps.add(step(steps.length));
      }
      return render(t, active: active);
    }

    // The pager sits above a page taller than the screen: a reader scrolls up
    // to reach it, which on its own already steps away from the newest step.
    Future<void> tapPager(WidgetTester t, Finder control) async {
      await t.ensureVisible(control);
      await t.pumpAndSettle();
      await t.tap(control);
    }

    Rect newest(WidgetTester t) =>
        t.getRect(find.byKey(ValueKey('work-step-s-${steps.length - 1}')));

    void expectNewestVisible(WidgetTester t) {
      final rect = newest(t);
      expect(rect.top, greaterThanOrEqualTo(0));
      expect(rect.bottom, lessThanOrEqualTo(viewHeight));
    }

    Future<void> openAtTail(WidgetTester t) async {
      t.view.physicalSize = const Size(390, viewHeight);
      t.view.devicePixelRatio = 1;
      addTearDown(t.view.resetPhysicalSize);
      addTearDown(t.view.resetDevicePixelRatio);
      steps = List.generate(150, step);
      scroll = ScrollController();
      addTearDown(scroll.dispose);
      await render(t);
      await t.tap(find.byKey(const Key('turn-work-toggle')));
      await t.pumpAndSettle();
      expect(find.text('Steps 126–150 of 150'), findsOneWidget);
      // The transcript keeps a follower pinned to its end.
      scroll.jumpTo(scroll.position.maxScrollExtent);
      await t.pump();
      expectNewestVisible(t);
    }

    testWidgets('continuous appends advance the page and stay on screen', (
      t,
    ) async {
      await openAtTail(t);
      for (var n = 151; n <= 180; n++) {
        await append(t);
        expect(find.text('Steps ${n - 24}–$n of $n'), findsOneWidget);
        expectNewestVisible(t);
        expect(t.widgetList(find.byType(ActivityCard)).length, 25);
      }
      // A burst between two frames lands on the newest step too.
      await append(t, count: 7);
      expect(find.text('Steps 163–187 of 187'), findsOneWidget);
      expectNewestVisible(t);
      expect(t.takeException(), isNull);
    });

    testWidgets('an earlier page holds while steps arrive', (t) async {
      await openAtTail(t);
      await tapPager(t, find.byTooltip('Previous steps'));
      await t.pumpAndSettle();
      expect(find.text('Steps 101–125 of 150'), findsOneWidget);
      final offset = scroll.offset;
      await append(t, count: 3);
      expect(find.text('Steps 101–125 of 153'), findsOneWidget);
      expect(scroll.offset, offset);
      expect(t.takeException(), isNull);
    });

    testWidgets('a chosen old step holds while steps arrive', (t) async {
      await openAtTail(t);
      await tapPager(t, find.byKey(const Key('work-step-picker')));
      await t.pumpAndSettle();
      await t.enterText(find.byType(TextFormField), '10');
      await t.tap(find.text('OK'));
      await t.pumpAndSettle();
      expect(find.text('Steps 10–34 of 150'), findsOneWidget);
      final offset = scroll.offset;
      await append(t, count: 3);
      expect(find.text('Steps 10–34 of 153'), findsOneWidget);
      expect(find.byKey(const ValueKey('work-step-s-9')), findsOneWidget);
      expect(scroll.offset, offset);
      expect(t.takeException(), isNull);
    });

    testWidgets('scrolling up pauses; latest steps resumes for good', (
      t,
    ) async {
      await openAtTail(t);
      await t.drag(find.byType(Scrollable).first, const Offset(0, 400));
      await t.pumpAndSettle();
      final offset = scroll.offset;
      await append(t, count: 2);
      // The reader's page and place both hold.
      expect(find.text('Steps 126–150 of 152'), findsOneWidget);
      expect(scroll.offset, offset);

      await tapPager(t, find.text('Latest steps'));
      await t.pumpAndSettle();
      expect(find.text('Steps 128–152 of 152'), findsOneWidget);
      expectNewestVisible(t);
      for (var n = 153; n <= 160; n++) {
        await append(t);
        expect(find.text('Steps ${n - 24}–$n of $n'), findsOneWidget);
        expectNewestVisible(t);
      }
      expect(t.takeException(), isNull);
    });

    testWidgets('latest steps from an old page shows the newest step', (
      t,
    ) async {
      await openAtTail(t);
      await tapPager(t, find.byTooltip('Previous steps'));
      await t.pumpAndSettle();
      await tapPager(t, find.byTooltip('Previous steps'));
      await t.pumpAndSettle();
      expect(find.text('Steps 76–100 of 150'), findsOneWidget);
      await tapPager(t, find.text('Latest steps'));
      await t.pumpAndSettle();
      expect(find.text('Steps 126–150 of 150'), findsOneWidget);
      expectNewestVisible(t);
      await append(t);
      expect(find.text('Steps 127–151 of 151'), findsOneWidget);
      expectNewestVisible(t);
      expect(t.takeException(), isNull);
    });

    testWidgets('completion shows a follower the final step, then holds', (
      t,
    ) async {
      await openAtTail(t);
      await append(t, count: 2, active: false);
      expect(find.text('Steps 128–152 of 152'), findsOneWidget);
      expectNewestVisible(t);
      // A settled turn's history refresh does not move the page.
      await append(t, active: false);
      expect(find.text('Steps 128–152 of 153'), findsOneWidget);
      expect(t.takeException(), isNull);
    });

    testWidgets('completion keeps the reader where they are', (t) async {
      await openAtTail(t);
      await tapPager(t, find.byTooltip('Previous steps'));
      await t.pumpAndSettle();
      final offset = scroll.offset;
      // The final step and the end of the turn arrive together.
      await append(t, active: false);
      expect(find.text('Steps 101–125 of 151'), findsOneWidget);
      expect(scroll.offset, offset);
      expect(t.takeException(), isNull);
    });
  });

  testWidgets('a fold that outgrows inline rows opens on its newest page', (
    t,
  ) async {
    t.view.physicalSize = const Size(390, 844);
    t.view.devicePixelRatio = 1;
    addTearDown(t.view.resetPhysicalSize);
    addTearDown(t.view.resetDevicePixelRatio);
    final steps = List.generate(
      40,
      (i) => TranscriptItem(
        id: 's-$i',
        // Alternating types keep every step its own row below the threshold.
        type: i.isEven ? 'commandExecution' : 'webSearch',
        title: 'step $i',
        text: '/project',
        turnId: 'turn',
      ),
    );
    Future<void> render() async {
      await t.pumpWidget(
        host(
          SingleChildScrollView(
            child: TurnWorkCard(work: TurnWork([...steps]), active: true),
          ),
        ),
      );
      await t.pumpAndSettle();
    }

    await render();
    await t.tap(find.byKey(const Key('turn-work-toggle')));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('work-step-picker')), findsNothing);
    steps.add(
      TranscriptItem(
        id: 's-40',
        type: 'commandExecution',
        title: 'step 40',
        text: '/project',
        turnId: 'turn',
      ),
    );
    await render();
    expect(find.text('Steps 17–41 of 41'), findsOneWidget);
    expect(find.byKey(const ValueKey('work-step-s-40')), findsOneWidget);
    expect(t.takeException(), isNull);
  });

  testWidgets(
    'a long expanded fold collapses from its foot and returns to its header',
    (t) async {
      t.view.physicalSize = const Size(390, 844);
      t.view.devicePixelRatio = 1;
      addTearDown(t.view.resetPhysicalSize);
      addTearDown(t.view.resetDevicePixelRatio);
      final steps = List.generate(
        30,
        (i) => TranscriptItem(
          id: 's-$i',
          // Agent prose renders Markdown, so a fenced block puts a sideways
          // scroller between the fold and the page, the case that tripped
          // ensureVisible's size assertion.
          type: i.isEven ? 'commandExecution' : 'agentMessage',
          title: 'step $i',
          text: i.isEven ? '/project' : '```\n${'x' * 400}\n```',
          turnId: 'turn',
        ),
      );
      final scroll = ScrollController();
      addTearDown(scroll.dispose);
      await t.pumpWidget(
        host(
          SingleChildScrollView(
            controller: scroll,
            child: Column(
              children: [
                TurnWorkCard(work: TurnWork(steps)),
                const SizedBox(height: 2000),
              ],
            ),
          ),
        ),
      );
      await t.tap(find.byKey(const Key('turn-work-toggle')));
      await t.pumpAndSettle();
      final collapse = find.byKey(const Key('turn-work-collapse'));
      await t.scrollUntilVisible(
        collapse,
        300,
        scrollable: find.byType(Scrollable).first,
      );
      expect(scroll.offset, greaterThan(0));
      await t.tap(collapse);
      await t.pumpAndSettle();
      expect(collapse, findsNothing);
      // Back at the header, not stranded somewhere below where the steps were.
      final header = t.getRect(find.byKey(const Key('turn-work-toggle')));
      expect(header.top, greaterThanOrEqualTo(0));
      expect(header.bottom, lessThanOrEqualTo(844));
      expect(t.takeException(), isNull);
    },
  );
}
