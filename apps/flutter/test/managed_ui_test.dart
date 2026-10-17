// Remote computer management from another device (a phone included): the
// `host:` entry keeps its computer listed with no services, routes to the
// management panel only for other computers, and every mutation names what
// was read (revision, incarnation). Conflicts keep edits; lost answers
// refresh and are never resent; secrets and masked values are kept.

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/managed/default_project_tile.dart';
import 'package:pocket_codex/src/managed/managed_host_editor.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/services_screen.dart';
import 'package:pocket_codex/src/service_key.dart';
import 'package:pocket_codex/src/theme.dart';

import 'fake_bridge_api.dart';
import 'support/ui_preview.dart';

const _mgmt = 'pcx:studio:host:studio';
final _en = lookupAppLocalizations(const Locale('en'));

ManagedHost _running(
  String name, {
  int revision = 3,
  String source = 'custom',
}) => ManagedHost(
  name: name,
  restore: true,
  baseUrlMasked: true,
  status: AppServeStatus(
    name: name,
    device: 'studio',
    alive: true,
    appServiceKey: 'pcx:studio:app:$name',
    incarnation: 'inc-$name',
  ),
  config: HostConfig(
    source: source,
    protocol: 'responses',
    baseUrl: 'https://gateway.example/v1?key=***',
    model: 'gpt-x',
    keySet: true,
    revision: revision,
    runtime: const HostRuntime.codex(port: 18100, proxy: 'http://***@p:1'),
    active: HostActive(
      incarnation: 'inc-$name',
      runtime: const HostRuntime.codex(port: 18100),
      source: source,
      revision: revision,
    ),
  ),
);

ManagedHost _stopped(String name, {String? problem}) => ManagedHost(
  name: name,
  config: HostConfig(
    revision: 2,
    runtime: const HostRuntime.opencode(),
    problem: problem,
  ),
);

FakeBridgeApi _fake({
  List<ManagedHost> hosts = const [],
  List<ServiceEntry>? services,
}) {
  final api = FakeBridgeApi(
    services:
        services ??
        const [
          ServiceEntry(
            device: 'studio',
            kind: 'host',
            name: 'studio',
            key: _mgmt,
          ),
        ],
  );
  api.managedDevices[_mgmt] = ManagedDevice(
    device: 'studio',
    os: 'macOS',
    appVersion: '0.9.0',
    hosts: hosts,
  );
  return api;
}

Widget _app(Widget child, BridgeApi api, {double textScale = 1}) =>
    ProviderScope(
      overrides: [bridgeApiProvider.overrideWithValue(api)],
      child: MaterialApp(
        locale: const Locale('en'),
        theme: lightTheme(),
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(
            context,
          ).copyWith(textScaler: TextScaler.linear(textScale)),
          child: child!,
        ),
        home: child,
      ),
    );

Future<void> _size(WidgetTester t, Size size) async {
  t.view.physicalSize = size;
  t.view.devicePixelRatio = 1;
  addTearDown(t.view.reset);
}

/// Open the services page on a phone and drill into device [device].
Future<void> _openServices(
  WidgetTester t,
  FakeBridgeApi api, {
  String device = 'studio',
  Size size = const Size(393, 852),
  double textScale = 1,
}) async {
  await _size(t, size);
  await t.pumpWidget(_app(const ServicesScreen(), api, textScale: textScale));
  await t.pumpAndSettle();
  await t.tap(find.byKey(Key('device-$device')));
  await t.pumpAndSettle();
}

Future<void> _tapVisible(WidgetTester t, Finder finder) async {
  await t.ensureVisible(finder);
  await t.pumpAndSettle();
  await t.tap(finder);
  await t.pumpAndSettle();
}

void main() {
  // Real fonts, so text is measured as shipped.
  setUpAll(() => loadPreviewFonts(always: true));

  test('a management key parses but is never a conversation service', () {
    expect(parseServiceKey(_mgmt).kind, 'host');
    expect(parseServiceKey('pcxu:u1:studio:host:studio').device, 'studio');
    expect(isManagementKey(_mgmt), isTrue);
    expect(isSessionKind('host'), isFalse);
  });

  testWidgets('a computer with only its management entry stays listed and '
      'opens management, never a chat row', (t) async {
    final api = _fake();
    await _openServices(t, api);
    expect(find.byKey(const Key('managed-device-studio')), findsOneWidget);
    expect(find.byKey(const Key('managed-no-services')), findsOneWidget);
    expect(
      find.text(_en.managedComputerSummary('macOS', '0.9.0')),
      findsOneWidget,
    );
    expect(find.byKey(const Key('device-capability-$_mgmt')), findsNothing);
    expect(find.byKey(const Key('managed-add-service')), findsOneWidget);
    expect(t.takeException(), isNull);
  });

  testWidgets('with management off and no local host, local controls stay '
      'on their own device instead of a remote computer', (t) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    try {
      // No local host and no published key: nothing names this device.
      final api = _fake();
      await _openServices(t, api);
      expect(find.byKey(const Key('device-management-switch')), findsNothing);
      expect(find.byKey(const Key('add-local-host-card')), findsNothing);
      await t.tap(find.byKey(const Key('device-back')));
      await t.pumpAndSettle();
      await t.tap(find.byKey(const Key('device-@local')));
      await t.pumpAndSettle();
      await _tapVisible(t, find.byKey(const Key('device-management-switch')));
      expect(api.deviceManagementSets, [true]);
      await t.pumpWidget(const SizedBox());
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('stop names the observed incarnation and the stopped service '
      'stays listed with Start', (t) async {
    final api = _fake(hosts: [_running('work')]);
    await _openServices(t, api);
    await _tapVisible(t, find.byKey(const Key('managed-stop-work')));
    await t.tap(find.byKey(const Key('managed-confirm-ok')));
    await t.pumpAndSettle();
    final stop = api.managedCalls.single;
    expect((stop.op, stop.name, stop.arg), ('stop', 'work', 'inc-work'));
    expect(find.byKey(const Key('managed-host-work')), findsOneWidget);
    expect(find.byKey(const Key('managed-start-work')), findsOneWidget);
    expect(find.text(_en.managedStopped('work')), findsOneWidget);
    // Start uses the reviewed saved revision.
    await _tapVisible(t, find.byKey(const Key('managed-start-work')));
    expect(api.managedCalls.last.op, 'startSaved');
    expect(api.managedCalls.last.arg, 3);
    expect(find.byKey(const Key('managed-stop-work')), findsOneWidget);
  });

  testWidgets('a lost answer is reported as unknown, refreshed, and never '
      'sent again', (t) async {
    final api = _fake(hosts: [_stopped('oc')])
      ..managedLoseNext.add('startSaved');
    await _openServices(t, api);
    await _tapVisible(t, find.byKey(const Key('managed-start-oc')));
    expect(api.managedCalls.where((c) => c.op == 'startSaved'), hasLength(1));
    expect(find.text(_en.managedOutcomeUnknown), findsOneWidget);
    // The refresh shows what the host actually did.
    expect(find.byKey(const Key('managed-stop-oc')), findsOneWidget);
  });

  testWidgets('an unreadable configuration can still be stopped, never saved '
      'or started', (t) async {
    final broken = _running('work');
    final api = _fake(
      hosts: [
        ManagedHost(
          name: 'work',
          status: broken.status,
          config: HostConfig(
            revision: 3,
            problem: 'the saved profile cannot be read',
            active: broken.config.active,
          ),
        ),
      ],
    );
    await _openServices(t, api);
    await _tapVisible(t, find.byKey(const Key('managed-edit-work')));
    expect(find.byKey(const Key('managed-config-problem')), findsOneWidget);
    expect(find.byKey(const Key('managed-save')), findsNothing);
    expect(find.byKey(const Key('host-model-service')), findsNothing);
    expect(find.byKey(const Key('managed-stop')), findsOneWidget);
  });

  group('remote editor', () {
    Future<FakeBridgeApi> openEditor(WidgetTester t, ManagedHost host) async {
      final api = _fake(hosts: [host]);
      await _size(t, const Size(393, 852));
      await t.pumpWidget(
        _app(
          ManagedHostEditor(serviceKey: _mgmt, device: 'studio', host: host),
          api,
        ),
      );
      await t.pumpAndSettle();
      return api;
    }

    testWidgets('an unedited save keeps the masked URL, the key and the '
        'runtime', (t) async {
      final api = await openEditor(t, _running('work'));
      await t.enterText(find.byKey(const Key('host-model')), 'gpt-y');
      await t.pumpAndSettle();
      await _tapVisible(t, find.byKey(const Key('managed-save')));
      final edit = api.managedCalls.single.arg! as ManagedHostEdit;
      expect(edit.expectedRevision, 3);
      expect(edit.keepBaseUrl, isTrue);
      expect(edit.keyAction, HostKeyAction.keep);
      expect(edit.key, isNull);
      expect(edit.runtime, isNull, reason: 'private proxy/path kept');
      expect(edit.input.model, 'gpt-y');
    });

    testWidgets('a port change keeps the stored proxy', (t) async {
      final api = await openEditor(t, _running('work'));
      await t.enterText(find.byKey(const Key('managed-port')), '18200');
      await t.pumpAndSettle();
      await _tapVisible(t, find.byKey(const Key('managed-save')));
      final edit = api.managedCalls.single.arg! as ManagedHostEdit;
      expect(edit.runtime!.port, 18200);
      expect(edit.runtime!.keepProxy, isTrue);
      expect(edit.runtime!.proxy, isNull);
    });

    testWidgets('a conflict keeps the edits and asks for a reload; nothing is '
        'overwritten', (t) async {
      final api = await openEditor(t, _running('work'));
      // Another device saves meanwhile.
      api.managedDevices[_mgmt] = ManagedDevice(
        device: 'studio',
        os: 'macOS',
        appVersion: '0.9.0',
        hosts: [_running('work', revision: 4)],
      );
      await t.enterText(find.byKey(const Key('host-model')), 'mine');
      await t.pumpAndSettle();
      await _tapVisible(t, find.byKey(const Key('managed-save')));
      expect(api.managedCalls, hasLength(1));
      expect(find.byKey(const Key('managed-conflict')), findsOneWidget);
      expect(
        t
            .widget<TextField>(find.byKey(const Key('host-model')))
            .controller!
            .text,
        'mine',
      );
      // Save stays unavailable until an explicit reload.
      final save = t.widget<OutlinedButton>(
        find.byKey(const Key('managed-save')),
      );
      expect(save.onPressed, isNull);
      await _tapVisible(t, find.byKey(const Key('managed-reload')));
      expect(find.byKey(const Key('managed-conflict')), findsNothing);
      expect(
        t
            .widget<TextField>(find.byKey(const Key('host-model')))
            .controller!
            .text,
        'gpt-x',
      );
      expect(api.managedCalls, hasLength(1), reason: 'never resent');
    });

    testWidgets('apply names the revision and incarnation that were read', (
      t,
    ) async {
      final pending = _running('work');
      final host = ManagedHost(
        name: 'work',
        status: pending.status,
        baseUrlMasked: true,
        config: HostConfig(
          source: 'custom',
          protocol: 'responses',
          baseUrl: pending.config.baseUrl,
          model: 'gpt-x',
          keySet: true,
          revision: 5,
          runtime: pending.config.runtime,
          active: pending.config.active,
        ),
      );
      final api = await openEditor(t, host);
      await _tapVisible(t, find.byKey(const Key('managed-apply')));
      await t.tap(find.byKey(const Key('managed-confirm-ok')));
      await t.pumpAndSettle();
      expect(api.managedCalls.single.op, 'apply');
      expect(api.managedCalls.single.arg, ('inc-work', 5));
      expect(find.text(_en.hostApplied), findsOneWidget);
    });

    testWidgets('a new service is created once with only a restricted '
        'runtime', (t) async {
      final api = _fake();
      await _size(t, const Size(320, 700));
      await t.pumpWidget(
        _app(
          const ManagedHostEditor(
            serviceKey: _mgmt,
            device: 'studio',
            kind: 'opencode',
          ),
          api,
          textScale: 2,
        ),
      );
      await t.pumpAndSettle();
      expect(t.takeException(), isNull);
      expect(find.byKey(const Key('managed-port')), findsNothing);
      await _tapVisible(t, find.byKey(const Key('managed-create')));
      final call = api.managedCalls.single;
      expect(call.op, 'start');
      final arg =
          call.arg!
              as ({
                ManagedRuntime runtime,
                HostConfigInput input,
                HostKeyAction keyAction,
                String? key,
              });
      expect(arg.runtime.kind, 'opencode');
      expect(arg.keyAction, HostKeyAction.remove);
      expect(find.byKey(const Key('managed-stop')), findsOneWidget);
    });
  });

  testWidgets('the management panel and editor fit a 320 px phone at 2x '
      'text', (t) async {
    final api = _fake(
      hosts: [_running('a-very-long-service-name-on-studio'), _stopped('oc')],
    );
    await _openServices(t, api, size: const Size(320, 700), textScale: 2);
    expect(t.takeException(), isNull);
    await _tapVisible(
      t,
      find.byKey(const Key('managed-edit-a-very-long-service-name-on-studio')),
    );
    expect(t.takeException(), isNull);
    // The keyboard takes half the screen: the form still scrolls to Save.
    t.view.viewInsets = FakeViewPadding(bottom: 380);
    addTearDown(t.view.resetViewInsets);
    await t.pumpAndSettle();
    await t.ensureVisible(find.byKey(const Key('managed-save')));
    await t.pumpAndSettle();
    expect(t.takeException(), isNull);
    expect(find.byKey(const Key('managed-save')).hitTestable(), findsOneWidget);
  });

  group('default project tile', () {
    Future<void> pumpTile(WidgetTester t, FakeBridgeApi api) async {
      await _size(t, const Size(393, 852));
      await t.pumpWidget(
        _app(
          const Scaffold(
            body: SingleChildScrollView(
              child: DefaultProjectTile(serviceKey: _mgmt),
            ),
          ),
          api,
        ),
      );
      await t.pumpAndSettle();
    }

    testWidgets('shows the automatic folder without creating it, and resets '
        'a chosen one to automatic with the read revision', (t) async {
      final api = _fake()
        ..projectSettings[_mgmt] = const ProjectSettings(
          revision: '7',
          effectiveDefault: '/Users/me/PocketCodex',
          automatic: true,
        );
      await pumpTile(t, api);
      expect(find.text('/Users/me/PocketCodex'), findsOneWidget);
      expect(
        find.byKey(const Key('default-project-automatic-label')),
        findsOneWidget,
      );
      expect(find.text(_en.defaultProjectCreatedLater), findsOneWidget);
      expect(find.byKey(const Key('default-project-automatic')), findsNothing);

      api.projectSettings[_mgmt] = const ProjectSettings(
        revision: '8',
        defaultProject: '/srv/app',
        effectiveDefault: '/srv/app',
        exists: true,
      );
      await t.pumpWidget(const SizedBox());
      await pumpTile(t, api);
      await _tapVisible(t, find.byKey(const Key('default-project-automatic')));
      final save = api.projectSettingsSaves.single;
      expect(save.def, isNull);
      expect(save.rev, '8');
    });

    testWidgets('a project-settings failure leaves service management usable', (
      t,
    ) async {
      final api = _fake(hosts: [_stopped('oc')])
        ..projectSettingsError = Exception('meta unavailable');
      await _openServices(t, api);
      expect(find.byKey(const Key('default-project-error')), findsOneWidget);
      await _tapVisible(t, find.byKey(const Key('managed-start-oc')));
      expect(api.managedCalls.single.op, 'startSaved');
    });
  });
}
