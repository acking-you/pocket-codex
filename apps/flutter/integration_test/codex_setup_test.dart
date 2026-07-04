// Native end-to-end test for the 自带-codex setup path: loads the real Rust
// dylib and exercises detection → provider write → prompt toggle across the FRB
// boundary, then drives the actual [CodexSetupScreen] widget with the real
// bridge. The write assertions run only when `CODEX_HOME` points at a temp dir
// (set by the launcher) so this never clobbers a real `~/.codex`.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api_rust.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/codex_setup_screen.dart';
import 'package:pocket_codex/src/rust/api/bridge.dart' as frb;
import 'package:pocket_codex/src/rust/frb_generated.dart';

/// True when CODEX_HOME is an isolated temp dir we may freely write to.
bool get _isolatedCodexHome {
  final home = Platform.environment['CODEX_HOME'];
  if (home == null || home.isEmpty) return false;
  final tmp = Directory.systemTemp.path.toLowerCase();
  return home.toLowerCase().startsWith(tmp);
}

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() async {
    await RustLib.init();
    final support = await Directory.systemTemp.createTemp('pcx-it-support-');
    await frb.initBridge(supportDir: support.path);
  });

  testWidgets('codex setup status round-trips across the FRB boundary', (
    tester,
  ) async {
    // Read-only: proves the new DTO + call marshal correctly on the real dylib.
    final status = await frb.codexSetupStatus();
    expect(status.codexHome, isNotEmpty);
    // promptVariant is always one of the three known tags.
    expect(
      ['default', 'non_degraded', 'custom'].contains(status.promptVariant),
      isTrue,
      reason: 'unexpected prompt variant ${status.promptVariant}',
    );
  });

  testWidgets('provider write + prompt toggle land on disk', (tester) async {
    if (!_isolatedCodexHome) {
      // No isolated CODEX_HOME — skip the write path rather than touch a real
      // ~/.codex. The read-only test above still ran.
      return;
    }
    final codexHome = Platform.environment['CODEX_HOME']!;
    final configFile = File('$codexHome${Platform.pathSeparator}config.toml');

    // 1. Provider write.
    await frb.codexSetupProvider(
      baseUrl: 'https://it.example.com/v1/',
      apiKey: 'sk-it-secret',
      model: 'gpt-5.5',
    );
    final afterProvider = await frb.codexSetupStatus();
    expect(afterProvider.hasCustomProvider, isTrue);
    expect(afterProvider.needsSetup, isFalse);

    final config = await configFile.readAsString();
    expect(config, contains('model_provider = "pocket"'));
    expect(config, contains('[model_providers.pocket]'));
    expect(config, contains('base_url = "https://it.example.com/v1"'));
    expect(config, contains('experimental_bearer_token = "sk-it-secret"'));

    // 2. Non-degraded prompt toggle sets model_instructions_file.
    await frb.codexSetPromptVariant(variant: 'non_degraded');
    expect(await frb.codexPromptVariant(), 'non_degraded');
    expect(
      await configFile.readAsString(),
      contains('model_instructions_file'),
    );

    // 3. Back to default clears it, preserving the provider.
    await frb.codexSetPromptVariant(variant: 'default');
    expect(await frb.codexPromptVariant(), 'default');
    final back = await configFile.readAsString();
    expect(back.contains('model_instructions_file'), isFalse);
    expect(back, contains('[model_providers.pocket]'));
  });

  testWidgets('CodexSetupScreen drives the real bridge to write a provider', (
    tester,
  ) async {
    if (!_isolatedCodexHome) return;
    final codexHome = Platform.environment['CODEX_HOME']!;
    final configFile = File('$codexHome${Platform.pathSeparator}config.toml');
    // Start clean so this test's assertion is unambiguous.
    if (await configFile.exists()) await configFile.delete();

    await tester.pumpWidget(
      ProviderScope(
        overrides: [bridgeApiProvider.overrideWithValue(const RustBridgeApi())],
        child: MaterialApp(
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          home: const CodexSetupScreen(),
        ),
      ),
    );
    await tester.pumpAndSettle();

    await tester.enterText(
      find.byKey(const Key('codex-base-url')),
      'https://ui.example.com/v1',
    );
    await tester.enterText(
      find.byKey(const Key('codex-api-key')),
      'sk-ui-secret',
    );
    await tester.tap(find.byKey(const Key('codex-save-provider')));
    await tester.pumpAndSettle();

    // The success line renders...
    expect(find.byKey(const Key('codex-setup-info')), findsOneWidget);
    // ...and the provider actually reached CODEX_HOME through the real bridge.
    final config = await configFile.readAsString();
    expect(config, contains('experimental_bearer_token = "sk-ui-secret"'));
    expect(config, contains('base_url = "https://ui.example.com/v1"'));
  });
}
