import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:path_provider/path_provider.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/providers.dart';
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
  // Seed the locale provider from the persisted config (read in this same
  // bridge call — no extra latency, no language flash). null = follow system.
  final locale = cfg.locale == null ? null : Locale(cfg.locale!);
  runApp(
    ProviderScope(
      overrides: [localeProvider.overrideWith((ref) => locale)],
      child: PocketCodexApp(initialLocation: start),
    ),
  );
}

/// Shown on the web target, which the engine does not support. Localized so
/// the notice respects the system language.
class _UnsupportedPlatformApp extends StatelessWidget {
  const _UnsupportedPlatformApp();

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      onGenerateTitle: (context) => AppLocalizations.of(context).appTitle,
      theme: lightTheme(),
      darkTheme: darkTheme(),
      themeMode: ThemeMode.system,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      home: Scaffold(
        body: Center(
          child: Builder(
            builder: (context) => Padding(
              padding: const EdgeInsets.all(24),
              child: Text(
                AppLocalizations.of(context).webUnsupported,
                textAlign: TextAlign.center,
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// Root app: Material 3 light/dark following the system, go_router nav,
/// locale driven by [localeProvider].
class PocketCodexApp extends ConsumerStatefulWidget {
  /// [initialLocation] decides onboarding vs services on cold start.
  const PocketCodexApp({super.key, required this.initialLocation});

  /// Route to open on launch.
  final String initialLocation;

  @override
  ConsumerState<PocketCodexApp> createState() => _PocketCodexAppState();
}

class _PocketCodexAppState extends ConsumerState<PocketCodexApp> {
  // Built once so a locale change (which rebuilds this widget) does not
  // recreate the router and reset the navigation stack.
  late final _router = buildRouter(initialLocation: widget.initialLocation);

  @override
  Widget build(BuildContext context) {
    final locale = ref.watch(localeProvider);
    return MaterialApp.router(
      onGenerateTitle: (context) => AppLocalizations.of(context).appTitle,
      theme: lightTheme(),
      darkTheme: darkTheme(),
      themeMode: ThemeMode.system,
      locale: locale,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      routerConfig: _router,
    );
  }
}
