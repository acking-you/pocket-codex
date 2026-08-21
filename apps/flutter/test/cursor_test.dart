// Desktop pointer feedback: what the cursor says about what's under it.
//
// Flutter's default for ink widgets (`WidgetStateMouseCursor.adaptiveClickable`)
// resolves to a hand ONLY on web — on a native desktop build it returns the
// plain arrow, so every button and row in the app hovered as if it were inert
// content. These tests pin the intended mapping so a new control (or a Flutter
// upgrade) can't quietly regress it.
import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/widgets/links.dart';

import 'fake_bridge_api.dart';

/// The session screen under the REAL desktop theme. Both matter: the theme is
/// where most controls get their cursor, and it only applies on a desktop
/// platform (`flutter test` reports android otherwise).
Widget _desktopHost(BridgeApi api, {String? threadId}) => ProviderScope(
  overrides: [bridgeApiProvider.overrideWithValue(api)],
  child: MaterialApp(
    locale: const Locale('zh'),
    theme: lightTheme(),
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: AppSessionScreen(
      serviceKey: 'pcx:lb7666:app:default',
      threadId: threadId,
      home: true,
    ),
  ),
);

void main() {
  /// Hover the centre of [f] and report the cursor the app hands out.
  Future<MouseCursor?> cursorOver(
    WidgetTester t,
    TestGesture g,
    Finder f,
  ) async {
    await g.moveTo(t.getCenter(f.first));
    await t.pumpAndSettle();
    return RendererBinding.instance.mouseTracker.debugDeviceActiveCursor(1);
  }

  testWidgets('Desktop hover: a hand over controls, an I-beam over text', (
    t,
  ) async {
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.add(
      const ThreadMeta(
        id: 't1',
        preview: 'a conversation',
        cwd: '/work/alpha',
        updatedAt: 0,
      ),
    );
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1400, 950);
    addTearDown(t.view.reset);
    await t.pumpWidget(_desktopHost(api, threadId: 't1'));
    await t.pumpAndSettle();

    final g = await t.createGesture(kind: PointerDeviceKind.mouse);
    await g.addPointer(location: Offset.zero);
    addTearDown(() => g.removePointer());

    // Clickable things: a pointing hand.
    for (final target in <(String, Finder)>[
      ('conversation row', find.text('a conversation')),
      ('project heading', find.text('alpha')),
      ('new conversation', find.byKey(const Key('new-conversation-btn'))),
      ('sidebar shortcut', find.byKey(const Key('sidebar-settings-btn'))),
    ]) {
      expect(
        await cursorOver(t, g, target.$2),
        SystemMouseCursors.click,
        reason: '${target.$1} should hover as clickable',
      );
    }

    // The composer is a text field, so its own padding reads as text too.
    expect(await cursorOver(t, g, find.text('输入消息…')), SystemMouseCursors.text);
    // The title edits in place rather than acting as a button.
    expect(
      await cursorOver(t, g, find.byKey(const Key('bar-title-tap'))),
      SystemMouseCursors.text,
    );

    // A disabled control says so: the plain arrow, not a hand promising an
    // action that won't happen. (Nothing typed yet, so send is disabled.)
    expect(
      await cursorOver(t, g, find.byKey(const Key('send-btn'))),
      SystemMouseCursors.basic,
    );
    await t.enterText(find.byKey(const Key('composer-input')), 'hi');
    await t.pumpAndSettle();
    expect(
      await cursorOver(t, g, find.byKey(const Key('send-btn'))),
      SystemMouseCursors.click,
    );

    // Must be cleared inside the body: the framework's end-of-test invariant
    // check runs before addTearDown callbacks.
    debugDefaultTargetPlatformOverride = null;
  });

  testWidgets('A link hovers as clickable, the text around it as selectable', (
    t,
  ) async {
    await t.pumpWidget(
      MaterialApp(
        theme: lightTheme(),
        home: Scaffold(
          body: SelectionArea(
            child: Center(
              child: Builder(
                builder: (ctx) => linkifyText(ctx, 'AA https://example.com ZZ'),
              ),
            ),
          ),
        ),
      ),
    );
    await t.pumpAndSettle();

    final g = await t.createGesture(kind: PointerDeviceKind.mouse);
    await g.addPointer(location: Offset.zero);
    addTearDown(() => g.removePointer());

    // Scan the line: the URL run reports a hand, the plain runs an I-beam. The
    // link's own span cursor has to survive the enclosing SelectionArea, which
    // sets a text cursor for the whole paragraph.
    final box = t.getRect(find.textContaining('example.com').first);
    final seen = <MouseCursor?>{};
    for (var frac = 0.05; frac < 1.0; frac += 0.1) {
      await g.moveTo(Offset(box.left + box.width * frac, box.center.dy));
      await t.pumpAndSettle();
      seen.add(
        RendererBinding.instance.mouseTracker.debugDeviceActiveCursor(1),
      );
    }
    expect(seen, contains(SystemMouseCursors.click)); // over the URL
    expect(seen, contains(SystemMouseCursors.text)); // over "AA" / "ZZ"
  });
}
