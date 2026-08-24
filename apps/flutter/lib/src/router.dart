import 'package:go_router/go_router.dart';
import 'package:pocket_codex/src/screens/api_service_screen.dart';
import 'package:pocket_codex/src/screens/app_service_screen.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/screens/codex_setup_screen.dart';
import 'package:pocket_codex/src/screens/home_screen.dart';
import 'package:pocket_codex/src/screens/local_session_view_screen.dart';
import 'package:pocket_codex/src/screens/account_onboarding_screen.dart';
import 'package:pocket_codex/src/screens/local_sessions_screen.dart';
import 'package:pocket_codex/src/screens/log_view_screen.dart';
import 'package:pocket_codex/src/screens/onboarding_screen.dart';
import 'package:pocket_codex/src/screens/services_screen.dart';
import 'package:pocket_codex/src/screens/settings_screen.dart';
import 'package:pocket_codex/src/screens/welcome_guide_screen.dart';

/// The live router instance, captured by [buildRouter] so code outside the
/// widget tree — namely the desktop tray's "Settings" item — can navigate. Null
/// until the app has built its router (so [openSettingsFromTray] is a no-op
/// before then).
GoRouter? _appRouter;

/// Open the settings screen from outside the widget tree (the desktop tray).
/// Safe to call before the router exists (no-op) and needs no BuildContext.
///
/// Uses `push`, not `go`, so compact layouts retain their conventional back
/// path. Wide desktop settings has its explicit Conversation origin button,
/// while mobile still relies on the pushed route's implied back button.
void openSettingsFromTray() {
  final router = _appRouter;
  if (router == null) return;
  // The tray item can be clicked repeatedly; don't stack duplicate Settings
  // pages when it's already the current route.
  if (router.routerDelegate.currentConfiguration.uri.path == '/settings') {
    return;
  }
  router.push('/settings');
}

/// Build the app router. [initialLocation] is `/onboarding` on first run
/// (no relay configured) and `/` otherwise.
GoRouter buildRouter({
  required String initialLocation,
}) => _appRouter = GoRouter(
  initialLocation: initialLocation,
  routes: [
    // Account login is the default onboarding; self-host is the advanced path.
    GoRoute(
      path: '/onboarding',
      builder: (c, s) => const AccountOnboardingScreen(),
    ),
    GoRoute(
      path: '/onboarding/self-host',
      builder: (c, s) => const OnboardingScreen(),
    ),
    // Chat-first home: resolves an app service + the latest conversation and
    // lands the user directly in the chat. All management moved to /manage.
    GoRoute(path: '/', builder: (c, s) => const HomeScreen()),
    // First-run welcome guide, shown once per device after the first sign-in.
    GoRoute(path: '/welcome', builder: (c, s) => const WelcomeGuideScreen()),
    GoRoute(path: '/manage', builder: (c, s) => const ServicesScreen()),
    GoRoute(path: '/settings', builder: (c, s) => const SettingsScreen()),
    // 自带 codex 配置向导: 检测 CODEX_HOME、填 provider 或官方登录、切换 prompt。
    GoRoute(path: '/setup/codex', builder: (c, s) => const CodexSetupScreen()),
    GoRoute(path: '/logs', builder: (c, s) => const LogViewScreen()),
    // Session browser: no param = this machine's CODEX_HOME; ?svc=<key> = the
    // host behind that app service, read over its meta tunnel.
    GoRoute(
      path: '/sessions',
      builder: (c, s) {
        final svc = s.uri.queryParameters['svc'];
        return LocalSessionsScreen(
          source: (svc == null || svc.isEmpty)
              ? const SessionSource.local()
              : SessionSource.remote(svc),
        );
      },
    ),
    GoRoute(
      path: '/sessions/view',
      builder: (c, s) => LocalSessionViewScreen(
        threadId: s.uri.queryParameters['tid']!,
        cwd: s.uri.queryParameters['cwd'],
        preview: s.uri.queryParameters['preview'],
        serviceKey: s.uri.queryParameters['svc'],
      ),
    ),
    GoRoute(
      path: '/api/:key',
      builder: (c, s) => ApiServiceScreen(serviceKey: s.pathParameters['key']!),
    ),
    GoRoute(
      path: '/app/:key',
      builder: (c, s) => AppServiceScreen(serviceKey: s.pathParameters['key']!),
    ),
    GoRoute(
      path: '/app/:key/session',
      builder: (c, s) => AppSessionScreen(
        serviceKey: s.pathParameters['key']!,
        threadId: s.uri.queryParameters['tid'],
        cwd: s.uri.queryParameters['cwd'],
      ),
    ),
  ],
);
