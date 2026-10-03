// ACP hosting in the local host dialog (TRD §4.7.2): the agent list, install
// progress, starting and remembering the host, login, stopping, and the
// details of an existing ACP host.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/widgets/local_host_dialog.dart';

import 'fake_bridge_api.dart';

const _installed = AcpAgent(
  id: 'claude-acp',
  name: 'Claude Code',
  pinnedVersion: '0.84.0',
  installedVersion: '0.84.0',
  state: 'installed',
  approxSizeMb: 320,
  needsNode: true,
);

const _missing = AcpAgent(
  id: 'codex-acp',
  name: 'Codex (ACP)',
  pinnedVersion: '2.0.1',
  state: 'not_installed',
  approxSizeMb: 60,
);

Future<ProviderContainer> _pumpDialog(
  WidgetTester t,
  FakeBridgeApi api, {
  AppServeStatus? existing,
}) async {
  final container = ProviderContainer(
    overrides: [bridgeApiProvider.overrideWithValue(api)],
  );
  addTearDown(container.dispose);
  await t.pumpWidget(
    UncontrolledProviderScope(
      container: container,
      child: MaterialApp(
        locale: const Locale('en'),
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: Builder(
          builder: (context) => Scaffold(
            body: TextButton(
              onPressed: () => showDialog<void>(
                context: context,
                builder: (_) => LocalHostDialog(existing: existing),
              ),
              child: const Text('open'),
            ),
          ),
        ),
      ),
    ),
  );
  await t.tap(find.text('open'));
  await t.pumpAndSettle();
  return container;
}

Future<void> _chooseAcp(WidgetTester t) async {
  await t.tap(find.byKey(const Key('provider-acp')));
  await t.pumpAndSettle();
}

FilledButton _startButton(WidgetTester t) =>
    t.widget<FilledButton>(find.byKey(const Key('start-hosting-btn')));

void main() {
  testWidgets('choosing ACP lists the agents with their state', (t) async {
    final api = FakeBridgeApi()..acpAgentList = const [_missing, _installed];
    await _pumpDialog(t, api);
    await _chooseAcp(t);
    expect(find.byKey(const Key('acp-agent-picker')), findsOneWidget);
    // The installed agent is preselected and its name fills the instance name.
    expect(find.text('Claude Code · Installed'), findsOneWidget);
    final name = t.widget<TextField>(find.byKey(const Key('acp-name')));
    expect(name.controller!.text, 'claude-acp');
    expect(find.text('Installed 0.84.0'), findsOneWidget);
    expect(find.text('About 320 MB'), findsOneWidget);
    expect(_startButton(t).onPressed, isNotNull);
    expect(find.text('Port'), findsNothing);

    await t.tap(find.byKey(const Key('acp-agent-picker')));
    await t.pumpAndSettle();
    await t.tap(find.text('Codex (ACP) · Not installed').last);
    await t.pumpAndSettle();
    expect(_startButton(t).onPressed, isNull, reason: 'install first');
    expect(find.byKey(const Key('acp-install-btn')), findsOneWidget);
  });

  testWidgets('install shows progress, then the failure reason', (t) async {
    final api = FakeBridgeApi()..acpAgentList = const [_missing];
    await _pumpDialog(t, api);
    await _chooseAcp(t);
    await t.tap(find.byKey(const Key('acp-install-btn')));
    await t.pump();
    expect(api.acpInstallCalls, [(null, 'codex-acp')]);
    api.acpJobs['job-1'] = const AcpJob(
      id: 'job-1',
      kind: 'install',
      agentId: 'codex-acp',
      state: 'downloading',
      bytes: 50,
      total: 100,
    );
    await t.pump(const Duration(milliseconds: 600));
    final bar = t.widget<LinearProgressIndicator>(
      find.byKey(const Key('acp-install-progress')),
    );
    expect(bar.value, 0.5);
    api.acpJobs['job-1'] = const AcpJob(
      id: 'job-1',
      kind: 'install',
      agentId: 'codex-acp',
      state: 'failed',
      message: 'sha mismatch',
      errorCode: 'acp.integrity_mismatch',
    );
    await t.pump(const Duration(milliseconds: 600));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('acp-install-progress')), findsNothing);
    expect(find.text('Install failed: sha mismatch'), findsOneWidget);
  });

  testWidgets('starting remembers autoHostAcp and closes', (t) async {
    final api = FakeBridgeApi()..acpAgentList = const [_installed];
    final c = await _pumpDialog(t, api);
    await _chooseAcp(t);
    await t.enterText(find.byKey(const Key('acp-name')), 'work');
    await t.tap(find.byKey(const Key('start-hosting-btn')));
    await t.pumpAndSettle();
    expect(api.acpServeCalls, [('work', 'claude-acp')]);
    expect(find.byType(LocalHostDialog), findsNothing);
    final saved = c.read(uiPrefsProvider).valueOrNull!.autoHostAcp;
    expect(saved.map((p) => (p.name, p.agentId)), [('work', 'claude-acp')]);
    expect(api.serveHosts.single.isAcp, isTrue);
  });

  testWidgets('a login requirement keeps the dialog on the login section', (
    t,
  ) async {
    final api = FakeBridgeApi()
      ..acpAgentList = const [_installed]
      ..acpAuth = const AcpAuth(
        status: 'required',
        methods: [
          AcpAuthMethod(
            id: 'login',
            name: 'Log in',
            kind: 'agent',
            remote: true,
          ),
          AcpAuthMethod(id: 'console', name: 'Console login', kind: 'terminal'),
          AcpAuthMethod(
            id: 'legacy',
            name: 'Other CLI',
            kind: 'terminal',
            available: false,
          ),
        ],
      );
    await _pumpDialog(t, api);
    await _chooseAcp(t);
    await t.tap(find.byKey(const Key('start-hosting-btn')));
    await t.pumpAndSettle();
    expect(find.byType(LocalHostDialog), findsOneWidget);
    expect(find.byKey(const Key('acp-auth-required')), findsOneWidget);
    expect(find.byKey(const Key('stop-hosting-btn')), findsOneWidget);
    await t.tap(find.byKey(const Key('acp-login-login')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('acp-login-console')));
    await t.pumpAndSettle();
    final legacy = t.widget<OutlinedButton>(
      find.byKey(const Key('acp-login-legacy')),
    );
    expect(legacy.onPressed, isNull, reason: 'cannot be reproduced safely');
    await t.tap(find.byKey(const Key('acp-recheck-btn')));
    await t.pumpAndSettle();
    expect(api.acpAuthCalls, [
      ('agent', 'claude-acp', 'login'),
      ('terminal', 'claude-acp', 'console'),
      ('recheck', 'claude-acp', null),
    ]);
  });

  testWidgets('a configured gateway folds the other logins away', (t) async {
    final api = FakeBridgeApi()
      ..acpAgentList = const [_installed]
      ..acpAuth = const AcpAuth(
        status: 'required',
        methods: [
          AcpAuthMethod(
            id: 'gateway',
            name: 'Custom model gateway',
            kind: 'gateway',
            gatewayProtocol: 'anthropic',
            gatewayConfigured: true,
          ),
          AcpAuthMethod(id: 'login', name: 'Log in', kind: 'agent'),
        ],
      )
      ..acpSettingsValue = const AcpSettings(
        gateways: [
          AcpGateway(
            agentId: 'claude-acp',
            baseUrl: 'https://relay.example.com',
            hasToken: true,
          ),
        ],
      );
    await _pumpDialog(t, api);
    await _chooseAcp(t);
    await t.tap(find.byKey(const Key('start-hosting-btn')));
    await t.pumpAndSettle();
    expect(
      find.text('Model gateway configured: https://relay.example.com'),
      findsOneWidget,
    );
    expect(find.byKey(const Key('acp-other-logins')), findsOneWidget);
    expect(find.byKey(const Key('acp-login-login')), findsNothing);
  });

  testWidgets('stopping with a running conversation asks first', (t) async {
    final api = FakeBridgeApi();
    final hosted = await api.appServeStartAcp(
      name: 'work',
      agentId: 'claude-acp',
    );
    final c = await _pumpDialog(t, api, existing: api.serveHosts.single);
    c
        .read(uiPrefsProvider.notifier)
        .setAutoHostAcp(
          const AutoHostAcpPrefs(name: 'work', agentId: 'claude-acp'),
        );
    api.runningThreads[hosted.serviceKey] = ['s1'];
    await t.tap(find.byKey(const Key('stop-hosting-btn')));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('acp-stop-confirm')), findsOneWidget);
    await t.tap(find.text('Cancel').last);
    await t.pumpAndSettle();
    expect(api.serveHosts, hasLength(1));
    await t.tap(find.byKey(const Key('stop-hosting-btn')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('acp-stop-confirm-ok')));
    await t.pumpAndSettle();
    expect(api.serveHosts, isEmpty);
    expect(find.byType(LocalHostDialog), findsNothing);
    expect(c.read(uiPrefsProvider).valueOrNull!.autoHostAcp, isEmpty);
    expect(c.read(pendingRemovalProvider), contains(hosted.serviceKey));
  });

  testWidgets('an existing ACP host shows agent, version and keys, no API', (
    t,
  ) async {
    final api = FakeBridgeApi();
    await api.appServeStartAcp(name: 'work', agentId: 'claude-acp');
    final host = api.serveHosts.single;
    final unpinned = AppServeStatus(
      name: host.name,
      device: host.device,
      alive: true,
      appListenAddr: host.appListenAddr,
      appServiceKey: host.appServiceKey,
      metaListenAddr: host.metaListenAddr,
      metaServiceKey: host.metaServiceKey,
      provider: 'acp',
      providerVersion: '0.90.0',
      agentId: 'claude-acp',
      agentName: 'Claude Code',
    );
    await _pumpDialog(t, api, existing: unpinned);
    expect(find.text('claude-acp · 0.90.0'), findsOneWidget);
    expect(find.byKey(const Key('acp-unpinned')), findsOneWidget);
    expect(find.text('pcx:local:acp:work'), findsOneWidget);
    expect(find.text('pcx:local:meta:work'), findsOneWidget);
    expect(find.text('Claude Code'), findsWidgets);
    expect(find.text('API'), findsNothing);
    expect(find.byKey(const Key('acp-recheck-btn')), findsOneWidget);
  });
}
