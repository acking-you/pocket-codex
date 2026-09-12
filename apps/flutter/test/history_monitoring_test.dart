import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/screens/app_session/activity_cards.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/widgets/turn_minimap.dart';

import 'fake_bridge_api.dart';
import 'support/screen_harness.dart';

ThreadItem item(String id, String turn, String type, {int? duration}) =>
    ThreadItem(
      id: id,
      turnId: turn,
      itemType: type,
      title: '',
      text: id,
      turnDurationMs: duration,
    );

Future<void> frames(WidgetTester tester) async {
  for (var i = 0; i < 12; i++) {
    await tester.pump(const Duration(milliseconds: 100));
  }
}

void main() {
  testWidgets(
    'monitoring preserves a completed gap after continuation and a stale cache refresh',
    (t) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
      addTearDown(() => debugDefaultTargetPlatformOverride = null);
      await t.binding.setSurfaceSize(const Size(1600, 1400));
      addTearDown(() => t.binding.setSurfaceSize(null));
      const service = 'pcx:host:app:default';
      const thread = 'gap-monitor';
      const turns = [
        TurnSummary(
          turnId: 'older',
          userText: 'older request',
          assistantText: '',
          loaded: false,
        ),
        TurnSummary(
          turnId: 'early',
          userText: 'early request',
          assistantText: '',
          loaded: false,
        ),
        TurnSummary(
          turnId: 'middle',
          userText: 'middle request',
          assistantText: '',
          loaded: false,
        ),
        TurnSummary(
          turnId: 'latest',
          userText: 'latest request',
          assistantText: '',
          loaded: true,
        ),
      ];
      const liveness = SessionLiveness(
        threadId: thread,
        turnState: 'incomplete',
        heldOpen: true,
        safety: 'ownedRunning',
        allowsResume: false,
        requiresTakeover: false,
        holders: [],
      );
      final opening = TurnItemsPage(
        turnId: 'early',
        hasMore: true,
        items: [
          item('early request', 'early', 'userMessage'),
          item('early work', 'early', 'commandExecution', duration: 12987579),
          item('early steering', 'early', 'userMessage'),
          item('later work', 'early', 'commandExecution', duration: 12987579),
        ],
      );
      final latest = [
        item('latest request', 'latest', 'userMessage'),
        item('old segment', 'latest', 'commandExecution'),
        item('latest steering', 'latest', 'userMessage'),
        item('current work', 'latest', 'commandExecution'),
      ];
      final api =
          FakeBridgeApi(
              config: const ConfigInfo(relay: 'relay:7666', hasKey: true),
            )
            ..metadataOnlyFollow = true
            ..appThreadResumeError = StateError(
              'thread already has an active writer',
            )
            ..readResult = ThreadHistory(
              items: latest,
              running: true,
              turns: turns,
              hasOlder: true,
              firstTurnId: 'older',
            );
      api.liveness[thread] = liveness;
      api.turnPages['early'] = [
        opening,
        TurnItemsPage(
          turnId: 'early',
          hasMore: false,
          items: [item('early answer', 'early', 'agentMessage')],
        ),
      ];
      api.turnItems['middle'] = [
        item('middle request', 'middle', 'userMessage'),
        item('interrupted work', 'middle', 'commandExecution'),
        item('middle answer', 'middle', 'agentMessage'),
      ];
      await api.appConnect(service, 28080);
      await t.pumpWidget(
        host(
          const AppSessionScreen(serviceKey: service, threadId: thread),
          api,
        ),
      );
      await frames(t);
      final rail = t.widget<TurnMinimap>(find.byType(TurnMinimap));
      rail.onSelect(rail.items.firstWhere((item) => item.turnId == 'early'));
      await frames(t);
      final gaps = find.byWidgetPredicate(
        (widget) =>
            widget is TextButton &&
            widget.key is ValueKey<String> &&
            (widget.key! as ValueKey<String>).value.startsWith('history-gap-'),
      );
      expect(gaps, findsOneWidget);
      expect(find.text('已处理 3:36:28'), findsNothing);
      expect(
        t
            .widgetList<TurnWorkCard>(find.byType(TurnWorkCard))
            .where((card) => card.active),
        hasLength(1),
      );

      await t.tap(gaps);
      await frames(t);
      expect(gaps, findsOneWidget);
      expect(find.text('已处理 3:36:28'), findsOneWidget);
      await t.tap(gaps);
      await frames(t);
      expect(gaps, findsNothing);

      api.readResult = ThreadHistory(
        items: [...latest, item('new work', 'latest', 'commandExecution')],
        running: true,
        turns: turns,
        firstTurnId: 'older',
        hasOlder: true,
        turnPages: [opening],
      );
      api.pushMetaSessionUpdate(
        thread,
        const SessionFollowUpdate(
          liveness: liveness,
          items: [],
          historyRevision: 'new-tail',
        ),
      );
      await frames(t);
      expect(
        gaps,
        findsNothing,
        reason: 'an older cached prefix cannot resurrect an exhausted gap',
      );
      expect(find.text('已处理 3:36:28'), findsOneWidget);
      expect(
        t
            .widgetList<TurnWorkCard>(find.byType(TurnWorkCard))
            .where((card) => card.active),
        hasLength(1),
      );
      expect(api.turnItemCalls, ['early', 'early', 'middle']);
      expect(api.threadReadIncludesPages.last, isFalse);
      expect(t.takeException(), isNull);
      await t.pumpWidget(const SizedBox.shrink());
      debugDefaultTargetPlatformOverride = null;
    },
  );
}
