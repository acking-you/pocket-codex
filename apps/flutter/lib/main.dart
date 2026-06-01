import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:path_provider/path_provider.dart';
import 'package:pocket_codex/src/router.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/rust/api/bridge.dart' as frb;
import 'package:pocket_codex/src/rust/frb_generated.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();

  // Web is unsupported: the browser sandbox has no path_provider support dir
  // and no raw TCP, so the bridge (config.toml persistence + pb-mapper
  // subscribe) cannot run. Render an explicit notice instead of crashing on
  // getApplicationSupportDirectory() or showing a blank page.
  if (kIsWeb) {
    runApp(const _UnsupportedPlatformApp());
    return;
  }

  await RustLib.init();
  final dir = await getApplicationSupportDirectory();
  await frb.initBridge(supportDir: dir.path);
  final cfg = await frb.getConfig();
  final relay = cfg.relay?.trim();
  final start = (relay == null || relay.isEmpty) ? '/onboarding' : '/';
  runApp(ProviderScope(child: PocketCodexApp(initialLocation: start)));
}

/// Shown on the web target, which the engine does not support.
class _UnsupportedPlatformApp extends StatelessWidget {
  const _UnsupportedPlatformApp();

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Pocket-Codex',
      theme: lightTheme(),
      darkTheme: darkTheme(),
      themeMode: ThemeMode.system,
      home: const Scaffold(
        body: Center(
          child: Padding(
            padding: EdgeInsets.all(24),
            child: Text(
              'Pocket-Codex 需要本地网络与文件访问,暂不支持 Web。\n'
              '请使用 Android / iOS / 桌面版。',
              textAlign: TextAlign.center,
            ),
          ),
        ),
      ),
    );
  }
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
