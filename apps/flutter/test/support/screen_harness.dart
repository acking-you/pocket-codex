/// Shared scaffolding for the screen suites in `test/screens/`.
///
/// Belongs here: a fake for a platform channel that cannot run headless, and a
/// helper that mounts a screen or drives a widget every suite uses. NOT here:
/// an assertion, or a helper only one suite needs — that stays in its suite, so
/// this file does not become the place everything accumulates.
library;

import 'dart:convert';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:file_selector_platform_interface/file_selector_platform_interface.dart'
    as fsel;
import 'package:image/image.dart' as img;
import 'package:image_picker_platform_interface/image_picker_platform_interface.dart';
import 'package:plugin_platform_interface/plugin_platform_interface.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/web_authenticator.dart';

/// Fake browser hand-off: returns a canned redirect URL (or throws) instead of
/// driving the real platform-channel plugin.
class FakeWebAuthenticator implements WebAuthenticator {
  FakeWebAuthenticator(this.result, {this.error});

  /// Redirect URL to return from [authenticate] on success.
  final String result;

  /// When set, [authenticate] throws this instead of returning.
  final Object? error;

  @override
  Future<String> authenticate({
    required String url,
    required String callbackUrlScheme,
  }) async {
    if (error != null) throw error!;
    return result;
  }
}

/// Fake image picker: returns canned [XFile]s instead of driving the real
/// platform-channel picker (which can't run headless).
class FakeImagePicker extends ImagePickerPlatform
    with MockPlatformInterfaceMixin {
  /// Files the next pick returns; empty = user cancelled.
  List<XFile> files = [];

  @override
  Future<List<XFile>> getMultiImageWithOptions({
    MultiImagePickerOptions options = const MultiImagePickerOptions(),
  }) async => files;

  @override
  Future<XFile?> getImageFromSource({
    required ImageSource source,
    ImagePickerOptions options = const ImagePickerOptions(),
  }) async => files.isEmpty ? null : files.first;
}

/// Fake file selector: returns canned [XFile]s instead of the native dialog.
class FakeFileSelector extends fsel.FileSelectorPlatform
    with MockPlatformInterfaceMixin {
  /// Files the next pick returns; empty = user cancelled.
  List<XFile> files = [];

  @override
  Future<List<XFile>> openFiles({
    List<fsel.XTypeGroup>? acceptedTypeGroups,
    String? initialDirectory,
    String? confirmButtonText,
  }) async => files;
}

/// In-memory [XFile] for picker fixtures. `XFile.fromData` won't do: on
/// dart:io it ignores `name` (path stays empty), and a real temp file's
/// `readAsBytes` does real IO that never completes under the fake test clock.
/// Using the name as the path makes `.name` work; the byte read is overridden
/// to resolve in-memory.
class MemXFile extends XFile {
  MemXFile(this._bytes, String name) : super(name);
  final Uint8List _bytes;

  @override
  Future<Uint8List> readAsBytes() async => _bytes;

  @override
  Future<int> length() async => _bytes.length;
}

/// A tiny in-memory PNG for attachment fixtures.
Uint8List tinyPng() {
  final im = img.Image(width: 6, height: 4);
  img.fill(im, color: img.ColorRgb8(200, 40, 40));
  return img.encodePng(im);
}

/// The same PNG as the `data:` URL history/echo would carry.
String tinyPngDataUrl() => 'data:image/png;base64,${base64Encode(tinyPng())}';

/// Mount [child] with a fake bridge and localizations. Defaults to the
/// Chinese locale so the existing zh assertions hold; pass [locale] to test
/// other languages.
Widget host(
  Widget child,
  BridgeApi api, {
  Locale locale = const Locale('zh'),
}) => ProviderScope(
  overrides: [bridgeApiProvider.overrideWithValue(api)],
  child: MaterialApp(
    locale: locale,
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: child,
  ),
);

/// Mount under a GoRouter so screens that call `context.go(...)` navigate; each
/// extra [stubs] entry (path → label) renders a Text so a route can be asserted.
Widget routerHost(
  BridgeApi api, {
  required String initial,
  required List<GoRoute> routes,
  List<Override> overrides = const [],
}) => ProviderScope(
  overrides: [bridgeApiProvider.overrideWithValue(api), ...overrides],
  child: MaterialApp.router(
    locale: const Locale('zh'),
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    routerConfig: GoRouter(initialLocation: initial, routes: routes),
  ),
);

GoRoute stub(String path, String label) => GoRoute(
  path: path,
  builder: (_, _) => Scaffold(body: Text(label)),
);

/// Open the selected device's capabilities, which is where every kind — chat,
/// API, session sharing — is now listed together.
///
/// Services used to be split into protocol tabs, and each of these tests began
/// by tapping the one it cared about. The page is organised by device now, so
/// there is no tab to pick and the rows are already on screen; narrow layouts do
/// gate them behind picking a device, which is what this still does.
Future<void> openDevice(WidgetTester t, [String? device]) async {
  // Wide auto-selects a device, so the capabilities are already up and there is
  // nothing to tap. Narrow shows the list first; tap whichever device is asked
  // for, or the only one when the caller doesn't care.
  final tile = device != null
      ? find.byKey(Key('device-$device'))
      : find.byWidgetPredicate((w) {
          final key = w.key;
          if (key is! ValueKey<String>) return false;
          final name = key.value;
          // `device-<name>` only — not `device-capability-*`/`device-back`.
          return name.startsWith('device-') &&
              !name.startsWith('device-capability-') &&
              name != 'device-back' &&
              name != 'device-clean-unreachable';
        });
  if (tile.evaluate().isEmpty) {
    // Nothing to tap is only legitimate when the capabilities are already up
    // (wide auto-selects) or when there are no devices at all. If the page
    // rendered neither, say so here rather than letting the caller's assertions
    // pass or fail for a reason that has nothing to do with what they test.
    final onDetail = find
        .byWidgetPredicate(
          (w) =>
              w.key is ValueKey<String> &&
              (w.key! as ValueKey<String>).value.startsWith(
                'device-capability-',
              ),
        )
        .evaluate()
        .isNotEmpty;
    final empty = find.text('此设备暂时没有可用能力').evaluate().isNotEmpty;
    expect(
      onDetail || empty,
      isTrue,
      reason: 'no device tile to open and no capabilities on screen either',
    );
    return;
  }
  await t.tap(tile.first);
  await t.pumpAndSettle();
}

/// A 1×1 transparent PNG — the smallest thing `Image.memory` will decode, so a
/// host-image test can assert on a real thumbnail rather than a broken one.
final Uint8List onePixelPng = base64Decode(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=',
);

/// Open the composer's turn-settings sheet (fronted by the model chip) and tap
/// the row for [value] — 'model', 'effort', 'plan' or 'project'. Plan toggles
/// on the spot; the others open their own picker sheet.
Future<void> turnSetting(WidgetTester t, String value) async {
  await t.tap(find.byKey(const Key('model-chip')));
  await t.pumpAndSettle();
  await t.tap(find.byKey(ValueKey('opt-$value')));
  await t.pumpAndSettle();
}

/// Open the composer's `+` attachment menu and tap the item keyed [key].
Future<void> attachMenu(WidgetTester t, String key) async {
  await t.tap(find.byKey(const Key('attach-menu-btn')));
  await t.pumpAndSettle();
  await t.tap(find.byKey(Key(key)));
  await t.pumpAndSettle();
}

/// Every conversation row in the sessions pane. Rows carry no leading icon
/// (each one is a conversation, so a glyph per row was just noise), so counting
/// them means matching their `conv-tile-<id>` keys.
Finder convTiles() => find.byWidgetPredicate(
  (w) => w.key is ValueKey<String> && '${w.key}'.contains('conv-tile-'),
);

/// Expand a turn's folded work so its tool calls and reasoning are on screen.
///
/// A finished turn presents its activity collapsed behind a "已处理 {duration}"
/// row, so a test that asserts on a tool call has to open it first. Opens every
/// fold present, since a test may have produced more than one turn; one that is
/// still running is already open and is left alone.
///
/// Pumps a fixed duration rather than settling: a screen with a running turn
/// anywhere on it holds a spinner, and `pumpAndSettle` would wait for an
/// animation that never ends.
Future<void> openTurnWork(WidgetTester t) async {
  for (final toggle
      in find.byKey(const Key('turn-work-toggle')).evaluate().toList()) {
    final chevron = find.descendant(
      of: find.byWidget(toggle.widget),
      matching: find.byIcon(Icons.keyboard_arrow_right),
    );
    if (chevron.evaluate().isEmpty) continue; // already open, or still running
    await t.tap(find.byWidget(toggle.widget));
    if (find.byType(CircularProgressIndicator).evaluate().isEmpty) {
      await t.pumpAndSettle();
    } else {
      // A spinner is on screen and never settles. Pump enough frames for the
      // fold's 160 ms expand to lay out instead.
      for (var i = 0; i < 8; i++) {
        await t.pump(const Duration(milliseconds: 50));
      }
    }
  }
}
