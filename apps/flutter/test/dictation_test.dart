import 'dart:async';
import 'dart:typed_data';

import 'package:fake_async/fake_async.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/voice/dictation.dart';

import 'fake_bridge_api.dart';

const _service = 'pcx:host:app:default';

class _Recorder implements DictationRecorder {
  bool permitted = true;
  bool started = false;
  bool cancelled = false;
  bool disposed = false;
  double current = 0.36;
  DictationAudio? take = (
    bytes: Uint8List.fromList(List.filled(800, 7)),
    mime: 'audio/mp4',
    fileName: 'codex.m4a',
  );

  @override
  Future<bool> hasPermission() async => permitted;

  @override
  Future<void> start() async => started = true;

  @override
  Future<double> level() async => current;

  @override
  Future<DictationAudio?> stop() async => take;

  @override
  Future<void> cancel() async => cancelled = true;

  @override
  Future<void> dispose() async => disposed = true;
}

void main() {
  test('a take records, levels move, and the host transcribes it', () {
    fakeAsync((async) {
      final api = FakeBridgeApi()..dictationResult = '把首页改成骨架屏';
      final rec = _Recorder();
      final d = DictationController(
        api: api,
        serviceKey: _service,
        createRecorder: () => rec,
        stopwatch: async.getClock(DateTime(2026)).stopwatch,
      );
      d.start();
      async.flushMicrotasks();
      expect(d.phase, DictationPhase.recording);
      expect(rec.started, isTrue);
      async.elapse(const Duration(milliseconds: 600));
      // sqrt(0.36) = 0.6, reached through the smoothing.
      expect(d.level.value, closeTo(0.6, 0.02));
      String? text;
      d.finish(language: 'zh').then((t) => text = t);
      async.flushMicrotasks();
      expect(text, '把首页改成骨架屏');
      expect(d.phase, DictationPhase.idle);
      expect(d.level.value, 0);
      expect(api.dictations.single, ('audio/mp4', 'codex.m4a', 'zh', 800));
      d.dispose();
      async.flushMicrotasks();
      expect(rec.disposed, isTrue);
    });
  });

  test('a tap too short to be speech is not sent', () {
    fakeAsync((async) {
      final api = FakeBridgeApi();
      final d = DictationController(
        api: api,
        serviceKey: _service,
        createRecorder: _Recorder.new,
        stopwatch: async.getClock(DateTime(2026)).stopwatch,
      );
      d.start();
      async.flushMicrotasks();
      async.elapse(const Duration(milliseconds: 100));
      String? text = 'unset';
      d.finish().then((t) => text = t);
      async.flushMicrotasks();
      expect(text, isNull);
      expect(d.failure, DictationFailure.tooShort);
      expect(api.dictations, isEmpty);
      d.dispose();
      async.flushMicrotasks();
    });
  });

  test('denied permission and host errors surface as failures', () {
    fakeAsync((async) {
      final api = FakeBridgeApi()..dictationResult = StateError('503 busy');
      final rec = _Recorder()..permitted = false;
      final d = DictationController(
        api: api,
        serviceKey: _service,
        createRecorder: () => rec,
        stopwatch: async.getClock(DateTime(2026)).stopwatch,
      );
      d.start();
      async.flushMicrotasks();
      expect(d.phase, DictationPhase.idle);
      expect(d.failure, DictationFailure.permission);
      expect(rec.started, isFalse);

      rec.permitted = true;
      d.start();
      async.flushMicrotasks();
      async.elapse(const Duration(seconds: 1));
      d.finish();
      async.flushMicrotasks();
      expect(d.failure, DictationFailure.transcription);
      expect(d.error, contains('503 busy'));
      expect(d.phase, DictationPhase.idle);
      d.dispose();
      async.flushMicrotasks();
    });
  });

  test('cancel drops the take; a reply after cancel is ignored', () {
    fakeAsync((async) {
      final api = FakeBridgeApi()..dictationGate = Completer<void>();
      final rec = _Recorder();
      final d = DictationController(
        api: api,
        serviceKey: _service,
        createRecorder: () => rec,
        stopwatch: async.getClock(DateTime(2026)).stopwatch,
      );
      d.start();
      async.flushMicrotasks();
      d.cancel();
      async.flushMicrotasks();
      expect(rec.cancelled, isTrue);
      expect(d.phase, DictationPhase.idle);

      d.start();
      async.flushMicrotasks();
      async.elapse(const Duration(seconds: 1));
      String? text = 'unset';
      d.finish().then((t) => text = t);
      async.flushMicrotasks();
      expect(d.phase, DictationPhase.transcribing);
      d.cancel();
      api.dictationGate!.complete();
      async.flushMicrotasks();
      expect(text, isNull);
      expect(d.phase, DictationPhase.idle);
      d.dispose();
      async.flushMicrotasks();
    });
  });

  test('a take stops itself at the cap and is transcribed', () {
    fakeAsync((async) {
      final api = FakeBridgeApi();
      final d = DictationController(
        api: api,
        serviceKey: _service,
        createRecorder: _Recorder.new,
        stopwatch: async.getClock(DateTime(2026)).stopwatch,
      );
      d.start();
      async.flushMicrotasks();
      async.elapse(
        DictationController.maxDuration + const Duration(seconds: 1),
      );
      expect(api.dictations, hasLength(1));
      expect(d.phase, DictationPhase.idle);
      d.dispose();
      async.flushMicrotasks();
    });
  });

  testWidgets('the composer mic records, shows a waveform, inserts text', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    )..dictationResult = 'world';
    await api.appConnect(_service, 28080);
    await t.pumpWidget(
      ProviderScope(
        overrides: [
          bridgeApiProvider.overrideWithValue(api),
          dictationRecorderFactoryProvider.overrideWithValue(_Recorder.new),
        ],
        child: MaterialApp(
          theme: lightTheme(),
          locale: const Locale('en'),
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          home: const AppSessionScreen(serviceKey: _service),
        ),
      ),
    );
    await t.pumpAndSettle();
    await t.enterText(find.byType(TextField).last, 'hello');
    await t.pump();

    await t.tap(find.byKey(const Key('dictate')));
    await t.pump();
    await t.pump(const Duration(milliseconds: 300));
    expect(find.byKey(const Key('dictation-strip')), findsOneWidget);
    expect(find.byKey(const Key('dictation-waveform')), findsOneWidget);

    // Long enough to count as speech.
    await t.runAsync(
      () => Future<void>.delayed(const Duration(milliseconds: 300)),
    );
    await t.tap(find.byKey(const Key('dictate')));
    await t.pump();
    await t.pump(const Duration(milliseconds: 300));
    expect(find.byKey(const Key('dictation-strip')), findsNothing);
    final field = t.widget<TextField>(find.byType(TextField).last);
    // Latin words are spaced; the cursor lands after the inserted text.
    expect(field.controller!.text, 'hello world');
    expect(field.controller!.selection.baseOffset, 'hello world'.length);
    expect(api.dictations.single.$1, 'audio/mp4');
    await t.pumpWidget(const SizedBox());
    await t.pump(const Duration(seconds: 1));
  });
}
