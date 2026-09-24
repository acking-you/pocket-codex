import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';

import 'fake_bridge_api.dart';
import 'support/screen_harness.dart';

const service = 'pcx:cache-test:app:default';
const thread = 'cached-thread';

ThreadHistory history(String text, {String epoch = 'one'}) => ThreadHistory(
  running: false,
  historyEpoch: epoch,
  items: [
    ThreadItem(
      id: 'message',
      turnId: 'turn',
      itemType: 'agentMessage',
      title: '',
      text: text,
    ),
  ],
);

Future<void> frames(WidgetTester tester) async {
  for (var i = 0; i < 8; i++) {
    await tester.pump(const Duration(milliseconds: 100));
  }
}

class FailingSyncApi extends FakeBridgeApi {
  @override
  Future<bool> appHistorySyncPrepare(String serviceKey) async =>
      throw StateError('sync unavailable');
}

class PrefetchApi extends FakeBridgeApi {
  final prefetched = <String>[];
  final pending = Completer<void>();

  @override
  Future<List<LocalSession>> metaSessions(
    String serviceKey, {
    bool runningOnly = false,
  }) async => [
    for (final id in ['one', 'two', 'three'])
      LocalSession(
        threadId: id,
        preview: id,
        updatedAt: 1,
        turnState: 'incomplete',
        heldOpen: true,
        safety: 'ownedRunning',
        allowsResume: false,
        requiresTakeover: false,
      ),
  ];

  @override
  Future<void> appHistoryPrefetch(String serviceKey, String threadId) async {
    prefetched.add(threadId);
    if (prefetched.length == 1) await pending.future;
  }
}

void main() {
  for (final dark in [false, true]) {
    for (final width in [390.0, 800.0, 1440.0]) {
      testWidgets(
        'cached first paint precedes resume at width $width dark=$dark',
        (tester) async {
          final previousPlatform = debugDefaultTargetPlatformOverride;
          debugDefaultTargetPlatformOverride = width < 1100
              ? TargetPlatform.android
              : TargetPlatform.linux;
          addTearDown(
            () => debugDefaultTargetPlatformOverride = previousPlatform,
          );
          tester.view.physicalSize = Size(width, 900);
          tester.view.devicePixelRatio = 1;
          addTearDown(tester.view.resetPhysicalSize);
          addTearDown(tester.view.resetDevicePixelRatio);
          final resume = Completer<void>();
          final api = FakeBridgeApi()
            ..cachedHistories[thread] = history('Persisted before restart')
            ..pendingResumes[thread] = [resume.future]
            ..readResult = history('Synchronized replacement', epoch: 'two');
          await api.appConnect(service, 0);
          await tester.pumpWidget(
            host(
              Theme(
                data: dark ? darkTheme() : lightTheme(),
                child: const AppSessionScreen(
                  serviceKey: service,
                  threadId: thread,
                ),
              ),
              api,
            ),
          );
          await frames(tester);
          expect(find.text('Persisted before restart'), findsOneWidget);
          expect(find.byKey(const Key('history-sync-status')), findsOneWidget);
          expect(api.threadReads, isEmpty);
          resume.complete();
          await frames(tester);
          expect(find.text('Synchronized replacement'), findsOneWidget);
          expect(find.text('Persisted before restart'), findsNothing);
          expect(find.byKey(const Key('history-sync-status')), findsNothing);
          expect(tester.takeException(), isNull);
          await tester.pumpWidget(const SizedBox.shrink());
          debugDefaultTargetPlatformOverride = previousPlatform;
        },
      );
    }
  }

  testWidgets(
    'sync failure retains cached content and cannot send from stale state',
    (tester) async {
      final api = FailingSyncApi()
        ..cachedHistories[thread] = history('Readable offline');
      await api.appConnect(service, 0);
      await tester.pumpWidget(
        host(
          const AppSessionScreen(serviceKey: service, threadId: thread),
          api,
        ),
      );
      await frames(tester);
      expect(find.text('Readable offline'), findsOneWidget);
      expect(find.text('本地缓存 · 等待同步'), findsOneWidget);
      expect(api.threadReads, isEmpty);
      final button = tester.widget<IconButton>(
        find.byKey(const Key('send-btn')),
      );
      expect(button.onPressed, isNull);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );

  testWidgets(
    'prefetch is serialized, stops in background, and resumes fairly',
    (tester) async {
      final api = PrefetchApi();
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
      await tester.pumpWidget(
        ProviderScope(
          overrides: [bridgeApiProvider.overrideWithValue(api)],
          child: Consumer(
            builder: (context, ref, child) {
              ref.watch(runningSessionInventoryProvider(service));
              return const SizedBox.shrink();
            },
          ),
        ),
      );
      await frames(tester);
      expect(api.prefetched, ['one']);
      await tester.pump(const Duration(seconds: 10));
      expect(api.prefetched, ['one']);
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
      api.pending.complete();
      await frames(tester);
      await tester.pump(const Duration(seconds: 20));
      expect(api.prefetched, ['one']);
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
      await frames(tester);
      expect(api.prefetched, ['one', 'two', 'three']);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );
}
