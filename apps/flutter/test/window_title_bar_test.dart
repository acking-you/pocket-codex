import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/widgets/window_title_bar.dart';
import 'package:window_manager/window_manager.dart';

Widget _wrap(PreferredSizeWidget bar) => MaterialApp(
  locale: const Locale('en'),
  localizationsDelegates: AppLocalizations.localizationsDelegates,
  supportedLocales: AppLocalizations.supportedLocales,
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

  testWidgets('windows: draws its own minimize/maximize/close buttons', (
    tester,
  ) async {
    // Windows loses its native caption buttons to TitleBarStyle.hidden, so the
    // bar must draw replacements (macOS keeps native traffic lights instead).
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    try {
      await tester.pumpWidget(
        _wrap(
          const WindowTitleBar(
            title: Text('Title'),
            actions: [Icon(Icons.settings)],
          ),
        ),
      );
      await tester.pump(); // let _syncMaximized settle (no-op without a window)
      // The three caption glyphs are present alongside the app's own action.
      expect(find.byIcon(Icons.remove), findsOneWidget); // minimize
      expect(find.byIcon(Icons.crop_square), findsOneWidget); // maximize
      expect(find.byIcon(Icons.close), findsOneWidget); // close
      expect(find.byIcon(Icons.settings), findsOneWidget);
      expect(find.byType(DragToMoveArea), findsOneWidget);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });

  testWidgets('macOS: no custom caption buttons (native traffic lights)', (
    tester,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    try {
      await tester.pumpWidget(
        _wrap(const WindowTitleBar(title: Text('Title'))),
      );
      // macOS relies on native window buttons — the bar draws none of its own.
      expect(find.byIcon(Icons.close), findsNothing);
      expect(find.byIcon(Icons.remove), findsNothing);
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
