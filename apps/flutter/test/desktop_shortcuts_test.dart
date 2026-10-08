import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/theme.dart';

import 'fake_bridge_api.dart';

void main() {
  setUp(AppSessionScreen.debugResetThreadMemory);

  Future<FakeBridgeApi> pump(WidgetTester t) async {
    t.view.devicePixelRatio = 1;
    t.view.physicalSize = const Size(1280, 800);
    addTearDown(t.view.reset);
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'relay:7666', hasKey: true),
    );
    final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    api.appThreads.addAll([
      for (var i = 0; i < 8; i++)
        ThreadMeta(
          id: 't$i',
          preview: 'conversation $i',
          cwd: '/work/app',
          updatedAt: now - i * 60,
        ),
    ]);
    await api.appConnect('pcx:host:app:default', 28080);
    await t.pumpWidget(
      ProviderScope(
        overrides: [bridgeApiProvider.overrideWithValue(api)],
        child: MaterialApp(
          locale: const Locale('en'),
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          theme: lightTheme(),
          home: const AppSessionScreen(
            serviceKey: 'pcx:host:app:default',
            home: true,
          ),
        ),
      ),
    );
    await t.pumpAndSettle();
    return api;
  }

  Future<void> chord(WidgetTester t, LogicalKeyboardKey key) async {
    await t.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await t.sendKeyEvent(key);
    await t.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await t.pumpAndSettle();
  }

  bool focused(WidgetTester t, Key key) {
    final field = t.widget<TextField>(find.byKey(key));
    return field.focusNode?.hasFocus ?? false;
  }

  testWidgets(
    'Ctrl+B toggles the sidebar and Ctrl+K reopens it focused on search',
    (t) async {
      await pump(t);
      expect(find.byKey(const Key('sidebar-collapse-btn')), findsOneWidget);

      await chord(t, LogicalKeyboardKey.keyB);
      expect(find.byKey(const Key('sidebar-expand-btn')), findsOneWidget);
      expect(find.byKey(const Key('conv-search')), findsNothing);

      await chord(t, LogicalKeyboardKey.keyK);
      expect(find.byKey(const Key('conv-search')), findsOneWidget);
      expect(focused(t, const Key('conv-search')), isTrue);

      await chord(t, LogicalKeyboardKey.keyL);
      expect(focused(t, const Key('composer-input')), isTrue);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets(
    'Ctrl+Tab walks the conversation list in sidebar order',
    (t) async {
      final api = await pump(t);
      expect(find.byKey(const Key('conv-tile-t0')), findsOneWidget);

      await chord(t, LogicalKeyboardKey.tab);
      // From a new draft the first step lands on the newest conversation.
      expect(api.lastResumed, 't0');

      await chord(t, LogicalKeyboardKey.tab);
      expect(api.lastResumed, 't1');

      // Shift steps back.
      await t.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
      await t.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
      await t.sendKeyEvent(LogicalKeyboardKey.tab);
      await t.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
      await t.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
      await t.pumpAndSettle();
      expect(api.lastResumed, 't0');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );
}
