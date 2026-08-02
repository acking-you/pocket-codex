/// Parsing for codex's **realtime (Live voice) handoff** messages.
///
/// A Live conversation runs against the realtime model; when it needs the text
/// model it delegates by posting a user-role message shaped like this
/// (`realtime_conversation.rs`, `wrap_realtime_delegation_input`):
///
/// ```xml
/// <realtime_delegation>
///   <source>transcript_tail_flush</source>
///   <input>切换到 Live 模式</input>
///   <transcript_delta>assistant: 我看看怎么切换。
/// user: 现在要怎么说</transcript_delta>
/// </realtime_delegation>
/// ```
///
/// `<input>` is the turn that triggered the handoff; `<transcript_delta>` is
/// the spoken conversation since the last handoff — the part actually worth
/// reading. Both are XML-escaped upstream (`escape_xml_text`), so they are
/// unescaped here.
///
/// This is NOT one of the injected context fragments in `ide_context.dart`:
/// those are machinery to hide, this is the person talking.
library;

/// One spoken turn inside a handoff's transcript.
class RealtimeTurn {
  /// Creates a turn.
  const RealtimeTurn({required this.isUser, required this.text});

  /// True for the person, false for the assistant.
  final bool isUser;

  /// What was said, with any continuation lines joined in.
  final String text;
}

/// A parsed `<realtime_delegation>` message.
class RealtimeHandoff {
  /// Creates a handoff.
  const RealtimeHandoff({
    required this.input,
    required this.turns,
    this.source,
  });

  /// Why the handoff happened, when codex says so — `transcript_tail_flush`
  /// means the user ended the voice session and this is the tail.
  final String? source;

  /// The turn that triggered the handoff. Often repeats the last spoken turn.
  final String input;

  /// The spoken conversation, oldest first. May be empty.
  final List<RealtimeTurn> turns;

  /// True when the voice session ended and this is its closing tail.
  bool get isSessionTail => source == 'transcript_tail_flush';

  /// Whether [input] adds anything worth showing under the transcript.
  ///
  /// Two ways it doesn't: on a session tail it is codex's own note TO the
  /// model ("The user just ended their realtime session…"), which the user
  /// never said; otherwise it is normally the last spoken turn repeated,
  /// because saying something is what triggered the handoff.
  bool get inputIsRedundant =>
      input.isEmpty ||
      isSessionTail ||
      turns.isNotEmpty && turns.last.isUser && turns.last.text == input;
}

String? _element(String xml, String tag) {
  final open = xml.indexOf('<$tag>');
  if (open < 0) return null;
  final close = xml.indexOf('</$tag>', open);
  if (close < 0) return null;
  return _unescape(xml.substring(open + tag.length + 2, close));
}

// Mirrors upstream `escape_xml_text`. `&amp;` last, or `&amp;lt;` would
// decode twice.
String _unescape(String s) =>
    s.replaceAll('&lt;', '<').replaceAll('&gt;', '>').replaceAll('&amp;', '&');

// `user: …` / `assistant: …`. A line without a speaker continues the one
// before it (the transcript wraps long utterances).
final _speakerLine = RegExp(r'^(user|assistant):\s?(.*)$');

/// Parse a `<realtime_delegation>` message, or null when [raw] isn't one.
RealtimeHandoff? parseRealtimeDelegation(String raw) {
  final trimmed = raw.trim();
  if (!trimmed.startsWith('<realtime_delegation>') ||
      !trimmed.endsWith('</realtime_delegation>')) {
    return null;
  }
  final input = _element(trimmed, 'input');
  if (input == null) return null;
  final turns = <RealtimeTurn>[];
  for (final line in (_element(trimmed, 'transcript_delta') ?? '').split(
    '\n',
  )) {
    final match = _speakerLine.firstMatch(line.trim());
    if (match != null) {
      turns.add(
        RealtimeTurn(isUser: match[1] == 'user', text: match[2]!.trim()),
      );
    } else if (line.trim().isNotEmpty && turns.isNotEmpty) {
      final prev = turns.removeLast();
      turns.add(
        RealtimeTurn(isUser: prev.isUser, text: '${prev.text}\n${line.trim()}'),
      );
    }
  }
  turns.removeWhere((t) => t.text.isEmpty);
  return RealtimeHandoff(
    source: _element(trimmed, 'source'),
    input: input.trim(),
    turns: _dropSupersededPartials(turns),
  );
}

// Live transcription streams partial utterances and then the settled one, and
// the delta carries both — so a card built from the raw turns reads as
// "有bug好像 / 。怎么 / 换 / 那个" before the sentence those pieces became.
// A partial is always contained in the utterance that supersedes it, once
// whitespace is ignored (the chunk boundaries fall mid-word). Keeping only the
// last turn per speaker that no later same-speaker turn contains collapses the
// stream back to what was actually said. Worst case a genuinely repeated short
// phrase is folded into the longer one that follows it — far better than the
// wall of fragments.
List<RealtimeTurn> _dropSupersededPartials(List<RealtimeTurn> turns) {
  String squash(String s) => s.replaceAll(RegExp(r'\s+'), '');
  final kept = <RealtimeTurn>[];
  for (var i = 0; i < turns.length; i++) {
    final mine = squash(turns[i].text);
    final superseded = turns
        .skip(i + 1)
        .any(
          (later) =>
              later.isUser == turns[i].isUser &&
              squash(later.text) != mine &&
              squash(later.text).contains(mine),
        );
    if (!superseded) kept.add(turns[i]);
  }
  return kept;
}
