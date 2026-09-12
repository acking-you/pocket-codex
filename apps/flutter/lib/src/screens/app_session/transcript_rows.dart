import 'transcript_model.dart';
import 'activity_cards.dart' show AgentTurn, TurnWork;

/// Build stable, collapsed timeline rows in linear time.
List<Object> buildTranscriptRows(List<TranscriptItem> items) {
  final finalReply = List<bool>.filled(items.length, false);
  var activityAfter = false;
  for (var i = items.length - 1; i >= 0; i--) {
    final item = items[i];
    if (item.isUser) {
      activityAfter = false;
    } else if (item.isAgent) {
      finalReply[i] = !activityAfter;
    } else if (!item.standsAlone) {
      activityAfter = true;
    }
  }
  final out = <Object>[];
  var i = 0;
  while (i < items.length) {
    final it = items[i];
    // One reply, one block. A turn's prose arrives as several `agentMessage`
    // items (the server gives each its own id — a preamble before a tool
    // batch, then the final answer), and rendering one block per item chopped
    // a single answer into pieces that each carried their own hover actions.
    //
    // The run is bounded by the server's own `turnId` where it is known, so
    // this is the real turn boundary rather than "consecutive agent prose".
    // Items whose turn is unknown (empty id — a rollout file read from disk,
    // or a live item that arrived before `turn/started`) fall back to
    // adjacency, which is what the sequence can tell us.
    if (it.isAgent && finalReply[i]) {
      var j = i + 1;
      while (j < items.length &&
          items[j].isAgent &&
          items[j].turnId == it.turnId) {
        j++;
      }
      out.add(j - i >= 2 ? AgentTurn(items.sublist(i, j)) : it);
      i = j;
      continue;
    }
    // A user message or a turn footnote always stands alone.
    if (it.isUser || it.standsAlone) {
      out.add(it);
      i++;
      continue;
    }
    // Everything up to the next user message or turn footnote is this turn's
    // work — including compaction notices and the agent's own intermediate
    // prose, which are things it did on the way to the answer.
    //
    // Deliberately spans them rather than stopping at them. Stopping produced
    // one 已处理 row per stretch of tool calls, so a turn that thought out loud
    // between batches rendered as three or four rows carrying the SAME duration
    // — visibly one turn, presented as several. The final reply is the run's
    // boundary, so it stays where it is, beneath the fold.
    var j = i + 1;
    while (j < items.length &&
        !items[j].isUser &&
        !items[j].standsAlone &&
        !finalReply[j]) {
      j++;
    }
    out.add(TurnWork(items.sublist(i, j)));
    i = j;
  }
  return out;
}
