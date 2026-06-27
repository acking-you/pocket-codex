import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_linkify/flutter_linkify.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/account_onboarding_screen.dart';
import 'package:pocket_codex/src/screens/api_service_screen.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/screens/app_service_screen.dart';
import 'package:pocket_codex/src/screens/services_screen.dart';
import 'package:pocket_codex/src/screens/settings_screen.dart';
import 'package:pocket_codex/src/widgets/loading.dart';
import 'package:pocket_codex/src/widgets/status_dots.dart';
import 'fake_bridge_api.dart';

/// Mount [child] with a fake bridge and localizations. Defaults to the
/// Chinese locale so the existing zh assertions hold; pass [locale] to test
/// other languages.
Widget _host(
  Widget child,
  BridgeApi api, {
  Locale locale = const Locale('zh'),
}) => ProviderScope(
  overrides: [bridgeApiProvider.overrideWithValue(api)],
  child: MaterialApp(
    locale: locale,
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: child,
  ),
);

/// Mount under a GoRouter so screens that call `context.go(...)` navigate; each
/// extra [stubs] entry (path → label) renders a Text so a route can be asserted.
Widget _routerHost(
  BridgeApi api, {
  required String initial,
  required List<GoRoute> routes,
}) => ProviderScope(
  overrides: [bridgeApiProvider.overrideWithValue(api)],
  child: MaterialApp.router(
    locale: const Locale('zh'),
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    routerConfig: GoRouter(initialLocation: initial, routes: routes),
  ),
);

GoRoute _stub(String path, String label) => GoRoute(
  path: path,
  builder: (_, _) => Scaffold(body: Text(label)),
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

  testWidgets('Wide layout switches the detail pane when another API is tapped', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      services: const [
        ServiceEntry(
          device: 'lb7666',
          kind: 'api',
          name: 'first',
          key: 'pcx:lb7666:api:first',
        ),
        ServiceEntry(
          device: 'lb7666',
          kind: 'api',
          name: 'second',
          key: 'pcx:lb7666:api:second',
        ),
      ],
    );
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1000, 900);
    addTearDown(t.view.reset);

    await t.pumpWidget(_host(const ServicesScreen(), api));
    await t.pumpAndSettle();
    // Default selection = first API; its full key shows only in the detail pane.
    expect(find.text('pcx:lb7666:api:first'), findsOneWidget);
    expect(find.text('pcx:lb7666:api:second'), findsNothing);

    // Tap the second service's list tile → detail pane switches to it.
    await t.tap(find.byKey(const Key('svc-pcx:lb7666:api:second')));
    await t.pumpAndSettle();
    expect(find.text('pcx:lb7666:api:second'), findsOneWidget);
    expect(find.text('pcx:lb7666:api:first'), findsNothing);
  });

  testWidgets('a registered-but-dead app-server reads "unreachable", not '
      '"online"', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      services: const [
        ServiceEntry(
          device: 'lb7666',
          kind: 'app',
          name: 'default',
          key: 'pcx:lb7666:app:default',
        ),
      ],
    )..reachable['pcx:lb7666:app:default'] = false; // backend probe fails
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(400, 900); // narrow: single-pane list
    addTearDown(t.view.reset);

    await t.pumpWidget(_host(const ServicesScreen(), api));
    await t.pumpAndSettle(); // let the reachability probe resolve

    // The probe says the backend is dead → honest "不可达" on the app-server.
    expect(find.text('不可达'), findsOneWidget); // statusUnreachable (zh)
    // "在线" appears once — for the RELAY only. The old bug would have shown it
    // a second time on the app-server (a false green "online").
    expect(find.text('在线'), findsOneWidget);
    // …and it spells out *why*: relay registration up, remote backend down.
    expect(find.textContaining('远端 app-server 没有响应'), findsOneWidget);
  });

  testWidgets('account mode shows the GitHub identity, not "(no relay)"', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(
        relay: '',
        hasKey: false,
        mode: 'account',
        accountLogin: 'acking-you',
      ),
      services: const [
        ServiceEntry(
          device: 'lb7666',
          kind: 'app',
          name: 'default',
          key: 'pcxu:u:lb7666:app:default',
        ),
      ],
    );
    await t.pumpWidget(_host(const ServicesScreen(), api));
    await t.pumpAndSettle();
    // The header shows the signed-in GitHub identity…
    expect(find.text('@acking-you'), findsOneWidget);
    // …and never the confusing "(no relay configured)" placeholder.
    expect(find.text('(未配置 relay)'), findsNothing);
  });

  testWidgets('a registered-but-dead API proxy reads "unreachable", not '
      '"online"', (t) async {
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
    )..reachable['pcx:lb7666:api:default'] = false; // proxy probe fails
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(400, 900); // narrow: single-pane list
    addTearDown(t.view.reset);

    await t.pumpWidget(_host(const ServicesScreen(), api));
    await t.pumpAndSettle(); // let the API probe resolve

    // The probe says the proxy is dead → honest "不可达" on the API service…
    expect(find.text('不可达'), findsOneWidget);
    // …and spells out that the dead link is the remote API service.
    expect(find.textContaining('远端 API 服务没有响应'), findsOneWidget);
  });

  testWidgets('app-server auto-re-probes: a recovered server flips to online '
      'without a manual refresh', (t) async {
    final api =
        FakeBridgeApi(
            config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
            services: const [
              ServiceEntry(
                device: 'lb7666',
                kind: 'app',
                name: 'default',
                key: 'pcx:lb7666:app:default',
              ),
            ],
          )
          ..reachable['pcx:lb7666:app:default'] =
              false; // starts registered-but-dead
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(400, 900); // narrow: single-pane list
    addTearDown(t.view.reset);

    await t.pumpWidget(_host(const ServicesScreen(), api));
    await t.pumpAndSettle(); // initial probe resolves
    expect(find.text('不可达'), findsOneWidget); // honest dead status

    // The remote app-server comes back up out from under us...
    api.reachable['pcx:lb7666:app:default'] = true;
    // ...and the periodic re-probe picks it up with NO manual refresh tap.
    await t.pump(const Duration(seconds: 16)); // fire the 15s re-probe timer
    await t.pumpAndSettle(); // let the fresh probe resolve

    expect(find.text('不可达'), findsNothing); // recovered on its own
    expect(find.text('在线'), findsNWidgets(2)); // relay + app-server both online
  });

  testWidgets('onboarding: sign in shows the code, then authorized navigates '
      'home', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: '', hasKey: false),
    )..accountPollStatus = 'authorized';
    await t.pumpWidget(
      _routerHost(
        api,
        initial: '/onboarding',
        routes: [
          GoRoute(
            path: '/onboarding',
            builder: (_, _) => const AccountOnboardingScreen(),
          ),
          _stub('/', 'HOME-ROUTE'),
        ],
      ),
    );
    await t.pumpAndSettle(); // initial onboarding (no spinner yet)
    await t.tap(find.text('使用 GitHub 登录')); // accountSignInButton (zh)
    // The polling spinner is a perpetual animation, so advance via bounded pumps
    // (pumpAndSettle would never settle while it spins).
    await t.pump(); // _start: accountLoginStart resolves
    await t.pump(); // setState shows the code + spinner
    expect(find.text('ABCD-1234'), findsOneWidget); // user code shown
    expect(find.text('打开 GitHub'), findsOneWidget); // accountOpenGitHub (zh)
    await t.pump(const Duration(seconds: 6)); // fire the 5s poll interval
    await t.pump(); // accountLoginPoll resolves → context.go('/')
    await t.pump(); // router rebuilds at '/'
    expect(find.text('HOME-ROUTE'), findsOneWidget); // navigated on authorize
  });

  testWidgets('onboarding: an expired code clears and shows the expired '
      'message without navigating', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: '', hasKey: false),
    )..accountPollStatus = 'expired';
    await t.pumpWidget(
      _routerHost(
        api,
        initial: '/onboarding',
        routes: [
          GoRoute(
            path: '/onboarding',
            builder: (_, _) => const AccountOnboardingScreen(),
          ),
          _stub('/', 'HOME-ROUTE'),
        ],
      ),
    );
    await t.pumpAndSettle(); // initial onboarding (no spinner yet)
    await t.tap(find.text('使用 GitHub 登录'));
    await t.pump(); // start resolves
    await t.pump(); // code + spinner show
    expect(find.text('ABCD-1234'), findsOneWidget);
    await t.pump(
      const Duration(seconds: 6),
    ); // poll fires → 'expired' → setState
    // 'expired' clears _device, so the spinner is gone and we can settle.
    await t.pumpAndSettle();
    expect(find.text('代码已过期,请重试。'), findsOneWidget); // accountCodeExpired (zh)
    expect(find.text('ABCD-1234'), findsNothing); // cleared, back to sign-in
    expect(find.text('HOME-ROUTE'), findsNothing); // did NOT navigate
  });

  testWidgets('settings: account sign-out clears the user and returns to '
      'onboarding', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(
        relay: '',
        hasKey: false,
        mode: 'account',
        accountLogin: 'octocat',
      ),
    )..accountUser = const AccountUser(login: 'octocat', accountId: '42');
    await t.pumpWidget(
      _routerHost(
        api,
        initial: '/settings',
        routes: [
          GoRoute(path: '/settings', builder: (_, _) => const SettingsScreen()),
          _stub('/onboarding', 'ONBOARDING-ROUTE'),
        ],
      ),
    );
    await t.pumpAndSettle();
    expect(find.text('@octocat'), findsOneWidget); // signed-in identity
    await t.tap(find.byKey(const Key('sign-out-btn')));
    await t.pumpAndSettle();
    expect(api.accountUser, isNull); // accountLogout ran
    expect(find.text('ONBOARDING-ROUTE'), findsOneWidget); // back to onboarding
  });

  testWidgets('ApiService rejects an out-of-range port before subscribing', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await t.pumpWidget(
      _host(const ApiServiceScreen(serviceKey: 'pcx:lb7666:api:default'), api),
    );
    await t.pumpAndSettle();

    // 70000 parses as an int but exceeds u16; must be rejected client-side.
    await t.enterText(find.byType(TextField), '70000');
    await t.tap(find.byKey(const Key('subscribe-btn')));
    await t.pumpAndSettle();

    expect(find.byKey(const Key('api-error')), findsOneWidget);
    // Still on the subscribe form (no base-url shown) — nothing was subscribed.
    expect(find.byKey(const Key('base-url')), findsNothing);
  });

  testWidgets('App session sends a turn and renders the streamed reply', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // Narrow so the sessions pane is a hidden drawer — the new session's preview
    // (also "hello") then can't collide with the transcript bubble below.
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(400, 800);
    addTearDown(t.view.reset);
    await t.pumpWidget(
      _host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // A brand-new conversation shows the guidance view (not a bare hint).
    expect(find.text('想让远程 Codex 做点什么?'), findsOneWidget);

    await t.enterText(find.byType(TextField), 'hello');
    await t.pump(); // let the send button enable for the non-empty input
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();

    // User bubble (plain Text) + the fake's echoed agent reply, which now
    // renders as Markdown (RichText), so match with findRichText.
    expect(find.text('hello'), findsOneWidget);
    expect(
      find.textContaining('echo: hello', findRichText: true),
      findsOneWidget,
    );
  });

  testWidgets('Messages are copyable (copy button puts text on clipboard)', (
    t,
  ) async {
    final copied = <String>[];
    // Intercept the clipboard channel to capture what gets copied.
    t.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      (call) async {
        if (call.method == 'Clipboard.setData') {
          copied.add((call.arguments as Map)['text'] as String);
        }
        return null;
      },
    );
    addTearDown(
      () => t.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        null,
      ),
    );

    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();
    await t.enterText(find.byType(TextField), 'hello');
    await t.pump(); // let the send button enable for the non-empty input
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();

    // The copy action appears on hover (desktop). Hover the agent message,
    // then tap its copy icon.
    final gesture = await t.createGesture(kind: PointerDeviceKind.mouse);
    await gesture.addPointer(location: Offset.zero);
    addTearDown(() => gesture.removePointer());
    await gesture.moveTo(t.getCenter(find.byType(MarkdownBody)));
    await t.pumpAndSettle();
    await t.tap(find.byIcon(Icons.content_copy_outlined).last);
    await t.pump();
    expect(copied, isNotEmpty);
  });

  testWidgets('Agent replies render as Markdown (headings, not a bubble)', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/agentMessage/delta',
        threadId: 't1',
        itemId: 'a1',
        itemType: 'agentMessage',
        text: '# Title\n\nsome **bold** body',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();
    // Markdown produces RichText spans, not a Text bubble.
    expect(find.byType(MarkdownBody), findsOneWidget);
    expect(find.textContaining('Title', findRichText: true), findsWidgets);
  });

  testWidgets("A new conversation ignores another thread's events", (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // A brand-new conversation: no thread id until the first turn starts.
    await t.pumpWidget(
      _host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // The app session is shared and another thread's turn may still be
    // streaming; its events must not be absorbed into the blank conversation.
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/agentMessage/delta',
        threadId: 'other-thread',
        itemId: 'x1',
        itemType: 'agentMessage',
        text: 'not mine',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();
    // Still the new-session guidance (the foreign event was dropped, no items).
    expect(find.text('想让远程 Codex 做点什么?'), findsOneWidget);
    expect(find.textContaining('not mine', findRichText: true), findsNothing);
  });

  testWidgets('A new conversation inherits the last-picked permission mode', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);
    await t.pumpWidget(
      _host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // The default permission mode is "自动" (auto). Switch it to "只读".
    await t.tap(find.text('自动'));
    await t.pumpAndSettle();
    await t.tap(find.text('只读'));
    await t.pumpAndSettle();
    expect(find.text('只读'), findsOneWidget); // the pill now reads read-only

    // Start a brand-new conversation: it inherits the read-only mode the user
    // last chose instead of resetting to the "自动" default.
    await t.tap(find.byIcon(Icons.add));
    await t.pumpAndSettle();
    expect(find.text('只读'), findsOneWidget);
    expect(find.text('自动'), findsNothing);
  });

  testWidgets('A new session appears in the sessions pane after first send', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);
    await t.pumpWidget(
      _host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // No conversations in the pane yet.
    expect(find.text('暂无会话'), findsOneWidget); // noThreads (zh)

    // Sending the first message surfaces the new session in the left pane.
    await t.enterText(find.byType(TextField), 'hello there');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();

    expect(find.text('暂无会话'), findsNothing);
    // The new session shows in the pane as a conversation tile, with its message
    // preserved as the preview (not "(未命名)" — the server preview is still
    // empty for a just-started thread, so the optimistic one must win).
    expect(find.byIcon(Icons.chat_bubble_outline), findsOneWidget);
    expect(find.text('(未命名)'), findsNothing);
  });

  testWidgets(
    'Conversations pane groups by time with relative-time subtitles',
    (t) async {
      final nowS = DateTime.now().millisecondsSinceEpoch ~/ 1000;
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect('pcx:lb7666:app:default', 28080);
      api.appThreads.addAll([
        ThreadMeta(
          id: 'tRecent',
          preview: 'recent chat',
          cwd: '',
          updatedAt: nowS - 120,
        ),
        ThreadMeta(
          id: 'tOld',
          preview: 'ancient chat',
          cwd: '',
          updatedAt: nowS - 5 * 86400,
        ),
      ]);
      t.view.devicePixelRatio = 1.0;
      t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
      addTearDown(t.view.reset);
      await t.pumpWidget(
        _host(
          const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'),
          api,
        ),
      );
      await t.pumpAndSettle();

      // Each conversation shows its preview + a relative-time subtitle, and the
      // older one is bucketed under "Earlier".
      expect(find.text('recent chat'), findsOneWidget);
      expect(find.text('ancient chat'), findsOneWidget);
      expect(find.text('2 分钟前'), findsOneWidget); // timeMinutesAgo (zh)
      expect(find.text('5 天前'), findsOneWidget); // timeDaysAgo (zh)
      expect(find.text('更早'), findsOneWidget); // groupEarlier (zh)
    },
  );

  testWidgets('Conversations search box filters the list', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // The search box only appears once there are enough conversations (>6).
    api.appThreads.addAll(const [
      ThreadMeta(id: 't1', preview: 'alpha', cwd: '', updatedAt: 0),
      ThreadMeta(id: 't2', preview: 'beta', cwd: '', updatedAt: 0),
      ThreadMeta(id: 't3', preview: 'gamma', cwd: '', updatedAt: 0),
      ThreadMeta(id: 't4', preview: 'delta', cwd: '', updatedAt: 0),
      ThreadMeta(id: 't5', preview: 'epsilon', cwd: '', updatedAt: 0),
      ThreadMeta(id: 't6', preview: 'zeta', cwd: '', updatedAt: 0),
      ThreadMeta(id: 't7', preview: 'needle', cwd: '', updatedAt: 0),
    ]);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);
    await t.pumpWidget(
      _host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    expect(find.text('alpha'), findsOneWidget);
    expect(find.text('needle'), findsOneWidget);

    // Typing a query filters the list to matching previews only — non-matches
    // disappear and exactly one conversation tile remains.
    await t.enterText(find.byKey(const Key('conv-search')), 'needle');
    await t.pumpAndSettle();
    expect(find.text('alpha'), findsNothing);
    expect(find.text('beta'), findsNothing);
    expect(find.byIcon(Icons.chat_bubble_outline), findsOneWidget);
  });

  testWidgets('Tapping a guidance card prefills the composer', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // The prompt shows once on the guidance card before a tap.
    const prompt = '介绍一下这个项目的结构、主要模块和技术栈。';
    expect(find.text(prompt), findsOneWidget);

    // Tapping the "了解项目" card prefills the composer (review-then-send).
    await t.tap(find.text('了解项目'));
    await t.pumpAndSettle();
    // The prompt now appears twice: the card subtitle + the composer field.
    expect(find.text(prompt), findsNWidgets(2));
    // The send button is enabled now that the composer is non-empty.
    final sendBtn = t.widget<IconButton>(find.byKey(const Key('send-btn')));
    expect(sendBtn.onPressed, isNotNull);
  });

  testWidgets('Tool calls render as expandable activity cards', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // A web-search tool item arrives as an item event.
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/completed',
        threadId: 't1',
        itemId: 's1',
        itemType: 'webSearch',
        title: 'rust async',
        text: '',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();
    // Localized label + the query are shown.
    expect(find.text('联网搜索'), findsOneWidget);
    expect(find.text('rust async'), findsOneWidget);

    // A command with output is expandable.
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/completed',
        threadId: 't1',
        itemId: 'c1',
        itemType: 'commandExecution',
        title: 'ls -la',
        text: 'total 0\n[exit 0]',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();
    expect(find.text('执行命令'), findsOneWidget);
    // Detail hidden until expanded.
    expect(find.textContaining('total 0', findRichText: true), findsNothing);
    await t.tap(find.text('ls -la'));
    await t.pumpAndSettle();
    expect(find.textContaining('total 0', findRichText: true), findsOneWidget);
  });

  testWidgets('a finished turn drops a 用时 duration footnote', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // No footnote before a turn runs.
    expect(find.textContaining('用时'), findsNothing);

    // A turn starts (elapsed clock begins) then completes.
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'turn/started',
        threadId: 't1',
        itemId: '',
        itemType: '',
        title: '',
        text: '',
        raw: '{}',
      ),
    );
    await t.pump();
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'turn/completed',
        threadId: 't1',
        itemId: '',
        itemType: '',
        title: '',
        text: '',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();

    // The per-turn footnote is dropped into the transcript (用时 0:00 for an
    // instant test turn).
    expect(find.textContaining('用时'), findsOneWidget);
  });

  testWidgets('composer pills wrap onto-screen on a narrow (mobile) width', (
    t,
  ) async {
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(360, 760); // a phone-ish viewport
    addTearDown(t.view.reset);
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // The effort pill is the last of five; a horizontal scroll left it clipped
    // off the right edge on a phone. Wrapped, it sits fully within the viewport.
    final effort = find.text('思考强度'); // l10n.effort (zh), no effort set
    expect(effort, findsOneWidget);
    expect(t.getRect(effort).right, lessThanOrEqualTo(360.0));
  });

  testWidgets('Opening an existing thread resumes it before reading', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'thread-42',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    // Must resume (load into session) before read/turn, else "thread not found".
    expect(api.lastResumed, 'thread-42');
  });

  testWidgets('Re-opening an in-flight thread restores history + thinking', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // History recovered from disk + a turn still running.
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(
          id: 'u1',
          itemType: 'userMessage',
          title: '',
          text: 'earlier question',
        ),
      ],
      running: true,
    );
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'thread-7',
        ),
        api,
      ),
    );
    // Not pumpAndSettle: the restored typing indicator animates forever.
    await t.pump();
    await t.pump(const Duration(milliseconds: 50));
    // Past message recovered, and the running state restored (composer shows
    // the stop button instead of send).
    expect(find.text('earlier question'), findsOneWidget);
    expect(find.byKey(const Key('stop-btn')), findsOneWidget);
    expect(find.byKey(const Key('send-btn')), findsNothing);
  });

  testWidgets('App session answers an approval prompt interactively', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // Server pushes an approval request (carries a request id).
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'execCommandApproval',
        requestId: '7',
        raw: '{"command":["ls"]}',
      ),
    );
    await t.pumpAndSettle();
    expect(find.byKey(const Key('approval-card')), findsOneWidget);

    await t.tap(find.byKey(const Key('approve-btn')));
    await t.pumpAndSettle();
    expect(api.lastApprovalDecision, 'accept');
    expect(find.byKey(const Key('approval-card')), findsNothing);
  });

  testWidgets('App session surfaces a turn failure with retry', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'turn/failed',
        threadId: 't1',
        text: 'model overloaded',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();
    expect(find.byKey(const Key('session-error')), findsOneWidget);
    expect(find.text('model overloaded'), findsOneWidget);
  });

  testWidgets('Plan mode toggle is sent with the turn', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // Toggle the plan pill on, then send.
    await t.tap(find.text('计划'));
    await t.pump();
    await t.enterText(find.byType(TextField), 'build a feature');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastCollaborationMode, 'plan');
  });

  testWidgets('A finished plan turn offers to implement, and implementing '
      'leaves plan mode', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // History ends on a plan item with the turn finished — the signature of a
    // completed plan-mode turn. This is what a resumed (restarted) thread looks
    // like, so the implement choice must persist from it.
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(
          id: 'u1',
          itemType: 'userMessage',
          title: '',
          text: 'plan a feature',
        ),
        ThreadItem(
          id: 'p1',
          itemType: 'plan',
          title: '',
          text: '# Step 1\n# Step 2',
        ),
      ],
      running: false,
      // The thread is genuinely in plan mode (sticky server setting), so
      // implementing must send "default" to leave it.
      collaborationMode: 'plan',
    );
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'thread-9',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    // The implement choice is shown (derived from the trailing plan item).
    expect(find.byKey(const Key('implement-btn')), findsOneWidget);

    // Implementing leaves plan mode (sends "default", since it's sticky) and
    // starts a normal turn with the implement prompt.
    await t.tap(find.byKey(const Key('implement-btn')));
    await t.pumpAndSettle();
    expect(api.lastCollaborationMode, 'default');
    expect(api.lastTurnText, '请按上面的计划开始实现。');
    // Once a new turn runs, the plan is no longer trailing → choice goes away.
    expect(find.byKey(const Key('implement-btn')), findsNothing);
  });

  testWidgets('Plan mode read from the server can be turned off', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // A resumed plan-mode thread whose LAST item is a normal reply (not a plan)
    // — the old "last item is plan" heuristic wrongly concluded plan mode was
    // off, so toggling it off never sent "default" and it stayed stuck on.
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(id: 'u1', itemType: 'userMessage', title: '', text: 'hi'),
        ThreadItem(
          id: 'a1',
          itemType: 'agentMessage',
          title: '',
          text: 'Yes, I am in plan mode.',
        ),
      ],
      running: false,
      collaborationMode: 'plan',
    );
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // The toggle is synced ON from the server mode; tap it OFF and send → the
    // turn carries "default", actually leaving plan mode.
    await t.tap(find.text('计划')); // planMode pill (zh), currently active
    await t.pump();
    await t.enterText(find.byType(TextField), 'continue');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastCollaborationMode, 'default');
  });

  testWidgets('Plan mode is remembered per thread across switching', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.addAll(const [
      ThreadMeta(id: 'tA', preview: 'chat A', cwd: '', updatedAt: 0),
      ThreadMeta(id: 'tB', preview: 'chat B', cwd: '', updatedAt: 0),
    ]);
    api.readResult = const ThreadHistory(items: [], running: false);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);

    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'tA',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // Enter plan mode in tA and send.
    await t.tap(find.text('计划'));
    await t.pump();
    await t.enterText(find.byType(TextField), 'plan it');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastCollaborationMode, 'plan');

    // Switch to tB and back to tA via the pane; tA's plan mode is remembered.
    await t.tap(find.text('chat B'));
    await t.pumpAndSettle();
    await t.tap(find.text('chat A'));
    await t.pumpAndSettle();

    // Turning plan OFF in tA now sends "default" (proving it was restored ON).
    await t.tap(find.text('计划'));
    await t.pump();
    await t.enterText(find.byType(TextField), 'stop planning');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastCollaborationMode, 'default');
  });

  testWidgets('Picking a reasoning effort sends it on the next turn', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(items: [], running: false);
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // A fresh turn with no effort picked sends nothing (Auto = model default).
    await t.enterText(find.byType(TextField), 'hi');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastReasoningEffort, isNull);

    // Open the effort picker (the chip shows the localized "Effort" label) and
    // choose High; the next turn carries "high". The pills scroll horizontally,
    // so scroll the chip into view before tapping.
    await t.ensureVisible(find.text('思考强度'));
    await t.tap(find.text('思考强度'));
    await t.pumpAndSettle();
    await t.tap(find.text('高'));
    await t.pumpAndSettle();
    await t.enterText(find.byType(TextField), 'think hard');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastReasoningEffort, 'high');
  });

  testWidgets('Effort picker offers the model-supported levels incl. xhigh', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(items: [], running: false);
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // The default model (gpt-5.5 in the fake) supports low/medium/high/xhigh.
    await t.ensureVisible(find.text('思考强度'));
    await t.tap(find.text('思考强度'));
    await t.pumpAndSettle();
    expect(find.text('极高'), findsOneWidget); // xhigh is offered
    expect(find.text('最低'), findsNothing); // minimal: not supported by gpt-5.5

    // Pick Extra-high and send → "xhigh" goes on the wire.
    await t.tap(find.text('极高'));
    await t.pumpAndSettle();
    await t.enterText(find.byType(TextField), 'think harder');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastReasoningEffort, 'xhigh');
  });

  testWidgets('Reasoning effort is restored from the thread on resume', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(
      items: [],
      running: false,
      reasoningEffort: 'high',
    );
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    // The chip reflects the thread's current effort ("Effort · High").
    expect(find.text('思考强度 · 高'), findsOneWidget);
  });

  testWidgets('Toggling plan re-asserts the effort instead of wiping it', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(items: [], running: false);
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // Set High and send.
    await t.ensureVisible(find.text('思考强度'));
    await t.tap(find.text('思考强度'));
    await t.pumpAndSettle();
    await t.tap(find.text('高'));
    await t.pumpAndSettle();
    await t.enterText(find.byType(TextField), 'one');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastReasoningEffort, 'high');

    // Now toggle plan ON (no new effort pick) and send: the collaborationMode
    // turn must still carry "high", not wipe the thread's effort to null.
    await t.ensureVisible(find.text('计划'));
    await t.tap(find.text('计划'));
    await t.pump();
    await t.enterText(find.byType(TextField), 'two');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastCollaborationMode, 'plan');
    expect(api.lastReasoningEffort, 'high');
  });

  testWidgets('An unsent effort pick does not leak across threads', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.addAll(const [
      ThreadMeta(id: 'tA', preview: 'chat A', cwd: '', updatedAt: 0),
      ThreadMeta(id: 'tB', preview: 'chat B', cwd: '', updatedAt: 0),
    ]);
    api.readResult = const ThreadHistory(items: [], running: false);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);

    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'tA',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // Pick High on tA but DON'T send.
    await t.ensureVisible(find.text('思考强度'));
    await t.tap(find.text('思考强度'));
    await t.pumpAndSettle();
    await t.tap(find.text('高'));
    await t.pumpAndSettle();
    expect(find.text('思考强度 · 高'), findsOneWidget);

    // Switch to tB and send: the unsent High pick must NOT carry over.
    await t.tap(find.text('chat B'));
    await t.pumpAndSettle();
    expect(find.text('思考强度 · 高'), findsNothing);
    await t.enterText(find.byType(TextField), 'hi from B');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastReasoningEffort, isNull);
  });

  testWidgets('URLs in a message render as tappable links', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(
          id: 'u1',
          itemType: 'userMessage',
          title: '',
          text: 'see https://example.com for details',
        ),
      ],
      running: false,
    );
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    // The user bubble linkifies the URL (highlighted + tappable) rather than
    // rendering it as inert text. flutter_linkify parses the bare URL into a
    // UrlElement, so the rendered Linkify carries the link.
    final linkifyWidgets = t.widgetList<Linkify>(find.byType(Linkify));
    expect(linkifyWidgets, isNotEmpty);
    final hasUrl = linkifyWidgets.any(
      (w) => w.text.contains('https://example.com'),
    );
    expect(hasUrl, isTrue);
  });

  testWidgets('A normal multi-step turn does not offer to implement', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // A plan item appears mid-turn but the turn ends on a message — not a
    // plan-mode turn, so no implement choice.
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(id: 'u1', itemType: 'userMessage', title: '', text: 'do it'),
        ThreadItem(id: 'p1', itemType: 'plan', title: '', text: '# Step 1'),
        ThreadItem(id: 'a1', itemType: 'agentMessage', title: '', text: 'done'),
      ],
      running: false,
    );
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'thread-10',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    expect(find.byKey(const Key('implement-btn')), findsNothing);
  });

  testWidgets('Leaving plan mode sends "default" once, then null thereafter', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // Turn 1: plan mode on → "plan".
    await t.tap(find.text('计划'));
    await t.pump();
    await t.enterText(find.byType(TextField), 'plan it');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastCollaborationMode, 'plan');

    // Turn 2: plan mode off → must send "default" to leave sticky plan mode.
    await t.tap(find.text('计划'));
    await t.pump();
    await t.enterText(find.byType(TextField), 'now normally');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastCollaborationMode, 'default');

    // Turn 3: still off → null, NOT "default" forever (the _planActive reset).
    await t.enterText(find.byType(TextField), 'and again');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastCollaborationMode, isNull);
  });

  testWidgets(
    '"Keep planning" dismisses the implement choice, keeping history',
    (t) async {
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect('pcx:lb7666:app:default', 28080);
      api.readResult = const ThreadHistory(
        items: [
          ThreadItem(
            id: 'u1',
            itemType: 'userMessage',
            title: '',
            text: 'plan a feature',
          ),
          ThreadItem(id: 'p1', itemType: 'plan', title: '', text: '# Step 1'),
        ],
        running: false,
      );
      await t.pumpWidget(
        _host(
          const AppSessionScreen(
            serviceKey: 'pcx:lb7666:app:default',
            threadId: 'thread-11',
          ),
          api,
        ),
      );
      await t.pumpAndSettle();
      expect(find.byKey(const Key('implement-btn')), findsOneWidget);

      // Dismiss → the whole bar (implement + keep-planning) hides, but the
      // conversation timeline is untouched.
      await t.tap(find.text('继续规划'));
      await t.pumpAndSettle();
      expect(find.byKey(const Key('implement-btn')), findsNothing);
      expect(find.text('继续规划'), findsNothing);
      expect(find.text('plan a feature'), findsOneWidget);
    },
  );

  testWidgets('Plan mode with no available model surfaces an error, no turn', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    )..emptyModelList = true;
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    await t.tap(find.text('计划'));
    await t.pump();
    await t.enterText(find.byType(TextField), 'plan it');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();

    // Collaboration mode can't apply without a model, so the turn is refused
    // (nothing sent) and an error is shown instead of silently dropping it.
    expect(find.byKey(const Key('session-error')), findsOneWidget);
    expect(api.lastCollaborationMode, isNull);
    expect(api.lastTurnText, isNull);
  });

  testWidgets('Context gauge appears on token usage and opens a detail sheet', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // No gauge until a token-usage event arrives.
    expect(find.text('10'), findsNothing);
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'thread/tokenUsage/updated',
        threadId: 't1',
        raw:
            '{"tokenUsage":{"last":{"totalTokens":20000},"modelContextWindow":200000}}',
      ),
    );
    await t.pumpAndSettle();
    // 20000 / 200000 = 10%.
    expect(find.text('10'), findsOneWidget);

    // Tapping the gauge opens the context/quota detail sheet.
    api.rateLimitsJson =
        '{"rateLimits":{"primary":{"usedPercent":42,"windowDurationMins":300}}}';
    await t.tap(find.text('10'));
    await t.pumpAndSettle();
    expect(find.text('上下文与用量'), findsOneWidget); // contextUsageTitle (zh)
    expect(find.text('5 小时额度'), findsOneWidget); // quota5h (zh)
  });

  testWidgets('Git branch badge shows changes and opens the diff viewer', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(id: 'u1', itemType: 'userMessage', title: '', text: 'hi'),
      ],
      running: false,
      branch: 'feature/x',
      cwd: '/proj', // needed so _loadGit fetches the diff
      tokensUsed: 5000,
      contextWindow: 100000,
    );
    api.gitDiffText =
        'diff --git a/lib/x.dart b/lib/x.dart\n'
        '--- a/lib/x.dart\n'
        '+++ b/lib/x.dart\n'
        '@@ -1 +1,2 @@\n'
        '-old\n'
        '+new\n'
        '+more\n';
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'thread-g',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // Branch + change counts in the unified status bar.
    expect(find.text('feature/x'), findsOneWidget);
    expect(find.text('+2'), findsWidgets); // 2 added
    expect(find.text('−1'), findsWidgets); // 1 removed

    // Tapping opens the diff viewer with the file path.
    await t.tap(find.text('feature/x'));
    await t.pumpAndSettle();
    expect(find.text('lib/x.dart'), findsOneWidget);
  });

  testWidgets('Compact menu action calls the bridge after confirm', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(id: 'u1', itemType: 'userMessage', title: '', text: 'hi'),
      ],
      running: false,
    );
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'thread-c',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    await t.tap(find.byType(PopupMenuButton<String>));
    await t.pumpAndSettle();
    await t.tap(find.text('压缩对话').last); // compact (zh)
    await t.pumpAndSettle();
    // Confirm dialog → tap the confirm button (the FilledButton).
    await t.tap(find.widgetWithText(FilledButton, '压缩对话'));
    await t.pumpAndSettle();
    expect(api.compacted, isTrue);
  });

  testWidgets('Consecutive same-type tool calls collapse into one group', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    for (final id in ['c1', 'c2', 'c3']) {
      api.pushEvent(
        'pcx:lb7666:app:default',
        AppEvent(
          kind: 'item/completed',
          threadId: 't1',
          itemId: id,
          itemType: 'commandExecution',
          title: 'cmd-$id',
          text: 'out-$id',
          raw: '{}',
        ),
      );
    }
    await t.pumpAndSettle();

    // Collapsed into one "Ran command ×3" row; individual commands hidden.
    expect(find.text('执行命令 ×3'), findsOneWidget);
    expect(find.text('cmd-c1'), findsNothing);

    // Expanding reveals the individual activity cards.
    await t.tap(find.text('执行命令 ×3'));
    await t.pumpAndSettle();
    expect(find.text('cmd-c1'), findsOneWidget);
    expect(find.text('cmd-c3'), findsOneWidget);
  });

  testWidgets('Sessions pane shows inline when wide, hidden when narrow', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.add(
      const ThreadMeta(id: 't9', preview: 'past chat', cwd: '', updatedAt: 0),
    );
    t.view.devicePixelRatio = 1.0;
    addTearDown(t.view.reset);

    // Wide: the left sessions pane is inline (header + thread visible).
    t.view.physicalSize = const Size(1200, 900);
    await t.pumpWidget(
      _host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();
    expect(find.text('会话'), findsOneWidget); // conversationsSection (zh)
    expect(find.text('past chat'), findsOneWidget);

    // Narrow: no inline pane (it moves into a closed drawer).
    t.view.physicalSize = const Size(400, 900);
    await t.pumpWidget(
      _host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();
    expect(find.text('past chat'), findsNothing);
  });

  testWidgets('Sessions pane buttons switch threads without crashing', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.add(
      const ThreadMeta(id: 't9', preview: 'old chat', cwd: '', updatedAt: 0),
    );
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(
          id: 'm1',
          itemType: 'agentMessage',
          title: '',
          text: 'hello',
        ),
      ],
      running: false,
    );
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900);
    addTearDown(t.view.reset);
    await t.pumpWidget(
      _host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // Tap an existing thread in the pane → resumes it (no Scaffold.of crash).
    await t.tap(find.text('old chat'));
    await t.pumpAndSettle();
    expect(t.takeException(), isNull);
    expect(api.lastResumed, 't9');

    // Tap "new conversation" (+) → clears to an empty conversation.
    await t.tap(find.byIcon(Icons.add));
    await t.pumpAndSettle();
    expect(t.takeException(), isNull);
    // Tapping "new conversation" shows the new-session guidance.
    expect(find.text('想让远程 Codex 做点什么?'), findsOneWidget); // guidance (zh)
  });

  testWidgets('Plan renders as a status-iconed checklist', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/completed',
        threadId: 't1',
        itemId: 'p1',
        itemType: 'plan',
        title: '',
        // Summarizer format: explanation + `- [x|~| ] step` lines.
        text: 'Add a feature\n- [x] research\n- [~] implement\n- [ ] test',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();
    // Checklist is shown expanded by default: header + progress + each step.
    expect(find.text('计划'), findsWidgets); // toolPlan label (pill + card)
    expect(find.text('1/3'), findsOneWidget); // 1 of 3 completed
    expect(find.text('research'), findsOneWidget);
    expect(find.text('implement'), findsOneWidget);
    expect(find.text('test'), findsOneWidget);
    // Completed step shows a filled check; pending shows an empty circle.
    expect(find.byIcon(Icons.check_circle_rounded), findsOneWidget);
    expect(find.byIcon(Icons.timelapse_rounded), findsOneWidget);
    expect(find.byIcon(Icons.radio_button_unchecked), findsOneWidget);
    // The explanation renders as Markdown above the steps.
    expect(
      find.textContaining('Add a feature', findRichText: true),
      findsWidgets,
    );
  });

  testWidgets('New conversation applies the chosen permission mode', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // Default mode = Auto → on-failure / workspace-write.
    await t.enterText(find.byType(TextField), 'hi');
    await t.pump(); // let the send button enable for the non-empty input
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastApproval, 'on-failure');
    expect(api.lastSandbox, 'workspace-write');
  });

  testWidgets('App-server tile is tappable (not disabled)', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      services: const [
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
    // Service rows are tappable cards now; the row is enabled iff its InkWell
    // carries an onTap (a disabled row would have a null callback).
    final ink = t.widget<InkWell>(
      find.descendant(
        of: find.byKey(const Key('svc-pcx:lb7666:app:default')),
        matching: find.byType(InkWell),
      ),
    );
    expect(ink.onTap, isNotNull);
  });

  testWidgets('Services renders English strings under Locale(en)', (t) async {
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
    await t.pumpWidget(
      _host(const ServicesScreen(), api, locale: const Locale('en')),
    );
    await t.pumpAndSettle();
    // English ARB values, proving the locale switch changes strings.
    expect(find.text('API services'), findsOneWidget);
    expect(find.text('API 服务'), findsNothing);
  });

  testWidgets('Stop button interrupts the running turn with its turn id', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // The server starts a turn carrying a turn id; the stop button appears.
    // A running turn animates the typing indicator forever, so pump one frame
    // rather than settling.
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'turn/started',
        threadId: 't1',
        raw: '{"turnId":"turn-42"}',
      ),
    );
    await t.pump(); // deliver the broadcast event
    await t.pump(); // build the resulting frame
    expect(find.byKey(const Key('stop-btn')), findsOneWidget);

    // Tapping stop sends turn/interrupt with the captured turn id (the server
    // rejects an interrupt that omits it).
    await t.tap(find.byKey(const Key('stop-btn')));
    await t.pump();
    expect(api.interrupted, isTrue);
    expect(api.lastInterruptTurnId, 'turn-42');
  });

  testWidgets('Stop works for a thread already running when opened', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // Resumed mid-turn: history.running=true, but the UI never saw turn/started
    // so it has no turn id. Stop must still fire (the engine supplies the id).
    api.readResult = const ThreadHistory(items: [], running: true);

    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pump(); // run the resume future
    await t
        .pump(); // build with _streaming=true (don't settle: typing animates)

    expect(find.byKey(const Key('stop-btn')), findsOneWidget);
    await t.tap(find.byKey(const Key('stop-btn')));
    await t.pump();
    expect(api.interrupted, isTrue);
    expect(api.lastInterruptTurnId, isNull); // UI had none; engine falls back
  });

  testWidgets('Status bar reflects ready then working state', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // Narrow so the sessions pane is a hidden drawer — the open thread's sidebar
    // "running" subtitle then can't collide with the status bar's working text.
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(400, 800);
    addTearDown(t.view.reset);
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // Idle → the status bar reads "Ready".
    expect(find.text('就绪'), findsOneWidget); // stateReady (zh)

    // A running turn flips it to "Working…" (pump one frame — the typing
    // indicator animates forever, so the tree never fully settles).
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'turn/started',
        threadId: 't1',
        raw: '{"turnId":"x"}',
      ),
    );
    await t.pump(); // deliver the broadcast event
    await t.pump(); // build the resulting frame
    expect(find.text('运行中…'), findsOneWidget); // stateWorking (zh)
    expect(find.text('就绪'), findsNothing);
  });

  testWidgets('File change shows ± counts and expands to the diff', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/completed',
        threadId: 't1',
        itemId: 'f1',
        itemType: 'fileChange',
        title: 'lib/x.dart',
        text:
            'diff --git a/lib/x.dart b/lib/x.dart\n'
            '--- a/lib/x.dart\n'
            '+++ b/lib/x.dart\n'
            '@@ -1 +1,2 @@\n'
            '-old\n'
            '+new\n'
            '+more\n',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();

    // Collapsed: an "Edited files" header with the path and the ± counts.
    expect(find.text('修改文件'), findsOneWidget); // toolEdited (zh)
    expect(find.text('lib/x.dart'), findsOneWidget);
    expect(find.text('+2'), findsWidgets);
    expect(find.text('−1'), findsWidgets);

    // Expanding reveals the colored diff lines for review.
    await t.tap(find.text('lib/x.dart'));
    await t.pumpAndSettle();
    expect(find.text('+new'), findsOneWidget);
    expect(find.text('−old'), findsOneWidget);
  });

  testWidgets('A compaction item shows a system notice in the transcript', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/completed',
        threadId: 't1',
        itemId: 'cc1',
        itemType: 'contextCompaction',
        title: '',
        text: '',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();
    expect(find.text('对话已压缩'), findsOneWidget); // compacted (zh)
  });

  testWidgets('Services list shows availability + subscription status', (
    t,
  ) async {
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
          kind: 'api',
          name: 'other',
          key: 'pcx:lb7666:api:other',
        ),
        ServiceEntry(
          device: 'lb7666',
          kind: 'app',
          name: 'default',
          key: 'pcx:lb7666:app:default',
        ),
      ],
    );
    // One API service is subscribed (alive) → it reads "subscribed"; the rest
    // are merely registered → "online".
    await api.apiSubscribe('pcx:lb7666:api:default', 28080);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(500, 900); // narrow → single list pane
    addTearDown(t.view.reset);

    await t.pumpWidget(_host(const ServicesScreen(), api));
    await t.pumpAndSettle();

    expect(find.text('已订阅'), findsOneWidget); // subscribedAlive (zh)
    expect(find.text('在线'), findsWidgets); // relay + unsubscribed api + app
    expect(find.byType(StatusDot), findsWidgets); // availability dots render
    expect(find.byType(PulsingDot), findsNothing); // nothing running
  });

  testWidgets('Running sessions show a pulsing badge in the sessions pane', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.addAll(const [
      ThreadMeta(id: 'tOpen', preview: 'open chat', cwd: '', updatedAt: 0),
      ThreadMeta(id: 't9', preview: 'other chat', cwd: '', updatedAt: 0),
    ]);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);

    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'tOpen',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    expect(find.byType(PulsingDot), findsNothing);

    // A turn starts on the OTHER thread (t9), not the open one. The session's
    // own handler ignores cross-thread events, but the shared provider tracks
    // it, so the pane shows a running badge + header count.
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(kind: 'turn/started', threadId: 't9', raw: '{}'),
    );
    await t.pump(); // deliver the broadcast event
    await t.pump(); // build the resulting frame
    // The running thread moves into the "Active" group with a pulsing dot.
    expect(find.text('进行中'), findsOneWidget); // groupActive (zh)
    expect(find.byType(PulsingDot), findsAtLeastNWidgets(1));

    // Turn completes → the badge clears.
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(kind: 'turn/completed', threadId: 't9', raw: '{}'),
    );
    await t.pump();
    await t.pump();
    expect(find.byType(PulsingDot), findsNothing);
  });

  testWidgets('Project picker shows a running badge before a session opens', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.add(
      const ThreadMeta(
        id: 't9',
        preview: 'busy chat',
        cwd: '/proj',
        updatedAt: 0,
      ),
    );

    await t.pumpWidget(
      _host(const AppServiceScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();
    expect(find.text('busy chat'), findsOneWidget);
    expect(find.byType(PulsingDot), findsNothing);

    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(kind: 'turn/started', threadId: 't9', raw: '{}'),
    );
    await t.pump(); // deliver
    await t.pump(); // build
    expect(find.byType(PulsingDot), findsAtLeastNWidgets(1)); // tile badge
  });

  testWidgets('Stopping a turn shows a "stopped" marker in the transcript', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // Opened mid-turn: streaming, stop button available.
    api.readResult = const ThreadHistory(items: [], running: true);

    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pump(); // run resume
    await t.pump(); // build with _streaming=true

    await t.tap(find.byKey(const Key('stop-btn')));
    await t.pump();
    expect(api.interrupted, isTrue);

    // The server ends the aborted turn; a "stopped" marker appears (and no
    // error banner).
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(kind: 'turn/completed', threadId: 't1', raw: '{}'),
    );
    await t.pump();
    await t.pump();
    expect(find.text('已停止'), findsOneWidget); // turnStopped (zh)
    expect(find.byKey(const Key('session-error')), findsNothing);
  });

  testWidgets('Services refresh button re-runs discovery', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      services: const [
        ServiceEntry(
          device: 'lb7666',
          kind: 'app',
          name: 'default',
          key: 'pcx:lb7666:app:default',
        ),
      ],
    );
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(500, 900);
    addTearDown(t.view.reset);

    await t.pumpWidget(_host(const ServicesScreen(), api));
    await t.pumpAndSettle();
    expect(find.text('default'), findsOneWidget);

    // Tapping refresh re-discovers (skeleton flashes, then data) without error.
    await t.tap(find.byKey(const Key('refresh-btn')));
    await t.pumpAndSettle();
    expect(find.text('default'), findsOneWidget);
  });

  testWidgets('Picker reconnect button forces a fresh connection', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.add(
      const ThreadMeta(id: 't1', preview: 'chat', cwd: '/p', updatedAt: 0),
    );
    await t.pumpWidget(
      _host(const AppServiceScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();
    final before = api.appConnectCount;

    await t.tap(find.byKey(const Key('reconnect-btn')));
    await t.pumpAndSettle(const Duration(seconds: 2));
    expect(api.appConnectCount, greaterThan(before)); // reconnected
    expect(find.text('chat'), findsOneWidget);
    expect(find.byKey(const Key('app-connect-error')), findsNothing);
  });

  testWidgets('Picker recovers from a stale connection by reconnecting', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.add(
      const ThreadMeta(id: 't1', preview: 'past chat', cwd: '/p', updatedAt: 0),
    );
    // First thread/list fails (stale socket shown as "online"); the picker must
    // disconnect + reconnect and retry, surfacing the threads, not the error.
    api.failNextThreadList = true;

    await t.pumpWidget(
      _host(const AppServiceScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    expect(find.byKey(const Key('app-connect-error')), findsNothing);
    expect(find.text('past chat'), findsOneWidget);
  });

  testWidgets('Consecutive notices are not folded into a group', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      _host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    for (final id in ['c1', 'c2']) {
      api.pushEvent(
        'pcx:lb7666:app:default',
        AppEvent(
          kind: 'item/completed',
          threadId: 't1',
          itemId: id,
          itemType: 'contextCompaction',
          title: '',
          text: '',
          raw: '{}',
        ),
      );
    }
    await t.pumpAndSettle();
    // Both render as their own notice (not collapsed into one "×2" group).
    expect(find.text('对话已压缩'), findsNWidgets(2));
  });

  testWidgets('Services screen shows a loading skeleton before data', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      services: const [
        ServiceEntry(
          device: 'lb7666',
          kind: 'app',
          name: 'default',
          key: 'pcx:lb7666:app:default',
        ),
      ],
    );

    await t.pumpWidget(_host(const ServicesScreen(), api));
    // First frame: discovery future hasn't resolved → skeleton.
    expect(find.byType(ListLoadingSkeleton), findsOneWidget);
    await t.pumpAndSettle();
    // Data arrived → skeleton gone, the service is listed.
    expect(find.byType(ListLoadingSkeleton), findsNothing);
    expect(find.text('default'), findsOneWidget);
  });

  testWidgets('Chat loading skeleton renders a shimmer', (t) async {
    await t.pumpWidget(
      const MaterialApp(home: Scaffold(body: ChatLoadingSkeleton())),
    );
    await t.pump(); // shimmer animates forever — don't settle
    expect(find.byType(Shimmer), findsOneWidget);
    expect(find.byType(SkeletonBox), findsWidgets);
  });
}
