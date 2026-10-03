// ACP agent management (TRD §4.7.3): local install / upgrade / uninstall,
// settings with the D16 and D19 confirmations, gateways, catalog switches and
// custom agents; and the remote mode through the host's meta service.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/screens/acp_agents_screen.dart';

import 'fake_bridge_api.dart';
import 'support/screen_harness.dart';

const _fresh = AcpAgent(
  id: 'codex-acp',
  name: 'Codex (ACP)',
  pinnedVersion: '2.0.1',
  state: 'not_installed',
  needsNode: true,
  remoteInstallAllowed: true,
);
const _outdated = AcpAgent(
  id: 'claude-acp',
  name: 'Claude Code',
  pinnedVersion: '0.84.0',
  installedVersion: '0.83.0',
  state: 'installed',
  needsNode: true,
  remoteInstallAllowed: true,
);
const _hosted = AcpAgent(
  id: 'opencode-acp',
  name: 'OpenCode 1.x',
  pinnedVersion: '1.18.33',
  installedVersion: '1.18.33',
  state: 'installed',
  hostedNames: ['oc'],
);
const _free = AcpAgent(
  id: 'opencode2-acp',
  name: 'OpenCode 2.x',
  pinnedVersion: '2.0.20',
  installedVersion: '2.0.20',
  state: 'installed',
  registryVersion: '2.1.0',
);

Future<void> _pump(WidgetTester t, FakeBridgeApi api, {String? svc}) async {
  t.view.devicePixelRatio = 1.0;
  t.view.physicalSize = const Size(1200, 2400);
  addTearDown(t.view.reset);
  await t.pumpWidget(
    host(AcpAgentsScreen(serviceKey: svc), api, locale: const Locale('en')),
  );
  await t.pumpAndSettle();
}

/// Tap [key]; with a job running the progress bar animates forever, so
/// [settle] = false only pumps a few frames.
Future<void> _tapKey(WidgetTester t, String key, {bool settle = true}) async {
  final finder = find.byKey(Key(key));
  await t.ensureVisible(finder);
  await (settle ? t.pumpAndSettle() : t.pump());
  await t.tap(finder);
  if (settle) {
    await t.pumpAndSettle();
  } else {
    for (var i = 0; i < 5; i++) {
      await t.pump(const Duration(milliseconds: 50));
    }
  }
}

/// Mark every fake job done and let the page's poll pick it up.
Future<void> _finishJobs(WidgetTester t, FakeBridgeApi api) async {
  for (final id in api.acpJobs.keys.toList()) {
    final job = api.acpJobs[id]!;
    api.acpJobs[id] = AcpJob(
      id: id,
      kind: job.kind,
      agentId: job.agentId,
      state: 'done',
    );
  }
  await t.pump(const Duration(milliseconds: 1100));
  await t.pumpAndSettle();
}

T _widget<T extends Widget>(WidgetTester t, String key) =>
    t.widget<T>(find.byKey(Key(key)));

void main() {
  testWidgets('local mode installs, upgrades and uninstalls', (t) async {
    final api = FakeBridgeApi()
      ..acpAgentList = const [_fresh, _outdated, _hosted, _free];
    await _pump(t, api);
    await _tapKey(t, 'acp-install-codex-acp', settle: false);
    expect(api.acpInstallCalls, [(null, 'codex-acp')]);
    expect(find.byKey(const Key('acp-progress-codex-acp')), findsOneWidget);
    await _tapKey(t, 'acp-upgrade-claude-acp', settle: false);
    expect(api.acpInstallCalls.last, (null, 'claude-acp'));
    await _finishJobs(t, api);
    // An agent with a hosted instance cannot be uninstalled.
    expect(
      _widget<OutlinedButton>(t, 'acp-uninstall-opencode-acp').onPressed,
      isNull,
    );
    expect(find.text('Hosted as oc'), findsOneWidget);
    await _tapKey(t, 'acp-uninstall-opencode2-acp');
    expect(find.byKey(const Key('acp-uninstall-confirm')), findsOneWidget);
    await _tapKey(t, 'acp-confirm-ok');
    expect(
      api.acpAgentList.firstWhere((a) => a.id == 'opencode2-acp').state,
      'not_installed',
    );
  });

  testWidgets('a registry version installs only after the warning', (t) async {
    final api = FakeBridgeApi()..acpAgentList = const [_free];
    await _pump(t, api);
    await _tapKey(t, 'acp-more-opencode2-acp');
    await t.tap(find.byKey(const Key('acp-registry-install-opencode2-acp')));
    await t.pumpAndSettle();
    expect(
      find.byKey(const Key('acp-registry-install-confirm')),
      findsOneWidget,
    );
    await _tapKey(t, 'acp-confirm-ok', settle: false);
    expect(api.acpInstallCalls, [(null, 'opencode2-acp')]);
    await _finishJobs(t, api);
  });

  testWidgets('settings save with the shared-data and coexistence prompts', (
    t,
  ) async {
    final api = FakeBridgeApi()
      ..acpAgentList = const [_hosted]
      ..acpSettingsValue = const AcpSettings(
        opencodeData: [AcpDataMode(family: 'opencode-v1', mode: 'auto')],
      )
      ..acpSettingsWarnings = const ['opencode2-acp is installed'];
    await _pump(t, api);
    await _tapKey(t, 'acp-remote-toggle');
    await t.enterText(
      find.byKey(const Key('acp-npm-registry')),
      'https://registry.npmmirror.com/',
    );
    await _tapKey(t, 'acp-data-opencode-v1');
    await t.tap(find.text('Shared').last);
    await t.pumpAndSettle();
    expect(find.byKey(const Key('acp-shared-data-confirm')), findsOneWidget);
    await _tapKey(t, 'acp-confirm-ok');
    await _tapKey(t, 'acp-settings-save');
    expect(find.byKey(const Key('acp-save-warning-confirm')), findsOneWidget);
    expect(find.text('opencode2-acp is installed'), findsOneWidget);
    await _tapKey(t, 'acp-confirm-ok');
    final saved = api.acpSettingsValue;
    expect(saved.remoteManagement, isFalse);
    expect(saved.npmRegistry, 'https://registry.npmmirror.com/');
    expect(saved.opencodeData.single.mode, 'shared');
  });

  testWidgets('the gateway dialog sends a write-only token', (t) async {
    final api = FakeBridgeApi()..acpAgentList = const [_outdated];
    await _pump(t, api);
    await _tapKey(t, 'acp-gateway-claude-acp');
    expect(find.byKey(const Key('acp-gateway-dialog')), findsOneWidget);
    expect(find.text('Automatic (first gateway method)'), findsOneWidget);
    await t.enterText(
      find.byKey(const Key('acp-gateway-url')),
      'https://relay.example.com',
    );
    await t.enterText(find.byKey(const Key('acp-gateway-token')), 'sk-test');
    await t.enterText(
      find.byKey(const Key('acp-gateway-headers')),
      'X-Team=a\nbad line',
    );
    await _tapKey(t, 'acp-gateway-save');
    final gateway = api.acpSettingsValue.gateways.single;
    expect(gateway.agentId, 'claude-acp');
    expect(gateway.baseUrl, 'https://relay.example.com');
    expect(gateway.token, 'sk-test');
    expect(gateway.extraHeaders.map((h) => '${h.name}=${h.value}'), [
      'X-Team=a',
    ]);
  });

  testWidgets('an existing gateway keeps its token when left empty', (t) async {
    final api = FakeBridgeApi()
      ..acpAgentList = const [_outdated]
      ..acpSettingsValue = const AcpSettings(
        gateways: [
          AcpGateway(
            agentId: 'claude-acp',
            baseUrl: 'http://192.168.1.5:3000',
            hasToken: true,
          ),
        ],
      );
    await _pump(t, api);
    await _tapKey(t, 'acp-gateway-claude-acp');
    expect(find.text('Saved — leave empty to keep it'), findsOneWidget);
    expect(find.textContaining('Plain http'), findsOneWidget);
    await _tapKey(t, 'acp-gateway-save');
    expect(api.acpSettingsValue.gateways.single.token, isNull);
  });

  testWidgets('the subscription switch asks first and needs a restart', (
    t,
  ) async {
    final api = FakeBridgeApi()
      ..acpAgentList = const [_outdated]
      ..acpSettingsValue = const AcpSettings(
        flags: [
          AcpAgentFlag(
            agentId: 'claude-acp',
            setting: 'allow_subscription_login',
            labelKey: 'acpSubscriptionLogin',
            confirmKey: 'acpSubscriptionLoginConfirmBody',
            value: false,
          ),
        ],
      );
    await _pump(t, api);
    await _tapKey(t, 'acp-advanced-claude-acp');
    expect(find.text('Allow Claude subscription login'), findsOneWidget);
    await _tapKey(t, 'acp-flag-claude-acp-allow_subscription_login');
    expect(find.byKey(const Key('acp-flag-confirm')), findsOneWidget);
    expect(find.text('Allow subscription login?'), findsOneWidget);
    await _tapKey(t, 'acp-confirm-ok');
    expect(api.acpSettingsValue.flags.single.value, isTrue);
    expect(
      find.text('Restart the agent for this to take effect.'),
      findsOneWidget,
    );
  });

  testWidgets('custom agents are added, edited and deleted', (t) async {
    final api = FakeBridgeApi()..acpAgentList = const [];
    await _pump(t, api);
    await _tapKey(t, 'acp-custom-add');
    expect(find.textContaining('plain text'), findsOneWidget);
    await t.enterText(find.byKey(const Key('acp-custom-id')), 'gemini');
    await t.enterText(find.byKey(const Key('acp-custom-name')), 'Gemini CLI');
    await t.enterText(
      find.byKey(const Key('acp-custom-command')),
      '/opt/homebrew/bin/gemini',
    );
    await t.enterText(find.byKey(const Key('acp-custom-args')), '--acp\n');
    await t.enterText(find.byKey(const Key('acp-custom-env')), 'KEY=v');
    await _tapKey(t, 'acp-custom-save');
    final added = api.acpCustom.single;
    expect((added.id, added.name), ('gemini', 'Gemini CLI'));
    expect(added.args, ['--acp']);
    expect(added.env.single.name, 'KEY');
    await _tapKey(t, 'acp-custom-edit-gemini');
    final id = _widget<TextField>(t, 'acp-custom-id');
    expect(id.readOnly, isTrue);
    await t.enterText(find.byKey(const Key('acp-custom-name')), 'Gemini');
    await _tapKey(t, 'acp-custom-save');
    expect(api.acpCustom.single.name, 'Gemini');
    await _tapKey(t, 'acp-custom-delete-gemini');
    expect(api.acpCustom, isEmpty);
  });

  testWidgets('remote mode is read-only while remote management is off', (
    t,
  ) async {
    final api = FakeBridgeApi()
      ..acpAgentList = const [_fresh, _free]
      ..acpRemoteManagement = false;
    await _pump(t, api, svc: 'pcx:mac:app:default');
    expect(find.byKey(const Key('acp-remote-disabled')), findsOneWidget);
    expect(_widget<FilledButton>(t, 'acp-install-codex-acp').onPressed, isNull);
    expect(_widget<FilledButton>(t, 'acp-host-btn').onPressed, isNull);
    expect(find.byKey(const Key('acp-remote-toggle')), findsNothing);
    expect(find.byKey(const Key('acp-custom-add')), findsNothing);
    expect(find.byKey(const Key('acp-gateway-codex-acp')), findsNothing);
  });

  testWidgets('remote install and start hosting poll the host job', (t) async {
    final api = FakeBridgeApi()..acpAgentList = const [_fresh, _free];
    const svc = 'pcx:mac:app:default';
    await _pump(t, api, svc: svc);
    await _tapKey(t, 'acp-install-codex-acp', settle: false);
    expect(api.acpInstallCalls, [(svc, 'codex-acp')]);
    await _tapKey(t, 'acp-host-btn', settle: false);
    expect(api.acpHostCalls, [(svc, 'opencode2-acp', null)]);
    api.acpJobs['job-2'] = const AcpJob(
      id: 'job-2',
      kind: 'host',
      agentId: 'opencode2-acp',
      state: 'failed',
      errorCode: 'acp.remote_management_disabled',
      message: 'off',
    );
    api.acpJobs['job-1'] = const AcpJob(
      id: 'job-1',
      kind: 'install',
      agentId: 'codex-acp',
      state: 'done',
    );
    await t.pump(const Duration(milliseconds: 1100));
    await t.pumpAndSettle();
    expect(
      find.text(
        "Remote management is turned off on this host. Turn it on in the host's ACP settings.",
      ),
      findsOneWidget,
    );
  });
}
