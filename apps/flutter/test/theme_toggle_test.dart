import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/theme_transition.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/widgets/theme_toggle.dart';

class _MemoryPrefs extends UiPrefsStore {
  @override
  Future<UiPrefs> build() async => const UiPrefs(themeMode: 'light');
}

/// Shared by widget tests and the native Profile regression. No bridge, saved
/// user settings, host process, or network connection is initialized.
void themeToggleTests() {
  for (final size in [
    const Size(390, 844),
    const Size(900, 900),
    const Size(1300, 900),
  ]) {
    testWidgets('theme toggle changes the rendered theme at $size', (t) async {
      t.view.devicePixelRatio = 1;
      t.view.physicalSize = size;
      addTearDown(t.view.reset);
      await t.pumpWidget(
        ProviderScope(
          overrides: [uiPrefsProvider.overrideWith(_MemoryPrefs.new)],
          child: Consumer(
            builder: (context, ref, _) => MaterialApp(
              localizationsDelegates: AppLocalizations.localizationsDelegates,
              supportedLocales: AppLocalizations.supportedLocales,
              theme: lightTheme(),
              darkTheme: darkTheme(),
              themeMode:
                  ref.watch(uiPrefsProvider).valueOrNull?.themeMode == 'dark'
                  ? ThemeMode.dark
                  : ThemeMode.light,
              builder: (context, child) => ThemeTransitionHost(child: child!),
              home: Scaffold(
                body: Column(
                  children: [
                    const ThemeToggle(),
                    Builder(
                      builder: (context) =>
                          Text(Theme.of(context).brightness.name),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      );
      await t.pumpAndSettle();
      expect(find.text('light'), findsOneWidget);
      for (final expected in ['dark', 'light', 'dark']) {
        await t.tap(find.byKey(const Key('theme-toggle-btn')));
        await t.pumpAndSettle();
        expect(find.text(expected), findsOneWidget);
        expect(t.takeException(), isNull);
      }
      await t.pumpWidget(const SizedBox.shrink());
    });
  }

  testWidgets('missing capture host still applies the preference', (t) async {
    var calls = 0;
    await t.pumpWidget(
      MaterialApp(
        home: Builder(
          builder: (context) {
            return TextButton(
              onPressed: () => ThemeTransition.run(context, () => calls++),
              child: const Text('switch'),
            );
          },
        ),
      ),
    );
    await t.tap(find.text('switch'));
    expect(calls, 1);
    expect(t.takeException(), isNull);
  });
}

void main() => themeToggleTests();
