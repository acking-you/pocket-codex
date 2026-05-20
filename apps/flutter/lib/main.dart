// Pocket-Codex Flutter front-end.
//
// This entry point is intentionally minimal during the bootstrap
// phase: it loads the `flutter_rust_bridge`-generated Rust library
// and renders a single screen that demonstrates a successful
// Dart ↔ Rust round-trip. Real UI flows (codex thread management,
// pb-mapper session control, etc.) will land in subsequent
// milestones.

import 'package:flutter/material.dart';
import 'package:pocket_codex/src/rust/api/simple.dart';
import 'package:pocket_codex/src/rust/frb_generated.dart';

Future<void> main() async {
  await RustLib.init();
  runApp(const PocketCodexApp());
}

/// Root widget for the Pocket-Codex Flutter front-end.
///
/// `home` is overridable so unit tests can mount the app without
/// initialising the Rust bridge.
class PocketCodexApp extends StatelessWidget {
  /// Default constructor — uses [HomeScreen] which calls into the
  /// FRB-generated bindings.
  const PocketCodexApp({super.key, this.home = const HomeScreen()});

  /// Page rendered as the app's home; substitutable for tests.
  final Widget home;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Pocket-Codex',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: Colors.indigo),
        useMaterial3: true,
      ),
      home: home,
    );
  }
}

/// Default home screen which exercises the Rust↔Dart bridge.
class HomeScreen extends StatelessWidget {
  /// Default constructor.
  const HomeScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final greeting = greet(name: 'Pocket-Codex');
    final version = bridgeVersion();
    return Scaffold(
      appBar: AppBar(title: const Text('Pocket-Codex (WIP)')),
      body: Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(greeting, style: Theme.of(context).textTheme.headlineSmall),
              const SizedBox(height: 8),
              Text('bridge version: $version'),
              const SizedBox(height: 24),
              const Text(
                'This is a placeholder UI. The real app will manage '
                'codex app-server sessions and pb-mapper relays from here.',
              ),
            ],
          ),
        ),
      ),
    );
  }
}
