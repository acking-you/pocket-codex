import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/widgets/window_title_bar.dart';
import 'package:window_manager/window_manager.dart';

Widget _wrap(PreferredSizeWidget bar) => MaterialApp(
  home: Scaffold(appBar: bar, body: const SizedBox()),
);

void main() {
  testWidgets('mobile: plain AppBar, no drag area', (tester) async {
    // Default test platform is android → mobile branch.
    await tester.pumpWidget(_wrap(const WindowTitleBar(title: Text('Title'))));
    expect(find.byType(AppBar), findsOneWidget);
    expect(find.byType(DragToMoveArea), findsNothing);
    expect(find.text('Title'), findsOneWidget);
  });

  testWidgets('desktop: draggable title bar that keeps its content', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    try {
      await tester.pumpWidget(
        _wrap(
          const WindowTitleBar(
            leading: Icon(Icons.menu),
            title: Text('Title'),
            actions: [Icon(Icons.settings)],
          ),
        ),
      );
      // The empty bar space drags the window; the title/leading/actions remain.
      expect(find.byType(DragToMoveArea), findsOneWidget);
      expect(find.text('Title'), findsOneWidget);
      expect(find.byIcon(Icons.menu), findsOneWidget);
      expect(find.byIcon(Icons.settings), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('preferredSize includes the bottom widget height', (
    tester,
  ) async {
    const bar = WindowTitleBar(
      title: Text('Title'),
      bottom: PreferredSize(
        preferredSize: Size.fromHeight(24),
        child: SizedBox(height: 24),
      ),
    );
    expect(bar.preferredSize.height, kToolbarHeight + 24);
  });
}
