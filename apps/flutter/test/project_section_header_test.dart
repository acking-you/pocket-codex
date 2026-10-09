import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/widgets/project_section_header.dart';

void main() {
  final header = find.byKey(const Key('project-header-/work/alpha'));
  final create = find.byKey(const Key('project-new-/work/alpha'));
  var toggles = 0;
  var creates = 0;
  Future<void> mount(WidgetTester t) async {
    toggles = creates = 0;
    await t.pumpWidget(
      MaterialApp(
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(
          body: Center(
            child: SizedBox(
              width: 260,
              child: ProjectSectionHeader(
                path: '/work/alpha',
                name: 'alpha',
                collapsed: false,
                count: 2,
                onToggle: () => toggles++,
                onNewConversation: () => creates++,
              ),
            ),
          ),
        ),
      ),
    );
  }

  double opacity(WidgetTester t) => t
      .widget<AnimatedOpacity>(
        find.ancestor(of: create, matching: find.byType(AnimatedOpacity)),
      )
      .opacity;

  testWidgets(
    'mouse hover reserves space and plus does not collapse the project',
    (t) async {
      await mount(t);
      final rect = t.getRect(header);
      expect(opacity(t), 0);
      final mouse = await t.createGesture(kind: PointerDeviceKind.mouse);
      await mouse.addPointer(location: Offset.zero);
      await mouse.moveTo(t.getCenter(header));
      await t.pumpAndSettle();
      expect(opacity(t), 1);
      expect(t.getRect(header), rect);
      await t.tap(create);
      await t.pumpAndSettle();
      expect(creates, 1);
      expect(toggles, 0);
      await mouse.moveTo(Offset.zero);
      await t.pumpAndSettle();
      await mouse.removePointer();
    },
  );

  testWidgets(
    'keyboard focus reveals plus and Enter creates without toggling',
    (t) async {
      await mount(t);
      await t.sendKeyEvent(LogicalKeyboardKey.tab);
      await t.pumpAndSettle();
      expect(opacity(t), 1);
      await t.sendKeyEvent(LogicalKeyboardKey.tab);
      await t.sendKeyEvent(LogicalKeyboardKey.enter);
      await t.pumpAndSettle();
      expect(creates, 1);
      expect(toggles, 0);
    },
  );

  testWidgets('an open project action survives a sidebar row refresh', (
    t,
  ) async {
    var showHeader = true;
    var created = false;
    late StateSetter refresh;
    await t.pumpWidget(
      MaterialApp(
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(
          body: StatefulBuilder(
            builder: (context, setState) {
              refresh = setState;
              return showHeader
                  ? ProjectSectionHeader(
                      path: '/work/alpha',
                      name: 'alpha',
                      collapsed: false,
                      count: 1,
                      onToggle: () {},
                      onNewConversation: () => created = true,
                    )
                  : const SizedBox();
            },
          ),
        ),
      ),
    );
    await t.longPress(header);
    await t.pumpAndSettle();
    refresh(() => showHeader = false);
    await t.pumpAndSettle();
    await t.tap(find.text('New conversation in alpha'));
    await t.pumpAndSettle();
    expect(created, isTrue);
    expect(t.takeException(), isNull);
  });

  testWidgets('touch collapse keeps the extra action hidden', (t) async {
    await mount(t);
    await t.tap(header);
    await t.pumpAndSettle();
    expect(toggles, 1);
    expect(opacity(t), 0);
  });

  testWidgets(
    'touch long press opens a sheet with the full path and no collapse',
    (t) async {
      await mount(t);
      await t.longPress(header);
      await t.pumpAndSettle();
      expect(find.byType(BottomSheet), findsOneWidget);
      expect(find.text('/work/alpha'), findsOneWidget);
      expect(toggles, 0);
      await t.tap(find.text('New conversation in alpha'));
      await t.pumpAndSettle();
      expect(creates, 1);
      expect(find.byType(BottomSheet), findsNothing);
    },
  );

  testWidgets('right click offers copy without changing collapse state', (
    t,
  ) async {
    await mount(t);
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
    await t.tap(
      header,
      buttons: kSecondaryMouseButton,
      kind: PointerDeviceKind.mouse,
    );
    await t.pumpAndSettle();
    await t.tap(find.text('Copy project path'));
    await t.pumpAndSettle();
    expect(copied, '/work/alpha');
    expect(toggles, 0);
    expect(creates, 0);
  });
}
