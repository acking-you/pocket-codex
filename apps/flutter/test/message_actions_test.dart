import 'package:file_selector_platform_interface/file_selector_platform_interface.dart'
    as fsel;
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/attachment_refs.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/ide_context.dart';
import 'package:pocket_codex/src/screens/app_session/transcript_model.dart';
import 'package:pocket_codex/src/screens/app_session/message_editor.dart';
import 'package:pocket_codex/src/screens/app_session/transcript_view.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/screens/local_session_view_screen.dart';
import 'package:pocket_codex/src/theme.dart';

import 'fake_bridge_api.dart';
import 'support/screen_harness.dart';

const _prompt = 'Please simplify this map.\nKeep only our route.';
const _shareChannel = MethodChannel('dev.fluttercommunity.plus/share');
final _copy = find.byKey(const Key('message-action-copy'));
final _edit = find.byKey(const Key('message-action-edit'));
final _select = find.byKey(const Key('message-action-select'));
final _share = find.byKey(const Key('message-action-share'));
final _input = find.byKey(const Key('composer-input'));

Future<void> _mount(
  WidgetTester t, {
  Size size = const Size(390, 844),
  TargetPlatform platform = TargetPlatform.android,
  Brightness brightness = Brightness.light,
  TranscriptItem? item,
  ValueChanged<String>? onEdit,
  double textScale = 1,
  Locale locale = const Locale('en'),
  Widget? message,
  ValueChanged<String?>? onSelectionChanged,
}) async {
  t.view.devicePixelRatio = 1;
  t.view.physicalSize = size;
  addTearDown(t.view.reset);
  await t.pumpWidget(
    MaterialApp(
      locale: locale,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      theme: (brightness == Brightness.light ? lightTheme() : darkTheme())
          .copyWith(platform: platform),
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(context).copyWith(
          textScaler: TextScaler.linear(textScale),
          padding: const EdgeInsets.only(top: 44, bottom: 24),
        ),
        child: child!,
      ),
      home: Scaffold(
        body: SafeArea(
          child: SelectionArea(
            onSelectionChanged: (selection) =>
                onSelectionChanged?.call(selection?.plainText),
            child: ListView(
              padding: const EdgeInsets.all(16),
              children: [
                const SizedBox(height: 100),
                message ??
                    MessageView(
                      item:
                          item ??
                          TranscriptItem(
                            id: 'prompt',
                            type: 'userMessage',
                            text: _prompt,
                          ),
                      onEdit: onEdit,
                    ),
                const SizedBox(height: 1000),
              ],
            ),
          ),
        ),
      ),
    ),
  );
  await t.pumpAndSettle();
}

Future<void> _open(WidgetTester t, [String text = _prompt]) async {
  await t.longPress(find.text(text));
  await t.pumpAndSettle();
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final clipboard = <String>[];
  final shares = <Map<Object?, Object?>>[];
  var failShare = false;

  setUp(() {
    AppSessionScreen.debugResetThreadMemory();
    clipboard.clear();
    shares.clear();
    failShare = false;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
      ..setMockMethodCallHandler(SystemChannels.platform, (call) async {
        if (call.method == 'Clipboard.setData') {
          clipboard.add((call.arguments as Map)['text'] as String);
        }
        return null;
      })
      ..setMockMethodCallHandler(_shareChannel, (call) async {
        if (failShare) throw PlatformException(code: 'unavailable');
        shares.add(Map<Object?, Object?>.from(call.arguments as Map));
        return 'dev.fluttercommunity.plus/share/dismissed';
      });
  });
  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
      ..setMockMethodCallHandler(SystemChannels.platform, null)
      ..setMockMethodCallHandler(_shareChannel, null);
  });

  for (final platform in [TargetPlatform.android, TargetPlatform.iOS]) {
    testWidgets('$platform long press copies the complete message', (t) async {
      await _mount(t, platform: platform, onEdit: (_) {});
      await _open(t);
      expect(_copy, findsOneWidget);
      expect(_select, findsOneWidget);
      expect(_edit, findsOneWidget);
      expect(_share, findsOneWidget);
      for (final entry in [_copy, _select, _edit, _share]) {
        expect(t.getSize(entry).height, greaterThanOrEqualTo(48));
      }
      await t.tap(_copy);
      await t.pumpAndSettle();
      expect(clipboard, [_prompt]);
      expect(_copy, findsNothing);
      expect(t.takeException(), isNull);
    });
  }

  testWidgets('selection page supports a partial selection and closes', (
    t,
  ) async {
    await _mount(t);
    await _open(t);
    await t.tap(_select);
    await t.pumpAndSettle();
    final selectable = find.byKey(const Key('message-selectable-text'));
    expect(t.widget<SelectableText>(selectable).data, _prompt);
    await t.longPressAt(t.getTopLeft(selectable) + const Offset(20, 10));
    await t.pumpAndSettle();
    final state = t.state<EditableTextState>(find.byType(EditableText));
    final value = state.widget.controller.value;
    expect(value.selection.isCollapsed, isFalse);
    expect(
      value.selection.textInside(value.text).length,
      lessThan(_prompt.length),
    );
    await t.tap(find.byType(CloseButton));
    await t.pumpAndSettle();
    expect(selectable, findsNothing);
    expect(t.takeException(), isNull);
  });

  testWidgets(
    'an open menu cannot edit after the conversation becomes read-only',
    (t) async {
      var editable = true;
      final edits = <String>[];
      late StateSetter update;
      await _mount(
        t,
        message: StatefulBuilder(
          builder: (context, setState) {
            update = setState;
            return MessageView(
              item: TranscriptItem(
                id: 'prompt',
                type: 'userMessage',
                text: _prompt,
              ),
              onEdit: editable ? edits.add : null,
            );
          },
        ),
      );
      await _open(t);
      expect(_edit, findsOneWidget);
      update(() => editable = false);
      await t.pumpAndSettle();
      await t.tap(_edit);
      await t.pumpAndSettle();
      expect(t.takeException(), isNull);
      expect(edits, isEmpty);
    },
  );

  for (final platform in [TargetPlatform.android, TargetPlatform.iOS]) {
    testWidgets('$platform keeps mouse selection alongside touch actions', (
      t,
    ) async {
      String? selected;
      await _mount(
        t,
        platform: platform,
        onSelectionChanged: (text) => selected = text,
      );
      final start = t.getTopLeft(find.text(_prompt)) + const Offset(4, 10);
      await t.dragFrom(
        start,
        const Offset(170, 0),
        kind: PointerDeviceKind.mouse,
      );
      await t.pumpAndSettle();
      expect(selected, isNotNull);
      expect(selected, isNotEmpty);
      expect(_copy, findsNothing);
      await t.tapAt(const Offset(20, 60));
      await t.pumpAndSettle();
      selected = null;
      await _open(t);
      expect(_copy, findsOneWidget);
      expect(selected, anyOf(isNull, isEmpty));
      await t.tap(_copy);
      await t.pumpAndSettle();
      expect(clipboard, [_prompt]);
      expect(t.takeException(), isNull);
    });
  }

  testWidgets('sharing uses visible prompt text and a nonempty iPad origin', (
    t,
  ) async {
    await _mount(
      t,
      size: const Size(1024, 768),
      platform: TargetPlatform.iOS,
      item: TranscriptItem(
        id: 'prompt',
        type: 'userMessage',
        text:
            'Hidden editor context\n$kUserMessageBegin\n${appendFileRefs(_prompt, ['/work/secret.txt'])}',
      ),
    );
    final point = t.getCenter(find.text(_prompt));
    await _open(t);
    await t.tap(_share);
    await t.pumpAndSettle();
    expect(shares.single['text'], _prompt);
    expect(shares.single['originX'], point.dx);
    expect(shares.single['originY'], point.dy);
    expect(shares.single['originWidth'], greaterThan(0));
    expect(shares.single['originHeight'], greaterThan(0));
    expect(t.takeException(), isNull);
  });

  testWidgets('sharing failure allows retry without a false success', (
    t,
  ) async {
    await _mount(t);
    failShare = true;
    await _open(t);
    await t.tap(_share);
    await t.pumpAndSettle();
    expect(
      find.text('Could not open sharing. You can copy the text instead.'),
      findsOneWidget,
    );
    failShare = false;
    await _open(t);
    await t.tap(_share);
    await t.pumpAndSettle();
    expect(shares, hasLength(1));
    expect(t.takeException(), isNull);
  });

  testWidgets('replies and read-only messages do not offer editing', (t) async {
    await _mount(t);
    await _open(t);
    expect(_edit, findsNothing);
    await t.tapAt(const Offset(20, 60));
    await t.pumpAndSettle();
    await _mount(
      t,
      item: TranscriptItem(id: 'answer', type: 'agentMessage', text: 'Answer'),
      onEdit: (_) => fail('An assistant reply cannot be edited'),
    );
    await _open(t, 'Answer');
    expect(_edit, findsNothing);
    expect(find.text('Share message'), findsOneWidget);
  });

  testWidgets(
    'share origin remains on screen after rotating with the menu open',
    (t) async {
      await _mount(
        t,
        size: const Size(1024, 768),
        platform: TargetPlatform.iOS,
      );
      await _open(t);
      t.view.physicalSize = const Size(390, 844);
      await t.pumpAndSettle();
      await t.tap(_share);
      await t.pumpAndSettle();
      expect(shares.single['originX'], inInclusiveRange(0, 389));
      expect(shares.single['originY'], inInclusiveRange(0, 843));
      expect(t.takeException(), isNull);
    },
  );

  testWidgets(
    'read-only viewer copies text without offering edit or takeover',
    (t) async {
      final api = FakeBridgeApi();
      api.transcripts['thread'] = const [
        ThreadItem(id: 'u', itemType: 'userMessage', title: '', text: _prompt),
      ];
      await t.pumpWidget(
        host(
          const LocalSessionViewScreen(
            threadId: 'thread',
            serviceKey: 'pcx:host:app:default',
          ),
          api,
          locale: const Locale('en'),
        ),
      );
      await t.pumpAndSettle();
      await _open(t);
      expect(_edit, findsNothing);
      await t.tap(_copy);
      await t.pumpAndSettle();
      expect(clipboard, [_prompt]);
      expect(api.lastResumed, isNull);
      expect(api.turnStartCount, 0);
      await t.pumpWidget(const SizedBox.shrink());
      await t.pumpAndSettle();
    },
  );

  testWidgets('editor stays usable with a landscape keyboard and large text', (
    t,
  ) async {
    await _mount(t, size: const Size(844, 390), textScale: 2);
    t.view.viewInsets = const FakeViewPadding(bottom: 180);
    final context = t.element(find.byType(MessageView));
    final result = showDialog<String>(
      context: context,
      builder: (_) => const MessageEditor(text: _prompt, hasDraft: true),
    );
    await t.pumpAndSettle();
    final editor = find.byKey(const Key('message-editor-input'));
    expect(t.getSize(editor).height, greaterThan(0));
    await t.enterText(editor, 'New text');
    await t.tap(find.byKey(const Key('message-editor-add')));
    await t.pumpAndSettle();
    expect(await result, 'New text');
    expect(t.takeException(), isNull);
  });

  testWidgets('a scroll gesture never opens the menu', (t) async {
    await _mount(t);
    await t.drag(find.text(_prompt), const Offset(0, -150));
    await t.pumpAndSettle();
    expect(_copy, findsNothing);
  });

  testWidgets('desktop preserves hover copy and drag selection', (t) async {
    await _mount(
      t,
      platform: TargetPlatform.linux,
      size: const Size(1280, 900),
    );
    final mouse = await t.createGesture(kind: PointerDeviceKind.mouse);
    await mouse.addPointer(location: Offset.zero);
    await mouse.moveTo(t.getCenter(find.text(_prompt)));
    await t.pumpAndSettle();
    await t.tap(find.byTooltip('Copy'));
    await t.pumpAndSettle();
    expect(clipboard, [_prompt]);
    expect(_copy, findsNothing);
    await mouse.removePointer();
    final start = t.getTopLeft(find.text(_prompt)) + const Offset(5, 10);
    await t.dragFrom(
      start,
      const Offset(170, 0),
      kind: PointerDeviceKind.mouse,
    );
    await t.pumpAndSettle();
    expect(t.takeException(), isNull);
  });

  for (final brightness in Brightness.values) {
    for (final size in [
      const Size(320, 568),
      const Size(844, 390),
      const Size(1024, 768),
    ]) {
      testWidgets('menu fits $size $brightness with large text', (t) async {
        await _mount(
          t,
          size: size,
          brightness: brightness,
          textScale: 2,
          locale: const Locale('zh'),
          onEdit: (_) {},
          item: TranscriptItem(
            id: 'prompt',
            type: 'userMessage',
            text: '只画我们走的路线。',
            turnCompletedAt: DateTime.now().millisecondsSinceEpoch ~/ 1000,
          ),
        );
        await _open(t, '只画我们走的路线。');
        await t.ensureVisible(_share);
        await t.pumpAndSettle();
        final rect = t.getRect(_share);
        expect(rect.left, greaterThanOrEqualTo(8));
        expect(rect.right, lessThanOrEqualTo(size.width - 8));
        expect(rect.bottom, lessThanOrEqualTo(size.height - 24));
        expect(t.takeException(), isNull);
      });
    }
  }

  testWidgets(
    'editing appends to the current draft without sending or changing history',
    (t) async {
      t.view.devicePixelRatio = 1;
      t.view.physicalSize = const Size(390, 844);
      addTearDown(t.view.reset);
      final api = FakeBridgeApi()
        ..readResult = const ThreadHistory(
          running: false,
          items: [
            ThreadItem(
              id: 'u',
              itemType: 'userMessage',
              title: '',
              text: _prompt,
            ),
          ],
        );
      await api.appConnect('pcx:host:app:default', 28080);
      await t.pumpWidget(
        host(
          const AppSessionScreen(
            serviceKey: 'pcx:host:app:default',
            threadId: 'a',
          ),
          api,
          locale: const Locale('en'),
        ),
      );
      await t.pumpAndSettle();
      await t.enterText(_input, 'Existing draft');
      final previousSelector = fsel.FileSelectorPlatform.instance;
      fsel.FileSelectorPlatform.instance = FakeFileSelector()
        ..files = [
          MemXFile(Uint8List.fromList([1, 2]), 'keep.txt'),
        ];
      addTearDown(() => fsel.FileSelectorPlatform.instance = previousSelector);
      await t.tap(find.byKey(const Key('attach-menu-btn')));
      await t.pumpAndSettle();
      await t.tap(find.byKey(const Key('attach-file-btn')));
      await t.pumpAndSettle();
      expect(find.text('keep.txt'), findsOneWidget);
      final controller = t.widget<TextField>(_input).controller!;
      const selection = TextSelection(baseOffset: 1, extentOffset: 5);
      controller.selection = selection;
      await _open(t);
      await t.tap(_edit);
      await t.pumpAndSettle();
      final editor = find.byKey(const Key('message-editor-input'));
      expect(t.widget<TextField>(editor).controller!.text, _prompt);
      await t.enterText(editor, 'Cancelled change');
      await t.tap(find.byType(CloseButton));
      await t.pumpAndSettle();
      expect(controller.text, 'Existing draft');
      expect(controller.selection, selection);
      await _open(t);
      await t.tap(_edit);
      await t.pumpAndSettle();
      await t.enterText(editor, 'Edited prompt');
      await t.tap(find.byKey(const Key('message-editor-add')));
      await t.pumpAndSettle();
      expect(controller.text, 'Existing draft\n\nEdited prompt');
      expect(find.text('keep.txt'), findsOneWidget);
      expect(find.text(_prompt), findsOneWidget);
      expect(api.turnStartCount, 0);
      expect(t.takeException(), isNull);
      await t.pumpWidget(const SizedBox.shrink());
      await t.pumpAndSettle();
    },
  );
}
