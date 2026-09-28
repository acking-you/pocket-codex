// Opt-in native end-to-end check against the user's real OpenCode service,
// through the real bridge and relay. Creates sessions only under
// $TMPDIR/pocket-opencode-e2e and sends at most five short prompts.
// Run: fvm flutter test integration_test/opencode_live_test.dart -d macos
//      --dart-define=PCX_OPENCODE_LIVE=true
import 'dart:async';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:path_provider/path_provider.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/bridge_api_rust.dart';
import 'package:pocket_codex/src/rust/api/bridge.dart' as frb;
import 'package:pocket_codex/src/rust/frb_generated.dart';

final _log = <String>[];
void _note(String line) {
  _log.add(line);
  // ignore: avoid_print
  print('[opencode-e2e] $line');
}

Future<AppEvent> _waitFor(
  Stream<AppEvent> events,
  bool Function(AppEvent) test, {
  Duration timeout = const Duration(minutes: 3),
}) => events.firstWhere(test).timeout(timeout);

bool _done(AppEvent e, String tid) =>
    e.threadId == tid && e.kind == 'turn/completed';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  testWidgets(
    'OpenCode hosting and session control end to end',
    (tester) async {
      if (!const bool.fromEnvironment('PCX_OPENCODE_LIVE')) {
        markTestSkipped('set PCX_OPENCODE_LIVE=true to run');
        return;
      }
      await RustLib.init();
      final dir = await getApplicationSupportDirectory();
      await frb.initBridge(supportDir: dir.path);
      const api = RustBridgeApi();

      // 1. Hosting.
      final hosted = await api.appServeStartOpencode(name: 'opencode');
      _note(
        '1 host ${hosted.serviceKey} v${hosted.version} '
        'verified=${hosted.verified} started=${hosted.startedService}',
      );
      expect(hosted.verified, isTrue);
      var registered = false;
      for (var i = 0; i < 60 && !registered; i++) {
        final rows = await api.appServeStatus();
        registered = rows.any(
          (r) =>
              r.provider == 'opencode' &&
              r.name == 'opencode' &&
              r.appRegistered,
        );
        if (!registered) await Future<void>.delayed(const Duration(seconds: 1));
      }
      _note('1 registered=$registered');
      expect(registered, isTrue);

      // 9. A Codex host may not reuse the OpenCode name.
      Object? clash;
      try {
        await api.appServeStart(port: 0, name: 'opencode', embedded: false);
      } catch (e) {
        clash = e;
      }
      _note('9 same-name codex refused=${clash != null}: $clash');
      expect(clash, isNotNull);

      // 3. Existing sessions, grouped by directory, and paging.
      final key = hosted.serviceKey;
      await api.appConnect(key, 0);
      final threads = await api.appThreadList(key);
      final dirs = threads.map((t) => t.cwd).toSet();
      _note('3 sessions=${threads.length} directories=${dirs.length}');
      expect(threads, isNotEmpty);
      expect(threads.every((t) => t.cwd.isNotEmpty), isTrue);
      final existing = threads.first;
      final history = await api.appThreadRead(key, existing.id);
      var older = 0;
      if (history.hasOlder) {
        older = (await api.appThreadOlderPage(key, existing.id)).items.length;
      }
      _note(
        '3 read items=${history.items.length} turns=${history.turns.length} '
        'hasOlder=${history.hasOlder} olderItems=$older',
      );
      expect(history.items, isNotEmpty);

      // 4. New session + streaming.
      final e2e = Directory('${Directory.systemTemp.path}/pocket-opencode-e2e')
        ..createSync(recursive: true);
      final events = api.appEvents(key).asBroadcastStream();
      final seen = <AppEvent>[];
      final sub = events.listen(seen.add);
      final tid = await api.appThreadStart(key, cwd: e2e.path);
      _note('4 created $tid in ${e2e.path}');
      final done1 = _waitFor(events, (e) => _done(e, tid));
      await api.appTurnStart(key, tid, '用一句话自我介绍');
      final end1 = await done1;
      final deltas = seen
          .where(
            (e) => e.threadId == tid && e.kind == 'item/agentMessage/delta',
          )
          .length;
      final read1 = await api.appThreadRead(key, tid);
      final reply = read1.items.where((i) => i.itemType == 'agentMessage');
      _note(
        '4 deltas=$deltas end=${end1.raw} running=${read1.running} '
        'reply=${reply.map((i) => i.text).join(' ').replaceAll('\n', ' ')}',
      );
      expect(reply, isNotEmpty);
      expect(read1.running, isFalse);

      // 5 + 7. A long turn: steer into it, queue another, then interrupt.
      final started = _waitFor(
        events,
        (e) => e.threadId == tid && e.kind == 'item/agentMessage/delta',
      );
      await api.appTurnStart(key, tid, '从1数到300，每行一个数字，不要省略。');
      await started;
      final steered = await api.appTurnSteer(key, tid, null, '数完后再说一句：完毕');
      _note('5 steer accepted turn=$steered');
      await api.appTurnStart(key, tid, '用一句话说你刚才做了什么');
      _note('5 queued prompt accepted');
      final interrupted = _waitFor(events, (e) => _done(e, tid));
      await api.appTurnInterrupt(key, tid);
      final end2 = await interrupted;
      _note('7 after interrupt: ${end2.raw}');
      expect(end2.raw, contains('interrupted'));
      // The queued prompt may now run; let the session settle.
      for (var i = 0; i < 120; i++) {
        if (!(await api.appThreadRead(key, tid)).running) break;
        await Future<void>.delayed(const Duration(seconds: 1));
      }
      final alive = await api.appProbeReason(key);
      _note('7 service still answers=${alive == null}');
      expect(alive, isNull);

      // 6. A shell permission, declined.
      final asked = _waitFor(
        events,
        (e) => e.threadId == tid && e.requestId != null,
        timeout: const Duration(minutes: 2),
      ).then<AppEvent?>((e) => e).catchError((_) => null);
      await api.appTurnStart(
        key,
        tid,
        '请用 shell 工具运行命令 `ls ${e2e.path}` 并告诉我结果。',
      );
      final request = await asked;
      if (request == null) {
        _note('6 no permission request arrived (OpenCode config allows shell)');
      } else {
        _note('6 approval kind=${request.kind} raw=${request.raw}');
        final resolved = _waitFor(
          events,
          (e) =>
              e.kind == 'serverRequest/resolved' &&
              e.raw.contains(request.requestId!),
          timeout: const Duration(seconds: 30),
        ).then<bool>((_) => true).catchError((_) => false);
        await api.appRespondApproval(key, request.requestId!, 'decline');
        _note('6 declined, card resolved=${await resolved}');
      }
      for (var i = 0; i < 120; i++) {
        if (!(await api.appThreadRead(key, tid)).running) break;
        await Future<void>.delayed(const Duration(seconds: 1));
      }

      // 8. Stop hosting: cached history stays, OpenCode keeps running.
      await sub.cancel();
      await api.appDisconnect(key);
      await api.appServeStop('opencode');
      final rows = await api.appServeStatus();
      final cached = await api.appHistoryCached(key, tid);
      final status = await Process.run(
        '${Platform.environment['HOME']}/.opencode/bin/opencode',
        ['service', 'status'],
      );
      _note(
        '8 hosted after stop=${rows.any((r) => r.provider == 'opencode')} '
        'cachedItems=${cached?.items.length} cachedRunning=${cached?.running} '
        'serviceStatus=${(status.stdout as String).trim()}',
      );
      expect(rows.any((r) => r.provider == 'opencode'), isFalse);
      expect(cached, isNotNull);
      expect(cached!.running, isFalse);
      expect((status.stdout as String).trim(), startsWith('http'));
      _note('SUMMARY\n${_log.join('\n')}');
    },
    timeout: const Timeout(Duration(minutes: 20)),
  );
}
