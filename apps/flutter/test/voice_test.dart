import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/voice/voice_controller.dart';
import 'package:pocket_codex/src/voice/voice_transport.dart';
import 'package:pocket_codex/src/voice/voice_widgets.dart';

import 'fake_bridge_api.dart';

const _service = 'pcx:host:app:default';

class _VoiceApi extends FakeBridgeApi {
  final calls = <(String, Map<String, dynamic>)>[];
  Completer<void>? startAck;
  Completer<void>? threadAck;
  Completer<void>? stopAck;
  bool rejectStart = false;
  @override
  Future<String> appRealtimeRequest(
    String serviceKey,
    String method,
    String paramsJson,
  ) async {
    final params = jsonDecode(paramsJson) as Map<String, dynamic>;
    calls.add((method, params));
    switch (method) {
      case 'thread/realtime/listVoices':
        return jsonEncode({
          'voices': {
            'v1': ['cove', 'breeze'],
            'v2': ['marin'],
            'defaultV1': 'cove',
          },
        });
      case 'thread/start':
        await threadAck?.future;
        appThreads.add(
          const ThreadMeta(
            id: 'voice',
            preview: '',
            cwd: '/repo',
            updatedAt: 1,
            threadSource: 'pocket-codex-voice',
          ),
        );
        return '{"thread":{"id":"voice","cwd":"/repo"}}';
      case 'thread/realtime/start':
        if (rejectStart) {
          throw StateError('Voice is unavailable for this account');
        }
        await startAck?.future;
      case 'thread/realtime/stop':
        await stopAck?.future;
      case 'thread/timeline/list':
        return jsonEncode({
          'data': [
            {
              'type': 'realtime',
              'position': 1,
              'item': {
                'id': 'speech',
                'type': 'transcriptSegment',
                'role': 'user',
                'text': 'Saved spoken request',
              },
            },
          ],
          'nextCursor': null,
        });
    }
    return '{}';
  }

  void event(
    String kind,
    Map<String, dynamic> data, {
    String thread = 'voice',
  }) => pushEvent(
    _service,
    AppEvent(
      kind: 'thread/realtime/$kind',
      threadId: thread,
      raw: jsonEncode(data),
    ),
  );
}

class _Media implements VoiceTransport {
  bool closed = false;
  bool muted = false;
  String? answerSdp;
  Completer<String>? pendingOffer;
  late VoidCallback connected;
  @override
  Future<String> offer({
    required void Function() onConnected,
    required void Function(String) onFailure,
  }) async {
    connected = onConnected;
    return pendingOffer == null ? 'offer' : await pendingOffer!.future;
  }

  @override
  Future<void> answer(String sdp) async {
    answerSdp = sdp;
    connected();
  }

  @override
  void setMuted(bool value) {
    muted = value;
  }

  @override
  Future<void> close() async {
    closed = true;
  }
}

Future<void> _tick() => Future<void>.delayed(Duration.zero);

void main() {
  test('voice source survives renaming', () {
    const t = ThreadMeta(
      id: 'v',
      preview: '',
      cwd: '',
      updatedAt: 0,
      threadSource: 'pocket-codex-voice',
    );
    expect(t.withName('Renamed').isVoice, isTrue);
  });
  test(
    'SDP, model, voice, mute and transcript use the selected thread',
    () async {
      final api = _VoiceApi();
      final media = _Media();
      final c = VoiceController(
        api: api,
        serviceKey: _service,
        createTransport: () => media,
      );
      await c.start('voice', model: 'voice-model', voice: 'cove');
      expect(c.phase, VoicePhase.connecting);
      final params = api.calls.single.$2;
      expect(params['transport'], {'type': 'webrtc', 'sdp': 'offer'});
      expect(params['model'], 'voice-model');
      expect(params['voice'], 'cove');
      expect(params['version'], 'v3');
      expect(params['clientManagedHandoffs'], false);
      api.event('sdp', {'sdp': 'foreign'}, thread: 'other');
      api.event('sdp', {'sdp': 'answer'});
      api.event('transcript/delta', {'role': 'user', 'delta': 'Hello'});
      await _tick();
      expect(media.answerSdp, 'answer');
      expect(c.phase, VoicePhase.active);
      expect(c.partial['user'], 'Hello');
      c.toggleMuted();
      expect(media.muted, isTrue);
      api.event('transcript/done', {'role': 'user', 'text': 'Hello world'});
      await _tick();
      expect(c.partial, isEmpty);
      expect(c.transcript.single.text, 'Hello world');
      await c.stop();
      expect(media.closed, isTrue);
      expect(api.calls.last.$1, 'thread/realtime/stop');
      c.dispose();
    },
  );
  test(
    'stop releases microphone before an unreachable host acknowledges',
    () async {
      final api = _VoiceApi()..stopAck = Completer<void>();
      final media = _Media();
      final c = VoiceController(
        api: api,
        serviceKey: _service,
        createTransport: () => media,
      );
      await c.start('voice');
      final stopping = c.stop();
      await _tick();
      expect(media.closed, isTrue);
      expect(c.phase, VoicePhase.stopping);
      api.stopAck!.complete();
      await stopping;
      c.dispose();
    },
  );
  test('cancelled microphone startup never starts a remote call', () async {
    final api = _VoiceApi();
    final media = _Media()..pendingOffer = Completer<String>();
    final c = VoiceController(
      api: api,
      serviceKey: _service,
      createTransport: () => media,
    );
    final starting = c.start('voice');
    await c.stop();
    media.pendingOffer!.complete('late-offer');
    await starting;
    expect(media.closed, isTrue);
    expect(api.calls.where((c) => c.$1.endsWith('/start')), isEmpty);
    c.dispose();
  });
  test(
    'late host startup is stopped and cannot overlap a replacement call',
    () async {
      final api = _VoiceApi()..startAck = Completer<void>();
      var created = 0;
      final c = VoiceController(
        api: api,
        serviceKey: _service,
        createTransport: () {
          created++;
          return _Media();
        },
      );
      final starting = c.start('voice');
      await _tick();
      await c.stop();
      await c.start('voice');
      expect(created, 1);
      api.startAck!.complete();
      await starting;
      expect(api.calls.where((c) => c.$1.endsWith('/stop')).length, 2);
      c.dispose();
    },
  );
  test(
    'asynchronous backend rejection is visible and releases media',
    () async {
      final api = _VoiceApi();
      final media = _Media();
      final c = VoiceController(
        api: api,
        serviceKey: _service,
        createTransport: () => media,
      );
      await c.start('voice');
      api.event('error', {'message': 'Account cannot use realtime'});
      await _tick();
      await _tick();
      expect(c.phase, VoicePhase.failed);
      expect(c.error, contains('Account cannot'));
      expect(media.closed, isTrue);
      c.dispose();
    },
  );
  testWidgets('startup deadline releases a pending microphone offer', (
    t,
  ) async {
    final api = _VoiceApi();
    final media = _Media()..pendingOffer = Completer<String>();
    final c = VoiceController(
      api: api,
      serviceKey: _service,
      createTransport: () => media,
    );
    final starting = c.start('voice');
    await t.pump(const Duration(seconds: 45));
    expect(media.closed, isTrue);
    // Broadcast cancellation can return Dart's cached real-zone future.
    await t.runAsync(_tick);
    await t.pump();
    expect(c.phase, VoicePhase.failed);
    media.pendingOffer!.complete('late-offer');
    await t.pump();
    await starting;
    expect(api.calls.where((c) => c.$1.endsWith('/start')), isEmpty);
    c.dispose();
  });
  testWidgets('backgrounding cancels a pending voice thread creation', (
    t,
  ) async {
    AppSessionScreen.debugResetThreadMemory();
    final api = _VoiceApi()..threadAck = Completer<void>();
    var mediaCreated = false;
    await t.pumpWidget(
      ProviderScope(
        overrides: [
          bridgeApiProvider.overrideWithValue(api),
          voiceTransportFactoryProvider.overrideWithValue(() {
            mediaCreated = true;
            return _Media();
          }),
        ],
        child: MaterialApp(
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          home: const AppSessionScreen(serviceKey: _service, cwd: '/repo'),
        ),
      ),
    );
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('voice-start')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('voice-start-confirm')));
    await t.pumpAndSettle();
    expect(api.calls.where((c) => c.$1 == 'thread/start'), hasLength(1));
    t.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    t.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    t.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    api.threadAck!.complete();
    await t.pumpAndSettle();
    expect(mediaCreated, isFalse);
    expect(api.calls.where((c) => c.$1 == 'thread/realtime/start'), isEmpty);
    t.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    t.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    t.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await t.pumpWidget(const SizedBox());
    await t.pumpAndSettle();
  });
  for (final (size, dark) in [
    (const Size(390, 844), false),
    (const Size(320, 740), true),
    (const Size(800, 1024), true),
    (const Size(1280, 900), false),
  ]) {
    testWidgets(
      'voice creation, foreground controls and background cleanup $size',
      (t) async {
        AppSessionScreen.debugResetThreadMemory();
        t.view.devicePixelRatio = 1;
        t.view.physicalSize = size;
        addTearDown(t.view.reset);
        final api = _VoiceApi();
        final media = _Media();
        await t.pumpWidget(
          ProviderScope(
            overrides: [
              bridgeApiProvider.overrideWithValue(api),
              voiceTransportFactoryProvider.overrideWithValue(() => media),
            ],
            child: MaterialApp(
              localizationsDelegates: AppLocalizations.localizationsDelegates,
              supportedLocales: AppLocalizations.supportedLocales,
              theme: dark ? darkTheme() : lightTheme(),
              home: const AppSessionScreen(serviceKey: _service, cwd: '/repo'),
            ),
          ),
        );
        await t.pumpAndSettle();
        await t.tap(find.byKey(const Key('voice-start')));
        await t.pumpAndSettle();
        expect(find.text('cove'), findsOneWidget);
        expect(find.text('marin'), findsNothing);
        await t.tap(find.byKey(const Key('voice-start-confirm')));
        await t.pumpAndSettle();
        expect(
          api.calls
              .firstWhere((c) => c.$1 == 'thread/start')
              .$2['threadSource'],
          'pocket-codex-voice',
        );
        api.event('sdp', {'sdp': 'answer'});
        await t.pumpAndSettle();
        expect(find.byKey(const Key('voice-mute')), findsOneWidget);
        await t.tap(find.byKey(const Key('voice-mute')));
        await t.pumpAndSettle();
        expect(media.muted, isTrue);
        expect(t.takeException(), isNull);
        t.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
        t.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
        t.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
        await t.pumpAndSettle();
        expect(media.closed, isTrue);
        t.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
        t.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
        t.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
        await t.pumpWidget(const SizedBox());
        await t.pumpAndSettle();
      },
    );
  }
  testWidgets('voice history restores canonical spoken segments', (t) async {
    await t.pumpWidget(
      MaterialApp(
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: VoiceHistoryDialog(
          api: _VoiceApi(),
          serviceKey: _service,
          threadId: 'voice',
        ),
      ),
    );
    await t.pumpAndSettle();
    expect(find.text('Saved spoken request'), findsOneWidget);
  });
}
