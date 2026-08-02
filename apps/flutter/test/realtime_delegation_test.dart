import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/realtime_delegation.dart';

// The shape upstream writes (`wrap_realtime_delegation_input`), with the
// `<source>` a session-tail flush adds.
const _handoff = '''
<realtime_delegation>
  <source>transcript_tail_flush</source>
  <input>现在要怎么说</input>
  <transcript_delta>assistant: 我看看怎么切换。
user: 不是，你这个怎么切到 live
assistant: 桌面端直接
user: 现在要怎么说</transcript_delta>
</realtime_delegation>''';

void main() {
  test('parses the spoken turns, the trigger and the source', () {
    final h = parseRealtimeDelegation(_handoff)!;
    expect(h.source, 'transcript_tail_flush');
    expect(h.isSessionTail, isTrue);
    expect(h.input, '现在要怎么说');
    expect(h.turns.map((t) => t.text), [
      '我看看怎么切换。',
      '不是，你这个怎么切到 live',
      '桌面端直接',
      '现在要怎么说',
    ]);
    expect(h.turns.map((t) => t.isUser), [false, true, false, true]);
    // The trigger repeats the last spoken turn, so the card must not show it
    // a second time.
    expect(h.inputIsRedundant, isTrue);
  });

  test('unescapes what upstream escaped', () {
    // `escape_xml_text` runs over both elements, so a spoken `<` arrives as
    // `&lt;` and would otherwise be read back as markup.
    final h = parseRealtimeDelegation(
      '<realtime_delegation>\n  <input>a &amp; b</input>\n'
      '  <transcript_delta>user: use &lt;div&gt; &amp; friends</transcript_delta>\n'
      '</realtime_delegation>',
    )!;
    expect(h.input, 'a & b');
    expect(h.turns.single.text, 'use <div> & friends');
  });

  test('a handoff without a transcript still parses', () {
    final h = parseRealtimeDelegation(
      '<realtime_delegation>\n  <input>run ls</input>\n</realtime_delegation>',
    )!;
    expect(h.turns, isEmpty);
    expect(h.input, 'run ls');
    expect(h.isSessionTail, isFalse);
    // Nothing else shows it, so the trigger is worth rendering here.
    expect(h.inputIsRedundant, isFalse);
  });

  test('joins a wrapped utterance onto its own turn', () {
    final h = parseRealtimeDelegation(
      '<realtime_delegation>\n  <input>x</input>\n'
      '  <transcript_delta>user: first line\nstill the same turn\n'
      'assistant: reply</transcript_delta>\n</realtime_delegation>',
    )!;
    expect(h.turns, hasLength(2));
    expect(h.turns.first.text, 'first line\nstill the same turn');
    expect(h.turns.last.text, 'reply');
  });

  test('streamed partials collapse into the settled utterance', () {
    // Live transcription emits the utterance as it firms up AND again when
    // settled; the delta carries both, so the card used to read as a wall of
    // one-word fragments.
    final h = parseRealtimeDelegation(
      '<realtime_delegation>\n  <input>x</input>\n'
      '  <transcript_delta>user: 有bug好像\n'
      'assistant: 你是想\n'
      'user: 。怎么\n'
      'user: 换 那个 live\n'
      'user: 这个好像刚 上线了,有bug 好像。怎么 换 那个 live\n'
      'assistant: 你是想确认能不能用吗?</transcript_delta>\n'
      '</realtime_delegation>',
    )!;
    expect(h.turns.map((t) => t.text), [
      '这个好像刚 上线了,有bug 好像。怎么 换 那个 live',
      '你是想确认能不能用吗?',
    ]);
  });

  test('a session tail never shows codex note as something the user said', () {
    // The tail flush's <input> is an instruction TO the model, not speech.
    final h = parseRealtimeDelegation(
      '<realtime_delegation>\n  <source>transcript_tail_flush</source>\n'
      '  <input>The user just ended their realtime session. …</input>\n'
      '  <transcript_delta>user: 没有披露这个数据</transcript_delta>\n'
      '</realtime_delegation>',
    )!;
    expect(h.isSessionTail, isTrue);
    expect(h.inputIsRedundant, isTrue);
  });

  test('anything that is not a handoff returns null', () {
    expect(parseRealtimeDelegation('just a message'), isNull);
    expect(
      parseRealtimeDelegation(
        '<realtime_delegation>no input</realtime_delegation>',
      ),
      isNull,
    );
    // A person quoting the tag mid-sentence still owns their message.
    expect(
      parseRealtimeDelegation('why does <realtime_delegation> appear?'),
      isNull,
    );
  });
}
