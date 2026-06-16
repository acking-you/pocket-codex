import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/context_status.dart';

void main() {
  group('ContextStatus.fromRaw', () {
    test('parses nested tokenUsage with last/total breakdown', () {
      final raw = jsonEncode({
        'threadId': 't1',
        'tokenUsage': {
          'last': {'totalTokens': 24000, 'inputTokens': 20000},
          'total': {'totalTokens': 99000},
          'modelContextWindow': 200000,
        },
      });
      final c = ContextStatus.fromRaw(raw);
      expect(c, isNotNull);
      // Context occupancy prefers the latest turn's total.
      expect(c!.tokensUsed, 24000);
      expect(c.contextWindow, 200000);
      expect(c.percent, 12);
      expect(c.fraction, closeTo(0.12, 1e-9));
    });

    test('accepts a top-level usage object (no tokenUsage wrapper)', () {
      final raw = jsonEncode({
        'total': {'totalTokens': 5000},
        'modelContextWindow': 100000,
      });
      final c = ContextStatus.fromRaw(raw);
      expect(c!.tokensUsed, 5000);
      expect(c.percent, 5);
    });

    test('returns null without a usable window', () {
      expect(ContextStatus.fromRaw(jsonEncode({'foo': 1})), isNull);
      expect(ContextStatus.fromRaw('not json'), isNull);
      expect(
        ContextStatus.fromRaw(jsonEncode({'modelContextWindow': 0})),
        isNull,
      );
    });

    test('fraction clamps to 1 when over budget', () {
      final c = ContextStatus(tokensUsed: 250000, contextWindow: 200000);
      expect(c.fraction, 1.0);
      expect(c.percent, 100);
    });
  });

  group('RateLimits.fromRaw', () {
    test('parses primary + secondary windows with field variants', () {
      final raw = jsonEncode({
        'rateLimits': {
          'primary': {
            'usedPercent': 42.5,
            'windowDurationMins': 300,
            'resetsInSeconds': 3600,
          },
          'secondary': {
            'usedPercent': 7,
            'windowMinutes': 10080,
            'resetsAt': 1900000000,
          },
        },
      });
      final r = RateLimits.fromRaw(raw);
      expect(r, isNotNull);
      expect(r!.primary!.usedPercent, 42.5);
      expect(r.primary!.windowMinutes, 300);
      expect(r.primary!.resetsInSeconds, 3600);
      expect(r.secondary!.usedPercent, 7);
      expect(r.secondary!.windowMinutes, 10080);
      // Epoch seconds get normalised to milliseconds.
      expect(r.secondary!.resetsAtEpochMs, 1900000000 * 1000);
    });

    test('returns null when no windows present', () {
      expect(RateLimits.fromRaw(jsonEncode({'foo': 1})), isNull);
      expect(RateLimits.fromRaw('garbage'), isNull);
    });

    test('parses v2 individualLimit (string amounts) + reset credits', () {
      final raw = jsonEncode({
        'rateLimits': {
          'primary': {'usedPercent': 10},
          'individualLimit': {
            'limit': '100.00',
            'used': '25.00',
            'remainingPercent': 75,
            'resetsAt': 1900000000,
          },
        },
        'rateLimitResetCredits': {'availableCount': 2},
      });
      final r = RateLimits.fromRaw(raw)!;
      expect(r.individualLimit!.limit, '100.00');
      expect(r.individualLimit!.used, '25.00');
      expect(r.individualLimit!.remainingPercent, 75);
      expect(r.individualLimit!.fraction, closeTo(0.25, 1e-9));
      expect(r.individualLimit!.resetsAtEpochMs, 1900000000 * 1000);
      expect(r.resetCreditsAvailable, 2);
    });

    test('merge keeps prior fields a sparse update omits', () {
      final full = RateLimits.fromRaw(
        jsonEncode({
          'rateLimits': {
            'primary': {'usedPercent': 40},
            'secondary': {'usedPercent': 5},
          },
          'rateLimitResetCredits': {'availableCount': 1},
        }),
      )!;
      // A rolling update that only re-sends the primary window.
      final sparse = RateLimits.fromRaw(
        jsonEncode({
          'rateLimits': {
            'primary': {'usedPercent': 55},
          },
        }),
      )!;
      final merged = full.merge(sparse);
      expect(merged.primary!.usedPercent, 55); // updated
      expect(merged.secondary!.usedPercent, 5); // preserved, not blanked
      expect(merged.resetCreditsAvailable, 1); // preserved
    });
  });
}
