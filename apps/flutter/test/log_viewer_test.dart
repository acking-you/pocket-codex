import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/log_manager.dart';
import 'package:pocket_codex/src/screens/log_view_screen.dart';

import 'fake_bridge_api.dart';

LogLine _line(String level, String msg, {String target = 'test', int ts = 0}) =>
    LogLine(level: level, target: target, message: msg, timestampMs: ts);

void main() {
  group('LogManager level helpers', () {
    test('normalizeLevel maps known levels, else UNKNOWN', () {
      expect(LogManager.normalizeLevel('info'), 'INFO');
      expect(LogManager.normalizeLevel(' Warn '), 'WARN');
      expect(LogManager.normalizeLevel('weird'), 'UNKNOWN');
    });

    test('includesThreshold is a minimum-level gate', () {
      expect(
        LogManager.includesThreshold(threshold: 'WARN', entry: 'ERROR'),
        isTrue,
      );
      expect(
        LogManager.includesThreshold(threshold: 'WARN', entry: 'INFO'),
        isFalse,
      );
      expect(
        LogManager.includesThreshold(threshold: 'WARN', entry: 'WARN'),
        isTrue,
      );
    });

    test('thresholdLabel appends + except ERROR', () {
      expect(LogManager.thresholdLabel('INFO'), 'INFO+');
      expect(LogManager.thresholdLabel('ERROR'), 'ERROR');
    });
  });

  group('LogManager buffering + filtering', () {
    setUp(() {
      LogManager.instance.dispose();
      LogManager.instance.clear();
    });

    test('buffers streamed lines and filters by level + keyword', () async {
      final fake = FakeBridgeApi();
      LogManager.instance.initialize(fake);
      fake.pushLog(_line('INFO', 'starting up'));
      fake.pushLog(_line('ERROR', 'boom happened', target: 'serve'));
      fake.pushLog(_line('DEBUG', 'tick'));
      await Future<void>.delayed(Duration.zero); // let the stream deliver

      expect(LogManager.instance.count, 3);
      // Level threshold: WARN+ keeps only the ERROR line.
      expect(LogManager.instance.filter(level: 'WARN').map((l) => l.message), [
        'boom happened',
      ]);
      // Keyword matches target/level/message, case-insensitive.
      expect(
        LogManager.instance.filter(keyword: 'SERVE').map((l) => l.message),
        ['boom happened'],
      );
      // Combined filters intersect.
      expect(
        LogManager.instance.filter(level: 'ERROR', keyword: 'tick'),
        isEmpty,
      );
    });

    test('clear empties the buffer', () async {
      final fake = FakeBridgeApi();
      LogManager.instance.initialize(fake);
      fake.pushLog(_line('INFO', 'a'));
      await Future<void>.delayed(Duration.zero);
      expect(LogManager.instance.count, 1);
      LogManager.instance.clear();
      expect(LogManager.instance.count, 0);
    });
  });

  group('LogViewScreen', () {
    setUp(() {
      LogManager.instance.dispose();
      LogManager.instance.clear();
    });

    testWidgets('renders streamed log lines', (t) async {
      final fake = FakeBridgeApi();
      LogManager.instance.initialize(fake);
      fake.pushLog(_line('ERROR', 'tunnel down', target: 'serve'));

      await t.pumpWidget(
        MaterialApp(
          locale: const Locale('zh'),
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          home: const LogViewScreen(),
        ),
      );
      await t.pump(); // flush the streamed line into the list

      expect(find.textContaining('tunnel down'), findsOneWidget);
    });
  });
}
