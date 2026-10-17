import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/screens/services_screen.dart';

import 'fake_bridge_api.dart';
import 'support/screen_harness.dart';

const _manager = 'pcx:studio:host:pocket';
const _windows = DeviceMetadata(
  os: 'windows',
  arch: 'x86_64',
  hostname: 'studio',
  instanceId: 'windows-app',
);
const _wsl = DeviceMetadata(
  os: 'linux',
  arch: 'x86_64',
  environment: 'wsl',
  hostname: 'studio',
  instanceId: 'wsl-cli',
);

Future<void> _show(
  WidgetTester t,
  FakeBridgeApi api, {
  double width = 1200,
}) async {
  t.view.physicalSize = Size(width, 1200);
  t.view.devicePixelRatio = 1;
  addTearDown(t.view.reset);
  await t.pumpWidget(host(const ServicesScreen(), api));
  await t.pumpAndSettle();
}

Future<void> _tap(WidgetTester t, String key) async {
  final target = find.byKey(Key(key));
  await t.ensureVisible(target);
  await t.pumpAndSettle();
  await t.tap(target);
  await t.pumpAndSettle();
}

void main() {
  final windows = TargetPlatformVariant.only(TargetPlatform.windows);

  testWidgets('Windows and WSL with the same hostname keep independent '
      'hosting controls and remote Claude support', (t) async {
    final api =
        FakeBridgeApi(
            services: const [
              ServiceEntry(
                device: 'studio',
                kind: 'host',
                name: 'pocket',
                key: _manager,
              ),
            ],
          )
          ..localMetadata = _windows
          ..acpHosting = false
          ..serveHosts.add(
            const AppServeStatus(
              name: 'local',
              device: 'studio',
              alive: true,
              appServiceKey: 'pcx:studio:app:local',
            ),
          );
    api.managedDevices[_manager] = const ManagedDevice(
      device: 'studio',
      os: 'linux',
      metadata: _wsl,
      appVersion: '0.3.0',
    );
    api.managedRuntimeLists[_manager] = const [
      ManagedRuntimeAvailability(kind: 'claude', ready: true),
    ];
    await _show(t, api);
    expect(find.byKey(const Key('device-@local')), findsOneWidget);
    expect(find.byKey(const Key('device-studio')), findsOneWidget);
    expect(find.text('Windows · x86_64'), findsWidgets);

    await _tap(t, 'device-studio');
    expect(
      t.widget<Text>(find.byKey(const Key('device-platform'))).data,
      'Linux · WSL · x86_64',
    );
    expect(find.byKey(const Key('managed-device-studio')), findsOneWidget);
    expect(find.byKey(const Key('add-local-host-card')), findsNothing);
    expect(find.byKey(const Key('device-management-switch')), findsNothing);
    expect(api.metadataReads, isEmpty, reason: 'reuse management metadata');

    await _tap(t, 'managed-add-service');
    await _tap(t, 'managed-runtime-claude');
    expect(find.byKey(const Key('managed-create')), findsOneWidget);
    expect(find.byKey(const Key('acp-hosting-unsupported')), findsNothing);
    expect(api.managedCalls, isEmpty, reason: 'opening never starts a service');
    expect(t.takeException(), isNull);
  }, variant: windows);

  testWidgets('a desktop with no local service never attaches its controls '
      'to the selected remote device, even on a narrow screen', (t) async {
    final api = FakeBridgeApi(
      services: const [
        ServiceEntry(
          device: 'studio',
          kind: 'host',
          name: 'pocket',
          key: _manager,
        ),
      ],
    )..localMetadata = _windows;
    api.managedDevices[_manager] = const ManagedDevice(
      device: 'studio',
      os: 'linux',
      metadata: _wsl,
      appVersion: '0.3.0',
    );
    await _show(t, api, width: 393);
    await _tap(t, 'device-studio');
    expect(find.byKey(const Key('add-local-host-card')), findsNothing);
    expect(find.byKey(const Key('device-management-switch')), findsNothing);
    expect(
      t.widget<Text>(find.byKey(const Key('device-platform'))).data,
      'Linux · WSL · x86_64',
    );
    await _tap(t, 'device-back');
    await _tap(t, 'device-@local');
    expect(find.byKey(const Key('add-local-host-card')), findsOneWidget);
    expect(find.byKey(const Key('managed-device-studio')), findsNothing);
    expect(find.text('Windows · x86_64'), findsOneWidget);
    expect(t.takeException(), isNull);
  }, variant: windows);

  testWidgets('metadata reads only the selected host, and a late response '
      'cannot replace another device platform', (t) async {
    const first = 'pcx:first:app:default';
    const second = 'pcx:second:app:default';
    final gate = Completer<void>();
    final api =
        FakeBridgeApi(
            services: const [
              ServiceEntry(
                device: 'first',
                kind: 'app',
                name: 'default',
                key: first,
              ),
              ServiceEntry(
                device: 'second',
                kind: 'app',
                name: 'default',
                key: second,
              ),
            ],
          )
          ..localMetadata = _windows
          ..metadataGate = gate;
    api.serviceMetadata[first] = _wsl;
    api.serviceMetadata[second] = const DeviceMetadata(
      os: 'macos',
      arch: 'aarch64',
      hostname: 'second',
      instanceId: 'mac-app',
    );
    await _show(t, api);
    // Wide layouts preserve the existing first-remote selection when the
    // independent local target has no service yet.
    expect(api.metadataReads, [first]);
    await _tap(t, 'device-first');
    expect(api.metadataReads, [first]);
    await _tap(t, 'device-second');
    expect(api.metadataReads, [first, second]);
    gate.complete();
    await t.pumpAndSettle();
    expect(
      t.widget<Text>(find.byKey(const Key('device-platform'))).data,
      'macOS · aarch64',
    );
    expect(find.text('Linux · WSL · x86_64'), findsNothing);
    expect(find.byKey(const Key('add-local-host-card')), findsNothing);
  }, variant: windows);

  testWidgets('legacy and unreachable hosts keep platform unknown and can '
      'recover through refresh', (t) async {
    const key = 'pcx:legacy:api:default';
    final api = FakeBridgeApi(
      services: const [
        ServiceEntry(device: 'legacy', kind: 'api', name: 'default', key: key),
      ],
    )..localMetadata = _windows;
    await _show(t, api);
    await _tap(t, 'device-legacy');
    expect(find.text('平台未知'), findsOneWidget);
    api.metadataError = StateError('host offline');
    await _tap(t, 'refresh-btn');
    expect(find.text('平台未知'), findsOneWidget);
    expect(find.byKey(const Key('add-local-host-card')), findsNothing);
    api.metadataError = null;
    api.serviceMetadata[key] = _wsl;
    await _tap(t, 'refresh-btn');
    expect(
      t.widget<Text>(find.byKey(const Key('device-platform'))).data,
      'Linux · WSL · x86_64',
    );
    expect(t.takeException(), isNull);
  }, variant: windows);
}
