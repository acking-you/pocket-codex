import 'package:go_router/go_router.dart';
import 'package:pocket_codex/src/screens/api_service_screen.dart';
import 'package:pocket_codex/src/screens/app_service_screen.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/screens/local_session_view_screen.dart';
import 'package:pocket_codex/src/screens/local_sessions_screen.dart';
import 'package:pocket_codex/src/screens/onboarding_screen.dart';
import 'package:pocket_codex/src/screens/services_screen.dart';
import 'package:pocket_codex/src/screens/settings_screen.dart';

/// The live router instance, captured by [buildRouter] so code outside the
/// widget tree — namely the desktop tray's "Settings" item — can navigate. Null
/// until the app has built its router (so [openSettingsFromTray] is a no-op
/// before then).
GoRouter? _appRouter;

/// Open the settings screen from outside the widget tree (the desktop tray).
/// Safe to call before the router exists (no-op) and needs no BuildContext.
///
/// Uses `push`, not `go`: settings is a top-level route, so `go` would REPLACE
/// the stack, leaving the screen with nothing to pop — its AppBar shows no back
/// button and the user is stranded there. `push` mirrors the in-app settings
/// button (`context.push('/settings')`), so the back button works.
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
    GoRoute(path: '/onboarding', builder: (c, s) => const OnboardingScreen()),
    GoRoute(path: '/', builder: (c, s) => const ServicesScreen()),
    GoRoute(path: '/settings', builder: (c, s) => const SettingsScreen()),
    GoRoute(path: '/sessions', builder: (c, s) => const LocalSessionsScreen()),
    GoRoute(
      path: '/sessions/view',
      builder: (c, s) => LocalSessionViewScreen(
        threadId: s.uri.queryParameters['tid']!,
        cwd: s.uri.queryParameters['cwd'],
        preview: s.uri.queryParameters['preview'],
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
