// Live desktop regression: real bridge -> relay -> app-server -> transcript.
// Use an isolated CODEX_HOME copy: resuming a thread can update its metadata.
// Run with --dart-define-from-file containing PCX_RELAY, PCX_KEY, PCX_SERVICE,
// PCX_HISTORY_THREAD (paginated, >100 items) and PCX_SECOND_THREAD (legacy).
// Both threads must be visible in thread/list; publish the matching meta
// service too, as the screen restores its per-thread configuration from it.
// This test never starts a model turn.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/bridge_api_rust.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/rust/api/bridge.dart' as frb;
import 'package:pocket_codex/src/rust/frb_generated.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:super_sliver_list/super_sliver_list.dart';
import 'package:window_manager/window_manager.dart';

const _relay = String.fromEnvironment('PCX_RELAY');
const _key = String.fromEnvironment('PCX_KEY');
const _service = String.fromEnvironment('PCX_SERVICE');
const _paginated = String.fromEnvironment('PCX_HISTORY_THREAD');
const _legacy = String.fromEnvironment('PCX_SECOND_THREAD');

class _MemoryPrefs extends UiPrefsStore {
  @override
  Future<UiPrefs> build() async => const UiPrefs();
}

class _ObservedBridge extends RustBridgeApi {
  final histories = <String, ThreadHistory>{};
  final olderPages = <OlderPage>[];

  @override
  Future<ThreadHistory> appThreadRead(
    String serviceKey,
    String threadId,
  ) async {
    final history = await super.appThreadRead(serviceKey, threadId);
    histories[threadId] = history;
    return history;
  }

  @override
  Future<OlderPage> appThreadOlderPage(
    String serviceKey,
    String threadId,
  ) async {
    final page = await super.appThreadOlderPage(serviceKey, threadId);
    olderPages.add(page);
    return page;
  }
}

Future<void> _until(WidgetTester tester, bool Function() ready) async {
  final deadline = DateTime.now().add(const Duration(seconds: 90));
  while (!ready() && DateTime.now().isBefore(deadline)) {
    await Future<void>.delayed(const Duration(milliseconds: 100));
  }
  expect(ready(), isTrue, reason: 'live history operation did not complete');
  await Future<void>.delayed(const Duration(milliseconds: 100));
}

void main() {
  final binding = IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  binding.framePolicy = LiveTestWidgetsFlutterBindingFramePolicy.fullyLive;
  testWidgets(
    'repeated thread opens and older history through the real relay',
    (tester) async {
      if ([_relay, _key, _service, _paginated, _legacy].any((v) => v.isEmpty)) {
        markTestSkipped('isolated live-history configuration not provided');
        return;
      }
      await RustLib.init();
      await windowManager.ensureInitialized();
      await windowManager.show();
      await windowManager.focus();
      final dir = await Directory.systemTemp.createTemp('pcx-history-it-');
      await frb.initBridge(supportDir: dir.path);
      await frb.setRelay(relay: _relay);
      await frb.setKey(key: _key);
      final api = _ObservedBridge();
      await api.appConnect(_service, 0);
      addTearDown(() async {
        await tester.pumpWidget(const SizedBox.shrink());
        await api.appDisconnect(_service);
        // The native logger keeps its file open until process exit on Windows.
      });

      for (var round = 0; round < 3; round++) {
        final threads = await api.appThreadList(_service);
        expect(threads.any((thread) => thread.id == _paginated), isTrue);
        expect(threads.any((thread) => thread.id == _legacy), isTrue);
      }

      for (final threadId in [_paginated, _legacy, _paginated]) {
        api.histories.remove(threadId);
        await tester.pumpWidget(
          ProviderScope(
            overrides: [
              bridgeApiProvider.overrideWithValue(api),
              uiPrefsProvider.overrideWith(_MemoryPrefs.new),
            ],
            child: MaterialApp(
              locale: const Locale('zh'),
              localizationsDelegates: AppLocalizations.localizationsDelegates,
              supportedLocales: AppLocalizations.supportedLocales,
              home: AppSessionScreen(
                key: ValueKey(threadId),
                serviceKey: _service,
                threadId: threadId,
                home: true,
              ),
            ),
          ),
        );
        await _until(tester, () => api.histories.containsKey(threadId));
        await _until(
          tester,
          () => find.byType(SuperListView).evaluate().isNotEmpty,
        );
        expect(api.histories[threadId]!.items, isNotEmpty);
        expect(api.appIsConnected(_service), isTrue);
        expect(tester.takeException(), isNull);
        debugPrint(
          'Live history rendered: ${api.histories[threadId]!.items.length} items',
        );
      }

      expect(api.histories[_paginated]!.turns, isNotEmpty);
      expect(api.histories[_paginated]!.hasOlder, isTrue);
      // Opening settles the transcript at its end over several layout frames.
      await Future<void>.delayed(const Duration(milliseconds: 500));
      final pagesBeforeScroll = api.olderPages.length;
      final transcript = tester.widget<SuperListView>(
        find.byType(SuperListView),
      );
      transcript.controller!.jumpTo(0);
      final olderButton = find.byKey(const Key('chat-older-history-load'));
      await _until(
        tester,
        () =>
            api.olderPages.length > pagesBeforeScroll ||
            olderButton.hitTestable().evaluate().isNotEmpty,
      );
      if (api.olderPages.length == pagesBeforeScroll) {
        await tester.tap(olderButton);
      }
      await _until(tester, () => api.olderPages.length > pagesBeforeScroll);
      expect(api.olderPages.last.items, isNotEmpty);
      expect(api.appIsConnected(_service), isTrue);
      expect(tester.takeException(), isNull);
      debugPrint(
        'Live older history loaded: ${api.olderPages.last.items.length} items',
      );
    },
    timeout: const Timeout(Duration(minutes: 8)),
  );
}
