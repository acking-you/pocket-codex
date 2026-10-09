import 'dart:async';

import 'package:file_selector_platform_interface/file_selector_platform_interface.dart'
    as fsel;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';

import 'fake_bridge_api.dart';
import 'support/screen_harness.dart';

const _service = 'pcx:host:app:default';
final _input = find.byKey(const Key('composer-input'));

Future<void> _mount(
  WidgetTester t,
  FakeBridgeApi api, {
  String service = _service,
  String? thread = 'a',
  String? cwd,
  bool settle = true,
}) async {
  await api.appConnect(service, 28080);
  await t.pumpWidget(
    host(
      AppSessionScreen(
        key: ValueKey(service),
        serviceKey: service,
        threadId: thread,
        cwd: cwd,
        home: true,
      ),
      api,
      locale: const Locale('en'),
    ),
  );
  if (settle) {
    await t.pumpAndSettle();
  } else {
    await _frames(t);
  }
}

Future<void> _frames(WidgetTester t) async {
  for (var i = 0; i < 8; i++) {
    await t.pump(const Duration(milliseconds: 50));
  }
}

TextEditingController _controller(WidgetTester t) =>
    t.widget<TextField>(_input).controller!;

class _UploadingApi extends FakeBridgeApi {
  final upload = Completer<String>();

  @override
  Future<String> metaUploadFile(
    String serviceKey,
    String fileName,
    Uint8List bytes,
  ) => upload.future;
}

class _StartingApi extends FakeBridgeApi {
  final start = Completer<void>();
  bool starting = false;
  Completer<void>? turnAck;
  final sentTo = <String>[];

  @override
  Future<String> appThreadStart(
    String serviceKey, {
    String? model,
    String? cwd,
    String? approvalPolicy,
    String? approvalsReviewer,
    String? serviceTier,
    String? sandbox,
  }) async {
    starting = true;
    await start.future;
    return super.appThreadStart(
      serviceKey,
      model: model,
      cwd: cwd,
      approvalPolicy: approvalPolicy,
      approvalsReviewer: approvalsReviewer,
      serviceTier: serviceTier,
      sandbox: sandbox,
    );
  }

  @override
  Future<void> appTurnStart(
    String serviceKey,
    String threadId,
    String text, {
    List<String> images = const [],
    String? model,
    String? approvalPolicy,
    String? approvalsReviewer,
    String? serviceTier,
    String? sandbox,
    String? collaborationMode,
    String? reasoningEffort,
  }) async {
    sentTo.add(threadId);
    await super.appTurnStart(
      serviceKey,
      threadId,
      text,
      images: images,
      model: model,
      approvalPolicy: approvalPolicy,
      approvalsReviewer: approvalsReviewer,
      serviceTier: serviceTier,
      sandbox: sandbox,
      collaborationMode: collaborationMode,
      reasoningEffort: reasoningEffort,
    );
    if (turnAck != null) await turnAck!.future;
  }
}

class _DelayedFileSelector extends FakeFileSelector {
  final selection = Completer<List<fsel.XFile>>();

  @override
  Future<List<fsel.XFile>> openFiles({
    List<fsel.XTypeGroup>? acceptedTypeGroups,
    String? initialDirectory,
    String? confirmButtonText,
  }) => selection.future;
}

class _LateDefaultStartingApi extends _StartingApi {
  final project = Completer<ProjectConfig>();

  @override
  Future<ProjectConfig> metaProjectConfig(String serviceKey) => project.future;
}

void main() {
  setUp(AppSessionScreen.debugResetThreadMemory);

  Future<void> switchProject(WidgetTester t, String cwd) async {
    if (find.byKey(const Key('project-switcher-btn')).evaluate().isEmpty) {
      await t.longPress(find.byKey(Key('project-header-$cwd')));
      await _frames(t);
      await t.tap(find.text('New conversation in ${cwd.split('/').last}'));
    } else {
      await t.tap(find.byKey(const Key('project-switcher-btn')));
      await _frames(t);
      await t.tap(find.byKey(Key('project-menu-item-$cwd')));
    }
    await _frames(t);
  }

  testWidgets(
    'a project draft keeps its send lock when reopened before thread creation',
    (t) async {
      t.view.devicePixelRatio = 1;
      t.view.physicalSize = const Size(1280, 900);
      addTearDown(t.view.reset);
      final api = _StartingApi()
        ..appThreads.addAll([
          const ThreadMeta(
            id: 'a',
            preview: 'Alpha history',
            cwd: '/alpha',
            updatedAt: 2,
          ),
          const ThreadMeta(
            id: 'b',
            preview: 'Beta history',
            cwd: '/beta',
            updatedAt: 1,
          ),
        ]);
      await _mount(t, api, thread: null, cwd: '/alpha');
      await t.enterText(_input, 'Start alpha');
      await t.pump();
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pump();
      await switchProject(t, '/beta');
      await switchProject(t, '/alpha');
      await t.enterText(_input, 'Alpha followup');
      await t.pump();
      expect(
        t.widget<IconButton>(find.byKey(const Key('send-btn'))).onPressed,
        isNull,
      );
      api.start.complete();
      await t.pumpAndSettle();
      expect(api.sentTo, ['thread-0']);
      expect(_controller(t).text, 'Alpha followup');
      expect(
        t.widget<IconButton>(find.byKey(const Key('send-btn'))).onPressed,
        isNotNull,
      );
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pumpAndSettle();
      expect(api.sentTo, ['thread-0', 'thread-0']);
      expect(api.appThreads, hasLength(3));
    },
  );

  testWidgets(
    'reopened project drains queued input after the pending first send',
    (t) async {
      t.view.devicePixelRatio = 1;
      t.view.physicalSize = const Size(1280, 900);
      addTearDown(t.view.reset);
      final api = _StartingApi()
        ..turnAck = Completer<void>()
        ..appThreads.addAll([
          const ThreadMeta(
            id: 'a',
            preview: 'Alpha history',
            cwd: '/alpha',
            updatedAt: 2,
          ),
          const ThreadMeta(
            id: 'b',
            preview: 'Beta history',
            cwd: '/beta',
            updatedAt: 1,
          ),
        ]);
      await _mount(t, api, thread: null, cwd: '/alpha');
      await t.enterText(_input, 'Start alpha');
      await t.pump();
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pump();
      await switchProject(t, '/beta');
      await switchProject(t, '/alpha');
      await t.enterText(_input, 'Queued alpha');
      await t.sendKeyEvent(LogicalKeyboardKey.enter);
      await t.pump();
      expect(find.byKey(const Key('queued-0')), findsOneWidget);
      api.start.complete();
      await _frames(t);
      expect(api.sentTo, ['thread-0']);
      api.turnAck!.complete();
      await t.pumpAndSettle();
      expect(api.sentTo, ['thread-0', 'thread-0']);
      expect(api.lastTurnText, 'Queued alpha');
      expect(find.byKey(const Key('queued-0')), findsNothing);
      expect(api.appThreads, hasLength(3));
    },
    variant: const TargetPlatformVariant({TargetPlatform.windows}),
  );

  testWidgets(
    'late defaults cannot replace a draft whose first send is pending',
    (t) async {
      final api = _LateDefaultStartingApi();
      await _mount(t, api, thread: null, cwd: '/alpha');
      await t.enterText(_input, 'Saved alpha draft');
      await t.pumpWidget(
        host(const SizedBox(), api, locale: const Locale('en')),
      );
      await _mount(t, api, thread: null, settle: false);
      await t.enterText(_input, 'Send in the current working directory');
      await t.pump();
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pump();
      expect(api.starting, isTrue);
      api.project.complete(const ProjectConfig(defaultProject: '/alpha'));
      await t.pumpAndSettle();
      expect(_controller(t).text, isEmpty);
      api.start.complete();
      await t.pumpAndSettle();
      expect(api.sentTo, ['thread-0']);
      expect(api.lastCwd, isNull);
      await t.enterText(_input, 'Followup');
      await t.pump();
      expect(
        t.widget<IconButton>(find.byKey(const Key('send-btn'))).onPressed,
        isNotNull,
      );
    },
  );

  testWidgets('new project drafts keep uploads in their originating project', (
    t,
  ) async {
    t.view.devicePixelRatio = 1;
    t.view.physicalSize = const Size(1280, 900);
    addTearDown(t.view.reset);
    final api = _UploadingApi()
      ..appThreads.add(
        const ThreadMeta(id: 'b', preview: 'Beta', cwd: '/beta', updatedAt: 0),
      );
    final previous = fsel.FileSelectorPlatform.instance;
    fsel.FileSelectorPlatform.instance = FakeFileSelector()
      ..files = [
        MemXFile(Uint8List.fromList([1, 2]), 'alpha.txt'),
      ];
    addTearDown(() => fsel.FileSelectorPlatform.instance = previous);
    await _mount(t, api, thread: null, cwd: '/alpha');
    await t.tap(find.byKey(const Key('attach-menu-btn')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('attach-file-btn')));
    await t.pump();
    await switchProject(t, '/beta');
    await t.enterText(_input, 'Beta draft');
    api.upload.complete('/host/alpha.txt');
    await t.pumpAndSettle();
    expect(find.text('alpha.txt'), findsNothing);
    await switchProject(t, '/alpha');
    expect(find.text('alpha.txt'), findsOneWidget);
    expect(_controller(t).text, isEmpty);
    await switchProject(t, '/beta');
    expect(_controller(t).text, 'Beta draft');
    expect(find.text('alpha.txt'), findsNothing);
  });

  testWidgets('a background first send adopts only its project draft', (
    t,
  ) async {
    t.view.devicePixelRatio = 1;
    t.view.physicalSize = const Size(1280, 900);
    addTearDown(t.view.reset);
    final api = _StartingApi()
      ..appThreads.add(
        const ThreadMeta(id: 'b', preview: 'Beta', cwd: '/beta', updatedAt: 0),
      );
    await _mount(t, api, thread: null, cwd: '/alpha');
    await t.enterText(_input, 'Start alpha');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pump();
    await t.enterText(_input, 'Alpha followup');
    await switchProject(t, '/beta');
    await t.enterText(_input, 'Beta draft');
    api.start.complete();
    await t.pumpAndSettle();
    expect(api.lastCwd, '/alpha');
    expect(api.sentTo, ['thread-0']);
    expect(_controller(t).text, 'Beta draft');
    await t.tap(find.byKey(const Key('conv-tile-thread-0')));
    await t.pumpAndSettle();
    expect(_controller(t).text, 'Alpha followup');
    await t.tap(find.byKey(const Key('new-conversation-btn')));
    await t.pumpAndSettle();
    expect(_controller(t).text, isEmpty);
    await switchProject(t, '/beta');
    expect(_controller(t).text, 'Beta draft');
  });

  testWidgets(
    'reopening the default project restores its existing unsent draft',
    (t) async {
      final api = FakeBridgeApi();
      api.projectConfigs[_service] = const ProjectConfig(
        defaultProject: '/alpha',
      );
      await _mount(t, api, thread: null, cwd: '/alpha');
      await t.enterText(_input, 'Remember alpha');
      await t.pumpWidget(
        host(const SizedBox(), api, locale: const Locale('en')),
      );
      await _mount(t, api, thread: null);
      expect(_controller(t).text, 'Remember alpha');
      expect(find.text('alpha'), findsWidgets);
    },
  );

  testWidgets('growing the composer preserves the message being read', (
    t,
  ) async {
    t.view.devicePixelRatio = 1;
    t.view.physicalSize = const Size(390, 844);
    addTearDown(t.view.reset);
    final api = FakeBridgeApi()
      ..readResult = ThreadHistory(
        items: List.generate(
          80,
          (i) => ThreadItem(
            id: 'u$i',
            itemType: 'userMessage',
            title: '',
            text: 'History message $i',
          ),
        ),
        running: false,
      );
    await _mount(t, api);
    final layer = find.byKey(const Key('chat-conversation-layer'));
    await t.drag(layer, const Offset(0, 400));
    await t.pumpAndSettle();
    final viewport = t.getRect(layer);
    final visible = find
        .textContaining('History message')
        .evaluate()
        .where((e) {
          final rect = t.getRect(find.byWidget(e.widget));
          return rect.top > viewport.top && rect.bottom < viewport.bottom;
        })
        .first
        .widget;
    final text = visible as Text;
    final anchorText = text.data ?? text.textSpan!.toPlainText();
    final anchor = find.byWidgetPredicate(
      (w) => w is Text && (w.data ?? w.textSpan?.toPlainText()) == anchorText,
    );
    final top = t.getTopLeft(anchor).dy;
    await t.enterText(_input, List.filled(20, 'A growing draft').join('\n'));
    await t.pumpAndSettle();
    expect(t.getTopLeft(anchor).dy, closeTo(top, 1));
    expect(t.takeException(), isNull);
  });

  testWidgets(
    'draft text and selection follow conversations and survive navigation',
    (t) async {
      t.view.devicePixelRatio = 1;
      t.view.physicalSize = const Size(1280, 900);
      addTearDown(t.view.reset);
      final api = FakeBridgeApi()
        ..appThreads.addAll([
          const ThreadMeta(
            id: 'a',
            preview: 'Alpha',
            cwd: '/project',
            updatedAt: 0,
          ),
          const ThreadMeta(
            id: 'b',
            preview: 'Beta',
            cwd: '/project',
            updatedAt: 0,
          ),
        ]);
      await _mount(t, api);
      await t.enterText(_input, 'Unfinished alpha');
      const selection = TextSelection(baseOffset: 3, extentOffset: 8);
      _controller(t).selection = selection;
      await t.pump();
      expect(
        find.descendant(
          of: find.byKey(const Key('conv-tile-a')),
          matching: find.textContaining('Draft'),
        ),
        findsOneWidget,
      );
      await t.tap(find.byKey(const Key('conv-tile-b')));
      await t.pumpAndSettle();
      expect(_controller(t).text, isEmpty);
      await t.enterText(_input, 'Unfinished beta');
      await t.tap(find.byKey(const Key('conv-tile-a')));
      await t.pumpAndSettle();
      expect(_controller(t).text, 'Unfinished alpha');
      expect(_controller(t).selection, selection);
      await t.pumpWidget(
        host(const SizedBox(), api, locale: const Locale('en')),
      );
      await _mount(t, api);
      expect(_controller(t).text, 'Unfinished alpha');
      await _mount(t, api, service: 'pcx:other:app:default');
      expect(
        _controller(t).text,
        isEmpty,
        reason: 'Hosts must not share drafts.',
      );
      await _mount(t, api);
      expect(_controller(t).text, 'Unfinished alpha');
      expect(t.takeException(), isNull);
    },
  );

  testWidgets(
    'a pending first send keeps its original conversation and draft',
    (t) async {
      t.view.devicePixelRatio = 1;
      t.view.physicalSize = const Size(1280, 900);
      addTearDown(t.view.reset);
      final api = _StartingApi()
        ..appThreads.add(
          const ThreadMeta(
            id: 'b',
            preview: 'Beta',
            cwd: '/project',
            updatedAt: 0,
          ),
        );
      await _mount(t, api, thread: null);
      await t.enterText(_input, 'First message');
      await t.pump();
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pump();
      expect(api.starting, isTrue);
      await t.enterText(_input, 'Next draft for the new conversation');
      await t.tap(find.byKey(const Key('conv-tile-b')));
      await t.pumpAndSettle();
      await t.enterText(_input, 'Unfinished beta');
      api.start.complete();
      await t.pumpAndSettle();
      expect(api.sentTo, ['thread-0']);
      expect(_controller(t).text, 'Unfinished beta');
      // A second send must still target the conversation the user selected.
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pumpAndSettle();
      expect(api.sentTo, ['thread-0', 'b']);
      await t.enterText(_input, 'Keep beta');
      await t.tap(find.byKey(const Key('conv-tile-thread-0')));
      await t.pumpAndSettle();
      expect(_controller(t).text, 'Next draft for the new conversation');
      await t.tap(find.byKey(const Key('conv-tile-b')));
      await t.pumpAndSettle();
      expect(_controller(t).text, 'Keep beta');
      expect(t.takeException(), isNull);
    },
  );

  testWidgets(
    'returning to a pending new conversation adopts its created thread',
    (t) async {
      t.view.devicePixelRatio = 1;
      t.view.physicalSize = const Size(1280, 900);
      addTearDown(t.view.reset);
      final api = _StartingApi()
        ..appThreads.add(
          const ThreadMeta(
            id: 'b',
            preview: 'Beta',
            cwd: '/project',
            updatedAt: 0,
          ),
        );
      await _mount(t, api, thread: null, cwd: '/project');
      await t.enterText(_input, 'First message');
      await t.pump();
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pump();
      expect(api.starting, isTrue);
      await t.enterText(_input, 'Next message');
      await t.tap(find.byKey(const Key('conv-tile-b')));
      await t.pumpAndSettle();
      await t.tap(find.byKey(const Key('new-conversation-btn')));
      await t.pumpAndSettle();
      api.start.complete();
      await t.pumpAndSettle();
      expect(_controller(t).text, 'Next message');
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pumpAndSettle();
      expect(api.sentTo, ['thread-0', 'thread-0']);
      expect(t.takeException(), isNull);
    },
  );

  testWidgets(
    'a failed background first send restores only its original draft',
    (t) async {
      t.view.devicePixelRatio = 1;
      t.view.physicalSize = const Size(1280, 900);
      addTearDown(t.view.reset);
      final api = _StartingApi()
        ..appThreads.add(
          const ThreadMeta(
            id: 'b',
            preview: 'Beta',
            cwd: '/project',
            updatedAt: 0,
          ),
        );
      await _mount(t, api, thread: null, cwd: '/project');
      await t.enterText(_input, 'First message');
      await t.pump();
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pump();
      expect(api.starting, isTrue);
      await t.enterText(_input, 'Next draft');
      await t.tap(find.byKey(const Key('conv-tile-b')));
      await t.pumpAndSettle();
      await t.enterText(_input, 'Unfinished beta');
      api.start.completeError(StateError('Thread creation failed'));
      await t.pumpAndSettle();
      expect(_controller(t).text, 'Unfinished beta');
      expect(api.sentTo, isEmpty);
      await t.tap(find.byKey(const Key('new-conversation-btn')));
      await t.pumpAndSettle();
      expect(_controller(t).text, 'First message\n\nNext draft');
      expect(
        t.widget<IconButton>(find.byKey(const Key('send-btn'))).onPressed,
        isNotNull,
      );
      expect(t.takeException(), isNull);
    },
  );

  for (final returnBeforeAck in [false, true]) {
    testWidgets(
      'accepted supplement clears only its original draft (return before ack: $returnBeforeAck)',
      (t) async {
        t.view.devicePixelRatio = 1;
        t.view.physicalSize = const Size(1280, 900);
        addTearDown(t.view.reset);
        final api = FakeBridgeApi()
          ..steerGate = Completer<void>()
          ..readResult = const ThreadHistory(items: [], running: true)
          ..appThreads.addAll([
            const ThreadMeta(
              id: 'a',
              preview: 'Alpha',
              cwd: '/project',
              updatedAt: 0,
            ),
            const ThreadMeta(
              id: 'b',
              preview: 'Beta',
              cwd: '/project',
              updatedAt: 0,
            ),
          ]);
        final previous = fsel.FileSelectorPlatform.instance;
        final selector = FakeFileSelector()
          ..files = [
            MemXFile(Uint8List.fromList([1, 2]), 'sent.txt'),
          ];
        fsel.FileSelectorPlatform.instance = selector;
        addTearDown(() => fsel.FileSelectorPlatform.instance = previous);
        await _mount(t, api, settle: false);
        await t.tap(find.byKey(const Key('attach-menu-btn')));
        await _frames(t);
        await t.tap(find.byKey(const Key('attach-file-btn')));
        await _frames(t);
        await t.tap(find.byKey(const Key('supplement-toggle')));
        await t.enterText(_input, 'Accepted supplement');
        await t.tap(find.byKey(const Key('send-btn')));
        await t.pump();
        await t.tap(find.byKey(const Key('conv-tile-b')));
        await _frames(t);
        await t.enterText(_input, 'Unfinished beta');
        if (returnBeforeAck) {
          await t.tap(find.byKey(const Key('conv-tile-a')));
          await _frames(t);
          await t.enterText(_input, 'Newer alpha draft');
          selector.files = [
            MemXFile(Uint8List.fromList([3]), 'new.txt'),
          ];
          await t.tap(find.byKey(const Key('attach-menu-btn')));
          await _frames(t);
          await t.tap(find.byKey(const Key('attach-file-btn')));
          await _frames(t);
        }
        api.steerGate!.complete();
        await _frames(t);
        if (!returnBeforeAck) {
          expect(_controller(t).text, 'Unfinished beta');
          await t.tap(find.byKey(const Key('conv-tile-a')));
          await _frames(t);
        }
        expect(
          _controller(t).text,
          returnBeforeAck ? 'Newer alpha draft' : isEmpty,
        );
        expect(find.text('sent.txt'), findsNothing);
        if (returnBeforeAck) expect(find.text('new.txt'), findsOneWidget);
        await t.tap(find.byKey(const Key('conv-tile-b')));
        await _frames(t);
        expect(_controller(t).text, 'Unfinished beta');
        expect(t.takeException(), isNull);
      },
    );
  }

  testWidgets(
    'a pending picker survives leaving and returning to an empty draft',
    (t) async {
      t.view.devicePixelRatio = 1;
      t.view.physicalSize = const Size(1280, 900);
      addTearDown(t.view.reset);
      final api = FakeBridgeApi()
        ..appThreads.addAll([
          const ThreadMeta(
            id: 'a',
            preview: 'Alpha',
            cwd: '/project',
            updatedAt: 0,
          ),
          const ThreadMeta(
            id: 'b',
            preview: 'Beta',
            cwd: '/project',
            updatedAt: 0,
          ),
        ]);
      final previous = fsel.FileSelectorPlatform.instance;
      final selector = _DelayedFileSelector();
      fsel.FileSelectorPlatform.instance = selector;
      addTearDown(() => fsel.FileSelectorPlatform.instance = previous);
      await _mount(t, api);
      await t.tap(find.byKey(const Key('attach-menu-btn')));
      await t.pumpAndSettle();
      await t.tap(find.byKey(const Key('attach-file-btn')));
      await t.pumpAndSettle();
      await t.tap(find.byKey(const Key('conv-tile-b')));
      await t.pumpAndSettle();
      await t.tap(find.byKey(const Key('conv-tile-a')));
      await t.pumpAndSettle();
      selector.selection.complete([
        MemXFile(Uint8List.fromList([1, 2]), 'late.txt'),
      ]);
      await t.pumpAndSettle();
      expect(find.text('late.txt'), findsOneWidget);
      expect(
        t.widget<IconButton>(find.byKey(const Key('send-btn'))).onPressed,
        isNotNull,
      );
      await t.enterText(_input, 'Keep this attachment');
      await t.pumpWidget(
        host(const SizedBox(), api, locale: const Locale('en')),
      );
      await _mount(t, api);
      expect(find.text('late.txt'), findsOneWidget);
      expect(_controller(t).text, 'Keep this attachment');
      expect(t.takeException(), isNull);
    },
  );

  testWidgets(
    'an upload finishes in its original draft after screen navigation',
    (t) async {
      final api = _UploadingApi();
      final previous = fsel.FileSelectorPlatform.instance;
      final selector = FakeFileSelector()
        ..files = [
          MemXFile(Uint8List.fromList([1, 2]), 'draft.txt'),
        ];
      fsel.FileSelectorPlatform.instance = selector;
      addTearDown(() => fsel.FileSelectorPlatform.instance = previous);
      await _mount(t, api);
      await t.tap(find.byKey(const Key('attach-menu-btn')));
      await t.pumpAndSettle();
      await t.tap(find.byKey(const Key('attach-file-btn')));
      await t.pump();
      expect(find.byKey(const Key('attachment-0')), findsOneWidget);
      await t.pumpWidget(
        host(const SizedBox(), api, locale: const Locale('en')),
      );
      api.upload.complete('/host/draft.txt');
      await t.pump();
      await _mount(t, api);
      expect(find.text('draft.txt'), findsOneWidget);
      expect(
        t.widget<IconButton>(find.byKey(const Key('send-btn'))).onPressed,
        isNotNull,
      );
      expect(t.takeException(), isNull);
    },
  );

  testWidgets(
    'desktop Enter sends, Shift+Enter adds a line, IME Enter never sends',
    (t) async {
      final api = FakeBridgeApi();
      await _mount(t, api);
      await t.enterText(_input, 'First line');
      await t.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
      await t.sendKeyEvent(LogicalKeyboardKey.enter);
      await t.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
      await t.pump();
      // The engine/IME, absent from widget tests, delivers newline text.
      expect(
        t.widget<TextField>(_input).textInputAction,
        TextInputAction.newline,
      );
      t.testTextInput.updateEditingValue(
        const TextEditingValue(
          text: 'First line\n',
          selection: TextSelection.collapsed(offset: 11),
        ),
      );
      expect(_controller(t).text, 'First line\n');
      expect(api.lastTurnText, isNull);
      t.testTextInput.updateEditingValue(
        const TextEditingValue(
          text: 'ni',
          selection: TextSelection.collapsed(offset: 2),
          composing: TextRange(start: 0, end: 2),
        ),
      );
      await t.sendKeyEvent(LogicalKeyboardKey.enter);
      await t.pump();
      expect(api.lastTurnText, isNull);
      await t.enterText(_input, 'Confirmed 你好');
      await t.sendKeyEvent(LogicalKeyboardKey.enter);
      await t.pumpAndSettle();
      expect(api.lastTurnText, 'Confirmed 你好');
      expect(_controller(t).text, isEmpty);
    },
    variant: const TargetPlatformVariant({TargetPlatform.windows}),
  );

  testWidgets(
    'mobile keyboard uses newline and the button sends',
    (t) async {
      final api = FakeBridgeApi();
      await _mount(t, api);
      expect(
        t.widget<TextField>(_input).textInputAction,
        TextInputAction.newline,
      );
      await t.enterText(_input, 'First\nSecond');
      await t.testTextInput.receiveAction(TextInputAction.newline);
      await t.pump();
      expect(api.lastTurnText, isNull);
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pumpAndSettle();
      expect(api.lastTurnText, 'First\nSecond');
    },
    variant: const TargetPlatformVariant({TargetPlatform.android}),
  );

  testWidgets(
    'queue action and stop coexist; queue flushing preserves a newer draft',
    (t) async {
      final api = FakeBridgeApi()..autoCompleteTurn = false;
      t.view.devicePixelRatio = 1;
      t.view.physicalSize = const Size(360, 800);
      addTearDown(t.view.reset);
      await _mount(t, api);
      await t.enterText(_input, 'First');
      await t.pump();
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pump();
      expect(find.byKey(const Key('stop-btn')), findsOneWidget);
      await t.enterText(_input, 'Second');
      await t.pump();
      expect(
        t.widget<IconButton>(find.byKey(const Key('send-btn'))).tooltip,
        'Queue for next turn',
      );
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pump();
      expect(find.byKey(const Key('queued-0')), findsOneWidget);
      expect(api.lastTurnText, 'First');
      await t.enterText(_input, 'Still editing the third');
      api.pushEvent(
        _service,
        const AppEvent(kind: 'turn/completed', threadId: 'a', raw: '{}'),
      );
      await t.pump();
      expect(api.lastTurnText, 'Second');
      expect(_controller(t).text, 'Still editing the third');
      expect(find.byKey(const Key('queued-0')), findsNothing);
      expect(t.takeException(), isNull);
    },
  );

  testWidgets(
    'expanded editor preserves selection and edits without sending',
    (t) async {
      final api = FakeBridgeApi();
      await _mount(t, api);
      await t.enterText(_input, 'A longer message');
      const selection = TextSelection(baseOffset: 2, extentOffset: 8);
      _controller(t).selection = selection;
      await t.tap(find.byKey(const Key('composer-expand')));
      await t.pumpAndSettle();
      final expanded = find.byKey(const Key('composer-expanded-input'));
      final controller = t.widget<TextField>(expanded).controller!;
      expect(controller.selection, selection);
      await t.enterText(expanded, 'Edited in a bigger space');
      await t.sendKeyEvent(LogicalKeyboardKey.enter);
      await t.pump();
      expect(api.lastTurnText, isNull);
      expect(
        t.widget<TextField>(expanded).textInputAction,
        TextInputAction.newline,
      );
      t.testTextInput.updateEditingValue(
        const TextEditingValue(
          text: 'Edited in a bigger space\n',
          selection: TextSelection.collapsed(offset: 24),
        ),
      );
      final editedSelection = controller.selection;
      await t.tap(find.byKey(const Key('composer-editor-done')));
      await t.pumpAndSettle();
      expect(_controller(t).text, 'Edited in a bigger space\n');
      expect(_controller(t).selection, editedSelection);
      expect(t.widget<TextField>(_input).focusNode!.hasFocus, isTrue);
      expect(t.takeException(), isNull);
    },
    variant: const TargetPlatformVariant({TargetPlatform.windows}),
  );

  testWidgets(
    'visible height reset keeps the draft and resumes compact automatic sizing',
    (t) async {
      final api = FakeBridgeApi();
      await _mount(t, api);
      final area = find.byKey(const Key('composer-input-area'));
      final initialHeight = t.getSize(area).height;
      await t.drag(
        find.byKey(const Key('composer-resize-handle')),
        const Offset(0, -100),
      );
      await t.pumpAndSettle();
      expect(t.getSize(area).height, greaterThan(initialHeight));
      await t.enterText(_input, 'Keep this');
      await t.tap(find.byKey(const Key('composer-reset-height')));
      await t.pumpAndSettle();
      expect(t.getSize(area).height, initialHeight);
      expect(_controller(t).text, 'Keep this');
      expect(find.byKey(const Key('composer-reset-height')), findsNothing);
    },
  );
}
