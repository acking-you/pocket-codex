import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/screens/app_session/activity_cards.dart';
import 'package:pocket_codex/src/screens/app_session/transcript_model.dart';
import 'package:pocket_codex/src/widgets/turn_minimap.dart';
import 'package:pocket_codex/src/widgets/turn_outline.dart';

Widget host(Widget child) => MaterialApp(
  locale: const Locale('en'),
  localizationsDelegates: AppLocalizations.localizationsDelegates,
  supportedLocales: AppLocalizations.supportedLocales,
  home: Scaffold(body: child),
);

void main() {
  for (final desktop in [false, true]) {
    testWidgets('searchable 10000-turn outline stays lazy, desktop=$desktop', (
      t,
    ) async {
      debugDefaultTargetPlatformOverride = desktop
          ? TargetPlatform.macOS
          : TargetPlatform.android;
      t.view.physicalSize = desktop
          ? const Size(1200, 900)
          : const Size(390, 844);
      t.view.devicePixelRatio = 1;
      addTearDown(t.view.resetPhysicalSize);
      addTearDown(t.view.resetDevicePixelRatio);
      final items = List.generate(
        10000,
        (i) => TurnMinimapItem(
          rowIndex: -1,
          turnId: 'turn-$i',
          userText: 'Request ${i + 1}',
          assistantText: 'Response ${i + 1}',
        ),
      );
      TurnMinimapItem? selected;
      await t.pumpWidget(
        host(
          Builder(
            builder: (context) => TextButton(
              onPressed: () async => selected = await showTurnOutline(
                context,
                items: items,
                current: 5000,
              ),
              child: const Text('Open'),
            ),
          ),
        ),
      );
      await t.tap(find.text('Open'));
      await t.pumpAndSettle();
      expect(t.widgetList(find.byType(ListTile)).length, lessThan(20));
      expect(find.text('Request 5001'), findsOneWidget);
      await t.enterText(find.byKey(const Key('turn-outline-search')), '9999');
      await t.pumpAndSettle();
      expect(find.byType(ListTile), findsOneWidget);
      expect(find.text('Request 9999'), findsOneWidget);
      expect(selected, isNull);
      await t.tap(find.text('Request 9999'));
      await t.pumpAndSettle();
      expect(selected?.turnId, 'turn-9998');
      expect(t.takeException(), isNull);
      await t.pumpWidget(const SizedBox());
      debugDefaultTargetPlatformOverride = null;
    });
  }

  testWidgets(
    '1000-step work stays bounded, jumps exactly, and preserves selection on append',
    (t) async {
      t.view.physicalSize = const Size(390, 844);
      t.view.devicePixelRatio = 1;
      addTearDown(t.view.resetPhysicalSize);
      addTearDown(t.view.resetDevicePixelRatio);
      final steps = List.generate(
        1000,
        (i) => TranscriptItem(
          id: 's-$i',
          type: 'commandExecution',
          title: 'pwd $i',
          text: '/project',
          turnId: 'turn',
        ),
      );
      Widget view() => host(
        SingleChildScrollView(child: TurnWorkCard(work: TurnWork(steps))),
      );
      await t.pumpWidget(view());
      await t.tap(find.byKey(const Key('turn-work-toggle')));
      await t.pumpAndSettle();
      expect(find.text('Steps 1–25 of 1000'), findsOneWidget);
      expect(t.widgetList(find.byType(ActivityCard)).length, lessThan(26));
      await t.tap(find.byKey(const Key('work-step-picker')));
      await t.pumpAndSettle();
      await t.enterText(find.byType(TextFormField), '701');
      await t.tap(find.text('OK'));
      await t.pumpAndSettle();
      expect(find.text('Steps 701–725 of 1000'), findsOneWidget);
      expect(find.byKey(const ValueKey('work-step-s-700')), findsOneWidget);
      steps.add(
        TranscriptItem(
          id: 'last',
          type: 'commandExecution',
          title: 'new',
          text: 'new',
        ),
      );
      await t.pumpWidget(view());
      await t.pumpAndSettle();
      expect(find.text('Steps 701–725 of 1001'), findsOneWidget);
      await t.tap(find.text('Latest steps'));
      await t.pumpAndSettle();
      expect(find.text('Steps 977–1001 of 1001'), findsOneWidget);
      expect(t.takeException(), isNull);
    },
  );
}
