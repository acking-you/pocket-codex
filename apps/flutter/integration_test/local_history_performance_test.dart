// Opt-in native regression using the configured local host and existing
// sessions. No model turn, takeover, deletion, or message is sent.
// Run with PCX_LIVE_UI=true and PCX_HISTORY_THREAD_IDS (comma-separated).
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:pocket_codex/main.dart' as application;
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api_rust.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/widgets/loading.dart';
import 'package:pocket_codex/src/widgets/turn_minimap.dart';
import 'package:super_sliver_list/super_sliver_list.dart';
import 'package:window_manager/window_manager.dart';

class _MemoryPrefs extends UiPrefsStore {
  @override
  Future<UiPrefs> build() async => const UiPrefs();

  @override
  void setComposerHeight(double height) {
    state = AsyncData(
      (state.valueOrNull ?? const UiPrefs()).copyWith(composerHeight: height),
    );
  }
}

Future<void> _until(bool Function() condition) async {
  final deadline = DateTime.now().add(const Duration(seconds: 60));
  while (!condition() && DateTime.now().isBefore(deadline)) {
    await Future<void>.delayed(const Duration(milliseconds: 50));
  }
  expect(condition(), isTrue, reason: 'native history did not become ready');
}

void main() {
  final binding = IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  binding.framePolicy = LiveTestWidgetsFlutterBindingFramePolicy.fullyLive;
  testWidgets(
    'native long-history opens, monitoring, navigation and resize',
    (tester) async {
      const enabled = bool.fromEnvironment('PCX_LIVE_UI');
      const ids = String.fromEnvironment('PCX_HISTORY_THREAD_IDS');
      if (!enabled || ids.isEmpty) {
        markTestSkipped('existing local-host sessions not configured');
        return;
      }
      await application.main();
      await windowManager.ensureInitialized();
      await windowManager.setSize(const Size(1440, 960));
      await windowManager.show();
      await _until(() => find.byType(AppSessionScreen).evaluate().isNotEmpty);
      final service = tester
          .widget<AppSessionScreen>(find.byType(AppSessionScreen))
          .serviceKey;
      final samples = <int>[];
      final threads = ids.split(',');
      for (var round = 0; round < 6; round++) {
        final thread = threads[round % threads.length];
        final start = Stopwatch()..start();
        await tester.pumpWidget(
          ProviderScope(
            key: ValueKey(round),
            overrides: [
              bridgeApiProvider.overrideWithValue(const RustBridgeApi()),
              uiPrefsProvider.overrideWith(_MemoryPrefs.new),
            ],
            child: MaterialApp(
              debugShowCheckedModeBanner: false,
              locale: const Locale('zh'),
              localizationsDelegates: AppLocalizations.localizationsDelegates,
              supportedLocales: AppLocalizations.supportedLocales,
              theme: lightTheme(),
              home: AppSessionScreen(
                serviceKey: service,
                threadId: thread,
                home: true,
              ),
            ),
          ),
        );
        await _until(
          () =>
              find.byType(SuperListView).evaluate().isNotEmpty &&
              find.byType(ChatLoadingSkeleton).evaluate().isEmpty,
        );
        samples.add(start.elapsedMilliseconds);
        await Future<void>.delayed(const Duration(milliseconds: 350));
        expect(tester.takeException(), isNull);
        final minimap = find.byType(TurnMinimap);
        if (minimap.evaluate().isNotEmpty) {
          final ticks = tester.widget<TurnMinimap>(minimap);
          if (ticks.items.isNotEmpty) {
            final selection = Stopwatch()..start();
            ticks.onSelect(ticks.items.first);
            await _until(() => find.text('对话开始').evaluate().isNotEmpty);
            debugPrint(
              'Native first-turn selection: ${selection.elapsedMilliseconds} ms',
            );
            expect(
              find.byKey(const Key('chat-older-history-load')),
              findsNothing,
            );
          }
        }
        final handle = find.byKey(const Key('composer-resize-handle'));
        if (handle.evaluate().isNotEmpty) {
          await tester.drag(handle, const Offset(0, -64));
          await tester.pump();
          expect(
            tester.getSize(find.byKey(const Key('composer-input-area'))).height,
            greaterThan(32),
          );
        }
        debugPrint(
          'Native open round $round: ${samples.last} ms; RSS ${ProcessInfo.currentRss ~/ (1024 * 1024)} MiB',
        );
        expect(tester.takeException(), isNull);
      }
      debugPrint('Native open samples (ms): $samples');
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    },
    timeout: const Timeout(Duration(minutes: 8)),
  );
}
