import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/screens/app_session/activity_cards.dart';
import 'package:pocket_codex/src/screens/app_session/transcript_model.dart';
import 'package:pocket_codex/src/screens/app_session/transcript_rows.dart';

void main() {
  test('a long preamble is grouped in one linear pass', () {
    final items = [
      TranscriptItem(id: 'user', type: 'userMessage'),
      for (var i = 0; i < 20000; i++)
        TranscriptItem(id: 'p$i', type: 'agentMessage', text: 'progress'),
      TranscriptItem(id: 'tool', type: 'commandExecution'),
      TranscriptItem(id: 'answer', type: 'agentMessage', text: 'done'),
    ];
    final timer = Stopwatch()..start();
    final rows = buildTranscriptRows(items);
    timer.stop();
    // Diagnostic only: correctness must not depend on the CI machine's speed.
    // ignore: avoid_print
    print('20,003 items grouped in ${timer.elapsedMicroseconds} μs');
    expect(rows, hasLength(3));
    expect((rows[1] as TurnWork).items, hasLength(20001));
    expect(identical(rows.last, items.last), isTrue);
  });

  test('final replies stay separate across known turn boundaries', () {
    final rows = buildTranscriptRows([
      TranscriptItem(id: 'a', type: 'agentMessage', turnId: 'one'),
      TranscriptItem(id: 'b', type: 'agentMessage', turnId: 'one'),
      TranscriptItem(id: 'c', type: 'agentMessage', turnId: 'two'),
    ]);
    expect(rows, hasLength(2));
    expect((rows.first as AgentTurn).items.map((item) => item.id), ['a', 'b']);
    expect((rows.last as TranscriptItem).id, 'c');
  });
}
