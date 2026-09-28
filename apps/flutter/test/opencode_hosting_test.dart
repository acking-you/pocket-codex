// OpenCode hosting: the dialog's provider switch, its start/stop flows, and the
// additive `autoHostOpenCode` record in ui_state.json.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/widgets/local_host_dialog.dart';

import 'fake_bridge_api.dart';

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

Future<void> _chooseOpenCode(WidgetTester t) async {
  await t.tap(find.byKey(const Key('provider-opencode')));
  await t.pumpAndSettle();
}

void main() {
  testWidgets('OpenCode mode hides Codex-only fields and starts OpenCode', (
    t,
  ) async {
    final api = FakeBridgeApi();
    final c = await _pumpDialog(t, api);
    expect(find.text('Port'), findsOneWidget);
    expect(find.byKey(const Key('use-proxy-switch')), findsOneWidget);

    await _chooseOpenCode(t);
    expect(find.text('Port'), findsNothing);
    expect(find.byKey(const Key('use-proxy-switch')), findsNothing);
    expect(find.byKey(const Key('proxy-field')), findsNothing);
    expect(find.byKey(const Key('codex-path-field')), findsNothing);
    expect(find.text('Built-in engine'), findsNothing);
    expect(find.textContaining('does not stop OpenCode'), findsOneWidget);
    final path = t.widget<TextField>(
      find.byKey(const Key('opencode-path-field')),
    );
    expect(path.controller!.text, api.opencodePath);

    await t.tap(find.byKey(const Key('start-hosting-btn')));
    await t.pumpAndSettle();
    // The detected binary is not pinned; the default name is `opencode`.
    expect(api.openCodeServeCalls, [('opencode', null)]);
    expect(api.lastServePort, isNull);
    expect(api.serveHosts.single.provider, 'opencode');
    expect(find.byType(LocalHostDialog), findsNothing);
    final saved = c.read(uiPrefsProvider).valueOrNull!.autoHostOpenCode!;
    expect(saved.name, 'opencode');
    expect(saved.binaryOverride, isNull);
    expect(c.read(uiPrefsProvider).valueOrNull!.autoHost, isNull);
  });

  testWidgets('a custom name and binary path are passed and remembered', (
    t,
  ) async {
    final api = FakeBridgeApi();
    final c = await _pumpDialog(t, api);
    await _chooseOpenCode(t);
    await t.enterText(find.byKey(const Key('opencode-name-field')), 'oc2');
    await t.enterText(
      find.byKey(const Key('opencode-path-field')),
      '/opt/opencode',
    );
    await t.tap(find.byKey(const Key('start-hosting-btn')));
    await t.pumpAndSettle();
    expect(api.openCodeServeCalls, [('oc2', '/opt/opencode')]);
    final saved = c.read(uiPrefsProvider).valueOrNull!.autoHostOpenCode!;
    expect((saved.name, saved.binaryOverride), ('oc2', '/opt/opencode'));
  });

  testWidgets('a bridge refusal is shown verbatim and keeps the dialog', (
    t,
  ) async {
    final api = FakeBridgeApi()
      ..openCodeServeError = '`default` is already hosted by Codex here';
    await _pumpDialog(t, api);
    await _chooseOpenCode(t);
    await t.tap(find.byKey(const Key('start-hosting-btn')));
    await t.pumpAndSettle();
    expect(
      find.textContaining('`default` is already hosted by Codex here'),
      findsOneWidget,
    );
    expect(find.byType(LocalHostDialog), findsOneWidget);
    expect(api.serveHosts, isEmpty);
  });

  testWidgets('a running OpenCode host shows its version, keys and no API', (
    t,
  ) async {
    final api = FakeBridgeApi()
      ..openCodeVersion = '2.1.0'
      ..openCodeVerified = false;
    await api.appServeStartOpencode();
    final host = api.serveHosts.single;
    final c = await _pumpDialog(t, api, existing: host);
    c
      ..read(
        uiPrefsProvider.notifier,
      ).setAutoHostOpenCode(const AutoHostOpenCodePrefs(name: 'opencode'))
      ..read(
        uiPrefsProvider.notifier,
      ).setAutoHost(const AutoHostPrefs(port: 18080, name: 'default'));
    expect(find.textContaining('2.1.0'), findsOneWidget);
    expect(find.byKey(const Key('opencode-unverified')), findsOneWidget);
    expect(find.text(host.appServiceKey), findsOneWidget);
    expect(find.text(host.metaServiceKey), findsOneWidget);
    expect(find.text('API'), findsNothing);
    expect(find.text('Proxy', findRichText: true), findsNothing);

    await t.tap(find.byKey(const Key('stop-hosting-btn')));
    await t.pumpAndSettle();
    expect(api.serveHosts, isEmpty);
    final prefs = c.read(uiPrefsProvider).valueOrNull!;
    // Only the OpenCode record is forgotten; the Codex one is untouched.
    expect(prefs.autoHostOpenCode, isNull);
    expect(prefs.autoHost?.name, 'default');
  });

  testWidgets('a verified OpenCode host has no unverified warning', (t) async {
    final api = FakeBridgeApi();
    await api.appServeStartOpencode();
    await _pumpDialog(t, api, existing: api.serveHosts.single);
    expect(find.textContaining('2.0.18 · Verified'), findsOneWidget);
    expect(find.byKey(const Key('opencode-unverified')), findsNothing);
  });

  group('autoHostOpenCode prefs', () {
    test('round-trips through JSON next to the Codex record', () {
      const prefs = UiPrefs(
        autoHost: AutoHostPrefs(port: 18080, name: 'default'),
        autoHostOpenCode: AutoHostOpenCodePrefs(
          name: 'oc',
          binaryOverride: '/opt/opencode',
        ),
      );
      final json = prefs.toJson();
      expect(json['autoHostOpenCode'], {
        'name': 'oc',
        'binaryOverride': '/opt/opencode',
      });
      expect(json['autoHost'], isA<Map<String, dynamic>>());
      final back = UiPrefs.fromJson(json);
      expect(back.autoHostOpenCode!.name, 'oc');
      expect(back.autoHostOpenCode!.binaryOverride, '/opt/opencode');
      expect(back.autoHost!.port, 18080);
    });

    test('an older file without the field still loads', () {
      final prefs = UiPrefs.fromJson({
        'lastServiceKey': 'pcx:mac:app:default',
        'autoHost': {'port': 18080, 'name': 'default', 'embedded': false},
        'guideSeen': true,
      });
      expect(prefs.autoHostOpenCode, isNull);
      expect(prefs.autoHost!.name, 'default');
      expect(prefs.lastServiceKey, 'pcx:mac:app:default');
      expect(prefs.guideSeen, isTrue);
      expect(prefs.toJson().containsKey('autoHostOpenCode'), isFalse);
    });

    test('clearing one record keeps the other; bad shapes degrade', () {
      const both = UiPrefs(
        autoHost: AutoHostPrefs(port: 1, name: 'a'),
        autoHostOpenCode: AutoHostOpenCodePrefs(name: 'b'),
      );
      expect(both.copyWith(clearAutoHostOpenCode: true).autoHost, isNotNull);
      expect(both.copyWith(clearAutoHost: true).autoHostOpenCode?.name, 'b');
      expect(
        UiPrefs.fromJson({'autoHostOpenCode': 'x'}).autoHostOpenCode,
        isNull,
      );
      final blank = UiPrefs.fromJson({
        'autoHostOpenCode': {'name': '', 'binaryOverride': 3},
      }).autoHostOpenCode!;
      expect((blank.name, blank.binaryOverride), ('opencode', null));
    });
  });
}
