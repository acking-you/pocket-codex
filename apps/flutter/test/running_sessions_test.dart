import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';

import 'fake_bridge_api.dart';
import 'support/screen_harness.dart';

const service = 'pcx:host:app:default';
const external = LocalSession(
  threadId: 'external',
  cwd: '/project',
  preview: 'external work',
  updatedAt: 1,
  turnState: 'incomplete',
  heldOpen: true,
  safety: 'ownedRunning',
  allowsResume: false,
  requiresTakeover: false,
);

class ProbeApi extends FakeBridgeApi {
  final probes = <Completer<List<LocalSession>>>[];

  @override
  Future<List<LocalSession>> metaSessions(
    String serviceKey, {
    bool runningOnly = false,
  }) {
    expect(serviceKey, service);
    expect(runningOnly, isTrue);
    final probe = Completer<List<LocalSession>>();
    probes.add(probe);
    return probe.future;
  }
}

Future<void> frames(WidgetTester tester) async {
  for (var i = 0; i < 6; i++) {
    await tester.pump(const Duration(milliseconds: 100));
  }
}

void main() {
  testWidgets(
    'external inventory starts, stops and never overlaps or revives a completed turn',
    (t) async {
      final api = ProbeApi();
      await t.pumpWidget(
        ProviderScope(
          overrides: [bridgeApiProvider.overrideWithValue(api)],
          child: Consumer(
            builder: (context, ref, _) {
              ref.watch(runningThreadsProvider(service));
              return const SizedBox.shrink();
            },
          ),
        ),
      );
      final container = ProviderScope.containerOf(
        t.element(find.byType(Consumer)),
      );
      Set<String> running() =>
          container.read(runningThreadsProvider(service)).valueOrNull ?? {};
      await frames(t);
      expect(api.probes, hasLength(1));
      await t.pump(const Duration(seconds: 20));
      expect(
        api.probes,
        hasLength(1),
        reason: 'a slow host must not accumulate requests',
      );
      api.probes.first.complete([external]);
      await frames(t);
      expect(running(), {'external'});

      await t.pump(const Duration(seconds: 5));
      expect(api.probes, hasLength(2));
      api.pushEvent(
        service,
        const AppEvent(kind: 'turn/completed', threadId: 'external', raw: '{}'),
      );
      await frames(t);
      expect(running(), isEmpty);
      api.probes[1].complete([external]);
      await frames(t);
      expect(
        running(),
        isEmpty,
        reason: 'the response predates the completion event',
      );

      await t.pump(const Duration(seconds: 5));
      api.probes[2].completeError(StateError('temporarily disconnected'));
      await frames(t);
      expect(running(), isEmpty);
      await t.pump(const Duration(seconds: 5));
      api.probes[3].complete([external]);
      await frames(t);
      expect(running(), {
        'external',
      }, reason: 'a new turn may start without our app-server events');
      await t.pump(const Duration(seconds: 5));
      api.probes[4].complete([]);
      await frames(t);
      expect(
        running(),
        isEmpty,
        reason: 'the host also discovers external completion',
      );

      await t.pump(const Duration(seconds: 5));
      await t.pumpWidget(const SizedBox.shrink());
      api.probes.last.complete([external]);
      await t.pump(const Duration(seconds: 20));
      expect(
        api.probes,
        hasLength(6),
        reason: 'disposing cancels polling, even with a probe in flight',
      );
    },
  );

  for (final width in [1280.0, 430.0]) {
    testWidgets(
      'unopened external session moves into and out of Active at width $width',
      (t) async {
        t.view.devicePixelRatio = 1;
        t.view.physicalSize = Size(width, 950);
        addTearDown(t.view.resetPhysicalSize);
        addTearDown(t.view.resetDevicePixelRatio);
        await t.binding.setSurfaceSize(Size(width, 950));
        addTearDown(() => t.binding.setSurfaceSize(null));
        final api = FakeBridgeApi()..localSessions = [external];
        await t.pumpWidget(
          host(const AppSessionScreen(serviceKey: service, home: true), api),
        );
        await frames(t);
        if (width < 720) {
          final scaffold = t.state<ScaffoldState>(find.byType(Scaffold).first);
          scaffold.openDrawer();
          await frames(t);
        }
        expect(find.text('进行中'), findsOneWidget);
        expect(find.byKey(const Key('conv-tile-external')), findsOneWidget);
        expect(api.turnItemCalls, isEmpty);
        api.localSessions = [];
        await t.pump(const Duration(seconds: 5));
        await frames(t);
        expect(find.text('进行中'), findsNothing);
        // The discovered session remains in the project history after completion.
        expect(find.text('project'), findsWidgets);
        await t.pumpWidget(const SizedBox.shrink());
      },
    );
  }
}
