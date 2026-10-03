// The shared session UI on an ACP service (TRD §4.7.5): controls gated by
// capability, the generic option panel, slash commands, the agent's own
// approval options, URL cards, hub banners, older-history and reload notices,
// and failed queued prompts.

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';

import 'fake_bridge_api.dart';
import 'support/screen_harness.dart';

const acp = 'pcx:dev:acp:claude';
const thread = 'ses-1';

class AcpApi extends FakeBridgeApi {
  int gitDiffCalls = 0;
  final List<String> steers = [];

  @override
  Future<String> appGitDiff(String serviceKey, String threadId) {
    gitDiffCalls++;
    return super.appGitDiff(serviceKey, threadId);
  }
}

Future<void> frames(WidgetTester t) async {
  for (var i = 0; i < 8; i++) {
    await t.pump(const Duration(milliseconds: 50));
  }
}

Future<AcpApi> open(
  WidgetTester t, {
  AcpApi? api,
  String? threadId = thread,
  ThreadHistory? history,
}) async {
  final a = api ?? AcpApi();
  if (history != null) a.readResult = history;
  t.view.devicePixelRatio = 1.0;
  t.view.physicalSize = const Size(1200, 900);
  addTearDown(t.view.reset);
  await a.appConnect(acp, 28080);
  await t.pumpWidget(
    host(
      AppSessionScreen(serviceKey: acp, threadId: threadId),
      a,
      locale: const Locale('en'),
    ),
  );
  await frames(t);
  return a;
}

const _history = ThreadHistory(
  running: false,
  items: [
    ThreadItem(
      id: 'u1',
      itemType: 'userMessage',
      title: '',
      text: 'hello',
      turnId: 't1',
    ),
    ThreadItem(
      id: 'a1',
      itemType: 'agentMessage',
      title: '',
      text: 'hi there',
      turnId: 't1',
    ),
  ],
  cwd: '/w',
);

AppEvent _event(String kind, String raw, {String? requestId, String? tid}) =>
    AppEvent(kind: kind, threadId: tid, requestId: requestId, raw: raw);

void main() {
  setUp(AppSessionScreen.debugResetThreadMemory);

  testWidgets('steer, rename, compact, diff, Fast and presets are hidden', (
    t,
  ) async {
    final api = await open(t, history: _history);
    expect(find.byKey(const Key('permission-chip')), findsNothing);
    expect(find.byKey(const Key('fast-mode-btn')), findsNothing);
    expect(find.byTooltip('More actions'), findsNothing);
    expect(api.gitDiffCalls, 0);
    // A running turn offers no "supplement" toggle.
    api.pushEvent(
      acp,
      _event('turn/started', '{"threadId":"ses-1","turnId":"t2"}', tid: thread),
    );
    await frames(t);
    expect(find.byKey(const Key('stop-btn')), findsOneWidget);
    expect(find.byKey(const Key('supplement-toggle')), findsNothing);
  });

  testWidgets('Plan and the option panel follow capabilities', (t) async {
    final api = AcpApi();
    api.acpConfig[thread] = const [
      AcpConfigOption(
        id: 'model',
        name: 'Model',
        role: 'model',
        kind: 'select',
        currentValue: 'm1',
        options: [AcpConfigValue(value: 'm1', name: 'One')],
      ),
      AcpConfigOption(
        id: 'mode',
        name: 'Mode',
        role: 'mode',
        kind: 'select',
        currentValue: 'code',
        options: [
          AcpConfigValue(value: 'code', name: 'Code'),
          AcpConfigValue(value: 'ask', name: 'Ask'),
        ],
      ),
      AcpConfigOption(
        id: 'web',
        name: 'Web search',
        role: 'other',
        kind: 'boolean',
        currentValue: 'false',
      ),
    ];
    // The popover is the desktop treatment.
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    addTearDown(() => debugDefaultTargetPlatformOverride = null);
    await open(t, api: api, history: _history);
    await t.tap(find.byKey(const Key('model-chip')));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('advanced-turn-settings')), findsOneWidget);
    expect(find.byKey(const Key('acp-config-panel')), findsOneWidget);
    expect(find.byKey(const Key('acp-config-model')), findsNothing);
    await t.tap(find.byKey(const Key('acp-config-web')));
    await t.pumpAndSettle();
    expect(api.acpConfigSets, [(thread, 'web', 'true')]);
    await t.tap(find.byKey(const Key('acp-config-mode')));
    await t.pumpAndSettle();
    await t.tap(find.text('Ask').last);
    await t.pumpAndSettle();
    expect(api.acpConfigSets.last, (thread, 'mode', 'ask'));
    debugDefaultTargetPlatformOverride = null;
  });

  testWidgets('a phone reaches the agent options from the config sheet', (
    t,
  ) async {
    final api = AcpApi();
    api.acpConfig[thread] = const [
      AcpConfigOption(
        id: 'web',
        name: 'Web search',
        role: 'other',
        kind: 'boolean',
        currentValue: 'true',
      ),
    ];
    await open(t, api: api, history: _history);
    await t.tap(find.byKey(const Key('model-chip')));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('opt-advanced')), findsOneWidget);
    await t.tap(find.byKey(const Key('opt-acp-options')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('acp-config-web')));
    await t.pumpAndSettle();
    expect(api.acpConfigSets, [(thread, 'web', 'false')]);
  });

  testWidgets('without plan mode the advanced entry is gone', (t) async {
    final caps = AcpApi().acpCaps;
    final api = AcpApi()
      ..acpCaps = AppCapabilities(
        provider: 'acp',
        fast: false,
        permissionPresets: false,
        guardian: false,
        rateLimits: false,
        takeover: false,
        externalWriterMonitor: false,
        localSessions: false,
        planMode: false,
        effortLabel: 'effort',
        approveAlwaysPersistsProject: false,
        multiSelectQuestions: true,
        childSessions: false,
        agentName: caps.agentName,
        approvalOptions: true,
        urlElicitation: true,
        runningViaThreads: true,
        historyPrefetch: true,
      );
    await open(t, api: api, history: _history);
    await t.tap(find.byKey(const Key('model-chip')));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('opt-advanced')), findsNothing);
    expect(find.byKey(const Key('opt-acp-options')), findsNothing);
  });

  testWidgets('the slash menu completes an agent command', (t) async {
    final api = AcpApi();
    api.acpCommands[thread] = const [
      AcpCommand(name: 'review', description: 'Review changes'),
      AcpCommand(name: 'init', description: 'Create AGENTS.md'),
    ];
    await open(t, api: api, history: _history);
    await t.enterText(find.byKey(const Key('composer-input')), '/re');
    await frames(t);
    expect(find.byKey(const Key('acp-slash-menu')), findsOneWidget);
    expect(find.byKey(const Key('acp-slash-init')), findsNothing);
    await t.tap(find.byKey(const Key('acp-slash-review')));
    await frames(t);
    final input = t.widget<TextField>(find.byKey(const Key('composer-input')));
    expect(input.controller!.text, '/review ');
    expect(find.byKey(const Key('acp-slash-menu')), findsNothing);
  });

  testWidgets('the agent options render as buttons and confirm "always"', (
    t,
  ) async {
    final api = await open(t, history: _history);
    api.pushEvent(
      acp,
      _event(
        'item/commandExecution/requestApproval',
        '{"threadId":"ses-1","command":"Run ls","acpOptions":['
            '{"optionId":"once","name":"Allow","kind":"allow_once"},'
            '{"optionId":"always","name":"Always allow","kind":"allow_always"},'
            '{"optionId":"no","name":"Reject","kind":"reject_once"}]}',
        requestId: 'r1',
        tid: thread,
      ),
    );
    await frames(t);
    expect(find.byKey(const Key('approve-btn')), findsNothing);
    expect(
      t.widget(find.byKey(const Key('acp-option-once'))),
      isA<FilledButton>(),
    );
    expect(t.widget(find.byKey(const Key('acp-option-no'))), isA<TextButton>());
    await t.tap(find.byKey(const Key('acp-option-always')));
    await frames(t);
    expect(find.byKey(const Key('acp-option-confirm')), findsOneWidget);
    await t.tap(find.byKey(const Key('acp-option-confirm-ok')));
    await frames(t);
    expect(api.acpAnswers, [('option', 'r1', 'always')]);
    expect(find.byKey(const Key('approval-card')), findsNothing);
  });

  testWidgets('a URL card shows the host and answers decline', (t) async {
    final api = await open(t, history: _history);
    // A hub-level elicitation has no thread and shows in any open session.
    api.pushEvent(
      acp,
      _event(
        'acp/elicitation/url',
        '{"message":"Sign in to continue","url":"https://login.example.com/device","host":"login.example.com"}',
        requestId: 'r7',
      ),
    );
    await frames(t);
    expect(find.byKey(const Key('acp-url-card')), findsOneWidget);
    expect(find.text('login.example.com'), findsOneWidget);
    expect(find.text('Sign in to continue'), findsOneWidget);
    await t.tap(find.byKey(const Key('acp-url-decline')));
    await frames(t);
    expect(api.acpAnswers, [('url', 'r7', 'false')]);
    expect(find.byKey(const Key('acp-url-card')), findsNothing);
  });

  testWidgets('the login banner starts an agent login on the host', (t) async {
    final api = await open(t, history: _history);
    api.pushEvent(
      acp,
      _event(
        'acp/hub/state',
        '{"auth":{"status":"required","methods":['
            '{"id":"login","name":"Log in with Claude","kind":"agent","remote":true},'
            '{"id":"term","name":"Terminal","kind":"terminal","remote":false}]},'
            '"process":{"state":"ready"}}',
      ),
    );
    await frames(t);
    expect(find.byKey(const Key('acp-auth-banner')), findsOneWidget);
    expect(
      find.textContaining('Finish this login on the host'),
      findsOneWidget,
    );
    expect(find.byKey(const Key('acp-banner-login-term')), findsNothing);
    await t.tap(find.byKey(const Key('acp-banner-login-login')));
    await frames(t);
    expect(api.acpAuthCalls, [('session', acp, 'login')]);
    expect(find.byKey(const Key('acp-auth-banner')), findsNothing);
  });

  testWidgets('a restarting agent shows the process banner', (t) async {
    final api = await open(t, history: _history);
    api.pushEvent(
      acp,
      _event(
        'acp/hub/state',
        '{"auth":{"status":"ok","methods":[]},'
            '"process":{"state":"restarting","attempt":1,"retryInMs":1000}}',
      ),
    );
    await frames(t);
    expect(find.byKey(const Key('acp-process-banner')), findsOneWidget);
    api.pushEvent(
      acp,
      _event(
        'acp/hub/state',
        '{"auth":{"status":"ok","methods":[]},"process":{"state":"ready"}}',
      ),
    );
    await frames(t);
    expect(find.byKey(const Key('acp-process-banner')), findsNothing);
  });

  testWidgets('a new conversation re-reads models once the hub is ready', (
    t,
  ) async {
    final api = AcpApi()..emptyModelList = true;
    await open(t, api: api, threadId: null);
    final before = api.modelListCalls;
    expect(before, greaterThan(0));
    api.emptyModelList = false;
    api.pushEvent(
      acp,
      _event(
        'acp/hub/state',
        '{"auth":{"status":"ok","methods":[]},"process":{"state":"starting"}}',
      ),
    );
    await frames(t);
    expect(api.modelListCalls, before);
    api.pushEvent(
      acp,
      _event(
        'acp/hub/state',
        '{"auth":{"status":"ok","methods":[]},"process":{"state":"ready"}}',
      ),
    );
    await frames(t);
    expect(api.modelListCalls, before + 1);
    // Known models are not re-read on later hub states.
    api.pushEvent(
      acp,
      _event(
        'acp/hub/state',
        '{"auth":{"status":"ok","methods":[]},"process":{"state":"ready"}}',
      ),
    );
    await frames(t);
    expect(api.modelListCalls, before + 1);
  });

  testWidgets('unavailable older history is announced at the top', (t) async {
    await open(
      t,
      history: const ThreadHistory(
        running: false,
        items: [
          ThreadItem(
            id: 'a9',
            itemType: 'agentMessage',
            title: '',
            text: 'latest',
            turnId: 't9',
          ),
        ],
        olderUnavailable: true,
      ),
    );
    expect(find.byKey(const Key('acp-older-unavailable')), findsOneWidget);
  });

  testWidgets('a change elsewhere offers a reload chip', (t) async {
    final api = await open(t, history: _history);
    final reads = api.threadReads.length;
    api.pushEvent(
      acp,
      _event('acp/session/changed', '{"threadId":"ses-1"}', tid: thread),
    );
    await frames(t);
    expect(find.byKey(const Key('acp-reload-chip')), findsOneWidget);
    await t.tap(find.byKey(const Key('acp-reload-chip')));
    await frames(t);
    expect(api.acpReloads, [thread]);
    expect(api.threadReads.length, greaterThan(reads));
    expect(find.byKey(const Key('acp-reload-chip')), findsNothing);
    // A new generation re-reads directly.
    final before = api.threadReads.length;
    api.pushEvent(
      acp,
      _event('acp/session/generation', '{"threadId":"ses-1"}', tid: thread),
    );
    await frames(t);
    expect(api.threadReads.length, greaterThan(before));
  });

  testWidgets('failed queued prompts raise a SnackBar with copy', (t) async {
    final api = await open(t, history: _history);
    api.pushEvent(
      acp,
      _event(
        'acp/queue/failed',
        '{"threadId":"ses-1","reason":"agent_exited","prompts":'
            '[{"submissionId":"q1","text":"one"},{"submissionId":"q2","text":"two"}]}',
        tid: thread,
      ),
    );
    await frames(t);
    expect(find.byKey(const Key('acp-queue-failed')), findsOneWidget);
    expect(find.text('2 queued messages were not sent'), findsOneWidget);
    expect(find.widgetWithText(SnackBarAction, 'Copy'), findsOneWidget);
  });
}
