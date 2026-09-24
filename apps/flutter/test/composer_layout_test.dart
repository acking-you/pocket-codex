import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/ui_prefs.dart';

import 'fake_bridge_api.dart';

void main() {
  setUp(AppSessionScreen.debugResetThreadMemory);

  for (final device in [
    (
      name: 'phone',
      platform: TargetPlatform.android,
      size: const Size(390, 844),
    ),
    (
      name: 'tablet',
      platform: TargetPlatform.android,
      size: const Size(820, 1180),
    ),
    (
      name: 'desktop',
      platform: TargetPlatform.windows,
      size: const Size(1280, 800),
    ),
  ]) {
    for (final brightness in Brightness.values) {
      for (final scale in [1.0, 2.0]) {
        testWidgets(
          'compact composer keeps text visible on ${device.name} $brightness at $scale',
          (t) async {
            t.view.devicePixelRatio = 1;
            t.view.physicalSize = device.size;
            addTearDown(t.view.reset);
            final api = FakeBridgeApi(
              config: const ConfigInfo(relay: 'relay:7666', hasKey: true),
            );
            await api.appConnect('pcx:host:app:default', 28080);
            await t.pumpWidget(
              ProviderScope(
                overrides: [bridgeApiProvider.overrideWithValue(api)],
                child: MaterialApp(
                  locale: const Locale('zh'),
                  localizationsDelegates:
                      AppLocalizations.localizationsDelegates,
                  supportedLocales: AppLocalizations.supportedLocales,
                  theme: brightness == Brightness.light
                      ? lightTheme()
                      : darkTheme(),
                  builder: (context, child) => MediaQuery(
                    data: MediaQuery.of(
                      context,
                    ).copyWith(textScaler: TextScaler.linear(scale)),
                    child: child!,
                  ),
                  home: const AppSessionScreen(
                    serviceKey: 'pcx:host:app:default',
                  ),
                ),
              ),
            );
            await t.pumpAndSettle();
            final input = find.byKey(const Key('composer-input'));
            final area = find.byKey(const Key('composer-input-area'));
            final container = ProviderScope.containerOf(t.element(input));
            // Exercise a height saved by an older release, then shrink by touch/mouse.
            container.read(uiPrefsProvider.notifier).setComposerHeight(28);
            await t.pumpAndSettle();
            if (device.platform == TargetPlatform.android) {
              t.view.viewInsets = const FakeViewPadding(bottom: 300);
            }
            const draft = 'hello 你好 gy';
            await t.enterText(input, draft);
            await t.pumpAndSettle();
            await t.drag(
              find.byKey(const Key('composer-resize-handle')),
              const Offset(0, 80),
            );
            await t.pumpAndSettle();

            final editable = t
                .state<EditableTextState>(
                  find.descendant(
                    of: input,
                    matching: find.byType(EditableText),
                  ),
                )
                .renderEditable;
            expect(
              editable.size.height,
              greaterThanOrEqualTo(editable.preferredLineHeight),
              reason:
                  'A complete text line must fit inside the actual text viewport.',
            );
            final caret = editable.getLocalRectForCaret(
              const TextPosition(offset: draft.length),
            );
            expect(caret.top, greaterThanOrEqualTo(0));
            expect(caret.bottom, lessThanOrEqualTo(editable.size.height));
            expect(
              t.getRect(area).bottom,
              lessThanOrEqualTo(
                t.getRect(find.byKey(const Key('send-btn'))).top,
              ),
            );
            expect(
              t.getRect(find.byKey(const Key('send-btn'))).bottom,
              lessThanOrEqualTo(device.size.height - t.view.viewInsets.bottom),
            );
            expect(t.takeException(), isNull);
          },
          variant: TargetPlatformVariant({device.platform}),
        );
      }
    }
  }
}
