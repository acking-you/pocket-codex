import 'package:pocket_codex/src/bridge_api.dart';
import 'transcript_model.dart';
import 'transcript_rows.dart';
import 'activity_cards.dart' show TurnWork;

/// Missing history at its actual position, separate from the oldest boundary.
class HistoryGap {
  /// A gap before an unread turn, or within a partially fetched turn.
  const HistoryGap(this.turnId, {this.continuation = false});

  /// Turn to fetch when this gap is approached or selected.
  final String turnId;

  /// Whether this is another ascending page of an already selected turn.
  final bool continuation;
}

/// Keep disjoint timeline jumps visibly separated until their gaps are read.
List<Object> buildHistoryRows(
  List<TranscriptItem> items, {
  required List<TurnSummary> turns,
  required Map<String, TurnItemsPage> windows,
  required Set<String> sequentialIds,
}) {
  if (windows.isEmpty) return summarizeTurnWork(buildTranscriptRows(items));
  final order = {for (var i = 0; i < turns.length; i++) turns[i].turnId: i};
  final out = <Object>[];
  void gap(HistoryGap next) {
    // Adjacent missing ranges are one visible gap. Finish the earlier range
    // first; recomputing rows will expose the next cursor when it is needed.
    if (out.isEmpty || out.last is! HistoryGap) out.add(next);
  }

  int? previous;
  for (var at = 0; at < items.length;) {
    final turnId = items[at].turnId;
    var end = at + 1;
    while (end < items.length && items[end].turnId == turnId) {
      end++;
    }
    final index = order[turnId];
    if (index != null && previous != null && index > previous + 1) {
      for (var missing = previous + 1; missing < index; missing++) {
        final id = turns[missing].turnId;
        final cached = windows[id];
        if (cached != null && cached.items.isEmpty && !cached.hasMore) continue;
        gap(HistoryGap(id));
        break;
      }
    }
    final group = items.sublist(at, end);
    final window = windows[turnId];
    final boundary = window?.items.lastOrNull?.id;
    final split = boundary == null
        ? -1
        : group.indexWhere((item) => item.id == boundary);
    if (window != null && window.hasMore && !sequentialIds.contains(boundary)) {
      // The sequential tail already covers everything after its first item.
      // An overlap with that tail closes the gap without re-fetching its pages.
      final prefixLength = split < 0 ? group.length : split + 1;
      out.addAll(buildTranscriptRows(group.sublist(0, prefixLength)));
      gap(HistoryGap(turnId, continuation: true));
      out.addAll(buildTranscriptRows(group.sublist(prefixLength)));
    } else {
      out.addAll(buildTranscriptRows(group));
    }
    previous = index ?? previous;
    at = end;
  }
  return summarizeTurnWork(out);
}

/// Show an entire turn's duration only at its final visible work segment.
/// A prefix ending at a gap has not reached that turn's end yet.
List<Object> summarizeTurnWork(List<Object> rows) {
  final seen = <String>{};
  for (var i = rows.length - 1; i >= 0; i--) {
    final row = rows[i];
    if (row is HistoryGap && row.continuation) {
      seen.add(row.turnId);
    } else if (row is TurnWork) {
      final turn = row.items.first.turnId;
      if (turn.isNotEmpty && !seen.add(turn)) {
        rows[i] = TurnWork(row.items, showDuration: false);
      }
    }
  }
  return rows;
}
