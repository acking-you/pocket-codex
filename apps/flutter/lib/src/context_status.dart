import 'dart:convert';

/// Current context-window occupancy for a thread, used by the status gauge.
class ContextStatus {
  /// Creates a context status.
  const ContextStatus({required this.tokensUsed, required this.contextWindow});

  /// Tokens currently occupying the model context window.
  final int tokensUsed;

  /// The model's context-window size in tokens.
  final int contextWindow;

  /// Fraction 0..1 of the context window in use (0 if the window is unknown).
  double get fraction =>
      contextWindow > 0 ? (tokensUsed / contextWindow).clamp(0.0, 1.0) : 0.0;

  /// Whole-percent context occupancy.
  int get percent => (fraction * 100).round();

  /// Parse a `thread/tokenUsage/updated` event's raw params (or a thread-read
  /// seed). Accepts either `{tokenUsage:{...}}` or a top-level usage object
  /// with `last`/`total` breakdowns plus `modelContextWindow`. Returns null if
  /// the shape doesn't carry a usable window + token count.
  static ContextStatus? fromRaw(String raw) {
    try {
      final decoded = jsonDecode(raw);
      if (decoded is! Map) return null;
      final usage = decoded['tokenUsage'] is Map
          ? decoded['tokenUsage'] as Map
          : decoded;
      final window = _int(usage['modelContextWindow']);
      if (window == null || window <= 0) return null;
      final used =
          _breakdownTotal(usage['last']) ??
          _breakdownTotal(usage['total']) ??
          _int(usage['totalTokens']);
      if (used == null) return null;
      return ContextStatus(tokensUsed: used, contextWindow: window);
    } catch (_) {
      return null;
    }
  }

  static int? _breakdownTotal(Object? b) =>
      b is Map ? _int(b['totalTokens']) : null;
}

/// One rate-limit window (e.g. the 5-hour or weekly quota).
class RateLimitWindow {
  /// Creates a rate-limit window.
  const RateLimitWindow({
    required this.usedPercent,
    this.windowMinutes,
    this.resetsAtEpochMs,
    this.resetsInSeconds,
  });

  /// Percent of this window consumed (0..100).
  final double usedPercent;

  /// Window length in minutes (≈300 for 5h, ≈10080 for weekly), if known.
  final int? windowMinutes;

  /// Absolute reset time in epoch milliseconds, if the server gave one.
  final int? resetsAtEpochMs;

  /// Seconds until reset, if the server gave a relative value instead.
  final int? resetsInSeconds;

  /// Fraction 0..1 consumed.
  double get fraction => (usedPercent / 100).clamp(0.0, 1.0);

  /// Parse one window object, defensively over field-name variants.
  static RateLimitWindow? fromMap(Object? v) {
    if (v is! Map) return null;
    final pct = _num(v['usedPercent']);
    if (pct == null) return null;
    return RateLimitWindow(
      usedPercent: pct.toDouble(),
      windowMinutes: _int(v['windowDurationMins']) ?? _int(v['windowMinutes']),
      resetsAtEpochMs: _resetMs(v['resetsAt']),
      resetsInSeconds: _int(v['resetsInSeconds']) ?? _int(v['resetsAtSeconds']),
    );
  }
}

/// A spend-control / "individual" limit window (codex v2 `individualLimit`).
/// Note the wire quirk: `limit` and `used` are STRINGS (money-ish amounts),
/// while `remainingPercent` is an int.
class SpendControlLimit {
  /// Creates a spend-control limit snapshot.
  const SpendControlLimit({
    required this.limit,
    required this.used,
    required this.remainingPercent,
    this.resetsAtEpochMs,
  });

  /// The cap, as the server's opaque string (e.g. a dollar amount).
  final String limit;

  /// The amount used so far, as the server's opaque string.
  final String used;

  /// Whole-percent of the cap still remaining (0..100).
  final int remainingPercent;

  /// Absolute reset time in epoch milliseconds, if given.
  final int? resetsAtEpochMs;

  /// Fraction 0..1 consumed.
  double get fraction => ((100 - remainingPercent) / 100).clamp(0.0, 1.0);

  /// Parse a `individualLimit` object, or null if it carries nothing usable.
  static SpendControlLimit? fromMap(Object? v) {
    if (v is! Map) return null;
    final limit = v['limit'];
    final used = v['used'];
    final pct = _int(v['remainingPercent']);
    if (limit is! String && used is! String && pct == null) return null;
    return SpendControlLimit(
      limit: limit is String ? limit : '',
      used: used is String ? used : '',
      remainingPercent: pct ?? 0,
      resetsAtEpochMs: _resetMs(v['resetsAt']),
    );
  }
}

/// The account's 5h + weekly quota snapshot, parsed from
/// `account/rateLimits/read` or the `account/rateLimits/updated` event.
class RateLimits {
  /// Creates a quota snapshot.
  const RateLimits({
    this.primary,
    this.secondary,
    this.individualLimit,
    this.resetCreditsAvailable,
  });

  /// The shorter window (typically 5 hours).
  final RateLimitWindow? primary;

  /// The longer window (typically weekly).
  final RateLimitWindow? secondary;

  /// The account's spend-control window (codex v2), if any.
  final SpendControlLimit? individualLimit;

  /// How many rate-limit reset credits the user can redeem (codex v2), if the
  /// server advertised it. Pairs with the `account/rateLimitResetCredit/consume`
  /// endpoint (not yet wired) to clear an active window.
  final int? resetCreditsAvailable;

  /// Whether nothing usable was present.
  bool get isEmpty =>
      primary == null &&
      secondary == null &&
      individualLimit == null &&
      resetCreditsAvailable == null;

  /// Overlay a (possibly sparse) update onto this snapshot, keeping prior values
  /// for fields the update omits. codex v2's `account/rateLimits/updated` is a
  /// rolling PARTIAL update, so a naive replace would blank out windows it didn't
  /// re-send — merge instead.
  RateLimits merge(RateLimits update) => RateLimits(
    primary: update.primary ?? primary,
    secondary: update.secondary ?? secondary,
    individualLimit: update.individualLimit ?? individualLimit,
    resetCreditsAvailable:
        update.resetCreditsAvailable ?? resetCreditsAvailable,
  );

  /// Parse the raw JSON, tolerating a few nesting variants.
  static RateLimits? fromRaw(String raw) {
    try {
      final decoded = jsonDecode(raw);
      if (decoded is! Map) return null;
      final snap = decoded['rateLimits'] is Map
          ? decoded['rateLimits'] as Map
          : decoded['rate_limits'] is Map
          ? decoded['rate_limits'] as Map
          : decoded;
      final credits = decoded['rateLimitResetCredits'];
      final limits = RateLimits(
        primary: RateLimitWindow.fromMap(snap['primary']),
        secondary: RateLimitWindow.fromMap(snap['secondary']),
        individualLimit: SpendControlLimit.fromMap(snap['individualLimit']),
        resetCreditsAvailable: credits is Map
            ? _int(credits['availableCount'])
            : null,
      );
      return limits.isEmpty ? null : limits;
    } catch (_) {
      return null;
    }
  }
}

int? _int(Object? v) => v is int
    ? v
    : v is num
    ? v.toInt()
    : v is String
    ? int.tryParse(v)
    : null;

num? _num(Object? v) => v is num ? v : (v is String ? num.tryParse(v) : null);

/// Normalise a `resetsAt` value to epoch milliseconds. Accepts epoch seconds,
/// epoch milliseconds, or an ISO-8601 string.
int? _resetMs(Object? v) {
  if (v is num) {
    final n = v.toInt();
    // Heuristic: < ~10^12 is seconds, otherwise milliseconds.
    return n < 1000000000000 ? n * 1000 : n;
  }
  if (v is String) {
    final asInt = int.tryParse(v);
    if (asInt != null) return _resetMs(asInt);
    return DateTime.tryParse(v)?.millisecondsSinceEpoch;
  }
  return null;
}
