part of '../services_test.dart';

void _registerLocalHostLifecycle() {
  testWidgets('desktop account mode: add a host, then stop it', (t) async {
    final api = accountFake();
    await showServices(t, api);

    // No hosts yet → only the "host another" entry (desktop + account mode).
    expect(find.byKey(const Key('add-local-host-card')), findsOneWidget);

    // Add a host → start on the default port with the default proxy.
    await addLocalHost(t);
    expect(api.lastServePort, 18080);
    expect(api.lastServeProxy, 'http://127.0.0.1:11111');
    expect(api.serveHosts, hasLength(1));
    // Its app-server row carries the address it listens on.
    expect(
      find.descendant(
        of: find.byKey(const Key('device-capability-pcx:local:app:default')),
        matching: find.textContaining('127.0.0.1:18080'),
      ),
      findsOneWidget,
    );

    // The running host's card appears; open it → Stop tears it down.
    expect(find.byKey(const Key('local-host-default')), findsOneWidget);
    await t.tap(find.byKey(const Key('local-host-default')));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('stop-hosting-btn')), findsOneWidget);
    await t.tap(find.byKey(const Key('stop-hosting-btn')));
    await t.pumpAndSettle();
    expect(api.serveHosts, isEmpty);
  }, variant: desktop);

  testWidgets('a host whose codex did not exit stays listed and stoppable '
      'until a stop succeeds', (t) async {
    final api = accountFake()
      ..serveStopFailures = 1
      ..serveHosts.add(
        const AppServeStatus(
          name: 'stuck',
          device: 'local',
          provider: 'codex',
          protocol: 'codex-app-server',
          providerName: 'Codex',
          agentError: 'its codex did not exit; stop the host again',
        ),
      );
    await showServices(t, api);
    expect(find.byKey(const Key('local-host-stuck')), findsOneWidget);
    expect(find.byKey(const Key('local-host-stop-failed')), findsOneWidget);

    await t.tap(find.byKey(const Key('local-host-stuck')));
    await t.pumpAndSettle();
    final stop = find.byKey(const Key('stop-hosting-btn'));
    await t.ensureVisible(stop);
    await t.tap(stop);
    await t.pumpAndSettle();
    expect(api.serveHosts, hasLength(1), reason: 'the failed stop keeps it');
    expect(find.textContaining('did not exit'), findsWidgets);
    expect(
      t.widget<ButtonStyleButton>(stop).onPressed,
      isNotNull,
      reason: 'the retry stays reachable',
    );

    await t.tap(stop);
    await t.pumpAndSettle();
    expect(api.serveHosts, isEmpty);
  }, variant: desktop);

  testWidgets('local-host: turning off the proxy passes no proxy', (t) async {
    final api = accountFake();
    await showServices(t, api);
    await t.tap(find.byKey(const Key('add-local-host-card')));
    await t.pumpAndSettle();
    // The proxy field shows by default (proxy is mandatory); toggle it off.
    expect(find.byKey(const Key('proxy-field')), findsOneWidget);
    await tapRevealed(t, find.byKey(const Key('use-proxy-switch')));
    expect(find.byKey(const Key('proxy-field')), findsNothing);
    await t.tap(find.byKey(const Key('start-hosting-btn')));
    await t.pumpAndSettle();
    expect(api.lastServeProxy, isNull);
    expect(api.serveHosts, hasLength(1));
  }, variant: desktop);

  testWidgets('local-host: a second host coexists with the first', (t) async {
    // Tall enough that both host cards and the add button build: the detail is
    // a lazy ListView, so anything below the fold is absent rather than merely
    // scrolled away, and `ensureVisible` has nothing to scroll to.
    t.view.physicalSize = const Size(1200, 1600);
    t.view.devicePixelRatio = 1;
    addTearDown(t.view.reset);
    final api = accountFake();
    await showServices(t, api);

    // First host "default".
    await addLocalHost(t);

    // A second host defaults to a free name and an automatic port, so it
    // never collides with the first.
    await t.tap(find.byKey(const Key('add-local-host-card')));
    await t.pumpAndSettle();
    final name = t.widget<TextField>(find.byKey(const Key('codex-name-field')));
    expect(name.controller!.text, 'default-2');
    await t.tap(find.byKey(const Key('start-hosting-btn')));
    await t.pumpAndSettle();
    expect(api.lastServeName, 'default-2');
    expect(api.lastServePort, 0);

    // A third, named by hand, with an explicit port behind "Advanced".
    await t.tap(find.byKey(const Key('add-local-host-card')));
    await t.pumpAndSettle();
    await t.enterText(find.byKey(const Key('codex-name-field')), 'work');
    // The toggle spans the dialog's width with its button at the start;
    // reveal and tap the button itself, inside the dialog's scroll view.
    final advanced = find.descendant(
      of: find.byKey(const Key('codex-advanced-toggle')),
      matching: find.byType(TextButton),
    );
    await t.ensureVisible(advanced);
    await t.pumpAndSettle();
    await t.tap(advanced);
    await t.pumpAndSettle();
    final port = find.byKey(const Key('codex-port-field'));
    expect(port, findsOneWidget, reason: 'Advanced unfolded the port');
    await t.ensureVisible(port);
    await t.pumpAndSettle();
    await t.enterText(port, '18081');
    await t.ensureVisible(find.byKey(const Key('start-hosting-btn')));
    await t.tap(find.byKey(const Key('start-hosting-btn')));
    await t.pumpAndSettle();
    expect(api.lastServeName, 'work');
    expect(api.lastServePort, 18081);

    expect(api.serveHosts, hasLength(3));
    expect(find.byKey(const Key('local-host-default')), findsOneWidget);
    expect(find.byKey(const Key('local-host-default-2')), findsOneWidget);
    expect(find.byKey(const Key('local-host-work')), findsOneWidget);
    // The add entry stays available however many hosts run.
    expect(find.byKey(const Key('add-local-host-card')), findsOneWidget);

    // Stopping one Codex host forgets only its own restore record.
    final container = ProviderScope.containerOf(
      t.element(find.byType(ServicesScreen)),
    );
    final before = await container.read(uiPrefsProvider.future);
    expect(before.autoHostCodex.map((h) => h.name), [
      'default',
      'default-2',
      'work',
    ]);
    await t.tap(find.byKey(const Key('local-host-default-2')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('stop-hosting-btn')));
    await t.pumpAndSettle();
    final after = await container.read(uiPrefsProvider.future);
    expect(after.autoHostCodex.map((h) => h.name), ['default', 'work']);
    expect(api.serveHosts.map((h) => h.name), ['default', 'work']);
  }, variant: desktop);

  testWidgets('local-host: a hosted server is labeled local in the App-server '
      'list', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(
        relay: '',
        hasKey: false,
        mode: 'account',
        accountLogin: 'acking-you',
      ),
      // A discovered app-server whose key matches the host we'll start.
      services: const [
        ServiceEntry(
          device: 'local',
          kind: 'app',
          name: 'default',
          key: 'pcx:local:app:default',
        ),
      ],
    );
    await showServices(t, api);
    // The discovered label is remote until this process owns its service;
    // the independent local device already exists before hosting starts.
    expect(find.byKey(const Key('device-@local')), findsOneWidget);
    expect(
      find.descendant(
        of: find.byKey(const Key('device-local')),
        matching: find.textContaining('本机'),
      ),
      findsNothing,
    );

    // Host "default" locally → its key matches the discovered service. The
    // title bar's button is the route in when no local device is selected.
    await t.tap(find.byKey(const Key('host-this-device-btn')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('start-hosting-btn')));
    await t.pumpAndSettle();

    // Only the exact owned keys move to the local target.
    expect(find.text('本机'), findsWidgets);
    expect(find.byKey(const Key('local-host-default')), findsOneWidget);
    // Its app-server capability is the same discovered service as before.
    expect(
      find.byKey(const Key('device-capability-pcx:local:app:default')),
      findsOneWidget,
    );
  }, variant: desktop);

  // Each tunnel a host publishes unpublishes and re-publishes from its own
  // capability row; the app tunnel and the host stay up meanwhile.
  for (final (tunnel, menu, published) in [
    (
      'API',
      'capability-menu-pcx:local:api:default',
      (AppServeStatus h) => h.apiRegistered,
    ),
    (
      'session',
      'capability-menu-meta-pcx:local:app:default',
      (AppServeStatus h) => h.metaRegistered,
    ),
  ]) {
    testWidgets('local-host: the $tunnel tunnel can be taken down and put '
        'back', (t) async {
      final api = accountFake();
      await showServices(t, api);
      await addLocalHost(t);
      expect(published(api.serveHosts.single), isTrue);

      await t.tap(find.byKey(Key(menu)));
      await t.pumpAndSettle();
      await t.tap(find.text('注销')); // deregister (zh)
      await t.pumpAndSettle();
      await t.tap(find.byKey(const Key('deregister-confirm-btn')));
      await t.pumpAndSettle();
      expect(published(api.serveHosts.single), isFalse);
      expect(api.serveHosts.single.appRegistered, isTrue);

      // The row stays — listed as offline — so re-registering is still
      // reachable. Hiding it would strand the tunnel with no route back.
      expect(find.text('已下架'), findsOneWidget); // tunnelOffline (zh)
      await t.tap(find.byKey(Key(menu)));
      await t.pumpAndSettle();
      await t.tap(find.text('重新注册')); // reregister (zh)
      await t.pumpAndSettle();
      expect(published(api.serveHosts.single), isTrue);
    }, variant: desktop);
  }

  testWidgets('Codex and native OpenCode are hosted side by side', (t) async {
    t.view.physicalSize = const Size(1200, 1600);
    t.view.devicePixelRatio = 1;
    addTearDown(t.view.reset);
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'relay.example:7666', hasKey: true),
      services: const [],
    );
    await showServices(t, api);
    await addLocalHost(t);

    await t.tap(find.byKey(const Key('add-local-host-card')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('agent-choice-opencode')));
    await t.pump(const Duration(milliseconds: 400));
    await t.pumpAndSettle();
    await t.ensureVisible(find.byKey(const Key('start-hosting-btn')));
    await t.tap(find.byKey(const Key('start-hosting-btn')));
    await t.pumpAndSettle();

    expect(api.serveHosts.map((h) => (h.name, h.provider)), [
      ('default', 'codex'),
      ('opencode', 'opencode'),
    ]);
    // Each card leads with its own agent's mark, from its engine.
    expect(
      find.descendant(
        of: find.byKey(const Key('local-host-icon-default')),
        matching: find.byKey(const Key('agent-icon-codex')),
      ),
      findsOneWidget,
    );
    expect(
      find.descendant(
        of: find.byKey(const Key('local-host-icon-opencode')),
        matching: find.byKey(const Key('agent-icon-opencode')),
      ),
      findsOneWidget,
    );
  }, variant: desktop);
}
