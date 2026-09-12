import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/screens/app_session/history_rows.dart';
import 'package:pocket_codex/src/screens/app_session/transcript_model.dart';
import 'package:pocket_codex/src/screens/app_session/activity_cards.dart';

void main() {
  test('a turn continuation and following unread turns form one gap', () {
    final prefix = TranscriptItem(
      id: 'prefix',
      type: 'commandExecution',
      turnId: 'first',
      turnDurationMs: 12987579,
    );
    List<Object> rows(bool hasMore) => buildHistoryRows(
      [prefix, TranscriptItem(id: 'tail', type: 'userMessage', turnId: 'last')],
      turns: const [
        TurnSummary(
          turnId: 'first',
          userText: '',
          assistantText: '',
          loaded: true,
        ),
        TurnSummary(
          turnId: 'middle',
          userText: '',
          assistantText: '',
          loaded: false,
        ),
        TurnSummary(
          turnId: 'last',
          userText: '',
          assistantText: '',
          loaded: true,
        ),
      ],
      windows: {
        'first': TurnItemsPage(
          turnId: 'first',
          items: const [
            ThreadItem(
              id: 'prefix',
              itemType: 'commandExecution',
              title: '',
              text: '',
            ),
          ],
          hasMore: hasMore,
        ),
      },
      sequentialIds: {'tail'},
    );
    final partial = rows(true);
    expect(partial.whereType<HistoryGap>(), hasLength(1));
    expect(partial.whereType<HistoryGap>().single.turnId, 'first');
    expect(partial.whereType<HistoryGap>().single.continuation, isTrue);
    expect(partial.whereType<TurnWork>().single.durationMs, isNull);
    final complete = rows(false);
    expect(complete.whereType<HistoryGap>(), hasLength(1));
    expect(complete.whereType<HistoryGap>().single.turnId, 'middle');
    expect(complete.whereType<TurnWork>().single.durationMs, 12987579);
  });

  test('steering within a turn does not repeat its total duration', () {
    final rows = buildHistoryRows(
      [
        TranscriptItem(id: 'u1', type: 'userMessage', turnId: 'turn'),
        TranscriptItem(
          id: 'c1',
          type: 'commandExecution',
          turnId: 'turn',
          turnDurationMs: 12987579,
        ),
        TranscriptItem(id: 'u2', type: 'userMessage', turnId: 'turn'),
        TranscriptItem(
          id: 'c2',
          type: 'commandExecution',
          turnId: 'turn',
          turnDurationMs: 12987579,
        ),
      ],
      turns: const [],
      windows: const {},
      sequentialIds: const {},
    );
    expect(rows.whereType<TurnWork>().map((work) => work.durationMs), [
      null,
      12987579,
    ]);
  });

  const turns = [
    TurnSummary(turnId: 'first', userText: '', assistantText: '', loaded: true),
    TurnSummary(
      turnId: 'middle',
      userText: '',
      assistantText: '',
      loaded: false,
    ),
    TurnSummary(turnId: 'last', userText: '', assistantText: '', loaded: true),
  ];
  test('a jump to the first turn leaves the unread gap between turns', () {
    final rows = buildHistoryRows(
      [
        TranscriptItem(id: 'first-user', type: 'userMessage', turnId: 'first'),
        TranscriptItem(id: 'last-user', type: 'userMessage', turnId: 'last'),
      ],
      turns: turns,
      windows: {
        'first': const TurnItemsPage(
          turnId: 'first',
          items: [],
          hasMore: false,
        ),
      },
      sequentialIds: {'last-user'},
    );
    expect(rows, hasLength(3));
    expect((rows[1] as HistoryGap).turnId, 'middle');
  });
  test('a partially fetched long turn has a gap after its loaded prefix', () {
    final prefix = TranscriptItem(
      id: 'first-user',
      type: 'userMessage',
      turnId: 'first',
    );
    final tail = TranscriptItem(
      id: 'first-answer',
      type: 'agentMessage',
      turnId: 'first',
    );
    final rows = buildHistoryRows(
      [prefix, tail],
      turns: turns,
      windows: {
        'first': const TurnItemsPage(
          turnId: 'first',
          items: [
            ThreadItem(
              id: 'first-user',
              itemType: 'userMessage',
              title: '',
              text: '',
            ),
          ],
          hasMore: true,
        ),
      },
      sequentialIds: {'first-answer'},
    );
    expect(rows, hasLength(3));
    expect((rows[1] as HistoryGap).continuation, isTrue);
    expect(rows.last, same(tail));
  });
  test('an exhausted empty turn does not leave a repeating gap', () {
    final rows = buildHistoryRows(
      [
        TranscriptItem(id: 'first-user', type: 'userMessage', turnId: 'first'),
        TranscriptItem(id: 'last-user', type: 'userMessage', turnId: 'last'),
      ],
      turns: turns,
      windows: {
        'middle': const TurnItemsPage(
          turnId: 'middle',
          items: [],
          hasMore: false,
        ),
      },
      sequentialIds: {'last-user'},
    );
    expect(rows.whereType<HistoryGap>(), isEmpty);
  });
  test(
    'overlap with the sequential tail closes the gap without another fetch',
    () {
      final rows = buildHistoryRows(
        [TranscriptItem(id: 'overlap', type: 'agentMessage', turnId: 'first')],
        turns: turns,
        windows: {
          'first': const TurnItemsPage(
            turnId: 'first',
            items: [
              ThreadItem(
                id: 'overlap',
                itemType: 'agentMessage',
                title: '',
                text: '',
              ),
            ],
            hasMore: true,
          ),
        },
        sequentialIds: {'overlap'},
      );
      expect(rows.whereType<HistoryGap>(), isEmpty);
    },
  );
}
