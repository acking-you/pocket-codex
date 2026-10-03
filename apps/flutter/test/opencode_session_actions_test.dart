// Rename, compact, git diff and file attachments share the Codex entry points
// on an OpenCode service key: no capability flag hides them.
import 'dart:convert';

import 'package:file_selector_platform_interface/file_selector_platform_interface.dart'
    as fsel;
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/attachment_refs.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';

import 'fake_bridge_api.dart';
import 'support/screen_harness.dart';

const openCode = 'pcx:dev:opencode:default';

Future<FakeBridgeApi> openWide(
  WidgetTester t, {
  FakeBridgeApi? bridge,
  String? cwd,
}) async {
  final api = bridge ?? FakeBridgeApi();
  await api.appConnect(openCode, 28080);
  api.appThreads.add(
    const ThreadMeta(
      id: 'ses-1',
      preview: 'first prompt',
      cwd: '',
      updatedAt: 0,
    ),
  );
  api.readResult = ThreadHistory(
    items: const [
      ThreadItem(id: 'u1', itemType: 'userMessage', title: '', text: 'hi'),
    ],
    running: false,
    branch: cwd == null ? null : 'main',
    cwd: cwd,
  );
  t.view.devicePixelRatio = 1.0;
  t.view.physicalSize = const Size(1400, 900); // desktop top bar
  addTearDown(t.view.reset);
  await t.pumpWidget(
    host(const AppSessionScreen(serviceKey: openCode, threadId: 'ses-1'), api),
  );
  await t.pumpAndSettle();
  return api;
}

void main() {
  setUp(AppSessionScreen.debugResetThreadMemory);

  testWidgets('rename from the more-actions menu reaches appSetThreadName', (
    t,
  ) async {
    final api = await openWide(t);
    await t.tap(find.byType(PopupMenuButton<String>));
    await t.pumpAndSettle();
    await t.tap(find.text('重命名').last);
    await t.pumpAndSettle();
    await t.enterText(find.byKey(const Key('bar-title-field')), 'renamed');
    await t.testTextInput.receiveAction(TextInputAction.done);
    await t.pumpAndSettle();
    expect(api.setNames['ses-1'], 'renamed');
  });

  testWidgets('compact from the more-actions menu reaches appCompact', (
    t,
  ) async {
    final api = await openWide(t);
    await t.tap(find.byType(PopupMenuButton<String>));
    await t.pumpAndSettle();
    await t.tap(find.text('压缩对话').last);
    await t.pumpAndSettle();
    await t.tap(find.widgetWithText(FilledButton, '压缩对话'));
    await t.pumpAndSettle();
    expect(api.compacted, isTrue);
  });

  testWidgets('the git diff badge loads for the session directory', (t) async {
    final api = FakeBridgeApi()
      ..gitDiffText =
          'diff --git a/x.dart b/x.dart\n'
          '--- a/x.dart\n'
          '+++ b/x.dart\n'
          '@@ -1 +1,2 @@\n'
          '-old\n'
          '+new\n'
          '+more\n';
    await openWide(t, bridge: api, cwd: '/proj');
    expect(find.text('main'), findsWidgets);
    expect(find.text('+2'), findsWidgets);
    expect(find.text('−1'), findsWidgets);
  });

  testWidgets('file attachments upload through the OpenCode meta key', (
    t,
  ) async {
    final selector = FakeFileSelector();
    fsel.FileSelectorPlatform.instance = selector;
    final api = await openWide(t);
    selector.files = [MemXFile(utf8.encode('notes'), 'notes.txt')];
    await attachMenu(t, 'attach-file-btn');
    await t.pumpAndSettle();
    expect(api.lastUploadKey, openCode);
    await t.enterText(find.byKey(const Key('composer-input')), 'see file');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(
      api.lastTurnText,
      appendFileRefs('see file', ['/host/uploads/123/notes.txt']),
    );
  });
}
