import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/app_session/activity_cards.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';

import 'fake_bridge_api.dart';
import 'support/screen_harness.dart';

const openCode = 'pcx:dev:opencode:default';
const codex = 'pcx:dev:app:default';
const fastModel = ModelInfo(
  id: 'fast-capable',
  displayName: 'Fast capable',
  description: '',
  supportedServiceTiers: ['priority'],
  isDefault: true,
);

class OpenCodeApi extends FakeBridgeApi {
  final Map<String, ThreadHistory> histories = {};
  int metaSessionCalls = 0;
  int prefetchCalls = 0;
  int runningThreadCalls = 0;

  @override
  Future<List<ModelInfo>> appModelList(String serviceKey) async => [fastModel];

  @override
  Future<ThreadHistory> appThreadRead(
    String serviceKey,
    String threadId, {
    bool includeTurnPages = true,
  }) async {
    threadReads.add(threadId);
    return histories[threadId] ?? readResult;
  }

  @override
  Future<List<LocalSession>> metaSessions(
    String serviceKey, {
    bool runningOnly = false,
  }) {
    metaSessionCalls++;
    return super.metaSessions(serviceKey, runningOnly: runningOnly);
  }

  @override
  Future<void> appHistoryPrefetch(String serviceKey, String threadId) async =>
      prefetchCalls++;

  @override
  Future<List<String>> appRunningThreads(String serviceKey) {
    runningThreadCalls++;
    return super.appRunningThreads(serviceKey);
  }
}

Future<void> frames(WidgetTester t) async {
  for (var i = 0; i < 8; i++) {
    await t.pump(const Duration(milliseconds: 50));
  }
}

Future<void> open(
  WidgetTester t,
  OpenCodeApi api,
  String key, {
  String? thread,
}) async {
  await api.appConnect(key, 28080);
  await t.pumpWidget(
    host(AppSessionScreen(serviceKey: key, threadId: thread), api),
  );
  await frames(t);
}

AppEvent request(String id, String raw, {String kind = approvalKind}) =>
    AppEvent(kind: kind, threadId: 'ses-1', requestId: id, raw: raw);

const approvalKind = 'item/commandExecution/requestApproval';
const inputKind = 'item/tool/requestUserInput';

void main() {
  setUp(AppSessionScreen.debugResetThreadMemory);

  testWidgets('OpenCode hides Fast and permission presets and sends none', (
    t,
  ) async {
    final api = OpenCodeApi();
    await open(t, api, openCode);
    expect(find.byKey(const Key('permission-chip')), findsNothing);
    expect(find.byKey(const Key('fast-mode-btn')), findsNothing);
    await t.enterText(find.byKey(const Key('composer-input')), 'hi');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await frames(t);
    expect(api.lastTurnText, 'hi');
    expect(api.lastApproval, isNull);
    expect(api.lastSandbox, isNull);
    expect(api.lastApprovalsReviewer, isNull);
    expect(api.lastServiceTier, isNull);
  });

  testWidgets('Codex keeps Fast and permission presets', (t) async {
    final api = OpenCodeApi();
    await open(t, api, codex);
    expect(find.byKey(const Key('permission-chip')), findsOneWidget);
    expect(find.byKey(const Key('fast-mode-btn')), findsOneWidget);
  });

  testWidgets('project-wide always-allow asks before acceptForSession', (
    t,
  ) async {
    final api = OpenCodeApi();
    await open(t, api, openCode, thread: 'ses-1');
    api.pushEvent(
      openCode,
      request('r1', '{"command":"ls","persistsProject":true,"save":"ls *"}'),
    );
    await frames(t);
    expect(find.text('始终允许（项目）'), findsOneWidget);
    await t.tap(find.byKey(const Key('approve-session-btn')));
    await frames(t);
    expect(find.byType(AlertDialog), findsOneWidget);
    expect(api.lastApprovalDecision, isNull);
    await t.tap(find.byKey(const Key('approve-always-confirm')));
    await frames(t);
    expect(api.lastApprovalDecision, 'acceptForSession');
    expect(find.byKey(const Key('approval-card')), findsNothing);
  });

  testWidgets('multi-select answers send every chosen label', (t) async {
    final api = OpenCodeApi();
    await open(t, api, openCode, thread: 'ses-1');
    api.pushEvent(
      openCode,
      request(
        'q1',
        '{"questions":[{"id":"langs","question":"Pick","multiSelect":true,'
            '"options":[{"label":"Rust"},{"label":"Dart"},{"label":"Go"}]}]}',
        kind: inputKind,
      ),
    );
    await frames(t);
    await t.tap(find.widgetWithText(FilterChip, 'Rust'));
    await t.pump();
    await t.tap(find.widgetWithText(FilterChip, 'Go'));
    await t.pump();
    await t.tap(find.byKey(const Key('user-input-submit')));
    await frames(t);
    expect(api.lastUserInputAnswers, '{"langs":["Rust","Go"]}');
  });

  testWidgets('a form with an unsupported field can only be cancelled', (
    t,
  ) async {
    final api = OpenCodeApi();
    await open(t, api, openCode, thread: 'ses-1');
    api.pushEvent(
      openCode,
      request(
        'q2',
        '{"questions":[{"id":"name","question":"Name","options":[]},'
            '{"id":"__unsupported","unsupported":true,'
            '"question":"Finish this form in OpenCode."}]}',
        kind: inputKind,
      ),
    );
    await frames(t);
    expect(find.text('Finish this form in OpenCode.'), findsOneWidget);
    // The answerable field still takes input, but the form cannot be sent.
    await t.enterText(find.byType(TextField).last, 'Ada');
    await t.pump();
    final submit = t.widget<FilledButton>(
      find.byKey(const Key('user-input-submit')),
    );
    expect(submit.onPressed, isNull);
    await t.tap(find.widgetWithText(TextButton, '取消'));
    await frames(t);
    expect(api.lastUserInputAnswers, '{}');
  });

  testWidgets('serverRequest/resolved removes a card; replays dedupe', (
    t,
  ) async {
    final api = OpenCodeApi();
    await open(t, api, openCode, thread: 'ses-1');
    api.pushEvent(openCode, request('r2', '{"command":"ls"}'));
    api.pushEvent(openCode, request('r2', '{"command":"ls"}'));
    await frames(t);
    expect(find.byKey(const Key('approval-card')), findsOneWidget);
    api.pushEvent(
      openCode,
      const AppEvent(
        kind: 'serverRequest/resolved',
        threadId: 'ses-1',
        raw: '{"threadId":"ses-1","requestId":"r2"}',
      ),
    );
    await frames(t);
    expect(find.byKey(const Key('approval-card')), findsNothing);
    expect(api.lastApprovalDecision, isNull);
  });

  testWidgets('OpenCode running sessions come from appRunningThreads', (
    t,
  ) async {
    final api = OpenCodeApi()..runningThreads[openCode] = ['ses-2'];
    await t.pumpWidget(
      ProviderScope(
        overrides: [bridgeApiProvider.overrideWithValue(api)],
        child: Consumer(
          builder: (context, ref, _) {
            ref.watch(runningThreadsProvider(openCode));
            return const SizedBox.shrink();
          },
        ),
      ),
    );
    final container = ProviderScope.containerOf(
      t.element(find.byType(Consumer)),
    );
    await frames(t);
    expect(container.read(runningThreadsProvider(openCode)).valueOrNull, {
      'ses-2',
    });
    expect(api.runningThreadCalls, 1);
    expect(api.metaSessionCalls, 0);
    expect(api.prefetchCalls, 0);
    await t.pump(const Duration(seconds: 5));
    expect(api.runningThreadCalls, 2);
    expect(api.metaSessionCalls, 0);
    await t.pumpWidget(const SizedBox.shrink());
    await t.pump(const Duration(seconds: 20));
  });

  testWidgets('a sub-session opens read-only and returns to its parent', (
    t,
  ) async {
    final api = OpenCodeApi();
    api.histories['ses-1'] = const ThreadHistory(
      running: false,
      items: [
        ThreadItem(
          id: 'u1',
          itemType: 'userMessage',
          title: '',
          text: 'delegate',
          turnId: 't1',
        ),
        ThreadItem(
          id: 'c1',
          itemType: 'collabAgentToolCall',
          title: 'task explore',
          text: '{"status":"completed","receiverThreadIds":["ses-child"]}',
          turnId: 't1',
        ),
        ThreadItem(
          id: 'a1',
          itemType: 'agentMessage',
          title: '',
          text: 'done',
          turnId: 't1',
        ),
      ],
    );
    api.histories['ses-child'] = const ThreadHistory(
      running: false,
      items: [
        ThreadItem(
          id: 'cu',
          itemType: 'userMessage',
          title: '',
          text: 'child prompt',
          turnId: 'ct',
        ),
      ],
    );
    await open(t, api, openCode, thread: 'ses-1');
    await t.tap(find.byType(TurnWorkCard));
    await frames(t);
    await t.tap(find.byKey(const Key('view-sub-session-ses-child')));
    await frames(t);
    expect(find.byKey(const Key('sub-session-banner')), findsOneWidget);
    expect(find.text('子会话 · 只读'), findsOneWidget);
    expect(find.byKey(const Key('composer-input')), findsNothing);
    expect(find.text('child prompt'), findsOneWidget);
    await t.tap(find.byKey(const Key('sub-session-back')));
    await frames(t);
    expect(find.byKey(const Key('sub-session-banner')), findsNothing);
    expect(find.byKey(const Key('composer-input')), findsOneWidget);
    expect(find.text('delegate'), findsOneWidget);
  });

  testWidgets('Codex tool cards offer no sub-session action', (t) async {
    final api = OpenCodeApi();
    api.readResult = const ThreadHistory(
      running: false,
      items: [
        ThreadItem(
          id: 'u1',
          itemType: 'userMessage',
          title: '',
          text: 'delegate',
          turnId: 't1',
        ),
        ThreadItem(
          id: 'c1',
          itemType: 'collabAgentToolCall',
          title: 'spawn',
          text: '{"receiverThreadIds":["child"]}',
          turnId: 't1',
        ),
      ],
    );
    await open(t, api, codex, thread: 'ses-1');
    await t.tap(find.byType(TurnWorkCard));
    await frames(t);
    expect(find.byKey(const Key('view-sub-session-child')), findsNothing);
  });
}
