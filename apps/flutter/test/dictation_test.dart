import 'dart:async';
import 'dart:convert';

import 'package:fake_async/fake_async.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/voice/dictation.dart';
import 'package:pocket_codex/src/voice/voice_transport.dart';

import 'fake_bridge_api.dart';

const _service = 'pcx:host:app:default';

/// The host side of the line: answers the realtime calls the way the
/// app-server does, and lets a test push transcript events.
class _Api extends FakeBridgeApi {
  _Api() : super(config: const ConfigInfo(relay: 'r:1', hasKey: true));

  final calls = <(String, Map<String, dynamic>)>[];
  int _threads = 0;
  bool rejectStart = false;
  String? lastThread;

  @override
  Future<String> appRealtimeRequest(
    String serviceKey,
    String method,
    String paramsJson,
  ) async {
    final params = jsonDecode(paramsJson) as Map<String, dynamic>;
    calls.add((method, params));
    switch (method) {
      case 'thread/start':
        final id = 'dict-${++_threads}';
        lastThread = id;
        return jsonEncode({
          'thread': {'id': id},
        });
      case 'thread/realtime/start':
        if (rejectStart) throw StateError('realtime is off for this account');
        // The host answers the offer asynchronously, like the real one.
        scheduleMicrotask(
          () => realtime('sdp', {'sdp': 'answer'}, thread: params['threadId']),
        );
    }
    return '{}';
  }

  void realtime(String kind, Map<String, dynamic> data, {String? thread}) =>
      pushEvent(
        _service,
        AppEvent(
          kind: 'thread/realtime/$kind',
          threadId: thread ?? lastThread,
          raw: jsonEncode(data),
        ),
      );

  void said(String delta) =>
      realtime('transcript/delta', {'role': 'user', 'delta': delta});

  Iterable<String> get methods => calls.map((c) => c.$1);
}

class _Media implements DictationTransport {
  void Function()? connected;
  void Function(String)? failure;
  String? answered;
  bool micOpen = false;
  int opens = 0;
  bool closed = false;
  bool denyMicrophone = false;
  double level = 0.25;

  @override
  Future<String> offer({
    required void Function() onConnected,
    required void Function(String) onFailure,
  }) async {
    connected = onConnected;
    failure = onFailure;
    return 'offer';
  }

  @override
  Future<void> answer(String sdp) async {
    answered = sdp;
    connected?.call();
  }

  @override
  Future<void> openMicrophone() async {
    if (denyMicrophone) throw StateError('NotAllowedError');
    opens++;
    micOpen = true;
  }

  @override
  Future<void> closeMicrophone() async => micOpen = false;

  @override
  Future<double?> inputLevel() async => micOpen ? level : null;

  @override
  Future<void> close() async {
    closed = true;
    micOpen = false;
  }
}

/// A line wired to [api] and [media], with its clock on [async].
DictationLine _line(_Api api, _Media media, FakeAsync async) {
  return DictationLine(
    api: api,
    serviceKey: _service,
    createTransport: () => media,
    stopwatch: async.getClock(DateTime(2026)).stopwatch,
  );
}

void main() {
  setUp(() async {});

  test('warming opens a silent, ephemeral session with no microphone', () {
    fakeAsync((async) {
      final api = _Api();
      final media = _Media();
      api.appConnect(_service, 1);
      final line = _line(api, media, async);
      line.warm();
      async.flushMicrotasks();
      expect(line.link, DictationLink.ready);
      final start = api.calls.firstWhere((c) => c.$1 == 'thread/start').$2;
      expect(start['ephemeral'], isTrue);
      expect(start['threadSource'], DictationLine.threadSource);
      expect(start['sandbox'], 'read-only');
      final rt = api.calls
          .firstWhere((c) => c.$1 == 'thread/realtime/start')
          .$2;
      expect(rt['version'], 'v3');
      expect(rt['includeStartupContext'], isFalse);
      expect(rt['clientManagedHandoffs'], isTrue);
      expect(rt['delegationAckFiller'], isFalse);
      expect(rt['prompt'], DictationLine.prompt);
      expect(rt.containsKey('model'), isFalse);
      expect(media.answered, 'answer');
      // Warm, but deaf: nothing is captured until a take.
      expect(media.opens, 0);
      line.dispose();
      async.flushMicrotasks();
    });
  });

  test('a take streams words, then ends once the tail is quiet', () {
    fakeAsync((async) {
      final api = _Api();
      final media = _Media();
      api.appConnect(_service, 1);
      final line = _line(api, media, async);
      final got = <(int, String)>[];
      final ended = <bool>[];
      line
        ..onDelta = ((take, d) => got.add((take, d)))
        ..onTakeEnded = ((take, {required cancelled}) => ended.add(cancelled));
      line.warm();
      async.flushMicrotasks();
      // Words before a take are never typed.
      api.said('stray');
      async.flushMicrotasks();
      expect(got, isEmpty);

      line.startTake();
      async.flushMicrotasks();
      expect(line.take, DictationTake.listening);
      expect(media.micOpen, isTrue);
      final take = line.takeId;
      async.elapse(const Duration(milliseconds: 200));
      expect(line.level.value, greaterThan(0));
      api.said('帮我');
      api.said('改成蓝色');
      async.flushMicrotasks();
      expect(got, [(take, '帮我'), (take, '改成蓝色')]);
      async.elapse(const Duration(seconds: 2));

      line.finishTake();
      async.flushMicrotasks();
      expect(line.take, DictationTake.finishing);
      expect(media.micOpen, isFalse);
      // The last word lands after the microphone is detached.
      async.elapse(const Duration(milliseconds: 400));
      api.said('。');
      async.flushMicrotasks();
      expect(got.last, (take, '。'));
      async.elapse(DictationLine.tailQuiet);
      expect(line.take, DictationTake.idle);
      expect(ended, [false]);
      expect(line.heard, '帮我改成蓝色。');
      // The session stays up for the next take.
      expect(line.link, DictationLink.ready);
      expect(api.methods.where((m) => m == 'thread/realtime/stop'), isEmpty);
      line.dispose();
      async.flushMicrotasks();
    });
  });

  test('a finishing take ends at the tail limit even if words keep coming', () {
    fakeAsync((async) {
      final api = _Api();
      final media = _Media();
      api.appConnect(_service, 1);
      final line = _line(api, media, async);
      line.warm();
      async.flushMicrotasks();
      line.startTake();
      async.flushMicrotasks();
      line.finishTake();
      for (var i = 0; i < 8; i++) {
        async.elapse(const Duration(milliseconds: 400));
        api.said('x');
        async.flushMicrotasks();
      }
      expect(line.take, DictationTake.idle);
      line.dispose();
      async.flushMicrotasks();
    });
  });

  test('a cancelled take ignores words still in flight', () {
    fakeAsync((async) {
      final api = _Api();
      final media = _Media();
      api.appConnect(_service, 1);
      final line = _line(api, media, async);
      final got = <String>[];
      final ended = <bool>[];
      line
        ..onDelta = (take, d) {
          if (take == line.takeId) got.add(d);
        }
        ..onTakeEnded = (take, {required cancelled}) => ended.add(cancelled);
      line.warm();
      async.flushMicrotasks();
      line.startTake();
      async.flushMicrotasks();
      api.said('keep');
      async.flushMicrotasks();
      line.cancelTake();
      async.flushMicrotasks();
      api.said('late');
      async.flushMicrotasks();
      expect(got, ['keep']);
      expect(ended, [true]);
      expect(media.micOpen, isFalse);
      line.dispose();
      async.flushMicrotasks();
    });
  });

  test('a take opens the line itself when it is not warm', () {
    fakeAsync((async) {
      final api = _Api();
      final media = _Media();
      api.appConnect(_service, 1);
      final line = _line(api, media, async);
      line.startTake();
      expect(line.take, DictationTake.opening);
      async.flushMicrotasks();
      expect(line.link, DictationLink.ready);
      expect(line.take, DictationTake.listening);
      line.dispose();
      async.flushMicrotasks();
    });
  });

  test('a refused session fails the take and backs off background retries', () {
    fakeAsync((async) {
      final api = _Api()..rejectStart = true;
      final media = _Media();
      api.appConnect(_service, 1);
      final line = _line(api, media, async);
      line.startTake();
      async.flushMicrotasks();
      expect(line.link, DictationLink.failed);
      expect(line.take, DictationTake.idle);
      expect(line.failure, DictationFailure.unavailable);
      expect(line.error, contains('realtime is off'));
      // The thread it opened is let go.
      expect(api.methods, contains('thread/unsubscribe'));
      final starts = api.methods.where((m) => m == 'thread/start').length;
      line.warm();
      async.flushMicrotasks();
      expect(api.methods.where((m) => m == 'thread/start').length, starts);
      async.elapse(DictationLine.retryBackoff);
      api.rejectStart = false;
      line.warm();
      async.flushMicrotasks();
      expect(line.link, DictationLink.ready);
      line.dispose();
      async.flushMicrotasks();
    });
  });

  test('a host error mid-take keeps what was heard and closes the line', () {
    fakeAsync((async) {
      final api = _Api();
      final media = _Media();
      api.appConnect(_service, 1);
      final line = _line(api, media, async);
      final ended = <bool>[];
      line.onTakeEnded = (take, {required cancelled}) => ended.add(cancelled);
      line.warm();
      async.flushMicrotasks();
      line.startTake();
      async.flushMicrotasks();
      api.realtime('closed', {'reason': 'transport_closed'});
      async.flushMicrotasks();
      expect(line.link, DictationLink.off);
      expect(line.take, DictationTake.idle);
      expect(line.failure, DictationFailure.interrupted);
      // Not a discard: the words already typed stay.
      expect(ended, [false]);
      expect(media.closed, isTrue);
      line.dispose();
      async.flushMicrotasks();
    });
  });

  test('a denied microphone fails the take but keeps the line', () {
    fakeAsync((async) {
      final api = _Api();
      final media = _Media()..denyMicrophone = true;
      api.appConnect(_service, 1);
      final line = _line(api, media, async);
      line.warm();
      async.flushMicrotasks();
      line.startTake();
      async.flushMicrotasks();
      expect(line.failure, DictationFailure.permission);
      expect(line.take, DictationTake.idle);
      expect(line.link, DictationLink.ready);
      line.dispose();
      async.flushMicrotasks();
    });
  });

  test('a delegated turn on the line is interrupted at once', () {
    fakeAsync((async) {
      final api = _Api();
      final media = _Media();
      api.appConnect(_service, 1);
      final line = _line(api, media, async);
      line.warm();
      async.flushMicrotasks();
      api.pushEvent(
        _service,
        AppEvent(kind: 'turn/started', threadId: api.lastThread, raw: '{}'),
      );
      async.flushMicrotasks();
      expect(api.interrupted, isTrue);
      line.dispose();
      async.flushMicrotasks();
    });
  });

  test('an idle line closes itself and releases its thread', () {
    fakeAsync((async) {
      final api = _Api();
      final media = _Media();
      api.appConnect(_service, 1);
      final line = _line(api, media, async);
      line.warm();
      async.flushMicrotasks();
      async.elapse(line.idleTimeout + const Duration(seconds: 1));
      expect(line.link, DictationLink.off);
      expect(media.closed, isTrue);
      expect(
        api.methods,
        containsAllInOrder(['thread/realtime/stop', 'thread/unsubscribe']),
      );
      line.dispose();
      async.flushMicrotasks();
    });
  });

  test('stats levels come from the microphone source', () {
    expect(audioLevelsFromStats(const []), isNull);
  });

  group('composer', () {
    Future<(_Api, _Media)> pump(WidgetTester t) async {
      final api = _Api();
      final media = _Media();
      await api.appConnect(_service, 28080);
      await t.pumpWidget(
        ProviderScope(
          overrides: [
            bridgeApiProvider.overrideWithValue(api),
            dictationTransportFactoryProvider.overrideWithValue(() => media),
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
      return (api, media);
    }

    TextEditingController field(WidgetTester t) => t
        .widget<TextField>(find.byKey(const Key('composer-input')))
        .controller!;

    testWidgets('words land at the cursor as they are spoken', (t) async {
      final (api, media) = await pump(t);
      await t.enterText(find.byKey(const Key('composer-input')), 'hello');
      await t.pump();
      // Focusing the composer warms the line in the background.
      expect(api.methods, contains('thread/realtime/start'));
      await t.pump();
      expect(find.byKey(const Key('dictation-ready-dot')), findsOneWidget);

      await t.tap(find.byKey(const Key('dictate')));
      await t.pump();
      await t.pump();
      expect(media.micOpen, isTrue);
      expect(find.byKey(const Key('dictation-bar')), findsOneWidget);
      expect(find.byKey(const Key('dictation-waveform')), findsOneWidget);

      api.said(' world');
      await t.pump();
      expect(field(t).text, 'hello world');
      api.said(', again');
      await t.pump();
      expect(field(t).text, 'hello world, again');
      expect(field(t).selection.baseOffset, field(t).text.length);

      // Done: the take ends once the tail is quiet; the text stays.
      await t.tap(find.byKey(const Key('dictate')));
      await t.pump();
      expect(media.micOpen, isFalse);
      await t.pump(DictationLine.tailQuiet + const Duration(milliseconds: 50));
      await t.pump(const Duration(milliseconds: 300));
      expect(find.byKey(const Key('dictation-bar')), findsNothing);
      expect(field(t).text, 'hello world, again');
      // Nothing was sent: the user reviews before sending.
      expect(api.methods, isNot(contains('turn/start')));
      await t.pumpWidget(const SizedBox());
      await t.pump(const Duration(seconds: 1));
    });

    testWidgets(
      'Esc discards exactly what the take wrote',
      (t) async {
        final (api, _) = await pump(t);
        await t.enterText(find.byKey(const Key('composer-input')), '前缀');
        await t.pump();
        await t.pump();
        await t.tap(find.byKey(const Key('dictate')));
        await t.pump();
        await t.pump();
        api.said('帮我');
        api.said('改颜色');
        await t.pump();
        expect(field(t).text, '前缀帮我改颜色');
        await t.tap(find.byKey(const Key('composer-input')));
        await t.sendKeyEvent(LogicalKeyboardKey.escape);
        await t.pump();
        await t.pump(const Duration(milliseconds: 300));
        expect(field(t).text, '前缀');
        expect(find.byKey(const Key('dictation-bar')), findsNothing);
        await t.pumpWidget(const SizedBox());
        await t.pump(const Duration(seconds: 1));
      },
      variant: TargetPlatformVariant.only(TargetPlatform.linux),
    );

    testWidgets(
      'Enter during a take finishes it instead of sending',
      (t) async {
        final (api, media) = await pump(t);
        await t.tap(find.byKey(const Key('composer-input')));
        await t.pump();
        await t.tap(find.byKey(const Key('dictate')));
        await t.pump();
        await t.pump();
        api.said('run the tests');
        await t.pump();
        await t.tap(find.byKey(const Key('composer-input')));
        await t.sendKeyEvent(LogicalKeyboardKey.enter);
        await t.pump();
        expect(media.micOpen, isFalse);
        await t.pump(
          DictationLine.tailQuiet + const Duration(milliseconds: 50),
        );
        expect(field(t).text, 'run the tests');
        expect(api.methods, isNot(contains('turn/start')));
        await t.pumpWidget(const SizedBox());
        await t.pump(const Duration(seconds: 1));
      },
      variant: TargetPlatformVariant.only(TargetPlatform.linux),
    );
  });
}
