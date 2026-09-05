import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/screens/app_session/history_merge.dart';
import 'package:pocket_codex/src/screens/app_session/transcript_model.dart';

TranscriptItem item(String id, String turn) =>
    TranscriptItem(id: id, type: 'agentMessage', turnId: turn);

void main() {
  test('sequential pages respect previously fetched disjoint turns', () {
    final merged = mergeHistoryItems(
      [item('a1', 't1'), item('a5', 't5')],
      [item('a2', 't2'), item('a3', 't3')],
      turnOrder: ['t1', 't2', 't3', 't4', 't5'],
      olderPage: true,
    );
    expect(merged.map((i) => i.id), ['a1', 'a2', 'a3', 'a5']);
  });

  test('an overlapping turn places its opening before the existing tail', () {
    final tail = item('tail', 't1')..text = 'live text';
    final merged = mergeHistoryItems(
      [tail],
      [item('opening', 't1'), item('tail', 't1'), item('after', 't1')],
      turnOrder: ['t1'],
      olderPage: false,
    );
    expect(merged.map((i) => i.id), ['opening', 'tail', 'after']);
    expect(merged[1], same(tail));
  });

  test('nonoverlapping older items precede the tail of the same turn', () {
    final merged = mergeHistoryItems(
      [item('tail', 't1')],
      [item('older', 't1'), item('older', 't1')],
      turnOrder: ['t1'],
      olderPage: true,
    );
    expect(merged.map((i) => i.id), ['older', 'tail']);
  });

  test('live and optimistic turns stay after the snapshot', () {
    final merged = mergeHistoryItems(
      [
        item('old', 't1'),
        item('optimistic', ''),
        item('live', 't2'),
        item('next', ''),
      ],
      [item('older', 't0')],
      turnOrder: ['t0', 't1'],
      olderPage: true,
    );
    expect(merged.map((i) => i.id), [
      'older',
      'old',
      'optimistic',
      'live',
      'next',
    ]);
  });

  test('turns older than the skeleton retain their relative position', () {
    final merged = mergeHistoryItems(
      [item('outside', 't0'), item('latest', 't5')],
      [item('oldest', 'before'), item('middle', 't3')],
      turnOrder: ['t2', 't3', 't4', 't5'],
      olderPage: true,
    );
    expect(merged.map((i) => i.id), ['oldest', 'outside', 'middle', 'latest']);
  });
}
