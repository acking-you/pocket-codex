import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/screens/app_session/activity_cards.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';

import 'fake_bridge_api.dart';
import 'support/screen_harness.dart';

const service = 'pcx:host:app:default';
const thread = 'lifecycle';

Future<void> frames(WidgetTester tester) async {
  for (var i = 0; i < 8; i++) {
    await tester.pump(const Duration(milliseconds: 50));
  }
}

void event(FakeBridgeApi api, String kind, String turn) => api.pushEvent(
  service,
  AppEvent(kind: kind, threadId: thread, raw: '{"turn":{"id":"$turn"}}'),
);

void work(FakeBridgeApi api, String turn, String id, String text) =>
    api.pushEvent(
      service,
      AppEvent(
        kind: 'item/started',
        threadId: thread,
        itemId: id,
        itemType: 'commandExecution',
        text: text,
        raw: '{"turnId":"$turn"}',
      ),
    );

const runningHistory = ThreadHistory(
  running: true,
  activeTurnId: 'turn-1',
  items: [
    ThreadItem(
      id: 'user',
      turnId: 'turn-1',
      itemType: 'userMessage',
      title: '',
      text: 'Do the work',
    ),
    ThreadItem(
      id: 'command',
      turnId: 'turn-1',
      itemType: 'commandExecution',
      title: '',
      text: 'old output',
    ),
  ],
);

final completedHistory = ThreadHistory(
  running: false,
  items: [
    ...runningHistory.items,
    const ThreadItem(
      id: 'answer',
      turnId: 'turn-1',
      itemType: 'agentMessage',
      title: '',
      text: 'The completed answer',
    ),
  ],
);

SessionLiveness liveness(bool running) => SessionLiveness(
  threadId: thread,
  turnState: running ? 'incomplete' : 'completed',
  heldOpen: true,
  safety: running ? 'ownedRunning' : 'ownedIdle',
  allowsResume: false,
  requiresTakeover: false,
  holders: const [],
);

Future<void> mount(WidgetTester t, FakeBridgeApi api) async {
  await api.appConnect(service, 28080);
  await t.pumpWidget(
    host(
      const AppSessionScreen(serviceKey: service, threadId: thread),
      api,
      locale: const Locale('en'),
    ),
  );
  await frames(t);
}

void main() {
  setUp(AppSessionScreen.debugResetThreadMemory);

  for (final terminal in ['turn/completed', 'turn/failed']) {
    testWidgets('$terminal wins over an older running history response', (
      t,
    ) async {
      final read = Completer<ThreadHistory>();
      final api = FakeBridgeApi()..pendingReads[thread] = [read.future];
      await mount(t, api);
      expect(api.threadReads, [thread]);
      event(api, 'turn/started', 'turn-1');
      work(api, 'turn-1', 'command', 'final command output');
      api.pushEvent(
        service,
        const AppEvent(
          kind: 'item/completed',
          threadId: thread,
          itemId: 'answer',
          itemType: 'agentMessage',
          text: 'Final answer received live',
          raw: '{"turnId":"turn-1"}',
        ),
      );
      event(api, terminal, 'turn-1');
      await frames(t);
      expect(find.byKey(const Key('stop-btn')), findsNothing);

      read.complete(runningHistory);
      await frames(t);
      expect(find.byKey(const Key('stop-btn')), findsNothing);
      expect(find.byKey(const Key('chat-status-running-pulse')), findsNothing);
      expect(find.text('Final answer received live'), findsOneWidget);
      final card = t.widget<TurnWorkCard>(find.byType(TurnWorkCard));
      expect(card.active, isFalse);
      expect(card.work.items.single.text, 'final command output');
      expect(card.work.streaming, isFalse);
      await t.pump(const Duration(seconds: 20));
      expect(find.byKey(const Key('stop-btn')), findsNothing);
      expect(t.takeException(), isNull);
      await t.pumpWidget(const SizedBox.shrink());
    });
  }

  testWidgets('a new live turn wins over an older idle history response', (
    t,
  ) async {
    final read = Completer<ThreadHistory>();
    final api = FakeBridgeApi()..pendingReads[thread] = [read.future];
    await mount(t, api);
    event(api, 'turn/started', 'turn-new');
    work(api, 'turn-new', 'new-command', 'new work');
    await frames(t);
    read.complete(const ThreadHistory(items: [], running: false));
    await frames(t);
    expect(find.byKey(const Key('stop-btn')), findsOneWidget);
    expect(t.widget<TurnWorkCard>(find.byType(TurnWorkCard)).active, isTrue);
    await t.tap(find.byKey(const Key('stop-btn')));
    await frames(t);
    expect(api.lastInterruptTurnId, 'turn-new');
    expect(t.takeException(), isNull);
    await t.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('a delta without its prefix cannot truncate loaded history', (
    t,
  ) async {
    final read = Completer<ThreadHistory>();
    final api = FakeBridgeApi()..pendingReads[thread] = [read.future];
    await mount(t, api);
    event(api, 'turn/started', 'turn-1');
    api.pushEvent(
      service,
      const AppEvent(
        kind: 'item/agentMessage/delta',
        threadId: thread,
        itemId: 'answer',
        itemType: 'agentMessage',
        text: 'answer',
        raw: '{"turnId":"turn-1"}',
      ),
    );
    await frames(t);
    read.complete(
      ThreadHistory(
        items: completedHistory.items,
        running: true,
        activeTurnId: 'turn-1',
      ),
    );
    await frames(t);
    expect(find.text('The completed answer'), findsOneWidget);
    await t.pumpWidget(const SizedBox.shrink());
  });

  testWidgets(
    'a lost event feed recovers the final answer without a socket restart',
    (t) async {
      final api = FakeBridgeApi()..readResult = runningHistory;
      await mount(t, api);
      api.readResult = completedHistory;
      await api.closeAppEventStream(service);
      await frames(t);
      expect(api.appIsConnected(service), isTrue);
      expect(api.threadReads.length, 2);
      expect(find.text('The completed answer'), findsOneWidget);
      expect(find.byKey(const Key('stop-btn')), findsNothing);
      expect(t.widget<TurnWorkCard>(find.byType(TurnWorkCard)).active, isFalse);
      expect(t.takeException(), isNull);
      await t.pumpWidget(const SizedBox.shrink());
    },
  );

  testWidgets('a second event gap during recovery is retried', (t) async {
    final api = FakeBridgeApi()..readResult = runningHistory;
    await mount(t, api);
    final read = Completer<ThreadHistory>();
    api.pendingReads[thread] = [read.future];
    await api.closeAppEventStream(service);
    await frames(t);
    expect(api.threadReads.length, 2);
    await api.closeAppEventStream(service);
    read.complete(runningHistory);
    api.readResult = completedHistory;
    await frames(t);
    await t.pump(const Duration(seconds: 1));
    await frames(t);
    expect(api.appIsConnected(service), isTrue);
    expect(api.threadReads.length, 3);
    expect(find.text('The completed answer'), findsOneWidget);
    expect(find.byKey(const Key('stop-btn')), findsNothing);
    expect(t.takeException(), isNull);
    await t.pumpWidget(const SizedBox.shrink());
  });

  for (final completionDuringRead in [false, true]) {
    testWidgets(
      'monitor completion refreshes the same revision (in-flight: $completionDuringRead)',
      (t) async {
        final api = FakeBridgeApi()
          ..metadataOnlyFollow = true
          ..appThreadResumeError = StateError(
            'thread already has an active writer',
          )
          ..readResult = runningHistory;
        api.liveness[thread] = liveness(true);
        await mount(t, api);
        await t.pump(const Duration(seconds: 1));
        await frames(t);
        final read = Completer<ThreadHistory>();
        api.pendingReads[thread] = [read.future];
        api.pushMetaSessionUpdate(
          thread,
          SessionFollowUpdate(
            liveness: liveness(true),
            items: const [],
            historyRevision: 'final-revision',
          ),
        );
        await t.pump();
        await t.pump(const Duration(seconds: 1));
        await frames(t);
        final reads = api.threadReads.length;
        if (!completionDuringRead) {
          read.complete(runningHistory);
          await frames(t);
        }
        api.readResult = completedHistory;
        final completed = SessionFollowUpdate(
          liveness: liveness(false),
          items: const [],
          historyRevision: 'final-revision',
        );
        api.pushMetaSessionUpdate(thread, completed);
        await t.pump();
        if (completionDuringRead) read.complete(runningHistory);
        await frames(t);
        await t.pump(const Duration(seconds: 1));
        await frames(t);
        expect(api.threadReads.length, reads + 1);
        expect(api.threadReadIncludesPages.last, isFalse);
        expect(find.text('The completed answer'), findsOneWidget);
        expect(
          t.widget<TurnWorkCard>(find.byType(TurnWorkCard)).active,
          isFalse,
        );
        api.pushMetaSessionUpdate(thread, completed);
        await t.pump(const Duration(seconds: 2));
        await frames(t);
        expect(
          api.threadReads.length,
          reads + 1,
          reason: 'unchanged heartbeats must stay quiet',
        );
        expect(t.takeException(), isNull);
        await t.pumpWidget(const SizedBox.shrink());
      },
    );
  }
}
