import 'package:pocket_codex/l10n/gen/app_localizations.dart';

/// Whether [unixSeconds] falls on the same calendar day as [now]. A `0`/absent
/// timestamp is treated as not-today (so it buckets under "earlier").
bool isSameDay(int unixSeconds, DateTime now) {
  if (unixSeconds <= 0) return false;
  final d = DateTime.fromMillisecondsSinceEpoch(unixSeconds * 1000);
  return d.year == now.year && d.month == now.month && d.day == now.day;
}

/// A short localized "time ago" for a last-updated timestamp, relative to
/// [now]: 刚刚 / N 分钟前 / N 小时前 / 昨天 / N 天前. Returns an empty string
/// when the timestamp is missing (`0`).
///
/// Shared by the conversation list and the local-sessions list so both label
/// activity time identically.
String relativeTime(int unixSeconds, DateTime now, AppLocalizations l10n) {
  if (unixSeconds <= 0) return '';
  final then = DateTime.fromMillisecondsSinceEpoch(unixSeconds * 1000);
  final diff = now.difference(then);
  if (diff.inMinutes < 1) return l10n.timeJustNow;
  if (diff.inMinutes < 60) return l10n.timeMinutesAgo(diff.inMinutes);
  if (isSameDay(unixSeconds, now)) return l10n.timeHoursAgo(diff.inHours);
  final yesterday = now.subtract(const Duration(days: 1));
  if (then.year == yesterday.year &&
      then.month == yesterday.month &&
      then.day == yesterday.day) {
    return l10n.timeYesterday;
  }
  return l10n.timeDaysAgo(diff.inDays);
}
