import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/screens/app_session/approval_review_card.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
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
        expect(find.byType(ApprovalReviewCard), findsOneWidget);
        expect(
          t.widget<ApprovalReviewCard>(find.byType(ApprovalReviewCard)).status,
          status,
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
      expect(find.byType(ApprovalReviewCard), findsOneWidget);
      expect(api.turnStartCount, 0);
      await t.pumpWidget(const SizedBox());
    },
  );
}
