import 'dart:async';
import 'dart:convert';

import 'package:fake_async/fake_async.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart' show StatsReport;
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
  late void Function(String) failure;
  VoidCallback? interrupted;
  bool speakerSuppressed = false;
  final suppressions = <bool>[];
  @override
  Future<String> offer({
    required void Function() onConnected,
    required void Function(String) onFailure,
    void Function()? onInterrupted,
  }) async {
    connected = onConnected;
    failure = onFailure;
    interrupted = onInterrupted;
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
  void setSpeakerSuppressed(bool value) {
    speakerSuppressed = value;
    suppressions.add(value);
  }

  /// What [levels] reports; null until a test sets it.
  AudioLevels? level;

  @override
  Future<AudioLevels?> levels() async => level;

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
  test('audioLevelsFromStats reads the microphone and the remote audio', () {
    StatsReport r(String type, Map<String, dynamic> v) =>
        StatsReport('id', type, 0, v);
    expect(
      audioLevelsFromStats([
        r('media-source', {'kind': 'audio', 'audioLevel': 0.25}),
        r('inbound-rtp', {'kind': 'audio', 'audioLevel': 0.04}),
        r('inbound-rtp', {'kind': 'video', 'audioLevel': 0.9}),
        r('candidate-pair', {'bytesSent': 10}),
      ]),
      (input: 0.25, output: 0.04),
    );
    expect(audioLevelsFromStats([r('transport', {})]), isNull);
  });

  test('a live call polls levels; muting flattens the microphone', () {
    fakeAsync((async) {
      final api = _VoiceApi();
      final media = _Media()..level = (input: 0.25, output: 0);
      final c = VoiceController(
        api: api,
        serviceKey: _service,
        createTransport: () => media,
      );
      c.start('voice');
      async.flushMicrotasks();
      // Nothing is polled before audio flows.
      async.elapse(const Duration(milliseconds: 300));
      expect(c.levels.value.input, 0);
      media.connected();
      async.elapse(const Duration(milliseconds: 600));
      // sqrt(0.25) = 0.5, reached through the attack smoothing.
      expect(c.levels.value.input, closeTo(0.5, 0.01));
      c.toggleMuted();
      async.elapse(const Duration(milliseconds: 200));
      expect(c.levels.value.input, 0);
      c.stop();
      async.flushMicrotasks();
      expect(c.levels.value, (input: 0.0, output: 0.0));
      c.dispose();
      async.flushMicrotasks();
    });
  });

  group('barge-in', () {
    late _VoiceApi api;
    late _Media media;
    late VoiceController c;
    Future<void> say(String role, String delta) async {
      api.event('transcript/delta', {'role': role, 'delta': delta});
      await _tick();
    }

    Future<void> done(String role, String text) async {
      api.event('transcript/done', {'role': role, 'text': text});
      await _tick();
    }

    setUp(() async {
      api = _VoiceApi();
      media = _Media();
      c = VoiceController(
        api: api,
        serviceKey: _service,
        createTransport: () => media,
      );
      await c.start('voice');
      api.event('sdp', {'sdp': 'answer'});
      await _tick();
      expect(c.phase, VoicePhase.active);
    });
    tearDown(() => c.dispose());

    test('talking over a reply cuts its audio until the new reply', () async {
      await say('assistant', 'Here is a long');
      expect(c.turn, VoiceTurn.assistant);
      await say('user', 'wait');
      expect(media.speakerSuppressed, isTrue);
      expect(c.turn, VoiceTurn.user);
      // Stale chunks of the old reply keep arriving: still silent.
      await say('assistant', ' answer about');
      expect(media.speakerSuppressed, isTrue);
      await done('assistant', 'Here is a long answer about');
      expect(c.transcript.last.interrupted, isTrue);
      await done('user', 'wait, use the other file');
      // The reply to what the user just said is heard again.
      await say('assistant', 'Sure, the other file');
      expect(media.speakerSuppressed, isFalse);
      await done('assistant', 'Sure, the other file.');
      expect(c.transcript.last.interrupted, isFalse);
    });

    test(
      'a pause mid-sentence is one utterance, and can interrupt twice',
      () async {
        await say('assistant', 'Starting');
        await say('user', 'hmm');
        await say('user', ' ');
        await say('user', ' actually');
        expect(media.suppressions.where((s) => s).length, 1);
        await done('user', 'hmm actually');
        await done('assistant', 'Starting');
        await say('assistant', 'OK so');
        expect(media.speakerSuppressed, isFalse);
        await say('user', 'wait, one more thing');
        expect(media.speakerSuppressed, isTrue);
        expect(media.suppressions.where((s) => s).length, 2);
      },
    );

    test('speech while muted never interrupts', () async {
      await say('assistant', 'Talking');
      c.toggleMuted();
      await say('user', 'echo');
      expect(media.speakerSuppressed, isFalse);
    });
  });
  testWidgets('a reply that carries on past the interjection is resumed', (
    t,
  ) async {
    final api = _VoiceApi();
    final media = _Media();
    final c = VoiceController(
      api: api,
      serviceKey: _service,
      createTransport: () => media,
    );
    await c.start('voice');
    api.event('sdp', {'sdp': 'answer'});
    await t.pump();
    void say(String role, String delta) =>
        api.event('transcript/delta', {'role': role, 'delta': delta});
    say('assistant', 'Go');
    await t.pump();
    say('user', 'ok');
    await t.pump();
    expect(media.speakerSuppressed, isTrue);
    api.event('transcript/done', {'role': 'user', 'text': 'ok'});
    await t.pump();
    // The server never ended the first reply; it is still talking.
    say('assistant', ' on');
    await t.pump();
    await t.pump(VoiceController.resumeFallback);
    expect(media.speakerSuppressed, isFalse);
    // Dispose clears the floor timers synchronously; the host stop it starts
    // runs on the real zone.
    c.dispose();
    await t.runAsync(_tick);
    await t.pump(VoiceController.speakingTail);
  });
  test('a transient drop reconnects; a lasting one fails visibly', () async {
    final api = _VoiceApi();
    final media = _Media();
    final c = VoiceController(
      api: api,
      serviceKey: _service,
      createTransport: () => media,
    );
    await c.start('voice');
    api.event('sdp', {'sdp': 'answer'});
    await _tick();
    media.interrupted!();
    expect(c.phase, VoicePhase.reconnecting);
    expect(c.busy, isTrue);
    media.connected();
    expect(c.phase, VoicePhase.active);
    media.failure('Audio connection interrupted');
    await _tick();
    await _tick();
    expect(c.phase, VoicePhase.failed);
    expect(c.error, 'Audio connection interrupted');
    expect(media.closed, isTrue);
    c.dispose();
  });
  test(
    'a host close keeps its reason; a hang-up is recorded as such',
    () async {
      final api = _VoiceApi();
      final c = VoiceController(
        api: api,
        serviceKey: _service,
        createTransport: _Media.new,
      );
      await c.start('voice');
      api.event('closed', {'reason': 'session timeout'});
      await _tick();
      await _tick();
      expect(c.phase, VoicePhase.idle);
      expect(c.endedReason, 'session timeout');
      expect(c.endedByUser, isFalse);
      await c.start('voice');
      await c.hangUp();
      expect(c.phase, VoicePhase.idle);
      expect(c.endedByUser, isTrue);
      expect(c.endedReason, isNull);
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
        // The app-server rejects resume/read of a thread whose rollout has
        // not been written yet, which is every freshly started voice thread.
        api.appThreadResumeError = StateError(
          'no rollout found for thread id voice',
        );
        await t.tap(find.byKey(const Key('voice-start-confirm')));
        await t.pumpAndSettle();
        expect(
          api.calls
              .firstWhere((c) => c.$1 == 'thread/start')
              .$2['threadSource'],
          'pocket-codex-voice',
        );
        expect(api.lastResumed, isNull);
        expect(find.textContaining('no rollout found'), findsNothing);
        // Once the session has spoken, the rollout exists and resume works.
        api.appThreadResumeError = null;
        api.event('sdp', {'sdp': 'answer'});
        // A live call animates its level bars indefinitely; pump frames
        // rather than waiting for a settle that never comes.
        await t.pump();
        await t.pump(const Duration(milliseconds: 300));
        expect(find.byKey(const Key('voice-mute')), findsOneWidget);
        await t.tap(find.byKey(const Key('voice-mute')));
        await t.pump();
        await t.pump(const Duration(milliseconds: 300));
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
  testWidgets('the sidebar keeps a live call visible and hangs it up', (
    t,
  ) async {
    AppSessionScreen.debugResetThreadMemory();
    t.view.devicePixelRatio = 1;
    t.view.physicalSize = const Size(1280, 900);
    addTearDown(t.view.reset);
    final api = _VoiceApi();
    api.appThreads.add(
      const ThreadMeta(
        id: 'other',
        preview: 'another conversation',
        cwd: '/repo',
        updatedAt: 0,
      ),
    );
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
          theme: lightTheme(),
          home: const AppSessionScreen(
            serviceKey: _service,
            cwd: '/repo',
            home: true,
          ),
        ),
      ),
    );
    await t.pumpAndSettle();
    expect(find.byKey(const Key('voice-status')), findsNothing);
    await t.tap(find.byKey(const Key('voice-start')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('voice-start-confirm')));
    await t.pump();
    await t.pump(const Duration(milliseconds: 300));
    api.event('sdp', {'sdp': 'answer'});
    await t.pump();
    await t.pump(const Duration(milliseconds: 300));
    expect(find.byKey(const Key('voice-status')), findsOneWidget);
    // Collapsing the sidebar moves the entry into the window strip.
    await t.tap(find.byKey(const Key('sidebar-collapse-btn')));
    await t.pump();
    await t.pump(const Duration(milliseconds: 400));
    expect(find.byKey(const Key('voice-status-compact')), findsOneWidget);
    await t.tap(find.byKey(const Key('voice-status-hangup')));
    await t.pump();
    await t.runAsync(_tick);
    await t.pump(const Duration(milliseconds: 300));
    expect(media.closed, isTrue);
    expect(api.calls.last.$1, 'thread/realtime/stop');
    await t.pumpWidget(const SizedBox());
    await t.pump(const Duration(seconds: 7));
  });
  testWidgets('opening another conversation ends the call and its capture', (
    t,
  ) async {
    AppSessionScreen.debugResetThreadMemory();
    t.view.devicePixelRatio = 1;
    t.view.physicalSize = const Size(1280, 900);
    addTearDown(t.view.reset);
    final api = _VoiceApi();
    api.appThreads.add(
      const ThreadMeta(
        id: 'other',
        preview: 'another conversation',
        cwd: '/repo',
        updatedAt: 0,
      ),
    );
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
          theme: lightTheme(),
          home: const AppSessionScreen(
            serviceKey: _service,
            cwd: '/repo',
            home: true,
          ),
        ),
      ),
    );
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('voice-start')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('voice-start-confirm')));
    await t.pump();
    await t.pump(const Duration(milliseconds: 300));
    api.event('sdp', {'sdp': 'answer'});
    await t.pump();
    await t.pump(const Duration(milliseconds: 300));
    expect(media.closed, isFalse);
    await t.tap(find.text('another conversation'));
    await t.pump();
    await t.runAsync(_tick);
    await t.pump(const Duration(milliseconds: 300));
    expect(media.closed, isTrue);
    expect(api.calls.map((c) => c.$1), contains('thread/realtime/stop'));
    await t.pumpWidget(const SizedBox());
    await t.pump(const Duration(seconds: 7));
  });
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
