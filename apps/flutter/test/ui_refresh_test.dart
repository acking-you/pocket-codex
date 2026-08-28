import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/services_screen.dart';
import 'package:pocket_codex/src/screens/settings_screen.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/widgets/utility_page.dart';

import 'fake_bridge_api.dart';

Widget _host(
  Widget child,
  FakeBridgeApi api, {
  ThemeMode themeMode = ThemeMode.system,
}) => ProviderScope(
  overrides: [bridgeApiProvider.overrideWithValue(api)],
  child: MaterialApp(
    locale: const Locale('zh'),
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    theme: lightTheme(),
    darkTheme: darkTheme(),
    themeMode: themeMode,
    home: child,
  ),
);

Widget _routerHost(FakeBridgeApi api, GoRouter router) => ProviderScope(
  overrides: [bridgeApiProvider.overrideWithValue(api)],
  child: MaterialApp.router(
    locale: const Locale('zh'),
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    routerConfig: router,
  ),
);

void _useSize(WidgetTester tester, Size size) {
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = size;
  addTearDown(tester.view.reset);
}

GoRoute _utilityRoute({List<Widget> actions = const []}) => GoRoute(
  path: '/manage',
  builder: (_, _) => UtilityPage(
    route: '/manage',
    title: '服务管理',
    actions: actions,
    body: const Center(child: Text('manage-body')),
  ),
);

GoRoute _logsRoute() => GoRoute(
  path: '/logs',
  builder: (_, _) => const UtilityPage(
    route: '/logs',
    title: '运行日志',
    body: Center(child: Text('logs-body')),
  ),
);

void main() {
  group('utility page shell', () {
    testWidgets('wide macOS shows the conversation origin and page menu', (
      tester,
    ) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
      _useSize(tester, const Size(1300, 768));
      final router = GoRouter(
        initialLocation: '/manage',
        routes: [
          GoRoute(
            path: '/',
            builder: (_, _) => const Scaffold(body: Text('chat-home')),
          ),
          _utilityRoute(),
        ],
      );
      addTearDown(router.dispose);

      try {
        await tester.pumpWidget(_routerHost(FakeBridgeApi(), router));
        await tester.pumpAndSettle();

        expect(find.byKey(const Key('utility-chat-origin')), findsOneWidget);
        expect(find.text('对话'), findsOneWidget);
        expect(find.text('/'), findsOneWidget);
        expect(find.text('服务管理'), findsOneWidget);
        expect(find.byKey(const Key('utility-page-menu')), findsOneWidget);
        expect(find.byType(BackButton), findsNothing);

        await tester.tap(find.byKey(const Key('utility-page-menu')));
        await tester.pumpAndSettle();
        expect(find.text('页面'), findsOneWidget);
        expect(find.text('本地会话'), findsOneWidget);
        expect(find.text('运行日志'), findsOneWidget);

        await tester.sendKeyEvent(LogicalKeyboardKey.escape);
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(const Key('utility-chat-origin')));
        await tester.pumpAndSettle();
        expect(find.text('chat-home'), findsOneWidget);
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    });

    testWidgets('a detail page names its list and climbs back to it', (
      tester,
    ) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
      _useSize(tester, const Size(1300, 768));
      final router = GoRouter(
        initialLocation: '/manage',
        routes: [
          GoRoute(
            path: '/',
            builder: (_, _) => const Scaffold(body: Text('chat-home')),
          ),
          GoRoute(
            path: '/manage',
            builder: (_, _) => UtilityPage(
              route: '/manage',
              title: '服务管理',
              body: Builder(
                builder: (context) => Center(
                  child: TextButton(
                    onPressed: () => context.push('/manage/detail'),
                    child: const Text('open-detail'),
                  ),
                ),
              ),
            ),
            routes: [
              GoRoute(
                path: 'detail',
                builder: (_, _) => const UtilityPage(
                  // A detail page reports its parent's section as its route so
                  // the page menu highlights correctly; the breadcrumb must
                  // still work despite that collision.
                  route: '/manage',
                  title: 'my-device',
                  parent: UtilityParent(title: '服务管理', route: '/manage'),
                  body: Center(child: Text('detail-body')),
                ),
              ),
            ],
          ),
        ],
      );
      addTearDown(router.dispose);

      try {
        await tester.pumpWidget(_routerHost(FakeBridgeApi(), router));
        await tester.pumpAndSettle();
        await tester.tap(find.text('open-detail'));
        await tester.pumpAndSettle();

        // Three segments: the conversation origin, the list, then this page.
        expect(find.text('detail-body'), findsOneWidget);
        expect(find.byKey(const Key('utility-chat-origin')), findsOneWidget);
        expect(find.byKey(const Key('utility-parent-origin')), findsOneWidget);
        expect(find.text('my-device'), findsOneWidget);

        // Tapping the list segment actually returns to it.
        await tester.tap(find.byKey(const Key('utility-parent-origin')));
        await tester.pumpAndSettle();
        expect(find.text('open-detail'), findsOneWidget);
        expect(find.text('detail-body'), findsNothing);
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    });

    testWidgets('compact Android keeps the implied back interaction', (
      tester,
    ) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.android;
      _useSize(tester, const Size(390, 760));
      final router = GoRouter(
        initialLocation: '/',
        routes: [
          GoRoute(
            path: '/',
            builder: (_, _) => const Scaffold(body: Text('chat-home')),
          ),
          _utilityRoute(),
        ],
      );
      addTearDown(router.dispose);

      try {
        await tester.pumpWidget(_routerHost(FakeBridgeApi(), router));
        await tester.pumpAndSettle();
        router.push('/manage');
        await tester.pumpAndSettle();

        expect(find.byType(BackButton), findsOneWidget);
        expect(find.byKey(const Key('utility-chat-origin')), findsNothing);
        expect(find.byKey(const Key('utility-page-menu')), findsNothing);

        await tester.tap(find.byType(BackButton));
        await tester.pumpAndSettle();
        expect(find.text('chat-home'), findsOneWidget);
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    });

    testWidgets('switching utility pages preserves the conversation beneath', (
      tester,
    ) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
      _useSize(tester, const Size(1300, 768));
      final router = GoRouter(
        initialLocation: '/',
        routes: [
          GoRoute(
            path: '/',
            builder: (_, _) => const AppPageShortcuts(
              currentRoute: '/',
              child: Scaffold(body: TextField(key: Key('chat-draft'))),
            ),
          ),
          _utilityRoute(),
          _logsRoute(),
        ],
      );
      addTearDown(router.dispose);

      try {
        await tester.pumpWidget(_routerHost(FakeBridgeApi(), router));
        await tester.enterText(
          find.byKey(const Key('chat-draft')),
          'unsent draft',
        );

        await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
        await tester.sendKeyEvent(LogicalKeyboardKey.digit1);
        await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
        await tester.pumpAndSettle();
        expect(find.text('manage-body'), findsOneWidget);
        await tester.tap(find.byKey(const Key('utility-page-menu')));
        await tester.pumpAndSettle();
        await tester.tap(find.text('运行日志'));
        await tester.pumpAndSettle();
        expect(find.text('logs-body'), findsOneWidget);

        await tester.tap(find.byKey(const Key('utility-chat-origin')));
        await tester.pumpAndSettle();
        final field = tester.widget<EditableText>(find.byType(EditableText));
        expect(field.controller.text, 'unsent draft');
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    });

    testWidgets('narrow Windows title bar does not overflow', (tester) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.windows;
      _useSize(tester, const Size(400, 700));
      final router = GoRouter(
        initialLocation: '/manage',
        routes: [
          GoRoute(
            path: '/',
            builder: (_, _) => const Scaffold(body: Text('chat-home')),
          ),
          _utilityRoute(
            actions: [
              IconButton(onPressed: null, icon: const Icon(Icons.copy)),
              IconButton(onPressed: null, icon: const Icon(Icons.clear_all)),
            ],
          ),
        ],
      );
      addTearDown(router.dispose);

      try {
        await tester.pumpWidget(_routerHost(FakeBridgeApi(), router));
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
        expect(find.byKey(const Key('utility-chat-origin')), findsOneWidget);
        expect(find.byKey(const Key('utility-page-menu')), findsOneWidget);
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    });
  });

  group('services responsive layout', () {
    final services = <ServiceEntry>[
      const ServiceEntry(
        device: 'alpha',
        kind: 'app',
        name: 'default',
        key: 'pcx:alpha:app:default',
      ),
      const ServiceEntry(
        device: 'alpha',
        kind: 'api',
        name: 'default',
        key: 'pcx:alpha:api:default',
      ),
      const ServiceEntry(
        device: 'beta',
        kind: 'app',
        name: 'work',
        key: 'pcx:beta:app:work',
      ),
    ];

    FakeBridgeApi accountApi() => FakeBridgeApi(
      config: const ConfigInfo(
        relay: '',
        hasKey: false,
        mode: 'account',
        accountLogin: 'octocat',
      ),
      services: services,
    );

    testWidgets('wide macOS groups capabilities by selectable device', (
      tester,
    ) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
      _useSize(tester, const Size(1300, 768));

      try {
        await tester.pumpWidget(_host(const ServicesScreen(), accountApi()));
        await tester.pumpAndSettle();

        expect(find.byKey(const Key('device-alpha')), findsOneWidget);
        expect(find.byKey(const Key('device-beta')), findsOneWidget);
        expect(
          find.byKey(const Key('device-capability-pcx:alpha:app:default')),
          findsOneWidget,
        );
        expect(
          find.byKey(const Key('device-capability-pcx:alpha:api:default')),
          findsOneWidget,
        );
        expect(
          find.byKey(const Key('device-capability-meta-pcx:alpha:app:default')),
          findsOneWidget,
        );
        expect(find.byKey(const Key('connect-other-device')), findsNothing);
        expect(find.text('打开 PocketCodex 对话与实时审批'), findsNothing);
        expect(find.text('OpenAI 兼容的 /v1/responses 端点'), findsNothing);
        expect(find.text('浏览该主机上的会话与附件'), findsNothing);
        expect(
          find.byKey(const Key('device-capability-pcx:beta:app:work')),
          findsNothing,
        );

        // Setting a default is a one-off, so it lives in the row's overflow
        // rather than competing with 打开 for the row's width.
        await tester.tap(
          find.byKey(const Key('capability-menu-pcx:alpha:app:default')),
        );
        await tester.pumpAndSettle();
        await tester.tap(find.text('设为默认'));
        await tester.pumpAndSettle();
        final container = ProviderScope.containerOf(
          tester.element(find.byType(ServicesScreen)),
        );
        expect(
          container.read(uiPrefsProvider).valueOrNull?.preferredAppServiceKey,
          'pcx:alpha:app:default',
        );
        expect(
          find.descendant(
            of: find.byKey(
              const Key('device-capability-pcx:alpha:app:default'),
            ),
            matching: find.text('默认'),
          ),
          findsOneWidget,
        );

        await tester.tap(find.byKey(const Key('device-beta')));
        await tester.pumpAndSettle();
        expect(
          find.byKey(const Key('device-capability-pcx:beta:app:work')),
          findsOneWidget,
        );
        expect(
          find.byKey(const Key('device-capability-meta-pcx:beta:app:work')),
          findsOneWidget,
        );
        expect(
          find.byKey(const Key('device-capability-pcx:alpha:app:default')),
          findsNothing,
        );
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    });

    testWidgets('compact keeps devices first, one level at a time', (
      tester,
    ) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.android;
      _useSize(tester, const Size(390, 760));

      try {
        await tester.pumpWidget(_host(const ServicesScreen(), accountApi()));
        await tester.pumpAndSettle();

        // Narrow opens on the device list. Nothing is auto-selected, because a
        // screen this size can only show one level at a time.
        expect(find.byKey(const Key('device-alpha')), findsOneWidget);
        expect(
          find.byKey(const Key('device-capability-pcx:alpha:api:default')),
          findsNothing,
        );

        await tester.tap(find.byKey(const Key('device-alpha')));
        await tester.pumpAndSettle();

        // Picking a device replaces the list with its capabilities…
        expect(
          find.byKey(const Key('device-capability-pcx:alpha:api:default')),
          findsOneWidget,
        );
        expect(find.byKey(const Key('device-alpha')), findsNothing);

        // …and the back row returns to it.
        await tester.tap(find.byKey(const Key('device-back')));
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('device-alpha')), findsOneWidget);
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    });
  });

  testWidgets(
    'wide macOS settings uses direct sections without explanatory copy',
    (tester) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
      _useSize(tester, const Size(1300, 768));
      final api = FakeBridgeApi(
        config: const ConfigInfo(
          relay: '',
          hasKey: false,
          mode: 'account',
          accountLogin: 'octocat',
        ),
      );

      try {
        await tester.pumpWidget(_host(const SettingsScreen(), api));
        await tester.pumpAndSettle();

        expect(find.byKey(const Key('settings-nav-0')), findsNothing);
        expect(find.text('通用'), findsOneWidget);
        expect(find.text('Codex'), findsOneWidget);
        expect(find.text('账户与连接'), findsOneWidget);
        expect(find.text('服务与订阅'), findsOneWidget);
        expect(find.text('高级'), findsOneWidget);
        expect(find.text('外观、语言与常用偏好。'), findsNothing);
        expect(find.text('Provider、认证方式与 Codex 行为。'), findsNothing);
        expect(find.text('配置导出与诊断工具。'), findsNothing);
        expect(find.byKey(const Key('theme-system')), findsOneWidget);
        expect(find.byKey(const Key('theme-light')), findsOneWidget);
        expect(find.byKey(const Key('theme-dark')), findsOneWidget);
        expect(find.text('在线'), findsNothing);
        final exportButton = tester.widget<OutlinedButton>(
          find.descendant(
            of: find.byKey(const Key('export-btn')),
            matching: find.byType(OutlinedButton),
          ),
        );
        expect(exportButton.onPressed, isNull);
        expect(
          find.descendant(
            of: find.byKey(const Key('theme-system')),
            matching: find.text('跟随系统'),
          ),
          findsOneWidget,
        );
        expect(
          find.descendant(
            of: find.byKey(const Key('theme-light')),
            matching: find.text('明亮'),
          ),
          findsOneWidget,
        );
        expect(
          find.descendant(
            of: find.byKey(const Key('theme-dark')),
            matching: find.text('暗黑'),
          ),
          findsOneWidget,
        );

        await tester.tap(find.byKey(const Key('theme-dark')));
        await tester.pumpAndSettle();
        final container = ProviderScope.containerOf(
          tester.element(find.byType(SettingsScreen)),
        );
        expect(container.read(uiPrefsProvider).valueOrNull?.themeMode, 'dark');
        expect(find.byType(SimpleDialog), findsNothing);

        expect(find.byKey(const Key('export-btn')), findsOneWidget);
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    },
  );

  testWidgets('wide redesigned pages render in light and dark themes', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    _useSize(tester, const Size(1300, 768));
    final api = FakeBridgeApi(
      config: const ConfigInfo(
        relay: 'relay.example:7666',
        hasKey: true,
        mode: 'account',
        accountLogin: 'octocat',
      ),
      services: const [
        ServiceEntry(
          device: 'desktop',
          kind: 'app',
          name: 'default',
          key: 'pcx:desktop:app:default',
        ),
        ServiceEntry(
          device: 'desktop',
          kind: 'api',
          name: 'default',
          key: 'pcx:desktop:api:default',
        ),
      ],
    );

    try {
      for (final mode in [ThemeMode.light, ThemeMode.dark]) {
        await tester.pumpWidget(
          _host(const SettingsScreen(), api, themeMode: mode),
        );
        await tester.pumpAndSettle();
        expect(
          Theme.of(tester.element(find.byType(SettingsScreen))).brightness,
          mode == ThemeMode.light ? Brightness.light : Brightness.dark,
        );
        expect(tester.takeException(), isNull);

        await tester.pumpWidget(
          _host(const ServicesScreen(), api, themeMode: mode),
        );
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
      }
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });
}
