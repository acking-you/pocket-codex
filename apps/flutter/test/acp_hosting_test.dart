// ACP hosting: the provider picker, the OpenCode preset and custom agents,
// verbatim argument vectors, saved agents, host details, restore planning,
// preference compatibility, and phone/tablet/desktop layouts in both themes.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/hosting/acp_host_form.dart';
import 'package:pocket_codex/src/hosting/host_restore.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/service_key.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/widgets/local_host_dialog.dart';
import 'package:pocket_codex/src/widgets/provider_badge.dart';

import 'fake_bridge_api.dart';

Future<ProviderContainer> _pumpDialog(
  WidgetTester t,
  FakeBridgeApi api, {
  AppServeStatus? existing,
  Size size = const Size(1280, 900),
  Brightness brightness = Brightness.light,
}) async {
  t.view.physicalSize = size;
  t.view.devicePixelRatio = 1;
  addTearDown(t.view.reset);
  final container = ProviderContainer(
    overrides: [bridgeApiProvider.overrideWithValue(api)],
  );
  addTearDown(container.dispose);
  await t.pumpWidget(
    UncontrolledProviderScope(
      container: container,
      child: MaterialApp(
        locale: const Locale('en'),
        theme: ThemeData(brightness: brightness),
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
  // Let the debounced program lookup settle.
  await t.pump(const Duration(milliseconds: 400));
  await t.pumpAndSettle();
}

Future<void> _start(WidgetTester t) async {
  await t.ensureVisible(find.byKey(const Key('start-hosting-btn')));
  await t.tap(find.byKey(const Key('start-hosting-btn')));
  await t.pumpAndSettle();
}

const _acpHost = AppServeStatus(
  name: 'beta',
  device: 'local',
  alive: true,
  appListenAddr: '127.0.0.1:18200',
  appServiceKey: 'pcx:local:acp:beta',
  appRegistered: true,
  metaListenAddr: '127.0.0.1:18201',
  metaServiceKey: 'pcx:local:meta:beta',
  metaRegistered: true,
  codexBinary: '/opt/agents/beta agent',
  provider: 'acp',
  protocol: 'acp',
  providerName: 'Beta agent',
  profileId: 'custom-beta',
  agentPhase: 'failed',
  agentError: 'the agent exited',
  authRequired: true,
);

void main() {
  testWidgets('the OpenCode preset hosts `opencode` with argv [acp]', (
    t,
  ) async {
    final api = FakeBridgeApi();
    final c = await _pumpDialog(t, api);
    expect(find.byType(ChoiceChip), findsNWidgets(2));
    expect(find.byKey(const Key('provider-codex')), findsOneWidget);
    expect(find.byKey(const Key('provider-acp')), findsOneWidget);
    await _chooseAcp(t);
    expect(find.byKey(const Key('acp-host-form')), findsOneWidget);
    final program = t.widget<TextField>(
      find.byKey(const Key('acp-program-field')),
    );
    expect(program.controller!.text, 'opencode');
    expect(find.byKey(const Key('acp-program-found')), findsOneWidget);
    final arg = t.widget<TextField>(find.byKey(const Key('acp-arg-field-0')));
    expect(arg.controller!.text, 'acp');
    // Codex-only fields are not part of an ACP host.
    expect(find.byKey(const Key('use-proxy-switch')), findsNothing);
    expect(find.byKey(const Key('codex-path-field')), findsNothing);
    await _start(t);
    expect(api.acpServeCalls, hasLength(1));
    final (name, spec) = api.acpServeCalls.single;
    expect(name, 'opencode');
    expect(spec.profileId, 'opencode');
    expect(spec.program, 'opencode');
    expect(spec.args, ['acp']);
    final prefs = await c.read(uiPrefsProvider.future);
    expect(prefs.autoHostAcp.single.name, 'opencode');
    expect(prefs.acpAgents, isEmpty, reason: 'presets are not saved');
  });

  testWidgets('custom agents keep every argument verbatim and are saved', (
    t,
  ) async {
    final api = FakeBridgeApi()..acpPrograms['my-agent'] = '/usr/bin/my-agent';
    final c = await _pumpDialog(t, api);
    await _chooseAcp(t);
    await t.tap(find.byKey(const Key('acp-agent-choice')));
    await t.pumpAndSettle();
    await t.tap(find.text('Custom agent').last);
    await t.pumpAndSettle();
    await t.enterText(
      find.byKey(const Key('acp-display-name-field')),
      'My Agent',
    );
    await t.enterText(
      find.byKey(const Key('acp-program-field')),
      '/Applications/My Agent.app/bin/agent',
    );
    for (final value in ['--flag=a b', r'$HOME;*', '']) {
      await t.ensureVisible(find.byKey(const Key('acp-arg-add')));
      await t.tap(find.byKey(const Key('acp-arg-add')));
      await t.pumpAndSettle();
      final index = find
          .byWidgetPredicate(
            (w) =>
                w.key is ValueKey<String> &&
                (w.key! as ValueKey<String>).value.startsWith('acp-arg-field-'),
          )
          .evaluate()
          .length;
      await t.enterText(find.byKey(Key('acp-arg-field-${index - 1}')), value);
      await t.pump();
    }
    // Reorder: move the last (empty) argument above the second.
    await t.ensureVisible(find.byKey(const Key('acp-arg-up-2')));
    await t.tap(find.byKey(const Key('acp-arg-up-2')));
    await t.pumpAndSettle();
    await _start(t);
    final (_, spec) = api.acpServeCalls.single;
    expect(spec.program, '/Applications/My Agent.app/bin/agent');
    expect(spec.args, ['--flag=a b', '', r'$HOME;*']);
    expect(spec.profileId, matches(RegExp(r'^custom-my-agent-[0-9a-f]{8}$')));
    final prefs = await c.read(uiPrefsProvider.future);
    expect(prefs.acpAgents.single, spec);
    expect(prefs.autoHostAcp.single.spec, spec);
  });

  testWidgets('a platform without ACP hosting says so', (t) async {
    final api = FakeBridgeApi()..acpHosting = false;
    await _pumpDialog(t, api);
    await _chooseAcp(t);
    expect(find.byKey(const Key('acp-hosting-unsupported')), findsOneWidget);
    expect(find.byKey(const Key('acp-program-field')), findsNothing);
  });

  testWidgets(
    'host details show failure, sign-in and restart; stop forgets it',
    (t) async {
      final api = FakeBridgeApi()..serveHosts.add(_acpHost);
      final c = await _pumpDialog(t, api, existing: _acpHost);
      c
          .read(uiPrefsProvider.notifier)
          .setAutoHostAcp(
            const AutoHostAcpPrefs(
              name: 'beta',
              spec: AcpAgentSpec(
                profileId: 'custom-beta',
                displayName: 'Beta agent',
                program: '/opt/agents/beta agent',
              ),
            ),
          );
      expect(find.text('Not running'), findsOneWidget);
      expect(find.text('the agent exited'), findsOneWidget);
      expect(find.byKey(const Key('acp-auth-required')), findsOneWidget);
      expect(find.byKey(const Key('acp-features-unknown')), findsOneWidget);
      expect(find.byKey(const Key('provider-badge-acp-tag')), findsOneWidget);
      await t.ensureVisible(find.byKey(const Key('acp-restart')));
      await t.tap(find.byKey(const Key('acp-restart')));
      await t.pumpAndSettle();
      expect(api.acpRestartCalls, ['beta']);
      await t.tap(find.byKey(const Key('stop-hosting-btn')));
      await t.pumpAndSettle();
      final prefs = await c.read(uiPrefsProvider.future);
      expect(prefs.autoHostAcp, isEmpty);
      expect(api.serveHosts, isEmpty);
    },
  );

  for (final (label, size) in [
    ('phone', const Size(320, 640)),
    ('tablet', const Size(800, 1100)),
    ('desktop', const Size(1440, 900)),
  ]) {
    for (final brightness in Brightness.values) {
      testWidgets('ACP form lays out on $label (${brightness.name})', (
        t,
      ) async {
        final api = FakeBridgeApi();
        await _pumpDialog(t, api, size: size, brightness: brightness);
        await _chooseAcp(t);
        expect(t.takeException(), isNull);
        expect(find.byKey(const Key('acp-arg-field-0')), findsOneWidget);
        // Reordering buttons only where the row stays wide enough to read.
        expect(
          find.byKey(const Key('acp-arg-up-0')),
          label == 'phone' ? findsNothing : findsOneWidget,
        );
        // Touch targets stay at least 48 dp tall.
        final remove = t.getSize(find.byKey(const Key('acp-arg-remove-0')));
        expect(remove.height, greaterThanOrEqualTo(48));
        final chip = t.getSize(find.byKey(const Key('provider-acp')));
        expect(chip.height, greaterThan(0));
      });
    }
  }

  test('restore plans by what hosts are, never by what they are not', () {
    const acpSpec = AcpAgentSpec(
      profileId: 'opencode',
      displayName: 'OpenCode',
      program: 'opencode',
      args: ['acp'],
    );
    const prefs = UiPrefs(
      autoHost: AutoHostPrefs(port: 0, name: 'default'),
      autoHostAcp: [
        AutoHostAcpPrefs(name: 'agent', spec: acpSpec),
        AutoHostAcpPrefs(name: 'other', spec: acpSpec),
      ],
    );
    // A running ACP host is not "a Codex host": Codex must still restore.
    final plan = planHostRestore(
      prefs,
      [_acpHost.copyForTest(name: 'agent')],
      codexAttempted: false,
      acpAttempted: const {},
      acpSupported: true,
    );
    expect(plan.codex?.name, 'default');
    expect(plan.acp.map((a) => a.name), ['other']);
    final attempted = planHostRestore(
      prefs,
      const [],
      codexAttempted: true,
      acpAttempted: const {'agent', 'other'},
      acpSupported: true,
    );
    expect(attempted.isEmpty, isTrue);
    final unsupported = planHostRestore(
      prefs,
      const [],
      codexAttempted: true,
      acpAttempted: const {},
      acpSupported: false,
    );
    expect(unsupported.acp, isEmpty);
  });

  test('restore runs every planned ACP host with its saved argv', () async {
    final api = FakeBridgeApi();
    const spec = AcpAgentSpec(
      profileId: 'custom-x',
      displayName: 'X',
      program: '/bin/x',
      args: ['a b', ''],
    );
    final restored = await runHostRestore(
      api,
      const HostRestorePlan(
        acp: [AutoHostAcpPrefs(name: 'x', spec: spec)],
      ),
    );
    expect(restored, isTrue);
    expect(api.acpServeCalls.single.$2.args, ['a b', '']);
  });

  test('preferences grow compatibly and drop malformed ACP records', () {
    final old = UiPrefs.fromJson({
      'autoHost': {'name': 'default', 'port': 0},
    });
    expect(old.acpAgents, isEmpty);
    expect(old.autoHostAcp, isEmpty);
    expect(old.toJson().containsKey('acpAgents'), isFalse);
    final parsed = UiPrefs.fromJson({
      'acpAgents': [
        {
          'profileId': 'custom-a',
          'displayName': 'A',
          'program': '/bin/a',
          'args': ['x', ''],
        },
        {'profileId': 'broken'},
        'garbage',
      ],
      'autoHostAcp': [
        {
          'name': 'a',
          'spec': {
            'profileId': 'custom-a',
            'displayName': 'A',
            'program': '/bin/a',
            'args': <String>[],
          },
        },
        {'name': '', 'spec': null},
      ],
    });
    expect(parsed.acpAgents.single.args, ['x', '']);
    expect(parsed.autoHostAcp.single.name, 'a');
    final again = UiPrefs.fromJson(parsed.toJson());
    expect(again.acpAgents.single, parsed.acpAgents.single);
  });

  test('protocols come from the key kind', () {
    expect(sessionProtocolOf('pcx:mac:acp:agent'), SessionProtocol.acp);
    expect(sessionProtocolOf('pcxu:u:mac:acp:agent'), SessionProtocol.acp);
    expect(
      sessionProtocolOf('pcx:mac:app:acp'),
      SessionProtocol.codexAppServer,
    );
    expect(isSessionKind('acp'), isTrue);
    expect(
      customProfileId('My Agent!', '0a1b2c3d'),
      'custom-my-agent-0a1b2c3d',
    );
    expect(customProfileId('***', '0a1b2c3d'), 'custom-agent-0a1b2c3d');
    // Names in other scripts share a slug; the salt keeps them apart.
    final first = customProfileId('我的智能体', newProfileSalt());
    final second = customProfileId('另一个智能体', newProfileSalt());
    expect(first, startsWith('custom-agent-'));
    expect(first, isNot(second));
    expect(newProfileSalt(), matches(RegExp(r'^[0-9a-f]{8}$')));
  });

  testWidgets('two custom agents with non-Latin names are saved separately', (
    t,
  ) async {
    final api = FakeBridgeApi();
    final c = await _pumpDialog(t, api);
    for (final (index, name) in ['我的智能体', '另一个智能体'].indexed) {
      if (index > 0) {
        await t.tap(find.text('open'));
        await t.pumpAndSettle();
      }
      await _chooseAcp(t);
      await t.tap(find.byKey(const Key('acp-agent-choice')));
      await t.pumpAndSettle();
      await t.tap(find.text('Custom agent').last);
      await t.pumpAndSettle();
      await t.enterText(find.byKey(const Key('acp-display-name-field')), name);
      await t.enterText(
        find.byKey(const Key('acp-instance-name-field')),
        'agent-$index',
      );
      await t.enterText(
        find.byKey(const Key('acp-program-field')),
        '/opt/agents/agent-$index',
      );
      await _start(t);
    }
    final prefs = await c.read(uiPrefsProvider.future);
    expect(prefs.acpAgents.map((a) => a.displayName), ['我的智能体', '另一个智能体']);
    expect(prefs.acpAgents.map((a) => a.profileId).toSet(), hasLength(2));
  });

  test('native Codex capability values are unchanged and ACP starts empty', () {
    expect(AppCapabilities.codex.voice, isTrue);
    expect(AppCapabilities.codex.nativeMetadata, isTrue);
    const acp = AppCapabilities.acp;
    expect(acp.negotiated, isFalse);
    expect([
      acp.voice,
      acp.dictation,
      acp.imageInput,
      acp.steer,
      acp.modelCatalog,
    ], everyElement(isFalse));
    expect(acp.permissionOptions, isTrue);
  });

  testWidgets('the ACP badge names the agent and tags the protocol', (t) async {
    await t.pumpWidget(
      MaterialApp(
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        locale: const Locale('en'),
        home: const Scaffold(
          body: Row(
            children: [
              ProviderBadge(provider: 'acp', name: 'Beta agent'),
              ProviderBadge(provider: 'codex'),
            ],
          ),
        ),
      ),
    );
    expect(find.text('Beta agent'), findsOneWidget);
    expect(find.text('ACP'), findsOneWidget);
    expect(find.text('Codex'), findsOneWidget);
  });
}

extension on AppServeStatus {
  AppServeStatus copyForTest({required String name}) => AppServeStatus(
    name: name,
    device: device,
    alive: alive,
    appServiceKey: 'pcx:local:acp:$name',
    provider: provider,
    protocol: protocol,
    providerName: providerName,
    agentPhase: 'ready',
  );
}
