import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/services_screen.dart';
import 'package:pocket_codex/src/screens/settings_screen.dart';
import 'fake_bridge_api.dart';

Widget _host(Widget child, BridgeApi api) => ProviderScope(
  overrides: [bridgeApiProvider.overrideWithValue(api)],
  child: MaterialApp(home: child),
);

void main() {
  testWidgets('Services groups api + app and shows relay', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      services: const [
        ServiceEntry(
          device: 'lb7666',
          kind: 'api',
          name: 'default',
          key: 'pcx:lb7666:api:default',
        ),
        ServiceEntry(
          device: 'lb7666',
          kind: 'app',
          name: 'default',
          key: 'pcx:lb7666:app:default',
        ),
      ],
    );
    await t.pumpWidget(_host(const ServicesScreen(), api));
    await t.pumpAndSettle();
    expect(find.text('API 服务'), findsOneWidget);
    expect(find.text('App-server 服务'), findsOneWidget);
    expect(find.byKey(const Key('svc-pcx:lb7666:api:default')), findsOneWidget);
    expect(find.text('lb7666.top:7666'), findsOneWidget);
  });

  testWidgets('Services shows error state with retry', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'r:1', hasKey: true),
    )..discoverError = Exception('relay down');
    await t.pumpWidget(_host(const ServicesScreen(), api));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('services-error')), findsOneWidget);
    expect(find.text('重试'), findsOneWidget);
  });

  testWidgets('Settings shows masked key and relay', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await t.pumpWidget(_host(const SettingsScreen(), api));
    await t.pumpAndSettle();
    expect(find.text('lb7666.top:7666'), findsOneWidget);
    expect(find.text('•••••••• (已设置)'), findsOneWidget);
    expect(find.byKey(const Key('export-btn')), findsOneWidget);
  });

  testWidgets('Services switches to master-detail at >=600 width', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      services: const [
        ServiceEntry(
          device: 'lb7666',
          kind: 'api',
          name: 'default',
          key: 'pcx:lb7666:api:default',
        ),
      ],
    );
    t.view.devicePixelRatio = 1.0;
    addTearDown(t.view.reset);

    // Narrow (<600): single column, no inline detail pane.
    t.view.physicalSize = const Size(400, 900);
    await t.pumpWidget(_host(const ServicesScreen(), api));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('subscribe-btn')), findsNothing);

    // Wide (>=600): list + embedded ApiServiceScreen detail (subscribe button).
    t.view.physicalSize = const Size(1000, 900);
    await t.pumpWidget(_host(const ServicesScreen(), api));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('subscribe-btn')), findsOneWidget);
  });
}
