import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:path_provider/path_provider.dart';
import 'package:pocket_codex/src/router.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/rust/api/bridge.dart' as frb;
import 'package:pocket_codex/src/rust/frb_generated.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  final dir = await getApplicationSupportDirectory();
  await frb.initBridge(supportDir: dir.path);
  final cfg = await frb.getConfig();
  final start = cfg.relay == null ? '/onboarding' : '/';
  runApp(ProviderScope(child: PocketCodexApp(initialLocation: start)));
}

/// Root app: Material 3 light/dark following the system, go_router nav.
class PocketCodexApp extends StatelessWidget {
  /// [initialLocation] decides onboarding vs services on cold start.
  const PocketCodexApp({super.key, required this.initialLocation});

  /// Route to open on launch.
  final String initialLocation;

  @override
  Widget build(BuildContext context) {
    return MaterialApp.router(
      title: 'Pocket-Codex',
      theme: lightTheme(),
      darkTheme: darkTheme(),
      themeMode: ThemeMode.system,
      routerConfig: buildRouter(initialLocation: initialLocation),
    );
  }
}
