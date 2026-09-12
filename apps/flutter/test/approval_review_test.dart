import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/screens/app_session/approval_review.dart';
import 'package:pocket_codex/src/screens/app_session/approval_review_card.dart';
import 'package:pocket_codex/src/screens/app_session/transcript_model.dart';
import 'package:pocket_codex/src/screens/app_session/transcript_view.dart';
import 'package:pocket_codex/src/theme.dart';

String request(String command) =>
    '''Retained context from previous steps.
>>> APPROVAL REQUEST START
Planned action JSON:
${jsonEncode({'tool': 'exec_command', 'cmd': command, 'cwd': '/project', 'justification': 'Inspect build logs'})}
>>> APPROVAL REQUEST END''';

const result =
    '{"risk_level":"low","user_authorization":"high","outcome":"allow","rationale":"Read-only inspection within the requested project."}';

Widget host(Widget child, {bool dark = false}) => MaterialApp(
  locale: const Locale('en'),
  localizationsDelegates: AppLocalizations.localizationsDelegates,
  supportedLocales: AppLocalizations.supportedLocales,
  theme: dark ? darkTheme() : lightTheme(),
  home: Scaffold(body: SingleChildScrollView(child: child)),
);

void main() {
  test(
    'only explicit request envelopes and complete assessment schemas match',
    () {
      final parsed = ApprovalReviewRequest.parse(request('cat build.log'))!;
      expect(parsed.tool, 'exec_command');
      expect(parsed.summary, 'Inspect build logs');
      expect(parsed.details, 'cat build.log');
      expect(parsed.cwd, '/project');
      expect(
        ApprovalReviewRequest.parse('{"tool":"exec_command","cmd":"pwd"}'),
        isNull,
      );
      expect(
        ApprovalReviewRequest.parse('${request('pwd')} ordinary text'),
        isNull,
      );
      expect(
        ApprovalReviewRequest.parse(
          '>>> APPROVAL REQUEST START\nPlanned action JSON:\n{broken}\n>>> APPROVAL REQUEST END',
        ),
        isNull,
      );
      expect(ApprovalReviewResult.parse(result)?.outcome, 'allow');
      expect(ApprovalReviewResult.parse('```json\n$result\n```')?.risk, 'low');
      expect(
        ApprovalReviewResult.parse(result.replaceAll('allow', 'deny'))?.outcome,
        'deny',
      );
      expect(
        ApprovalReviewResult.parse(
          result.replaceAll('"low"', '"critical"'),
        )?.risk,
        'critical',
      );
      for (final raw in [
        '{"outcome":"allow"}',
        'Example: $result',
        result.replaceAll('allow', 'maybe'),
        '{broken}',
      ]) {
        expect(ApprovalReviewResult.parse(raw), isNull);
      }
    },
  );

  for (final dark in [false, true]) {
    testWidgets(
      'review request and result use compact historical cards, dark=$dark',
      (t) async {
        t.view.physicalSize = const Size(390, 844);
        t.view.devicePixelRatio = 1;
        addTearDown(t.view.resetPhysicalSize);
        addTearDown(t.view.resetDevicePixelRatio);
        await t.pumpWidget(
          host(
            Column(
              children: [
                MessageView(
                  item: TranscriptItem(
                    id: 'u',
                    type: 'userMessage',
                    text: request('x' * 100000),
                  ),
                ),
                MessageView(
                  item: TranscriptItem(
                    id: 'a',
                    type: 'agentMessage',
                    text: result,
                  ),
                ),
              ],
            ),
            dark: dark,
          ),
        );
        await t.pumpAndSettle();
        expect(find.byType(ApprovalReviewCard), findsNWidgets(2));
        expect(find.text('Allowed'), findsOneWidget);
        expect(find.text('Risk: Low'), findsOneWidget);
        expect(find.text('Authorization: High'), findsOneWidget);
        expect(find.textContaining('APPROVAL REQUEST START'), findsNothing);
        expect(find.byType(SelectableText), findsNothing);
        expect(
          t.getSize(find.byKey(const Key('approval-review-request'))).height,
          lessThan(300),
        );
        expect(t.takeException(), isNull);
      },
    );
  }

  testWidgets(
    'large raw text is lazy, Unicode-safe, and copied without changes',
    (t) async {
      final raw = '${'x' * 799}😀${'y' * 200000}';
      String? copied;
      t.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        (call) async {
          if (call.method == 'Clipboard.setData') {
            copied = (call.arguments as Map)['text'] as String;
          }
          return null;
        },
      );
      addTearDown(
        () => t.binding.defaultBinaryMessenger.setMockMethodCallHandler(
          SystemChannels.platform,
          null,
        ),
      );
      await t.pumpWidget(
        host(
          ApprovalReviewCard(
            raw: raw,
            result: ApprovalReviewResult.parse(result),
          ),
        ),
      );
      await t.tap(find.byKey(const Key('review-raw-toggle')));
      await t.pumpAndSettle();
      expect(t.widgetList(find.byType(SelectableText)).length, lessThan(8));
      for (final text in t.widgetList<SelectableText>(
        find.byType(SelectableText),
      )) {
        final units = text.data!.codeUnits;
        expect(units.first >= 0xDC00 && units.first <= 0xDFFF, isFalse);
        expect(units.last >= 0xD800 && units.last <= 0xDBFF, isFalse);
      }
      await t.tap(find.byIcon(Icons.content_copy_outlined));
      await t.pump();
      expect(copied, raw);
      await t.pump(const Duration(seconds: 4));
      expect(t.takeException(), isNull);
    },
  );
}
