import 'package:pocket_codex/src/screens/app_session/transcript_model.dart';

typedef _TurnKey = ({String turn, String? item});

/// Merge a chronological page with previously loaded, possibly disjoint turns.
/// Shared item IDs anchor overlapping pages without replacing live snapshots.
List<TranscriptItem> mergeHistoryItems(
  List<TranscriptItem> current,
  List<TranscriptItem> incoming, {
  required Iterable<String> turnOrder,
  required bool olderPage,
}) {
  Map<_TurnKey, List<TranscriptItem>> group(List<TranscriptItem> items) {
    final groups = <_TurnKey, List<TranscriptItem>>{};
    for (final item in items) {
      // Optimistic user rows may not have a server turn ID yet. Keep each in
      // place rather than grouping unrelated anonymous rows into one turn.
      final key = (
        turn: item.turnId,
        item: item.turnId.isEmpty ? item.id : null,
      );
      (groups[key] ??= []).add(item);
    }
    return groups;
  }

  final old = group(current);
  final pages = group(incoming);
  final knownTurns = turnOrder
      .map((turn) => (turn: turn, item: null as String?))
      .toSet();
  final existingOrder = old.keys.toList();
  final firstKnown = existingOrder.indexWhere(knownTurns.contains);
  final order = <_TurnKey>{
    if (firstKnown >= 0) ...existingOrder.take(firstKnown),
    ...knownTurns,
    ...existingOrder,
  }.toList();
  final missing = pages.keys.where((id) => !order.contains(id)).toList();
  order.insertAll(olderPage ? 0 : order.length, missing);
  return [
    for (final turn in order)
      ..._mergeTurn(old[turn] ?? [], pages[turn] ?? [], olderPage),
  ];
}

List<TranscriptItem> _mergeTurn(
  List<TranscriptItem> current,
  List<TranscriptItem> incoming,
  bool olderPage,
) {
  final positions = {for (var i = 0; i < current.length; i++) current[i].id: i};
  final seen = <String>{};
  final out = <TranscriptItem>[];
  final pending = <TranscriptItem>[];
  var next = 0;
  var anchored = false;
  for (final item in incoming) {
    if (!seen.add(item.id)) continue;
    final at = positions[item.id];
    if (at == null) {
      pending.add(item);
    } else if (at >= next) {
      out
        ..addAll(current.getRange(next, at))
        ..addAll(pending)
        ..add(current[at]);
      pending.clear();
      next = at + 1;
      anchored = true;
    }
  }
  if (anchored || olderPage) {
    out
      ..addAll(pending)
      ..addAll(current.skip(next));
  } else {
    out
      ..addAll(current)
      ..addAll(pending);
  }
  return out;
}
