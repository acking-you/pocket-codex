import 'dart:async';

import 'package:pocket_codex/src/bridge_api.dart';

/// Buffers captured runtime log lines and exposes them — with level + keyword
/// filtering — to the log viewer.
///
/// A process-wide singleton subscribed once (at app start via [initialize]) to
/// [BridgeApi.logEvents], which replays retained history then streams live. So
/// the viewer shows recent logs even when opened long after boot, and multiple
/// viewers share one buffer.
class LogManager {
  LogManager._();

  /// The shared instance.
  static final LogManager instance = LogManager._();

  /// How many lines to retain (matches the Rust ring buffer).
  static const int maxLines = 2000;

  /// Levels low→high, for the threshold dropdown.
  static const List<String> levels = [
    'TRACE',
    'DEBUG',
    'INFO',
    'WARN',
    'ERROR',
  ];
  static const Map<String, int> _priority = {
    'TRACE': 0,
    'DEBUG': 1,
    'INFO': 2,
    'WARN': 3,
    'ERROR': 4,
  };

  final List<LogLine> _lines = [];
  final StreamController<List<LogLine>> _controller =
      StreamController<List<LogLine>>.broadcast();
  StreamSubscription<LogLine>? _sub;
  bool _initialized = false;

  /// Emits the full buffer on every change (a fresh unmodifiable snapshot).
  Stream<List<LogLine>> get stream => _controller.stream;

  /// The current buffer (oldest first).
  List<LogLine> get lines => List.unmodifiable(_lines);

  /// Number of buffered lines.
  int get count => _lines.length;

  /// Subscribe to the bridge's log stream. Idempotent — safe to call once at
  /// boot; later calls are no-ops.
  void initialize(BridgeApi api) {
    if (_initialized) return;
    _initialized = true;
    _sub = api.logEvents().listen(_add, onError: (_) {});
    _emit();
  }

  /// Stop capturing (test teardown).
  void dispose() {
    _sub?.cancel();
    _sub = null;
    _initialized = false;
  }

  void _add(LogLine line) {
    _lines.add(line);
    if (_lines.length > maxLines) {
      _lines.removeRange(0, _lines.length - maxLines);
    }
    _emit();
  }

  void _emit() {
    if (!_controller.isClosed) {
      _controller.add(List.unmodifiable(_lines));
    }
  }

  /// Drop all buffered lines (the source keeps streaming new ones).
  void clear() {
    _lines.clear();
    _emit();
  }

  /// Normalize a level string to one of [levels], else `UNKNOWN`.
  static String normalizeLevel(String level) {
    final n = level.trim().toUpperCase();
    return _priority.containsKey(n) ? n : 'UNKNOWN';
  }

  /// Whether `entry` is at or above the `threshold` level.
  static bool includesThreshold({
    required String threshold,
    required String entry,
  }) {
    final t = _priority[normalizeLevel(threshold)];
    final e = _priority[normalizeLevel(entry)];
    if (t == null) return true;
    if (e == null) return false;
    return e >= t;
  }

  /// Dropdown label for a threshold level (`WARN+`, but plain `ERROR`).
  static String thresholdLabel(String level) {
    final n = normalizeLevel(level);
    return (n == 'ERROR' || n == 'UNKNOWN') ? n : '$n+';
  }

  /// The buffer filtered by a minimum level threshold + a case-insensitive
  /// keyword (matched against level, target, and message).
  List<LogLine> filter({String? level, String keyword = ''}) {
    final lvl = (level == null || level.trim().isEmpty)
        ? null
        : normalizeLevel(level);
    final kw = keyword.trim().toLowerCase();
    return _lines.where((l) {
      if (lvl != null && !includesThreshold(threshold: lvl, entry: l.level)) {
        return false;
      }
      if (kw.isNotEmpty) {
        final hay = '${l.level} ${l.target} ${l.message}'.toLowerCase();
        if (!hay.contains(kw)) return false;
      }
      return true;
    }).toList();
  }
}
