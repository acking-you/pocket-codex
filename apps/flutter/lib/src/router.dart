import 'package:go_router/go_router.dart';
import 'package:pocket_codex/src/screens/api_service_screen.dart';
import 'package:pocket_codex/src/screens/app_service_screen.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/screens/onboarding_screen.dart';
import 'package:pocket_codex/src/screens/services_screen.dart';
import 'package:pocket_codex/src/screens/settings_screen.dart';

/// Build the app router. [initialLocation] is `/onboarding` on first run
/// (no relay configured) and `/` otherwise.
GoRouter buildRouter({required String initialLocation}) => GoRouter(
  initialLocation: initialLocation,
  routes: [
    GoRoute(path: '/onboarding', builder: (c, s) => const OnboardingScreen()),
    GoRoute(path: '/', builder: (c, s) => const ServicesScreen()),
    GoRoute(path: '/settings', builder: (c, s) => const SettingsScreen()),
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
