/// Getting in and getting set up: account sign-in, codex setup, settings, and
/// self-host onboarding.
library;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/account_onboarding_screen.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/screens/codex_setup_screen.dart';
import 'package:pocket_codex/src/screens/onboarding_screen.dart';
import 'package:pocket_codex/src/screens/settings_screen.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/widgets/loading.dart';

import '../fake_bridge_api.dart';
import '../support/screen_harness.dart';

void main() {
  // AppSessionScreen keeps per-thread plan/effort memory in process-wide static
  // maps (so a reopened thread restores its mode before the persisted config
  // lands). Reset it between tests so memory from one test can't leak into
  // another that reuses a thread id.
  setUp(AppSessionScreen.debugResetThreadMemory);

  group('Codex setup', () {
    /// Mount the wizard with [status] seeded, wide enough for the desktop card
    /// layout.
    Future<FakeBridgeApi> pump(
      WidgetTester t, {
      required CodexSetupStatus status,
    }) async {
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      )..codexStatus = status;
      t.view.devicePixelRatio = 1.0;
      t.view.physicalSize = const Size(1200, 1400);
      addTearDown(t.view.reset);
      await t.pumpWidget(host(const CodexSetupScreen(), api));
      await t.pumpAndSettle();
      return api;
    }

    const unconfigured = CodexSetupStatus(
      codexHome: r'C:\Users\test\.codex',
      hasConfig: false,
      hasAuth: false,
      hasCustomProvider: false,
      needsSetup: true,
      promptVariant: 'default',
    );
    const signedIn = CodexSetupStatus(
      codexHome: r'C:\Users\test\.codex',
      hasConfig: true,
      hasAuth: true,
      hasCustomProvider: false,
      authMode: 'chatgpt',
      needsSetup: false,
      promptVariant: 'default',
    );

    testWidgets('unconfigured leads with the provider form and says why', (
      t,
    ) async {
      await pump(t, status: unconfigured);

      // The state card answers "is this working" before any method is offered.
      expect(find.text('尚未配置'), findsOneWidget); // codexSetupStatusNeedSetup
      // Provider is the open method — it is the one that needs no running host.
      expect(find.byKey(const Key('codex-base-url')), findsOneWidget);
      // ChatGPT is present but collapsed: its button is not on screen, only the
      // way to switch to it.
      expect(find.byKey(const Key('codex-login-chatgpt')), findsNothing);
      expect(find.text('改用此项'), findsOneWidget); // codexSetupSwitchTo
    });

    testWidgets('signed in hides the provider fields it cannot use', (t) async {
      await pump(t, status: signedIn);

      // The live method leads, marked as such...
      expect(find.text('使用中'), findsOneWidget); // codexSetupInUse
      expect(find.byKey(const Key('codex-login-done')), findsOneWidget);
      expect(find.byKey(const Key('codex-logout-btn')), findsOneWidget);
      // ...and the three provider inputs — unusable while a credential is in
      // force — are behind the collapsed row rather than in the user's face.
      expect(find.byKey(const Key('codex-base-url')), findsNothing);
      expect(find.byKey(const Key('codex-api-key')), findsNothing);
    });

    testWidgets('switching to the collapsed method opens its controls', (
      t,
    ) async {
      await pump(t, status: signedIn);
      expect(find.byKey(const Key('codex-base-url')), findsNothing);

      await t.tap(find.text('改用此项')); // codexSetupSwitchTo
      await t.pumpAndSettle();

      // The provider form is now open; the ChatGPT row keeps its "in use" pill
      // (switching the disclosure must not pretend the live method changed).
      expect(find.byKey(const Key('codex-base-url')), findsOneWidget);
      expect(find.text('使用中'), findsOneWidget);
    });

    testWidgets('saving a provider reports success and re-reads the status', (
      t,
    ) async {
      final api = await pump(t, status: unconfigured);

      await t.enterText(
        find.byKey(const Key('codex-base-url')),
        'https://example.com/v1',
      );
      await t.enterText(find.byKey(const Key('codex-api-key')), 'sk-test');
      await t.tap(find.byKey(const Key('codex-save-provider')));
      await t.pumpAndSettle();

      expect(api.lastProvider?.baseUrl, 'https://example.com/v1');
      expect(find.byKey(const Key('codex-setup-info')), findsOneWidget);
      // The state card followed the write instead of still reading "not
      // configured" under a success message.
      expect(find.text('尚未配置'), findsNothing);
    });

    testWidgets('the prompt switch lives under Advanced and persists', (
      t,
    ) async {
      final api = await pump(t, status: signedIn);
      expect(find.text('高级'), findsOneWidget); // codexSetupAdvanced

      await t.tap(find.byKey(const Key('codex-nondegraded-toggle')));
      await t.pumpAndSettle();

      expect(api.codexStatus.promptVariant, 'non_degraded');
    });
  });

  testWidgets('Onboarding explains why an expired account must sign in', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(
        relay: '',
        hasKey: false,
        mode: 'account',
        accountLogin: 'octocat',
      ),
    );
    await t.pumpWidget(
      host(const AccountOnboardingScreen(sessionExpired: true), api),
    );
    await t.pumpAndSettle();

    expect(find.byKey(const Key('account-session-expired')), findsOneWidget);
    expect(find.text('登录已过期'), findsOneWidget);
    expect(find.textContaining('请重新登录以恢复服务连接'), findsOneWidget);
    expect(find.text('设备码登录'), findsOneWidget);
  });

  testWidgets('Settings shows masked key and relay', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await t.pumpWidget(host(const SettingsScreen(), api));
    await t.pumpAndSettle();
    expect(find.text('lb7666.top:7666'), findsOneWidget);
    expect(find.text('•••••••• (已设置)'), findsOneWidget);
    expect(find.byKey(const Key('export-btn')), findsOneWidget);
  });

  testWidgets('compact Settings can reach the logs', (t) async {
    // The page menu that carries Logs is desktop-only, and the other compact
    // shortcut is in the chat drawer — unopenable for the user this matters to,
    // whose host is unreachable. Without a row here the logs explaining the
    // failure were only reachable after it was fixed.
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await t.pumpWidget(
      routerHost(
        api,
        initial: '/settings',
        routes: [
          GoRoute(path: '/settings', builder: (_, _) => const SettingsScreen()),
          stub('/logs', 'logs-page'),
        ],
      ),
    );
    await t.pumpAndSettle();

    // The row is below the fold in a 600 px test viewport.
    await t.ensureVisible(find.byKey(const Key('diagnostics-btn')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('diagnostics-btn')));
    await t.pumpAndSettle();
    expect(find.text('logs-page'), findsOneWidget);
  });

  testWidgets('onboarding: sign in shows the code, then authorized opens the '
      'first-run guide', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: '', hasKey: false),
    )..accountPollStatus = 'authorized';
    await t.pumpWidget(
      routerHost(
        api,
        initial: '/onboarding',
        routes: [
          GoRoute(
            path: '/onboarding',
            builder: (_, _) => const AccountOnboardingScreen(),
          ),
          stub('/', 'HOME-ROUTE'),
          stub('/welcome', 'WELCOME-ROUTE'),
        ],
      ),
    );
    await t.pumpAndSettle(); // initial onboarding (no spinner yet)
    // Warm the prefs snapshot so the post-sign-in routing decides
    // synchronously (the real prefs file load never completes under the
    // test's fake async, and waiting out its bounded timeout would also
    // expire the success toast).
    ProviderScope.containerOf(
      t.element(find.byType(AccountOnboardingScreen)),
    ).read(uiPrefsProvider.notifier).setLastService('seed');
    await t.tap(find.text('设备码登录')); // accountUseDeviceCode (zh): device flow
    // The polling spinner is a perpetual animation, so advance via bounded pumps
    // (pumpAndSettle would never settle while it spins).
    await t.pump(); // _startDevice: accountLoginStart resolves
    await t.pump(); // setState shows the code + spinner
    expect(find.text('ABCD-1234'), findsOneWidget); // user code shown
    expect(find.text('打开 GitHub'), findsOneWidget); // accountOpenGitHub (zh)
    await t.pump(const Duration(seconds: 6)); // fire the 5s poll interval
    await t.pump(); // accountLoginPoll resolves → go('/welcome')
    await t.pump(); // router rebuilds
    // First sign-in on this device → the focused welcome guide, not the home.
    expect(find.text('WELCOME-ROUTE'), findsOneWidget);
  });

  testWidgets('onboarding: the device code copies itself and confirms', (
    t,
  ) async {
    final copied = <String>[];
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
    // 'expired' lets the poll reach a terminal state, so no spinner or timer is
    // left pending at teardown (same reason as the expired-code test below).
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: '', hasKey: false),
    )..accountPollStatus = 'expired';
    await t.pumpWidget(host(const AccountOnboardingScreen(), api));
    await t.pumpAndSettle();
    await t.tap(find.text('设备码登录')); // accountUseDeviceCode (zh)
    await t.pump(); // accountLoginStart resolves
    await t.pump(); // the code renders

    // The code is the tap target: copying is the only reason it is on screen,
    // so it doesn't hide behind a separate button.
    await t.tap(find.byKey(const Key('account-code-copy')));
    await t.pump();
    await t.pump(const Duration(milliseconds: 400));

    expect(copied.single, 'ABCD-1234');
    // And it says so — a silent clipboard write leaves the user unsure whether
    // to retype the code by hand.
    expect(find.text('已复制'), findsOneWidget); // copied (zh)

    await t.pump(const Duration(seconds: 6)); // poll fires → expired → stops
    await t.pumpAndSettle();
  });

  testWidgets('onboarding: an expired code clears and shows the expired '
      'message without navigating', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: '', hasKey: false),
    )..accountPollStatus = 'expired';
    await t.pumpWidget(
      routerHost(
        api,
        initial: '/onboarding',
        routes: [
          GoRoute(
            path: '/onboarding',
            builder: (_, _) => const AccountOnboardingScreen(),
          ),
          stub('/', 'HOME-ROUTE'),
        ],
      ),
    );
    await t.pumpAndSettle(); // initial onboarding (no spinner yet)
    await t.tap(find.text('设备码登录')); // device-code fallback
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

  testWidgets('onboarding: browser sign-in (default) exchanges and navigates '
      'home', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: '', hasKey: false),
    );
    await t.pumpWidget(
      routerHost(
        api,
        initial: '/onboarding',
        routes: [
          GoRoute(
            path: '/onboarding',
            builder: (_, _) => const AccountOnboardingScreen(),
          ),
          stub('/', 'HOME-ROUTE'),
          stub('/welcome', 'WELCOME-ROUTE'),
        ],
        // The browser hand-off returns a redirect whose state matches the fake
        // bridge's started flow ('fake-state'), carrying a one-time code.
        overrides: [
          webAuthenticatorProvider.overrideWithValue(
            FakeWebAuthenticator(
              'pocketcodex://auth?exchange_code=xc1&state=fake-state',
            ),
          ),
        ],
      ),
    );
    await t.pumpAndSettle();
    // Warm the prefs snapshot so the post-sign-in routing decides
    // synchronously (see the device-code test above).
    ProviderScope.containerOf(
      t.element(find.byType(AccountOnboardingScreen)),
    ).read(uiPrefsProvider.notifier).setLastService('seed');
    // The PRIMARY button is the browser flow (the convenient default).
    await t.tap(find.text('使用 GitHub 登录'));
    // Flush the async chain (start → authenticate → exchange → toast + go) with
    // bounded pumps; pumpAndSettle would advance past the toast's 3s auto-dismiss.
    for (var i = 0; i < 8; i++) {
      await t.pump(const Duration(milliseconds: 20));
    }
    // First sign-in on this device → the focused welcome guide.
    expect(find.text('WELCOME-ROUTE'), findsOneWidget);
    expect(api.lastWebRedirectUri, isNotNull); // the web flow ran
    // Success toast (root ScaffoldMessenger) survives the navigation.
    expect(find.textContaining('octocat'), findsOneWidget);
    await t.pumpAndSettle(); // drain the SnackBar timer
  });

  testWidgets('onboarding: a later sign-in on this device skips the guide', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: '', hasKey: false),
    )..accountPollStatus = 'authorized';
    await t.pumpWidget(
      routerHost(
        api,
        initial: '/onboarding',
        routes: [
          GoRoute(
            path: '/onboarding',
            builder: (_, _) => const AccountOnboardingScreen(),
          ),
          stub('/', 'HOME-ROUTE'),
          stub('/welcome', 'WELCOME-ROUTE'),
        ],
      ),
    );
    await t.pumpAndSettle();
    // The guide was already seen on this device (e.g. sign-out → sign-in).
    ProviderScope.containerOf(
      t.element(find.byType(AccountOnboardingScreen)),
    ).read(uiPrefsProvider.notifier).markGuideSeen();
    await t.tap(find.text('设备码登录'));
    await t.pump();
    await t.pump();
    await t.pump(const Duration(seconds: 6)); // fire the 5s poll interval
    await t.pump();
    await t.pump();
    expect(find.text('HOME-ROUTE'), findsOneWidget); // straight to the chat
    expect(find.text('WELCOME-ROUTE'), findsNothing);
  });

  testWidgets('onboarding: a cancelled browser sign-in guides to device code', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: '', hasKey: false),
    );
    await t.pumpWidget(
      routerHost(
        api,
        initial: '/onboarding',
        routes: [
          GoRoute(
            path: '/onboarding',
            builder: (_, _) => const AccountOnboardingScreen(),
          ),
          stub('/', 'HOME-ROUTE'),
        ],
        overrides: [
          webAuthenticatorProvider.overrideWithValue(
            FakeWebAuthenticator(
              '',
              error: PlatformException(code: 'CANCELED'),
            ),
          ),
        ],
      ),
    );
    await t.pumpAndSettle();
    await t.tap(find.text('使用 GitHub 登录'));
    await t.pumpAndSettle();
    // Cancelling the browser tab (e.g. after GitHub wouldn't load) does NOT
    // navigate; it surfaces guidance pointing at the reliable device-code path,
    // with that button still on screen.
    expect(find.text('HOME-ROUTE'), findsNothing);
    expect(
      find.textContaining('设备码'),
      findsWidgets,
    ); // guidance + button mention it
    expect(find.text('设备码登录'), findsOneWidget); // device-code fallback present
    expect(
      find.text('使用 GitHub 登录'),
      findsOneWidget,
    ); // can retry the browser too
  });

  testWidgets('onboarding: a browser sign-in that never returns also guides to '
      'the device code', (t) async {
    // Not every browser failure is a dismissal: the desktop loopback listener
    // can time out, and a redirect can land in a browser profile that isn't
    // signed in. Those used to surface as a raw transport error with no next
    // step, even though the remedy is the same as CANCELED's.
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: '', hasKey: false),
    );
    await t.pumpWidget(
      routerHost(
        api,
        initial: '/onboarding',
        routes: [
          GoRoute(
            path: '/onboarding',
            builder: (_, _) => const AccountOnboardingScreen(),
          ),
          stub('/', 'HOME-ROUTE'),
        ],
        overrides: [
          webAuthenticatorProvider.overrideWithValue(
            FakeWebAuthenticator('', error: StateError('listener timed out')),
          ),
        ],
      ),
    );
    await t.pumpAndSettle();
    await t.tap(find.text('使用 GitHub 登录'));
    await t.pumpAndSettle();

    expect(find.text('HOME-ROUTE'), findsNothing);
    // Leads with the remedy, and the device-code button is still reachable.
    expect(find.textContaining('设备码'), findsWidgets);
    expect(find.text('设备码登录'), findsOneWidget);
    // The cause is kept, so a real bug stays diagnosable from a screenshot.
    expect(find.textContaining('listener timed out'), findsOneWidget);
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
      routerHost(
        api,
        initial: '/settings',
        routes: [
          GoRoute(path: '/settings', builder: (_, _) => const SettingsScreen()),
          stub('/onboarding', 'ONBOARDING-ROUTE'),
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

  testWidgets('onboarding: signing in makes the app SEE the account', (
    t,
  ) async {
    // The bug this locks: config is a FutureProvider that had already resolved
    // (tokenless) before the login, and nothing invalidated it afterwards. The
    // token reached config.toml, but every `mode == 'account'` gate in the app
    // — Settings' account section, the home hosting CTA, the Sessions/Hosting
    // tabs — kept reading the pre-login snapshot and behaved as signed out.
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: '', hasKey: false),
    )..accountPollStatus = 'authorized';
    await t.pumpWidget(
      routerHost(
        api,
        initial: '/onboarding',
        routes: [
          GoRoute(
            path: '/onboarding',
            builder: (_, _) => const AccountOnboardingScreen(),
          ),
          stub('/', 'HOME-ROUTE'),
          // Land on the real Settings screen: it is the visible symptom, and it
          // renders the identity only when the config says we're signed in.
          GoRoute(path: '/welcome', builder: (_, _) => const SettingsScreen()),
        ],
      ),
    );
    await t.pumpAndSettle();
    final container = ProviderScope.containerOf(
      t.element(find.byType(AccountOnboardingScreen)),
    );
    container.read(uiPrefsProvider.notifier).setLastService('seed');

    // Warm the config cache the way the real app does — reading it BEFORE the
    // login is what made the stale value stick.
    await container.read(configProvider.future);
    expect(container.read(configProvider).valueOrNull?.mode, 'unconfigured');

    await t.tap(find.text('设备码登录'));
    await t.pump();
    await t.pump();
    await t.pump(const Duration(seconds: 6));
    await t.pump();
    await t.pumpAndSettle();

    // The provider re-read the post-login config, so the identity is on screen.
    expect(container.read(configProvider).valueOrNull?.mode, 'account');
    expect(find.text('@octocat'), findsOneWidget);
  });

  testWidgets('onboarding: desktop leads with the device code, mobile with the '
      'browser', (t) async {
    // The two idioms don't cost the same per platform: a desktop redirect comes
    // back through a loopback listener and usually opens whichever browser
    // profile is default — often not the one signed into GitHub — while the
    // device code has no redirect at all. On a phone the deep link returns
    // straight to the app, so tapping through is the shortest path.
    Future<void> mount() async {
      await t.pumpWidget(
        routerHost(
          FakeBridgeApi(config: const ConfigInfo(relay: '', hasKey: false)),
          initial: '/onboarding',
          routes: [
            GoRoute(
              path: '/onboarding',
              builder: (_, _) => const AccountOnboardingScreen(),
            ),
          ],
        ),
      );
      await t.pumpAndSettle();
    }

    // Primary action = the FilledButton; the other stays available as a
    // TextButton, so neither platform loses a way in.
    String primaryLabel() => t
        .widget<Text>(
          find.descendant(
            of: find.byType(FilledButton),
            matching: find.byType(Text),
          ),
        )
        .data!;

    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    try {
      await mount();
      expect(primaryLabel(), '设备码登录'); // accountUseDeviceCode
      expect(find.byType(TextButton), findsWidgets); // browser still offered
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }

    // The harness reports android, which is the mobile default.
    await mount();
    expect(primaryLabel(), '使用 GitHub 登录'); // accountSignInButton
  });

  testWidgets('Chat loading skeleton renders a shimmer', (t) async {
    await t.pumpWidget(
      const MaterialApp(home: Scaffold(body: ChatLoadingSkeleton())),
    );
    await t.pump(); // shimmer animates forever — don't settle
    expect(find.byType(Shimmer), findsOneWidget);
    expect(find.byType(SkeletonBox), findsWidgets);
  });

  testWidgets('self-host setup can be left again', (t) async {
    // Reached from account onboarding with `go`, which REPLACES the stack — so
    // there is nothing to pop, and without an explicit way back the only exits
    // are finishing self-host setup or killing the app. A user who opened
    // "Advanced" to look at the relay fields was stranded.
    await t.pumpWidget(
      routerHost(
        FakeBridgeApi(config: const ConfigInfo(relay: '', hasKey: false)),
        initial: '/onboarding',
        routes: [
          GoRoute(
            path: '/onboarding',
            builder: (_, _) => const AccountOnboardingScreen(),
          ),
          GoRoute(
            path: '/onboarding/self-host',
            builder: (_, _) => const OnboardingScreen(),
          ),
        ],
      ),
    );
    await t.pumpAndSettle();

    await t.tap(find.text('高级 / 自部署')); // accountAdvanced
    await t.pumpAndSettle();
    await t.tap(find.text('改用自建 relay')); // accountAdvancedSelfHost
    await t.pumpAndSettle();
    expect(find.byType(OnboardingScreen), findsOneWidget);

    await t.tap(find.byKey(const Key('self-host-back')));
    await t.pumpAndSettle();
    expect(
      find.byType(AccountOnboardingScreen),
      findsOneWidget,
      reason: 'back must return to account onboarding, not strand the user',
    );
  });
}
