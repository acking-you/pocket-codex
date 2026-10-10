// ACP sessions in the shared session screen: native-only calls are never
// made, native microphone features are absent, agent permissions are
// answered with the agent's own option ids, and per-session agent options
// replace the model catalog.

import 'dart:async';

import 'package:desktop_drop/desktop_drop.dart';
import 'package:file_selector_platform_interface/file_selector_platform_interface.dart'
    as fsel;
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/screens/app_session/activity_cards.dart';
import 'package:pocket_codex/src/screens/app_session/agent_permission_card.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/voice/dictation_widgets.dart';

import 'fake_bridge_api.dart';
import 'support/screen_harness.dart';

const acp = 'pcx:dev:acp:agent';
const codex = 'pcx:dev:app:default';

const negotiated = AppCapabilities(
  provider: 'acp',
  fast: false,
  permissionPresets: false,
  guardian: false,
  rateLimits: false,
  takeover: false,
  externalWriterMonitor: false,
  localSessions: false,
  planMode: false,
  protocol: 'acp',
  providerName: 'Beta agent',
  negotiated: true,
  generation: 3,
  sessionConfig: true,
  permissionOptions: true,
  sessionReopen: 'load',
  sessionList: 'agent',
  runningInventory: 'engine',
);

/// [negotiated], for an agent that advertised image prompts.
const negotiatedWithImages = AppCapabilities(
  provider: 'acp',
  fast: false,
  permissionPresets: false,
  guardian: false,
  rateLimits: false,
  takeover: false,
  externalWriterMonitor: false,
  localSessions: false,
  planMode: false,
  protocol: 'acp',
  providerName: 'Beta agent',
  negotiated: true,
  generation: 3,
  imageInput: true,
  sessionConfig: true,
  permissionOptions: true,
  sessionReopen: 'load',
  sessionList: 'agent',
  runningInventory: 'engine',
);

/// A file picker that answers only when the test completes [selection].
class PausedFileSelector extends FakeFileSelector {
  final selection = Completer<List<fsel.XFile>>();

  @override
  Future<List<fsel.XFile>> openFiles({
    List<fsel.XTypeGroup>? acceptedTypeGroups,
    String? initialDirectory,
    String? confirmButtonText,
  }) => selection.future;
}

/// A picked file whose contents are read only when the test completes
/// [read].
class SlowMemXFile extends MemXFile {
  SlowMemXFile(super.bytes, super.name);
  final read = Completer<void>();

  @override
  Future<Uint8List> readAsBytes() async {
    await read.future;
    return super.readAsBytes();
  }
}

/// The agent host behind the connection was lost.
const hostLost = AppEvent(kind: 'acp/host/state', raw: '{"connected":false}');

class AcpApi extends FakeBridgeApi {
  int modelListCalls = 0;
  int rateLimitCalls = 0;

  @override
  Future<List<ModelInfo>> appModelList(String serviceKey) async {
    modelListCalls++;
    return const [];
  }

  @override
  Future<String> appRateLimits(String serviceKey) {
    rateLimitCalls++;
    return super.appRateLimits(serviceKey);
  }
}

Future<void> frames(WidgetTester t) async {
  for (var i = 0; i < 8; i++) {
    await t.pump(const Duration(milliseconds: 50));
  }
}

Future<void> open(
  WidgetTester t,
  FakeBridgeApi api,
  String key, {
  String? thread,
}) async {
  await api.appConnect(key, 28080);
  await t.pumpWidget(
    host(AppSessionScreen(serviceKey: key, threadId: thread), api),
  );
  await frames(t);
}

/// Composer attachment chips (not their remove buttons).
Finder attachmentChips() => find.byWidgetPredicate((w) {
  final key = w.key;
  return key is ValueKey<String> &&
      key.value.startsWith('attachment-') &&
      !key.value.startsWith('attachment-remove-');
});

/// Run [body] as a desktop app (registers the paste key handler and the drop
/// target). Reset inside the body so the debug-variable check passes.
Future<void> onDesktop(Future<void> Function() body) async {
  debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
  try {
    await body();
  } finally {
    debugDefaultTargetPlatformOverride = null;
  }
}

/// Fake clipboard contents for Ctrl/Cmd+V, restored afterwards.
void fakeClipboard({Uint8List? image, List<String> files = const []}) {
  final previousImage = AppSessionScreen.debugClipboardImage;
  final previousFiles = AppSessionScreen.debugClipboardFiles;
  AppSessionScreen.debugClipboardImage = () async => image;
  AppSessionScreen.debugClipboardFiles = () async => files;
  addTearDown(() {
    AppSessionScreen.debugClipboardImage = previousImage;
    AppSessionScreen.debugClipboardFiles = previousFiles;
  });
}

Future<void> paste(WidgetTester t) async {
  await t.tap(find.byKey(const Key('composer-input')));
  await t.pump();
  await t.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
  await t.sendKeyEvent(LogicalKeyboardKey.keyV);
  await t.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
  await frames(t);
}

/// Fake file picker returning [files], restored afterwards.
void fakePicker(List<MemXFile> files) {
  final previous = fsel.FileSelectorPlatform.instance;
  fsel.FileSelectorPlatform.instance = FakeFileSelector()..files = files;
  addTearDown(() => fsel.FileSelectorPlatform.instance = previous);
}

AppEvent permission(String handle, String options) => AppEvent(
  kind: agentPermissionKind,
  threadId: 'ses-1',
  requestId: handle,
  title: 'Run ls',
  raw: '{"threadId":"ses-1","toolCall":{"title":"Run ls"},"options":$options}',
);

void main() {
  setUp(AppSessionScreen.debugResetThreadMemory);

  testWidgets('reopening an ACP session makes no native metadata call', (
    t,
  ) async {
    final api = AcpApi()..acpCapabilities[acp] = negotiated;
    await open(t, api, acp, thread: 'ses-1');
    expect(api.threadMetadataCalls, isEmpty);
    expect(api.threadReads, contains('ses-1'));
    expect(api.modelListCalls, 0, reason: 'no invented model catalog');
    expect(api.rateLimitCalls, 0);
  });

  testWidgets('Codex still runs the native metadata check before resuming', (
    t,
  ) async {
    final api = AcpApi();
    await open(t, api, codex, thread: 'ses-1');
    expect(api.threadMetadataCalls, contains(codex));
  });

  testWidgets('the fake refuses native metadata for non-native protocols', (
    t,
  ) async {
    final api = FakeBridgeApi();
    await expectLater(api.appThreadMetadata(acp, 'x'), throwsStateError);
  });

  testWidgets('ACP has no live voice, dictation, presets or model chip', (
    t,
  ) async {
    final api = AcpApi()..acpCapabilities[acp] = negotiated;
    await open(t, api, acp, thread: 'ses-1');
    expect(find.byKey(const Key('voice-start')), findsNothing);
    expect(find.byType(DictationButton), findsNothing);
    expect(find.byKey(const Key('permission-chip')), findsNothing);
    expect(find.byKey(const Key('fast-mode-btn')), findsNothing);
    expect(find.byKey(const Key('model-chip')), findsNothing);
    await t.tap(find.byKey(const Key('attach-menu-btn')));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('dictate-menu')), findsNothing);
    expect(
      find.byKey(const Key('attach-btn')),
      findsNothing,
      reason: 'the agent did not advertise image prompts',
    );
  });

  testWidgets('Codex keeps voice and dictation', (t) async {
    final api = AcpApi();
    await open(t, api, codex);
    expect(find.byKey(const Key('voice-start')), findsOneWidget);
  });

  testWidgets('agent options render as offered and answer the exact id', (
    t,
  ) async {
    final api = AcpApi()..acpCapabilities[acp] = negotiated;
    await open(t, api, acp, thread: 'ses-1');
    api.pushEvent(
      acp,
      permission(
        'h1',
        '[{"optionId":"opt:1 ✓","name":"Allow once","kind":"allow_once"},'
            '{"optionId":"later","name":"Ask me later","kind":"future_kind"},'
            '{"optionId":"no","name":"Reject","kind":"reject_once"}]',
      ),
    );
    await frames(t);
    expect(find.byKey(const Key('agent-permission-card')), findsOneWidget);
    expect(find.byKey(const Key('approval-card')), findsNothing);
    expect(find.text('Run ls'), findsWidgets);
    // Agent order and labels, unknown kinds included.
    final labels = [
      for (var i = 0; i < 3; i++)
        (t
                    .widget<ButtonStyleButton>(
                      find.byKey(Key('agent-permission-option-$i')),
                    )
                    .child!
                as Text)
            .data,
    ];
    expect(labels, ['Allow once', 'Ask me later', 'Reject']);
    await t.tap(find.byKey(const Key('agent-permission-option-0')));
    await frames(t);
    expect(api.lastApprovalDecision, 'opt:1 ✓');
    expect(find.byKey(const Key('agent-permission-card')), findsNothing);
  });

  testWidgets('remembering choices asks first', (t) async {
    final api = AcpApi()..acpCapabilities[acp] = negotiated;
    await open(t, api, acp, thread: 'ses-1');
    api.pushEvent(
      acp,
      permission(
        'h2',
        '[{"optionId":"always","name":"Always allow","kind":"allow_always"}]',
      ),
    );
    await frames(t);
    await t.tap(find.byKey(const Key('agent-permission-option-0')));
    await frames(t);
    expect(find.byType(AlertDialog), findsOneWidget);
    expect(api.lastApprovalDecision, isNull);
    await t.tap(find.byKey(const Key('agent-permission-remember-confirm')));
    await frames(t);
    expect(api.lastApprovalDecision, 'always');
  });

  testWidgets('a resolved agent request drops its card', (t) async {
    final api = AcpApi()..acpCapabilities[acp] = negotiated;
    await open(t, api, acp, thread: 'ses-1');
    api.pushEvent(
      acp,
      permission('h3', '[{"optionId":"a","name":"Allow","kind":"allow_once"}]'),
    );
    await frames(t);
    api.pushEvent(
      acp,
      const AppEvent(
        kind: 'serverRequest/resolved',
        threadId: 'ses-1',
        raw: '{"threadId":"ses-1","requestId":"h3"}',
      ),
    );
    await frames(t);
    expect(find.byKey(const Key('agent-permission-card')), findsNothing);
    expect(api.lastApprovalDecision, isNull);
  });

  testWidgets('per-session agent options send opaque value ids', (t) async {
    final api = AcpApi()..acpCapabilities[acp] = negotiated;
    api.sessionSettings['$acp|ses-1'] = const SessionSettings(
      configOptions: [
        SessionConfigOption(
          id: 'model',
          name: 'Model',
          category: 'model',
          currentValue: 'fast-1',
          values: [
            SessionConfigValue(value: 'fast-1', name: 'Fast'),
            SessionConfigValue(value: 'large-2', name: 'Two', group: 'Large'),
          ],
        ),
      ],
      currentMode: 'ask',
      modes: [
        SessionMode(id: 'ask', name: 'Ask'),
        SessionMode(id: 'code', name: 'Code'),
      ],
    );
    await open(t, api, acp, thread: 'ses-1');
    expect(find.byKey(const Key('agent-config-model')), findsOneWidget);
    expect(find.text('Fast'), findsOneWidget);
    await t.tap(find.byKey(const Key('agent-config-model')));
    await t.pumpAndSettle();
    await t.tap(find.text('Large · Two').last);
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('agent-mode')));
    await t.pumpAndSettle();
    await t.tap(find.text('Code').last);
    await t.pumpAndSettle();
    expect(api.sessionConfigCalls, [
      ('ses-1', 'model', 'large-2'),
      ('ses-1', 'mode', 'code'),
    ]);
  });

  testWidgets('a routine agent state change keeps the connection', (t) async {
    final api = AcpApi()..acpCapabilities[acp] = negotiated;
    await open(t, api, acp, thread: 'ses-1');
    final connects = api.appConnectCount;
    api.pushEvent(
      acp,
      const AppEvent(
        kind: 'acp/host/state',
        raw: '{"phase":"ready","authRequired":false,"connected":true}',
      ),
    );
    await frames(t);
    expect(api.appConnectCount, connects);
  });

  testWidgets('losing the agent host reconnects and re-reads the session', (
    t,
  ) async {
    final api = AcpApi()..acpCapabilities[acp] = negotiated;
    await open(t, api, acp, thread: 'ses-1');
    final connects = api.appConnectCount;
    final reads = api.threadReads.where((id) => id == 'ses-1').length;
    api.pushEvent(
      acp,
      const AppEvent(kind: 'acp/host/state', raw: '{"connected":false}'),
    );
    await frames(t);
    expect(api.appConnectCount, greaterThan(connects));
    expect(
      api.threadReads.where((id) => id == 'ses-1').length,
      greaterThan(reads),
    );
  });

  testWidgets('a replaced agent host re-reads the session', (t) async {
    final api = AcpApi()..acpCapabilities[acp] = negotiated;
    await open(t, api, acp, thread: 'ses-1');
    final reads = api.threadReads.where((id) => id == 'ses-1').length;
    api.pushEvent(
      acp,
      const AppEvent(
        kind: 'acp/host/state',
        raw: '{"generation":4,"connected":true,"replaced":true}',
      ),
    );
    await frames(t);
    expect(
      api.threadReads.where((id) => id == 'ses-1').length,
      greaterThan(reads),
    );
  });

  testWidgets('a late completion of an earlier turn keeps the running one', (
    t,
  ) async {
    final api = AcpApi()..acpCapabilities[acp] = negotiated;
    await open(t, api, acp, thread: 'ses-1');
    api.pushEvent(
      acp,
      const AppEvent(
        kind: 'turn/started',
        threadId: 'ses-1',
        raw: '{"threadId":"ses-1","turnId":"B","turn":{"id":"B"}}',
      ),
    );
    await frames(t);
    expect(find.byKey(const Key('stop-btn')), findsOneWidget);
    // A recovery reports that the previous turn A finished.
    api.pushEvent(
      acp,
      const AppEvent(
        kind: 'turn/completed',
        threadId: 'ses-1',
        raw: '{"threadId":"ses-1","turn":{"id":"A","status":"completed"}}',
      ),
    );
    await frames(t);
    expect(
      find.byKey(const Key('stop-btn')),
      findsOneWidget,
      reason: 'B is still running',
    );
    await t.tap(find.byKey(const Key('stop-btn')));
    await frames(t);
    expect(api.lastInterruptTurnId, 'B', reason: 'Stop targets B, not A');
    api.pushEvent(
      acp,
      const AppEvent(
        kind: 'turn/completed',
        threadId: 'ses-1',
        raw: '{"threadId":"ses-1","turn":{"id":"B","status":"interrupted"}}',
      ),
    );
    await frames(t);
    expect(find.byKey(const Key('stop-btn')), findsNothing);
    await t.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('picking an image file for a text-only agent attaches nothing', (
    t,
  ) async {
    fakePicker([
      MemXFile(tinyPng(), 'shot.png'),
      MemXFile(Uint8List.fromList([1, 2, 3]), 'notes.md'),
    ]);
    final api = AcpApi()..acpCapabilities[acp] = negotiated;
    await open(t, api, acp, thread: 'ses-1');
    await t.tap(find.byKey(const Key('attach-menu-btn')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('attach-file-btn')));
    await frames(t);
    expect(find.text('shot.png'), findsNothing, reason: 'no unsupported image');
    expect(
      find.text('notes.md'),
      findsOneWidget,
      reason: 'documents still work',
    );
    expect(attachmentChips(), findsOneWidget);
    expect(find.text('该智能体不接受图片'), findsOneWidget);
  });

  testWidgets('a native session still takes a picked image file', (t) async {
    fakePicker([MemXFile(tinyPng(), 'shot.png')]);
    final api = AcpApi();
    await open(t, api, codex, thread: 'ses-1');
    await t.tap(find.byKey(const Key('attach-menu-btn')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('attach-file-btn')));
    await frames(t);
    expect(attachmentChips(), findsOneWidget);
  });

  testWidgets('pasting an image into a text-only agent attaches nothing', (
    t,
  ) async {
    await onDesktop(() async {
      fakeClipboard(image: tinyPng());
      final api = AcpApi()..acpCapabilities[acp] = negotiated;
      await open(t, api, acp, thread: 'ses-1');
      await paste(t);
      expect(attachmentChips(), findsNothing);
      expect(find.text('该智能体不接受图片'), findsOneWidget);
      await t.pumpWidget(const SizedBox.shrink());
    });
  });

  testWidgets('pasting an image into a native session attaches it', (t) async {
    await onDesktop(() async {
      fakeClipboard(image: tinyPng());
      final api = AcpApi();
      await open(t, api, codex, thread: 'ses-1');
      await paste(t);
      expect(attachmentChips(), findsOneWidget);
      await t.pumpWidget(const SizedBox.shrink());
    });
  });

  for (final key in [acp, codex]) {
    testWidgets('desktop drop respects image support for $key', (t) async {
      await onDesktop(() async {
        final api = AcpApi()..acpCapabilities[acp] = negotiated;
        await open(t, api, key, thread: 'ses-1');
        final drop = t.widget<DropTarget>(find.byType(DropTarget));
        drop.onDragDone!(
          DropDoneDetails(
            files: [
              DropItemFile.fromData(tinyPng(), path: 'dropped.png'),
              DropItemFile.fromData(
                Uint8List.fromList('safe fixture'.codeUnits),
                path: 'dropped.md',
              ),
            ],
            localPosition: Offset.zero,
            globalPosition: Offset.zero,
          ),
        );
        await frames(t);
        expect(api.lastUploadName, 'dropped.md');
        expect(api.lastUploadKey, key);
        expect(attachmentChips(), findsNWidgets(key == acp ? 1 : 2));
        expect(find.text('dropped.md'), findsOneWidget);
        expect(
          find.text('该智能体不接受图片'),
          key == acp ? findsOneWidget : findsNothing,
        );
        await t.pumpWidget(const SizedBox.shrink());
      });
    });
  }

  testWidgets('a pick that completes after the agent host was lost attaches '
      'nothing', (t) async {
    final previous = fsel.FileSelectorPlatform.instance;
    final selector = PausedFileSelector();
    fsel.FileSelectorPlatform.instance = selector;
    addTearDown(() => fsel.FileSelectorPlatform.instance = previous);
    final api = AcpApi()..acpCapabilities[acp] = negotiated;
    await open(t, api, acp, thread: 'ses-1');
    await t.tap(find.byKey(const Key('attach-menu-btn')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('attach-file-btn')));
    await frames(t);
    // The picker is still open when the host goes away.
    api.pushEvent(acp, hostLost);
    await frames(t);
    selector.selection.complete([
      MemXFile(Uint8List.fromList('fixture'.codeUnits), 'notes.md'),
    ]);
    await frames(t);
    expect(attachmentChips(), findsNothing);
    expect(api.lastUploadName, isNull, reason: 'nothing reaches another host');
    await t.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('a paused pick with the host intact still attaches', (t) async {
    final previous = fsel.FileSelectorPlatform.instance;
    final selector = PausedFileSelector();
    fsel.FileSelectorPlatform.instance = selector;
    addTearDown(() => fsel.FileSelectorPlatform.instance = previous);
    final api = AcpApi()..acpCapabilities[acp] = negotiated;
    await open(t, api, acp, thread: 'ses-1');
    await t.tap(find.byKey(const Key('attach-menu-btn')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('attach-file-btn')));
    await frames(t);
    selector.selection.complete([
      MemXFile(Uint8List.fromList('fixture'.codeUnits), 'notes.md'),
    ]);
    await frames(t);
    expect(attachmentChips(), findsOneWidget);
    expect(api.lastUploadName, 'notes.md');
    await t.pumpWidget(const SizedBox.shrink());
  });

  testWidgets('a document still being read when the host is lost is never '
      'uploaded', (t) async {
    final slow = SlowMemXFile(
      Uint8List.fromList('must not upload'.codeUnits),
      'slow.md',
    );
    fakePicker([slow]);
    final api = AcpApi()..acpCapabilities[acp] = negotiated;
    await open(t, api, acp, thread: 'ses-1');
    await t.tap(find.byKey(const Key('attach-menu-btn')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('attach-file-btn')));
    await frames(t);
    expect(
      attachmentChips(),
      findsOneWidget,
      reason: 'admitted, still reading',
    );
    api.pushEvent(acp, hostLost);
    await frames(t);
    slow.read.complete();
    await frames(t);
    expect(api.lastUploadName, isNull);
    expect(t.takeException(), isNull);
    await t.pumpWidget(const SizedBox.shrink());
  });

  for (final leave in [false, true]) {
    for (final replace in [false, true]) {
      testWidgets(
        'pending file read retains destination (leave=$leave replace=$replace)',
        (t) async {
          final slow = SlowMemXFile(
            Uint8List.fromList('private fixture'.codeUnits),
            'pending.md',
          );
          fakePicker([slow]);
          final api = AcpApi()..acpCapabilities[acp] = negotiated;
          await open(t, api, acp, thread: 'ses-1');
          await t.tap(find.byKey(const Key('attach-menu-btn')));
          await t.pumpAndSettle();
          await t.tap(find.byKey(const Key('attach-file-btn')));
          await frames(t);
          expect(attachmentChips(), findsOneWidget);
          // Keep the ProviderScope/draft store alive, disposing only the screen.
          if (leave) await t.pumpWidget(host(const SizedBox(), api));
          if (replace) api.uploadContext = 'other-account-same-service-key';
          slow.read.complete();
          await frames(t);
          expect(api.lastUploadName, leave || replace ? isNull : 'pending.md');
          if (leave) {
            await open(t, api, acp, thread: 'ses-1');
            expect(
              find.text('pending.md'),
              findsOneWidget,
              reason: 'the original draft keeps its retryable attachment',
            );
          }
          expect(t.takeException(), isNull);
          await t.pumpWidget(const SizedBox.shrink());
        },
      );
    }
  }

  for (final guardian in [false, true]) {
    for (final reading in [false, true]) {
      testWidgets(
        '${reading ? 'a document read' : 'a file pick'} completing after '
        '${guardian ? 'Guardian discovery' : 'an active writer rejection'} '
        'never uploads',
        (t) async {
          final previous = fsel.FileSelectorPlatform.instance;
          final selector = PausedFileSelector();
          fsel.FileSelectorPlatform.instance = selector;
          addTearDown(() => fsel.FileSelectorPlatform.instance = previous);
          final api = AcpApi()..metadataOnlyFollow = true;
          await open(t, api, codex, thread: 'ses-1');
          await t.tap(find.byKey(const Key('attach-menu-btn')));
          await t.pumpAndSettle();
          await t.tap(find.byKey(const Key('attach-file-btn')));
          await frames(t);
          final file = SlowMemXFile(
            Uint8List.fromList('must not upload'.codeUnits),
            'pending.md',
          );
          if (reading) {
            selector.selection.complete([file]);
            await frames(t);
            expect(attachmentChips(), findsOneWidget);
          }
          // The normal event-driven history refresh discovers the new
          // ownership while the platform picker/file read is still pending.
          if (guardian) {
            api.appThreads.add(
              const ThreadMeta(
                id: 'ses-1',
                preview: 'Approval review',
                cwd: '/fixture',
                updatedAt: 1,
                parentThreadId: 'parent',
                threadSource: 'guardian_review',
              ),
            );
          } else {
            api.appThreadResumeError = StateError(
              'thread already has an active writer',
            );
          }
          api.pushEvent(
            codex,
            const AppEvent(
              kind: 'thread/compacted',
              threadId: 'ses-1',
              raw: '{}',
            ),
          );
          await frames(t);
          expect(find.byKey(const Key('composer-input')), findsNothing);
          if (guardian) {
            expect(find.byKey(const Key('guardian-read-only')), findsOneWidget);
          }
          if (!reading) selector.selection.complete([file]);
          file.read.complete();
          await frames(t);
          expect(api.lastUploadName, isNull);
          expect(api.lastUploadKey, isNull);
          expect(t.takeException(), isNull);
          await t.pumpWidget(const SizedBox.shrink());
        },
      );
    }
  }

  testWidgets('a clipboard image read when image support goes away attaches '
      'nothing', (t) async {
    await onDesktop(() async {
      final image = Completer<Uint8List?>();
      final previousImage = AppSessionScreen.debugClipboardImage;
      final previousFiles = AppSessionScreen.debugClipboardFiles;
      AppSessionScreen.debugClipboardImage = () => image.future;
      AppSessionScreen.debugClipboardFiles = () async => const [];
      addTearDown(() {
        AppSessionScreen.debugClipboardImage = previousImage;
        AppSessionScreen.debugClipboardFiles = previousFiles;
      });
      final api = AcpApi()..acpCapabilities[acp] = negotiatedWithImages;
      await open(t, api, acp, thread: 'ses-1');
      await paste(t);
      // The reconnected agent no longer takes images.
      api.acpCapabilities[acp] = negotiated;
      api.pushEvent(
        acp,
        const AppEvent(
          kind: 'acp/host/state',
          raw: '{"phase":"ready","authRequired":false,"connected":true}',
        ),
      );
      await frames(t);
      image.complete(tinyPng());
      await frames(t);
      expect(attachmentChips(), findsNothing);
      await t.pumpWidget(const SizedBox.shrink());
    });
  });

  testWidgets('an omission notice re-sent empty disappears and leaves the '
      'tool named like it', (t) async {
    final api = AcpApi()..acpCapabilities[acp] = negotiated;
    await open(t, api, acp, thread: 'ses-1');
    AppEvent item(String id, String type, String text, {bool done = true}) =>
        AppEvent(
          kind: done ? 'item/completed' : 'item/started',
          threadId: 'ses-1',
          itemId: id,
          itemType: type,
          title: type == 'contentOmitted' ? '' : 'Tool $id',
          text: text,
          raw: '{"threadId":"ses-1","turnId":"T","itemId":"$id"}',
        );
    api.pushEvent(
      acp,
      const AppEvent(
        kind: 'turn/started',
        threadId: 'ses-1',
        raw: '{"threadId":"ses-1","turnId":"T","turn":{"id":"T"}}',
      ),
    );
    // Tool `x` dropped an oversized diff; another tool is legally named
    // `x:omitted`.
    api.pushEvent(acp, item('tool:x', 'dynamicToolCall', '{}', done: false));
    api.pushEvent(acp, item('#omitted:tool:x', 'contentOmitted', 'diffs'));
    api.pushEvent(
      acp,
      item('tool:x:omitted', 'dynamicToolCall', '{}', done: false),
    );
    await frames(t);
    if (find.byType(ContentOmittedNotice).evaluate().isEmpty) {
      final toggle = find.byKey(const Key('turn-work-toggle'));
      if (toggle.evaluate().isNotEmpty) {
        await t.tap(toggle.first);
        await frames(t);
      }
    }
    expect(find.byType(ContentOmittedNotice), findsOneWidget);
    expect(find.text('Tool tool:x:omitted'), findsOneWidget);
    // The replacement diff fits: the bridge re-sends the notice empty.
    api.pushEvent(acp, item('#omitted:tool:x', 'contentOmitted', ''));
    await frames(t);
    expect(find.byType(ContentOmittedNotice), findsNothing);
    // With the notice removed, the now-adjacent tools form a collapsed
    // activity group. Open it through the same control the reader uses.
    final group = find.byType(GroupedActivityCard);
    expect(group, findsOneWidget);
    expect(
      t.widget<GroupedActivityCard>(group).group.items.map((item) => item.id),
      ['tool:x', 'tool:x:omitted'],
    );
    final toggle = find.descendant(of: group, matching: find.byType(InkWell));
    await t.ensureVisible(toggle);
    await t.tap(toggle);
    await frames(t);
    expect(find.text('Tool tool:x'), findsOneWidget);
    expect(find.text('Tool tool:x:omitted'), findsOneWidget);
    await t.pumpWidget(const SizedBox.shrink());
  });

  for (final alreadyVisible in [false, true]) {
    testWidgets(
      'omission deletion survives pending history (visible=$alreadyVisible)',
      (t) async {
        const notice = ThreadItem(
          id: '#omitted:tool:x',
          itemType: 'contentOmitted',
          title: '',
          text: 'diffs',
          turnId: 'T',
        );
        const tools = [
          ThreadItem(
            id: 'tool:x',
            itemType: 'dynamicToolCall',
            title: 'Tool x',
            text: '{}',
            turnId: 'T',
          ),
          ThreadItem(
            id: 'tool:x:omitted',
            itemType: 'dynamicToolCall',
            title: 'Tool x:omitted',
            text: '{}',
            turnId: 'T',
          ),
        ];
        final api = AcpApi()..acpCapabilities[acp] = negotiated;
        api.readResult = ThreadHistory(
          items: [tools.first, if (alreadyVisible) notice, tools.last],
          running: false,
        );
        await open(t, api, acp, thread: 'ses-1');
        final read = Completer<ThreadHistory>();
        api.pendingReads['ses-1'] = [read.future];
        api.pushEvent(
          acp,
          const AppEvent(
            kind: 'thread/compacted',
            threadId: 'ses-1',
            raw: '{}',
          ),
        );
        await frames(t);
        expect(
          api.pendingReads['ses-1'],
          isEmpty,
          reason: 'production history refresh must be waiting',
        );
        api.pushEvent(
          acp,
          const AppEvent(
            kind: 'item/completed',
            threadId: 'ses-1',
            itemId: '#omitted:tool:x',
            itemType: 'contentOmitted',
            text: '',
            raw: '{"threadId":"ses-1","turnId":"T"}',
          ),
        );
        await frames(t);
        read.complete(
          ThreadHistory(
            items: [tools.first, notice, tools.last],
            running: false,
          ),
        );
        await frames(t);
        final work = find.byKey(const Key('turn-work-toggle'));
        if (work.evaluate().isNotEmpty) {
          await t.tap(work.first);
          await frames(t);
        }
        expect(find.byType(ContentOmittedNotice), findsNothing);
        final group = find.byType(GroupedActivityCard);
        expect(group, findsOneWidget);
        expect(
          t
              .widget<GroupedActivityCard>(group)
              .group
              .items
              .map((item) => item.id),
          ['tool:x', 'tool:x:omitted'],
        );
        expect(t.takeException(), isNull);
        await t.pumpWidget(const SizedBox.shrink());
      },
    );
  }

  testWidgets('a Guardian view refuses dropped documents without uploading', (
    t,
  ) async {
    await onDesktop(() async {
      final api = AcpApi()
        ..appThreads.clear()
        ..appThreads.add(
          const ThreadMeta(
            id: 'reviewer',
            preview: 'Approval review',
            cwd: '/fixture',
            updatedAt: 1,
            parentThreadId: 'parent',
            threadSource: 'guardian_review',
          ),
        )
        ..metadataOnlyFollow = true
        ..readResult = const ThreadHistory(items: [], running: false);
      await open(t, api, codex, thread: 'reviewer');
      expect(find.byKey(const Key('guardian-read-only')), findsOneWidget);
      final drop = t.widget<DropTarget>(find.byType(DropTarget));
      drop.onDragDone!(
        DropDoneDetails(
          files: [
            DropItemFile.fromData(
              Uint8List.fromList('must not upload'.codeUnits),
              path: 'private.md',
            ),
          ],
          localPosition: Offset.zero,
          globalPosition: Offset.zero,
        ),
      );
      await frames(t);
      expect(api.lastUploadName, isNull);
      expect(attachmentChips(), findsNothing);
      expect(t.takeException(), isNull);
      await t.pumpWidget(const SizedBox.shrink());
    });
  });

  for (final (reasons, english) in [
    (
      'images,diffs',
      'Not kept because it was too large: images, file changes.',
    ),
    (
      'unavailable',
      'Part of this reply could not be read from the agent host.',
    ),
  ]) {
    testWidgets('omitted content is named: $reasons', (t) async {
      await t.pumpWidget(
        MaterialApp(
          locale: const Locale('en'),
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(body: ContentOmittedNotice(reasons: reasons)),
        ),
      );
      expect(find.text(english), findsOneWidget);
    });
  }

  testWidgets('an agent that reports no options shows no option chips', (
    t,
  ) async {
    final api = AcpApi()..acpCapabilities[acp] = negotiated;
    await open(t, api, acp, thread: 'ses-1');
    expect(find.byKey(const Key('agent-session-options')), findsNothing);
  });
}
