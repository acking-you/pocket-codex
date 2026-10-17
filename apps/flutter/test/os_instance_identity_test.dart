import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/screens/services_screen.dart';

import 'fake_bridge_api.dart';
import 'support/screen_harness.dart';

Future<void> _tap(WidgetTester t, String key) async {
  final target = find.byKey(Key(key));
  await t.ensureVisible(target);
  await t.pumpAndSettle();
  await t.tap(target);
  await t.pumpAndSettle();
}

void main() {
  testWidgets('same-name Windows and WSL installations stay separate and '
      'a stop reaches only the chosen installation', (t) async {
    t.view.physicalSize = const Size(1200, 1200);
    t.view.devicePixelRatio = 1;
    addTearDown(t.view.reset);
    const identities = [
      (os: 'windows', environment: null, source: null, id: '11111111'),
      (os: 'linux', environment: 'wsl', source: 'Ubuntu', id: '22222222'),
      (os: 'linux', environment: 'wsl', source: 'Debian', id: '33333333'),
      (os: 'linux', environment: 'wsl', source: 'Ubuntu', id: '44444444'),
    ];
    final api = FakeBridgeApi();
    final devices = <String>[];
    final managers = <String>[];
    for (final identity in identities) {
      final id = '${identity.id}-1234-4123-8123-123456789abc';
      final device =
          '${identity.environment ?? identity.os}'
          '${identity.source == null ? '' : '-${identity.source!.toLowerCase()}'}'
          '.studio.${id.replaceAll('-', '')}';
      final manager = 'pcx:$device:host:pocket';
      devices.add(device);
      managers.add(manager);
      api.services.add(
        ServiceEntry(
          device: device,
          kind: 'host',
          name: 'pocket',
          key: manager,
        ),
      );
      api.managedDevices[manager] = ManagedDevice(
        device: device,
        os: identity.os,
        metadata: DeviceMetadata(
          os: identity.os,
          environment: identity.environment,
          sourceName: identity.source,
          hostname: 'studio',
          arch: 'x86_64',
          hostId: id,
          instanceId: 'process-${identity.id}',
        ),
        appVersion: '0.3.0',
        hosts: [
          ManagedHost(
            name: 'work',
            status: AppServeStatus(
              name: 'work',
              device: device,
              alive: true,
              appServiceKey: 'pcx:$device:app:work',
              incarnation: 'inc-${identity.id}',
            ),
            config: HostConfig(
              revision: 1,
              runtime: const HostRuntime.codex(),
              active: HostActive(
                incarnation: 'inc-${identity.id}',
                runtime: const HostRuntime.codex(),
                revision: 1,
              ),
            ),
          ),
        ],
      );
    }
    await t.pumpWidget(host(const ServicesScreen(), api));
    await t.pumpAndSettle();
    for (var i = 0; i < devices.length; i++) {
      expect(find.byKey(Key('device-${devices[i]}')), findsOneWidget);
      await _tap(t, 'device-${devices[i]}');
      final metadata = api.managedDevices[managers[i]]!.metadata!;
      expect(
        t.widget<Text>(find.byKey(const Key('device-platform'))).data,
        metadata.instanceLabel,
      );
      expect(metadata.instanceLabel, contains(identities[i].id));
      expect(find.byKey(Key('managed-device-${devices[i]}')), findsOneWidget);
      expect(find.byKey(const Key('add-local-host-card')), findsNothing);
    }
    await _tap(t, 'device-${devices[1]}');
    await _tap(t, 'managed-stop-work');
    await _tap(t, 'managed-confirm-ok');
    final stop = api.managedCalls.single;
    expect(
      (stop.op, stop.key, stop.arg),
      ('stop', managers[1], 'inc-22222222'),
    );
    expect(api.managedDevices[managers[0]]!.hosts.single.status!.alive, isTrue);
    expect(api.managedDevices[managers[2]]!.hosts.single.status!.alive, isTrue);
    expect(api.managedDevices[managers[3]]!.hosts.single.status!.alive, isTrue);
    expect(t.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
