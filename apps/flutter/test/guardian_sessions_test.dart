import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/screens/app_session/approval_review_card.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/screens/local_sessions_screen.dart';
import 'package:pocket_codex/src/session_tree.dart';

import 'fake_bridge_api.dart';
import 'support/screen_harness.dart';

const service = 'pcx:guardian:app:default';
const parent = ThreadMeta(
  id: 'parent',
  preview: 'Main work',
  cwd: '/repo',
  updatedAt: 1,
);
const reviewer = ThreadMeta(
  id: 'reviewer',
  preview: 'Approval review',
  cwd: '/repo',
  updatedAt: 3,
  parentThreadId: 'parent',
  threadSource: 'guardian_review',
);

Future<void> frames(WidgetTester t) async {
  for (var i = 0; i < 10; i++) {
    await t.pump(const Duration(milliseconds: 50));
  }
}

String review(String id, String status) => jsonEncode({
  'threadId': 'parent',
  'turnId': 'turn',
  'reviewId': id,
  'review': {
    'status': status,
    'riskLevel': 'high',
    'userAuthorization': 'high',
    'rationale': 'The user authorized this action.',
  },
  'action': {'type': 'command', 'command': 'cargo test', 'cwd': '/repo'},
});

void main() {
  setUp(AppSessionScreen.debugResetThreadMemory);

  for (final runningId in ['parent', 'worker']) {
    testWidgets('running $runningId keeps children folded until expanded', (
      t,
    ) async {
      t.view.physicalSize = const Size(1280, 900);
      t.view.devicePixelRatio = 1;
      addTearDown(t.view.reset);
      const worker = ThreadMeta(
        id: 'worker',
        preview: 'Nested work',
        cwd: '/repo',
        updatedAt: 4,
        parentThreadId: 'reviewer',
      );
      final api = FakeBridgeApi()
        ..appThreads.clear()
        ..appThreads.addAll([worker, reviewer, parent]);
      await api.appConnect(service, 28080);
      await t.pumpWidget(
        host(
          const AppSessionScreen(
            serviceKey: service,
            threadId: 'parent',
            home: true,
          ),
          api,
        ),
      );
      await frames(t);
      final parentToggle = find.byKey(const Key('session-children-parent'));
      final reviewerRow = find.byKey(const Key('session-tree-reviewer'));
      final workerRow = find.byKey(const Key('session-tree-worker'));
      expect(reviewerRow, findsNothing);

      Future<void> activity(String kind) async {
        api.pushEvent(
          service,
          AppEvent(
            kind: kind,
            threadId: runningId,
            raw: '{"turn":{"id":"turn"}}',
          ),
        );
        await frames(t);
      }

      await activity('turn/started');
      expect(find.text('进行中'), findsOneWidget);
      expect(reviewerRow, findsNothing);
      expect(workerRow, findsNothing);
      expect(t.widget<IconButton>(parentToggle).isSelected, isFalse);
      await t.tap(parentToggle);
      await frames(t);
      expect(reviewerRow, findsOneWidget);
      expect(workerRow, findsNothing);
      await t.tap(find.byKey(const Key('session-children-reviewer')));
      await frames(t);
      expect(workerRow, findsOneWidget);
      await t.tap(parentToggle);
      await frames(t);
      expect(reviewerRow, findsNothing);
      expect(workerRow, findsNothing);

      // Live status changes and switching sidebar views must preserve the fold.
      await activity('turn/completed');
      expect(find.text('进行中'), findsNothing);
      expect(reviewerRow, findsNothing);
      await activity('turn/started');
      expect(find.text('进行中'), findsOneWidget);
      expect(reviewerRow, findsNothing);
      await t.tap(find.byKey(const Key('activity-view-btn')));
      await frames(t);
      expect(reviewerRow, findsNothing);
      await t.tap(parentToggle);
      await frames(t);
      expect(reviewerRow, findsOneWidget);
      expect(workerRow, findsOneWidget);
      expect(t.takeException(), isNull);
      await t.pumpWidget(const SizedBox());
    });
  }

  testWidgets('host inventory keeps running children folded across refresh', (
    t,
  ) async {
    final api = FakeBridgeApi()
      ..localSessions = const [
        LocalSession(
          threadId: 'parent',
          cwd: '/repo',
          preview: 'Main work',
          updatedAt: 1,
          turnState: 'completed',
          heldOpen: false,
          safety: 'resumable',
          allowsResume: true,
          requiresTakeover: false,
        ),
        LocalSession(
          threadId: 'reviewer',
          cwd: '/repo',
          preview: 'Approval review',
          updatedAt: 3,
          parentThreadId: 'parent',
          threadSource: 'guardian_review',
          turnState: 'incomplete',
          heldOpen: true,
          safety: 'ownedRunning',
          allowsResume: false,
          requiresTakeover: false,
        ),
      ];
    await t.pumpWidget(
      host(
        const LocalSessionsScreen(source: SessionSource.remote(service)),
        api,
      ),
    );
    await frames(t);
    final parentToggle = find.byKey(const Key('session-children-parent'));
    final reviewerRow = find.byKey(const Key('session-tree-reviewer'));
    expect(find.byKey(const Key('session-tree-parent')), findsOneWidget);
    expect(reviewerRow, findsNothing);
    expect(find.text('进行中'), findsOneWidget);
    await t.tap(parentToggle);
    await frames(t);
    expect(reviewerRow, findsOneWidget);
    await t.tap(parentToggle);
    await frames(t);
    expect(reviewerRow, findsNothing);
    await t.tap(find.byKey(const Key('local-sessions-refresh')));
    await frames(t);
    expect(find.text('进行中'), findsOneWidget);
    expect(reviewerRow, findsNothing);
    await t.tap(parentToggle);
    await frames(t);
    expect(reviewerRow, findsOneWidget);
    expect(t.takeException(), isNull);
    await t.pumpWidget(const SizedBox());
  });

  testWidgets('the selected child does not prevent its parent from folding', (
    t,
  ) async {
    t.view.physicalSize = const Size(1280, 900);
    t.view.devicePixelRatio = 1;
    addTearDown(t.view.reset);
    final api = FakeBridgeApi()
      ..appThreads.clear()
      ..appThreads.addAll([reviewer, parent])
      ..metadataOnlyFollow = true
      ..readResult = const ThreadHistory(items: [], running: false);
    await api.appConnect(service, 28080);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: service,
          threadId: 'reviewer',
          home: true,
        ),
        api,
      ),
    );
    await frames(t);
    final parentToggle = find.byKey(const Key('session-children-parent'));
    final reviewerRow = find.byKey(const Key('session-tree-reviewer'));
    expect(reviewerRow, findsOneWidget);
    await t.tap(parentToggle);
    await frames(t);
    expect(reviewerRow, findsNothing);
    expect(find.byKey(const Key('guardian-read-only')), findsOneWidget);
    api.pushEvent(
      service,
      const AppEvent(
        kind: 'turn/started',
        threadId: 'reviewer',
        raw: '{"turn":{"id":"turn"}}',
      ),
    );
    await frames(t);
    expect(reviewerRow, findsNothing);
    await t.tap(parentToggle);
    await frames(t);
    expect(reviewerRow, findsOneWidget);
    expect(api.lastResumed, isNull);
    expect(t.takeException(), isNull);
    await t.pumpWidget(const SizedBox());
  });

  test(
    'trees retain explicit ancestry, orphans and cycles without duplicates',
    () {
      const child = ThreadMeta(
        id: 'child',
        parentThreadId: 'reviewer',
        preview: '',
        cwd: '/other',
        updatedAt: 0,
      );
      const orphan = ThreadMeta(
        id: 'orphan',
        parentThreadId: 'missing',
        preview: '',
        cwd: '',
        updatedAt: 0,
      );
      const a = ThreadMeta(
        id: 'a',
        parentThreadId: 'b',
        preview: '',
        cwd: '',
        updatedAt: 0,
      );
      const b = ThreadMeta(
        id: 'b',
        parentThreadId: 'a',
        preview: '',
        cwd: '',
        updatedAt: 0,
      );
      final tree = SessionTree(
        [reviewer, child, parent, orphan, a, b],
        (ThreadMeta t) => t.id,
        (t) => t.parentThreadId,
      );
      expect(tree.roots.map((t) => t.id), ['parent', 'orphan', 'a', 'b']);
      expect(tree.withAncestors(['child']), {'parent', 'reviewer', 'child'});
      expect(tree.visible(parent, {'parent', 'reviewer'}).map((r) => r.depth), [
        0,
        1,
        2,
      ]);
      expect(tree.visible(parent, {}).map((r) => r.item.id), ['parent']);
    },
  );

  for (final width in [390.0, 900.0, 1280.0]) {
    testWidgets(
      'Guardian opens read-only and links to its parent at width $width',
      (t) async {
        t.view.physicalSize = Size(width, 900);
        t.view.devicePixelRatio = 1;
        addTearDown(t.view.resetPhysicalSize);
        addTearDown(t.view.resetDevicePixelRatio);
        final api = FakeBridgeApi()
          ..appThreads.clear()
          ..appThreads.addAll([reviewer, parent])
          ..metadataOnlyFollow = true
          ..readResult = const ThreadHistory(items: [], running: false);
        await api.appConnect(service, 28080);
        await t.pumpWidget(
          host(
            const AppSessionScreen(
              serviceKey: service,
              threadId: 'reviewer',
              home: true,
            ),
            api,
          ),
        );
        await frames(t);
        expect(api.lastResumed, isNull);
        expect(find.byKey(const Key('guardian-read-only')), findsOneWidget);
        expect(find.byKey(const Key('composer-input')), findsNothing);
        expect(find.byKey(const Key('chat-takeover-action')), findsNothing);
        await t.tap(find.byKey(const Key('open-parent-session')));
        await frames(t);
        expect(api.lastResumed, 'parent');
        expect(find.byKey(const Key('composer-input')), findsOneWidget);
        expect(find.byKey(const Key('guardian-read-only')), findsNothing);
        expect(t.takeException(), isNull);
        await t.pumpWidget(const SizedBox());
      },
    );
  }

  testWidgets('session tree expands a reviewer beneath its parent', (t) async {
    t.view.physicalSize = const Size(1280, 900);
    t.view.devicePixelRatio = 1;
    addTearDown(t.view.resetPhysicalSize);
    addTearDown(t.view.resetDevicePixelRatio);
    final api = FakeBridgeApi()
      ..appThreads.clear()
      ..appThreads.addAll([reviewer, parent]);
    await api.appConnect(service, 28080);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: service,
          threadId: 'parent',
          home: true,
        ),
        api,
      ),
    );
    await frames(t);
    expect(find.byKey(const Key('session-tree-reviewer')), findsNothing);
    await t.tap(find.byKey(const Key('session-children-parent')));
    await frames(t);
    expect(find.byKey(const Key('session-tree-reviewer')), findsOneWidget);
    expect(
      t.getTopLeft(find.byKey(const Key('session-tree-reviewer'))).dy,
      greaterThan(
        t.getTopLeft(find.byKey(const Key('session-tree-parent'))).dy,
      ),
    );
    await t.tap(find.byKey(const Key('session-children-parent')));
    await frames(t);
    expect(find.byKey(const Key('session-tree-reviewer')), findsNothing);
    await t.pumpWidget(const SizedBox());
  });

  testWidgets(
    'parent displays review progress and completion without an approval action',
    (t) async {
      final api = FakeBridgeApi()
        ..appThreads.clear()
        ..appThreads.add(parent);
      await api.appConnect(service, 28080);
      await t.pumpWidget(
        host(
          const AppSessionScreen(serviceKey: service, threadId: 'parent'),
          api,
        ),
      );
      await frames(t);
      for (final status in ['inProgress', 'approved']) {
        api.pushEvent(
          service,
          AppEvent(
            kind: status == 'inProgress'
                ? 'item/autoApprovalReview/started'
                : 'item/autoApprovalReview/completed',
            threadId: 'parent',
            itemId: 'auto-review:review',
            itemType: 'autoApprovalReview',
            title: status,
            text: review('review', status),
            raw: review('review', status),
          ),
        );
        await frames(t);
        // The review lives in the turn's fold next to the step it approves;
        // the fold header counts it even while closed.
        expect(find.byKey(const Key('turn-work-reviews')), findsOneWidget);
        await openTurnWork(t);
        expect(find.byType(ApprovalReviewCard), findsOneWidget);
        expect(
          t.widget<ApprovalReviewCard>(find.byType(ApprovalReviewCard)).status,
          status,
        );
        // What was approved is shown, not just that something was.
        expect(
          t.widget<Text>(find.byKey(const Key('approval-review-action'))).data,
          'cargo test',
        );
      }
      api.pushEvent(
        service,
        AppEvent(
          kind: 'item/autoApprovalReview/completed',
          threadId: 'another',
          itemId: 'auto-review:other',
          itemType: 'autoApprovalReview',
          text: review('other', 'denied'),
          raw: '{}',
        ),
      );
      await frames(t);
      await openTurnWork(t);
      expect(find.byType(ApprovalReviewCard), findsOneWidget);
      expect(api.turnStartCount, 0);
      await t.pumpWidget(const SizedBox());
    },
  );
}
