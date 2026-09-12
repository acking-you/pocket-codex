// Opt-in native regression using the configured local host and existing
// sessions. No model turn, takeover, deletion, or message is sent.
// Run with PCX_LIVE_UI=true and PCX_HISTORY_THREAD_IDS (comma-separated).
import 'dart:io';
import 'dart:ui' show FrameTiming;
import 'package:flutter/scheduler.dart' show SchedulerBinding;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:pocket_codex/main.dart' as application;
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api_rust.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/screens/app_session/activity_cards.dart';
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
      const gapTurn = String.fromEnvironment('PCX_HISTORY_GAP_TURN_ID');
      if (!enabled || ids.isEmpty) {
        markTestSkipped('existing local-host sessions not configured');
        return;
      }
      final timings = <FrameTiming>[];
      void record(List<FrameTiming> frames) => timings.addAll(frames);
      SchedulerBinding.instance.addTimingsCallback(record);
      addTearDown(
        () => SchedulerBinding.instance.removeTimingsCallback(record),
      );
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
              theme: round.isEven ? lightTheme() : darkTheme(),
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
          final target = tester
              .widget<TurnMinimap>(minimap)
              .items
              .where((item) => item.turnId == gapTurn)
              .firstOrNull;
          if (target != null) {
            tester.widget<TurnMinimap>(minimap).onSelect(target);
            await _until(
              () => tester
                  .widget<TurnMinimap>(minimap)
                  .items
                  .any((item) => item.turnId == gapTurn && item.rowIndex >= 0),
            );
            await Future<void>.delayed(const Duration(milliseconds: 500));
            final gap = find.byKey(Key('history-gap-$gapTurn'));
            if (gap.evaluate().isNotEmpty) {
              final button = tester.widget<TextButton>(gap);
              button.onPressed?.call();
              await _until(
                () =>
                    gap.evaluate().isEmpty ||
                    tester.widget<TextButton>(gap).onPressed != null,
              );
            }
            await Future<void>.delayed(const Duration(seconds: 2));
            expect(
              tester
                  .widgetList<TurnWorkCard>(find.byType(TurnWorkCard))
                  .where((work) => work.active)
                  .length,
              lessThanOrEqualTo(1),
            );
            expect(tester.takeException(), isNull);
            debugPrint('Native jump + continuation + monitor refresh passed');
          }
        }
        await _until(
          () => find
              .byKey(const Key('history-navigation-status'))
              .evaluate()
              .isEmpty,
        );
        for (var swipe = 0; swipe < 3; swipe++) {
          await tester.timedDrag(
            find.byType(SuperListView).first,
            const Offset(0, -260),
            const Duration(milliseconds: 400),
          );
          await Future<void>.delayed(const Duration(milliseconds: 250));
          expect(tester.takeException(), isNull);
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
      if (timings.isNotEmpty) {
        final builds =
            timings
                .map((frame) => frame.buildDuration.inMicroseconds / 1000)
                .toList()
              ..sort();
        final rasters =
            timings
                .map((frame) => frame.rasterDuration.inMicroseconds / 1000)
                .toList()
              ..sort();
        final at = ((timings.length - 1) * .95).floor();
        debugPrint(
          'Native debug frames: ${timings.length}; build p95 ${builds[at].toStringAsFixed(1)} ms; raster p95 ${rasters[at].toStringAsFixed(1)} ms',
        );
      }
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    },
    timeout: const Timeout(Duration(minutes: 8)),
  );
}
