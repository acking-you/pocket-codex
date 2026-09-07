import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';

import 'fake_bridge_api.dart';
import 'support/screen_harness.dart';

const service = 'pcx:host:app:default';
const questions = [
  {
    'title': 'Which platform?',
    'options': ['macOS', 'Windows'],
  },
];

Future<FakeBridgeApi> mount(
  WidgetTester tester, {
  ThreadHistory? history,
  ThreadConfig? config,
}) async {
  final api = FakeBridgeApi(
    config: const ConfigInfo(relay: 'localhost:7666', hasKey: true),
  );
  if (history != null) api.readResult = history;
  if (config != null) api.threadConfigs['thread-1'] = config;
  await api.appConnect(service, 28080);
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = const Size(400, 900);
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    host(
      const AppSessionScreen(serviceKey: service, threadId: 'thread-1'),
      api,
    ),
  );
  await tester.pumpAndSettle();
  return api;
}

void ask(FakeBridgeApi api) => api.pushEvent(
  service,
  AppEvent(
    kind: 'item/completed',
    threadId: 'thread-1',
    itemId: 'question-1',
    itemType: 'agentMessage',
    raw: jsonEncode({
      'item': {
        'id': 'question-1',
        'type': 'agentMessage',
        'text': '',
        'questions': questions,
      },
    }),
  ),
);

void main() {
  setUp(AppSessionScreen.debugResetThreadMemory);

  testWidgets('async answer steers its running turn and preserves the draft', (
    t,
  ) async {
    final api = await mount(t);
    await t.enterText(find.byType(TextField), 'unfinished draft');
    api.pushEvent(
      service,
      const AppEvent(
        kind: 'turn/started',
        threadId: 'thread-1',
        raw: '{"turn":{"id":"turn-1"}}',
      ),
    );
    ask(api);
    ask(api);
    await t.pump();
    await t.pump(const Duration(milliseconds: 50));
    expect(find.byKey(const Key('user-input-card')), findsOneWidget);
    expect(
      t
          .widget<FilledButton>(find.byKey(const Key('user-input-submit')))
          .onPressed,
      isNull,
    );
    await t.tap(find.text('macOS'));
    await t.pump();
    await t.tap(find.byKey(const Key('user-input-submit')));
    await t.pump();
    expect(api.lastSteerText, 'Which platform?\nmacOS');
    expect(api.lastSteerTurnId, 'turn-1');
    expect(api.turnStartCount, 0);
    expect(find.byKey(const Key('user-input-card')), findsNothing);
    expect(find.text('unfinished draft'), findsOneWidget);
    await t.pumpWidget(const SizedBox());
  });

  testWidgets('idle async answer starts a turn with free text', (t) async {
    final api = await mount(t);
    ask(api);
    await t.pumpAndSettle();
    await t.tap(find.text('其他…'));
    await t.pumpAndSettle();
    await t.enterText(
      find.descendant(
        of: find.byKey(const Key('user-input-card')),
        matching: find.byType(TextField),
      ),
      'Linux',
    );
    await t.pump();
    await t.tap(find.byKey(const Key('user-input-submit')));
    await t.pumpAndSettle();
    expect(api.lastTurnText, 'Which platform?\nLinux');
    expect(api.turnStartCount, 1);
    expect(api.lastSteerText, isNull);
    expect(find.byKey(const Key('user-input-card')), findsNothing);
  });

  testWidgets('a stale turn keeps the async answer editable for retry', (
    t,
  ) async {
    final api = await mount(t);
    api.steerError = 'expectedTurnId does not match';
    api.pushEvent(
      service,
      const AppEvent(
        kind: 'turn/started',
        threadId: 'thread-1',
        raw: '{"turn":{"id":"stale"}}',
      ),
    );
    ask(api);
    await t.pump();
    await t.pump(const Duration(milliseconds: 50));
    await t.tap(find.text('Windows'));
    await t.pump();
    await t.tap(find.byKey(const Key('user-input-submit')));
    await t.pump();
    expect(find.byKey(const Key('user-input-card')), findsOneWidget);
    expect(
      t
          .widget<FilledButton>(find.byKey(const Key('user-input-submit')))
          .onPressed,
      isNotNull,
    );
    expect(api.turnStartCount, 0);
    expect(api.lastSteerText, isNull);
    await t.pumpWidget(const SizedBox());
  });
  testWidgets('buffered live questions survive a history reload', (t) async {
    await mount(
      t,
      history: ThreadHistory(
        items: [
          ThreadItem(
            id: 'question-1',
            itemType: 'agentMessage',
            title: '',
            text: '',
            questionsJson: jsonEncode(questions),
          ),
        ],
        running: false,
      ),
    );
    expect(find.text('Which platform?'), findsOneWidget);
    expect(find.text('macOS'), findsOneWidget);
  });

  testWidgets(
    'answered buffered questions stay hidden after screen navigation',
    (t) async {
      final api = await mount(
        t,
        history: ThreadHistory(
          items: [
            ThreadItem(
              id: 'question-1',
              itemType: 'agentMessage',
              title: '',
              text: '',
              questionsJson: jsonEncode(questions),
            ),
          ],
          running: false,
        ),
      );
      await t.tap(find.text('macOS'));
      await t.pump();
      await t.tap(find.byKey(const Key('user-input-submit')));
      await t.pumpAndSettle();
      expect(api.turnStartCount, 1);
      await t.pumpWidget(host(const SizedBox(), api));
      await t.pumpAndSettle();
      await t.pumpWidget(
        host(
          const AppSessionScreen(serviceKey: service, threadId: 'thread-1'),
          api,
        ),
      );
      await t.pumpAndSettle();
      expect(find.byKey(const Key('user-input-card')), findsNothing);
    },
  );

  testWidgets(
    'a cleared server effort does not restore stale persisted effort',
    (t) async {
      final api = await mount(
        t,
        history: const ThreadHistory(
          items: [],
          running: false,
          model: 'gpt-5.5',
        ),
        config: const ThreadConfig(reasoningEffort: 'high'),
      );
      await t.enterText(find.byType(TextField), 'continue');
      await t.pump();
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pumpAndSettle();
      expect(api.lastReasoningEffort, isNull);
    },
  );
}
