/// The chat screen: sending turns, rendering the transcript, the composer, and
/// the panes and pickers around them.
library;

import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_linkify/flutter_linkify.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:file_selector_platform_interface/file_selector_platform_interface.dart'
    as fsel;
import 'package:image_picker_platform_interface/image_picker_platform_interface.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/attachment_refs.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/image_attachments.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/app_session/activity_cards.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/widgets/message_images.dart';
import 'package:pocket_codex/src/widgets/status_dots.dart';
import 'package:pocket_codex/src/widgets/turn_minimap.dart';

import '../fake_bridge_api.dart';
import '../support/screen_harness.dart';

void main() {
  // AppSessionScreen keeps per-thread plan/effort memory in process-wide static
  // maps (so a reopened thread restores its mode before the persisted config
  // lands). Reset it between tests so memory from one test can't leak into
  // another that reuses a thread id.
  setUp(AppSessionScreen.debugResetThreadMemory);

  testWidgets('App session sends a turn and renders the streamed reply', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // Narrow so the sessions pane is a hidden drawer — the new session's preview
    // (also "hello") then can't collide with the transcript bubble below.
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(400, 800);
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // A brand-new conversation shows the guidance view (not a bare hint).
    expect(find.text('我们该构建什么?'), findsOneWidget);

    await t.enterText(find.byType(TextField), 'hello');
    await t.pump(); // let the send button enable for the non-empty input
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();

    // User bubble (plain Text) + the app bar, which titles the conversation
    // with its preview — the same text. The agent reply renders as Markdown
    // (RichText), so match with findRichText.
    expect(find.text('hello'), findsNWidgets(2));
    expect(
      find.textContaining('echo: hello', findRichText: true),
      findsOneWidget,
    );
  });

  testWidgets('composer is one row on a 360 px phone, with 44 px targets', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(360, 780); // a small phone
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // Everything the composer offers is on screen at once — no expand toggle,
    // no wrapping, nothing pushed off the right edge.
    for (final k in ['attach-menu-btn', 'permission-chip', 'model-chip']) {
      final rect = t.getRect(find.byKey(Key(k)));
      expect(rect.right, lessThanOrEqualTo(360.0), reason: k);
      expect(rect.height, greaterThanOrEqualTo(44.0), reason: k);
    }
    expect(
      t.getRect(find.byKey(const Key('send-btn'))).right,
      lessThanOrEqualTo(360.0),
    );
    // The three rows share one line.
    final y = t.getRect(find.byKey(const Key('model-chip'))).center.dy;
    expect(t.getRect(find.byKey(const Key('permission-chip'))).center.dy, y);
    expect(t.takeException(), isNull); // no RenderFlex overflow
  });

  testWidgets('the attachment menu holds all three sources', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    await t.tap(find.byKey(const Key('attach-menu-btn')));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('attach-btn')), findsOneWidget);
    expect(find.byKey(const Key('attach-file-btn')), findsOneWidget);
    // Host files is desktop-only (it uses the save/open dialogs); the test
    // platform is android, so it is absent.
    expect(find.byKey(const Key('host-files-btn')), findsNothing);
  });

  testWidgets('a message from an IDE client shows the request, not the wire '
      'context, and its mentioned image becomes an attachment', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(
          id: 'u1',
          itemType: 'userMessage',
          title: '',
          text:
              '# Files mentioned by the user:\n\n'
              '## shot.png: C:/Users/u/AppData/Local/Temp/shot.png\n\n'
              '## My request for Codex:\n为什么仍然黑屏',
        ),
      ],
      running: false,
    );
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'th-ide',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    expect(find.text('为什么仍然黑屏'), findsOneWidget);
    expect(find.textContaining('My request for Codex'), findsNothing);
    expect(find.textContaining('Files mentioned'), findsNothing);
    // The mentioned image is surfaced as an attachment, named by its basename.
    expect(find.textContaining('shot.png'), findsOneWidget);
  });

  testWidgets('injected fragments are dropped and a voice handoff unwrapped', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(
          id: 'u1',
          itemType: 'userMessage',
          title: '',
          text:
              '<recommended_plugins>\n- Box (box@openai)\n</recommended_plugins>',
        ),
        ThreadItem(
          id: 'u2',
          itemType: 'userMessage',
          title: '',
          text:
              '<realtime_delegation>\n  <input>切换到 Live 模式</input>\n'
              '  <transcript_delta>user: …</transcript_delta>\n</realtime_delegation>',
        ),
      ],
      running: false,
    );
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'th-frag',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // The machine-written message is gone; the spoken one shows what was said.
    expect(find.textContaining('recommended_plugins'), findsNothing);
    expect(find.textContaining('transcript_delta'), findsNothing);
    expect(find.text('切换到 Live 模式'), findsOneWidget);
  });

  testWidgets('a reasoning card renders its summary as markdown', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(
          id: 'r1',
          itemType: 'reasoning',
          title: '',
          text: '**Planning emulator restart**\n\nChecking the GPU flag.',
        ),
      ],
      running: false,
    );
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'th-reason',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    await openTurnWork(t);

    // The collapsed peek shows the header with its markup removed.
    expect(find.text('Planning emulator restart'), findsOneWidget);
    expect(find.textContaining('**'), findsNothing);
  });

  testWidgets(
    'turn-navigation controls appear with ≥2 turns and are tappable',
    (t) async {
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect('pcx:lb7666:app:default', 28080);
      // A resumed thread with two user turns.
      api.readResult = const ThreadHistory(
        items: [
          ThreadItem(
            id: 'u1',
            itemType: 'userMessage',
            title: '',
            text: 'first',
          ),
          ThreadItem(
            id: 'a1',
            itemType: 'agentMessage',
            title: '',
            text: 'reply one',
          ),
          ThreadItem(
            id: 'u2',
            itemType: 'userMessage',
            title: '',
            text: 'second',
          ),
          ThreadItem(
            id: 'a2',
            itemType: 'agentMessage',
            title: '',
            text: 'reply two',
          ),
        ],
        running: false,
      );
      t.view.devicePixelRatio = 1.0;
      t.view.physicalSize = const Size(400, 800);
      addTearDown(t.view.reset);
      await t.pumpWidget(
        host(
          const AppSessionScreen(
            serviceKey: 'pcx:lb7666:app:default',
            threadId: 'th-nav',
          ),
          api,
        ),
      );
      await t.pumpAndSettle();

      // Two turns ⇒ prev/next-turn jumps are offered.
      expect(find.byKey(const Key('nav-prev-turn')), findsOneWidget);
      expect(find.byKey(const Key('nav-next-turn')), findsOneWidget);
      // They drive the list controller; tapping must not throw even when the
      // short transcript isn't scrollable.
      await t.tap(find.byKey(const Key('nav-next-turn')));
      await t.pumpAndSettle();
      await t.tap(find.byKey(const Key('nav-prev-turn')));
      await t.pumpAndSettle();
      // Still present and functional after use (and the taps threw nothing —
      // pumpAndSettle would have surfaced any exception).
      expect(find.byKey(const Key('nav-prev-turn')), findsOneWidget);
      expect(find.byKey(const Key('nav-next-turn')), findsOneWidget);
    },
  );

  group('runtime config visibility', () {
    const key = 'pcx:lb7666:app:default';

    testWidgets('resume adopts the server-reported model + permissions: the '
        'active-model chip and the pills show server truth', (t) async {
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect(key, 28080);
      // The resume response reported what this thread actually runs with.
      api.readResult = const ThreadHistory(
        items: [],
        running: false,
        model: 'gpt-5',
        modelProvider: 'openai',
        approvalPolicy: 'never',
        sandboxMode: 'danger-full-access',
        reasoningEffort: 'high',
      );
      await t.pumpWidget(
        host(const AppSessionScreen(serviceKey: key, threadId: 'th-1'), api),
      );
      await t.pumpAndSettle();
      // Status bar carries the active-model chip, resolved to its display name.
      final chip = find.byKey(const Key('active-model-chip'));
      expect(chip, findsOneWidget);
      expect(
        find.descendant(of: chip, matching: find.text('GPT-5')),
        findsOneWidget,
      );
      // The permission pill followed the server (never + danger-full-access =
      // 完全放行), not the local default (对话确认).
      expect(find.text('完全放行'), findsOneWidget);
      // The effort pill shows the server's sticky effort.
      expect(find.textContaining('· 高'), findsOneWidget);
      // The details sheet reports snapshot provenance (no live update yet).
      await t.tap(chip);
      await t.pumpAndSettle();
      expect(find.text('运行时配置'), findsOneWidget);
      expect(find.textContaining('来自服务器会话快照'), findsOneWidget);
    });

    testWidgets('a thread/settings/updated notification confirms a switch: '
        'chip, plan state and effort follow the server', (t) async {
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect(key, 28080);
      api.readResult = const ThreadHistory(items: [], running: false);
      await t.pumpWidget(
        host(const AppSessionScreen(serviceKey: key, threadId: 'th-2'), api),
      );
      await t.pumpAndSettle();
      // Nothing reported yet → no chip (never guess).
      expect(find.byKey(const Key('active-model-chip')), findsNothing);
      api.pushEvent(
        key,
        AppEvent(
          kind: 'thread/settings/updated',
          threadId: 'th-2',
          raw: jsonEncode({
            'threadId': 'th-2',
            'threadSettings': {
              'model': 'gpt-5.5-codex',
              'modelProvider': 'openai',
              'effort': 'xhigh',
              'approvalPolicy': 'never',
              'sandboxPolicy': {'type': 'dangerFullAccess'},
              'collaborationMode': {
                'mode': 'plan',
                'settings': {'model': 'gpt-5.5-codex'},
              },
            },
          }),
        ),
      );
      await t.pumpAndSettle();
      // The chip appears with the server-confirmed model (raw id — it isn't in
      // the cached model list, and a raw id is still truthful)...
      final chip = find.byKey(const Key('active-model-chip'));
      expect(chip, findsOneWidget);
      expect(
        find.descendant(of: chip, matching: find.text('gpt-5.5-codex')),
        findsOneWidget,
      );
      // ...the status chip flips to the server-reported plan mode...
      expect(find.text('计划模式'), findsOneWidget);
      // ...and the effort pill follows the server's sticky effort.
      expect(find.textContaining('· 极高'), findsOneWidget);
      // The details sheet reports live confirmation.
      await t.tap(chip);
      await t.pumpAndSettle();
      expect(find.textContaining('服务器已确认'), findsOneWidget);
    });

    testWidgets('each completed turn is stamped with the model that actually '
        'handled it', (t) async {
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect(key, 28080);
      t.view.devicePixelRatio = 1.0;
      t.view.physicalSize = const Size(400, 800);
      addTearDown(t.view.reset);
      await t.pumpWidget(host(const AppSessionScreen(serviceKey: key), api));
      await t.pumpAndSettle();
      // Turn 1: the server never reported and nothing explicit was sent → the
      // footnote carries no stamp (honest absence beats a guess).
      await t.enterText(find.byType(TextField), 'hello');
      await t.pump();
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pumpAndSettle();
      expect(find.textContaining('gpt-5.5-codex'), findsNothing);
      // The server then confirms the thread's effective settings...
      final tid = api.appThreads.first.id;
      api.pushEvent(
        key,
        AppEvent(
          kind: 'thread/settings/updated',
          threadId: tid,
          raw: jsonEncode({
            'threadId': tid,
            'threadSettings': {
              'model': 'gpt-5.5-codex',
              'effort': 'high',
              'approvalPolicy': 'on-request',
              'sandboxPolicy': {'type': 'workspaceWrite'},
            },
          }),
        ),
      );
      await t.pumpAndSettle();
      // ...and the next turn's footnote records what handled it. Target the
      // footnote stamp by key — the collapsed composer summary also shows the
      // active model, so a plain text match would be ambiguous.
      await t.enterText(find.byType(TextField), 'again');
      await t.pump();
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pumpAndSettle();
      final stamp = t.widget<Text>(find.byKey(const Key('turn-model-stamp')));
      expect(stamp.data, contains('gpt-5.5-codex'));
      expect(stamp.data, contains('高'));
    });
  });

  group('image attachments', () {
    late FakeImagePicker picker;
    setUp(() {
      // compute() spawns a real isolate whose completion never lands under the
      // fake test clock — run the processing pipeline inline instead.
      processImageImpl = (bytes) async => processImageBytes(bytes);
      picker = FakeImagePicker();
      ImagePickerPlatform.instance = picker;
    });
    tearDown(() {
      processImageImpl = (bytes) => compute(processImageBytes, bytes);
    });

    Future<FakeBridgeApi> pumpSession(
      WidgetTester t, {
      String? threadId,
    }) async {
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect('pcx:lb7666:app:default', 28080);
      // Narrow so the sessions pane is a hidden drawer (single TextField).
      t.view.devicePixelRatio = 1.0;
      t.view.physicalSize = const Size(400, 800);
      addTearDown(t.view.reset);
      await t.pumpWidget(
        host(
          AppSessionScreen(
            serviceKey: 'pcx:lb7666:app:default',
            threadId: threadId,
          ),
          api,
        ),
      );
      await t.pumpAndSettle();
      return api;
    }

    testWidgets('attach shows a preview chip; send carries data URLs and '
        'renders bubble thumbnails', (t) async {
      final api = await pumpSession(t);
      picker.files = [XFile.fromData(tinyPng(), name: 'shot.png')];

      await attachMenu(t, 'attach-btn');
      await t.pumpAndSettle();
      expect(find.byKey(const Key('attachment-0')), findsOneWidget);

      await t.enterText(find.byType(TextField), 'what is this?');
      await t.pump();
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pumpAndSettle();

      // The turn carried the processed image as a data URL…
      expect(api.lastTurnText, 'what is this?');
      expect(api.lastTurnImages, hasLength(1));
      expect(api.lastTurnImages.single, startsWith('data:image/png;base64,'));
      // …the optimistic bubble shows the thumbnail, and the composer strip
      // has been cleared for the next message.
      expect(find.byKey(const Key('msg-image-0')), findsOneWidget);
      expect(find.byKey(const Key('attachment-0')), findsNothing);
    });

    testWidgets('a sent message renders its image OUTSIDE the text bubble', (
      t,
    ) async {
      await pumpSession(t);
      picker.files = [XFile.fromData(tinyPng(), name: 'shot.png')];

      await attachMenu(t, 'attach-btn');
      await t.pumpAndSettle();
      await t.enterText(find.byType(TextField), 'who is this?');
      await t.pump();
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pumpAndSettle();

      final thumb = find.byKey(const Key('msg-image-0'));
      expect(thumb, findsOneWidget);
      // `find.text` also matches the top-bar conversation title (it previews
      // the first message), so scope to the linkified body in the transcript.
      final body = find.descendant(
        of: find.byType(Linkify),
        matching: find.text('who is this?'),
      );
      expect(body, findsOneWidget);
      final bubble = find
          .ancestor(of: body, matching: find.byType(Container))
          .first;
      // The image is a sibling of the bubble, not a descendant: nesting it made
      // the picture inherit the bubble's padding and background, and left an
      // image-only message as a mostly-empty bubble.
      expect(
        find.descendant(of: bubble, matching: thumb),
        findsNothing,
        reason: 'thumbnail must not live inside the text bubble',
      );
      // And it sits ABOVE the text, matching the reference layout.
      expect(t.getCenter(thumb).dy, lessThan(t.getCenter(body).dy));
    });

    testWidgets('an image-only message can be sent (no text)', (t) async {
      final api = await pumpSession(t);
      picker.files = [XFile.fromData(tinyPng(), name: 'shot.png')];

      await attachMenu(t, 'attach-btn');
      await t.pumpAndSettle();
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pumpAndSettle();

      expect(api.lastTurnText, '');
      expect(api.lastTurnImages, hasLength(1));
      expect(find.byKey(const Key('msg-image-0')), findsOneWidget);
    });

    testWidgets('a staged image opens the viewer before it is sent', (t) async {
      await pumpSession(t);
      picker.files = [
        XFile.fromData(tinyPng(), name: 'one.png'),
        XFile.fromData(tinyPng(), name: 'two.png'),
      ];

      await attachMenu(t, 'attach-btn');
      await t.pumpAndSettle();
      expect(find.byKey(const Key('attachment-0')), findsOneWidget);
      expect(find.byKey(const Key('attachment-1')), findsOneWidget);

      // Clicking the SECOND staged tile previews that one — checking what you
      // attached without having to send it first.
      await t.tap(find.byKey(const Key('attachment-1')));
      await t.pumpAndSettle();
      expect(find.byType(ImageViewerPage), findsOneWidget);
      // Both staged images are pageable, opened at the one clicked.
      expect(find.text('2/2'), findsOneWidget);

      // Closing returns to the composer with the attachments still staged.
      await t.tapAt(const Offset(40, 700));
      await t.pumpAndSettle();
      expect(find.byType(ImageViewerPage), findsNothing);
      expect(find.byKey(const Key('attachment-0')), findsOneWidget);
      expect(find.byKey(const Key('attachment-1')), findsOneWidget);
    });

    testWidgets('removing the pending attachment disables an image-only send', (
      t,
    ) async {
      await pumpSession(t);
      picker.files = [XFile.fromData(tinyPng(), name: 'shot.png')];

      await attachMenu(t, 'attach-btn');
      await t.pumpAndSettle();
      await t.tap(find.byKey(const Key('attachment-remove-0')));
      await t.pumpAndSettle();

      expect(find.byKey(const Key('attachment-0')), findsNothing);
      final btn = t.widget<IconButton>(find.byKey(const Key('send-btn')));
      expect(btn.onPressed, isNull, reason: 'nothing left to send');
    });

    testWidgets('history restores image thumbnails and tap opens the '
        'fullscreen viewer', (t) async {
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect('pcx:lb7666:app:default', 28080);
      api.readResult = ThreadHistory(
        items: [
          ThreadItem(
            id: 'u1',
            itemType: 'userMessage',
            title: '',
            text: 'look at this',
            images: [tinyPngDataUrl()],
          ),
        ],
        running: false,
      );
      t.view.devicePixelRatio = 1.0;
      t.view.physicalSize = const Size(400, 800);
      addTearDown(t.view.reset);
      await t.pumpWidget(
        host(
          const AppSessionScreen(
            serviceKey: 'pcx:lb7666:app:default',
            threadId: 'thread-7',
          ),
          api,
        ),
      );
      await t.pumpAndSettle();

      expect(find.text('look at this'), findsOneWidget);
      expect(find.byKey(const Key('msg-image-0')), findsOneWidget);

      await t.tap(find.byKey(const Key('msg-image-0')));
      await t.pumpAndSettle();
      expect(find.byKey(const Key('image-viewer-0')), findsOneWidget);
    });

    testWidgets('a host-only image (localImage path) renders a filename chip', (
      t,
    ) async {
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect('pcx:lb7666:app:default', 28080);
      api.readResult = const ThreadHistory(
        items: [
          ThreadItem(
            id: 'u1',
            itemType: 'userMessage',
            title: '',
            text: '',
            images: ['/home/user/screenshots/error.png'],
          ),
        ],
        running: false,
      );
      t.view.devicePixelRatio = 1.0;
      t.view.physicalSize = const Size(400, 800);
      addTearDown(t.view.reset);
      await t.pumpWidget(
        host(
          const AppSessionScreen(
            serviceKey: 'pcx:lb7666:app:default',
            threadId: 'thread-7',
          ),
          api,
        ),
      );
      await t.pumpAndSettle();

      // No pixels crossed the wire, and the host wouldn't serve the path — an
      // honest basename chip instead.
      expect(find.text('error.png'), findsOneWidget);
      expect(find.byKey(const Key('msg-image-0')), findsNothing);
    });

    testWidgets('a host image the host will serve renders as a thumbnail', (
      t,
    ) async {
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect('pcx:lb7666:app:default', 28080);
      // Inside the project roots, so even a host too old for the
      // transcript-image route hands the bytes over.
      api.fileBytes['/proj/shot.png'] = onePixelPng;
      api.readResult = const ThreadHistory(
        items: [
          ThreadItem(
            id: 'u1',
            itemType: 'userMessage',
            title: '',
            text: '',
            images: ['/proj/shot.png'],
          ),
        ],
        running: false,
      );
      t.view.devicePixelRatio = 1.0;
      t.view.physicalSize = const Size(400, 800);
      addTearDown(t.view.reset);
      await t.pumpWidget(
        host(
          const AppSessionScreen(
            serviceKey: 'pcx:lb7666:app:default',
            threadId: 'thread-8',
          ),
          api,
        ),
      );
      await t.pumpAndSettle();

      // The chip gave way to a real thumbnail, and it opens the viewer.
      expect(find.text('shot.png'), findsNothing);
      expect(find.byKey(const Key('msg-image-0')), findsOneWidget);
      await t.tap(find.byKey(const Key('msg-image-0')));
      await t.pumpAndSettle();
      expect(find.byKey(const Key('image-viewer-0')), findsOneWidget);
    });

    testWidgets('a temp-dir image the transcript references still renders', (
      t,
    ) async {
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect('pcx:lb7666:app:default', 28080);
      // Outside every project root — `metaReadFile` refuses it (the fake
      // returns no bytes), but the host authorises it against the thread's
      // own transcript.
      const temp = 'C:/Users/u/AppData/Local/Temp/codex-clipboard-1.png';
      api.threadImageBytes[temp] = onePixelPng;
      api.readResult = const ThreadHistory(
        items: [
          ThreadItem(
            id: 'u1',
            itemType: 'userMessage',
            title: '',
            text:
                '# Files mentioned by the user:\n\n'
                '## codex-clipboard-1.png: $temp\n\n'
                '## My request for Codex:\n为什么仍然黑屏',
          ),
        ],
        running: false,
      );
      t.view.devicePixelRatio = 1.0;
      t.view.physicalSize = const Size(400, 800);
      addTearDown(t.view.reset);
      await t.pumpWidget(
        host(
          const AppSessionScreen(
            serviceKey: 'pcx:lb7666:app:default',
            threadId: 'thread-9',
          ),
          api,
        ),
      );
      await t.pumpAndSettle();

      expect(find.text('为什么仍然黑屏'), findsOneWidget);
      expect(find.byKey(const Key('msg-image-0')), findsOneWidget);
      expect(find.text('codex-clipboard-1.png'), findsNothing);
    });
  });

  group('file attachments', () {
    late FakeFileSelector selector;
    setUp(() {
      // Inline image processing (see the image-attachments group) — a picked
      // .png routes to the image pipeline even from the file picker.
      processImageImpl = (bytes) async => processImageBytes(bytes);
      selector = FakeFileSelector();
      fsel.FileSelectorPlatform.instance = selector;
    });
    tearDown(() {
      processImageImpl = (bytes) => compute(processImageBytes, bytes);
    });

    XFile tmpFile(String name, List<int> bytes) =>
        MemXFile(Uint8List.fromList(bytes), name);

    Future<FakeBridgeApi> pumpSession(WidgetTester t) async {
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect('pcx:lb7666:app:default', 28080);
      t.view.devicePixelRatio = 1.0;
      t.view.physicalSize = const Size(400, 800);
      addTearDown(t.view.reset);
      await t.pumpWidget(
        host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
      );
      await t.pumpAndSettle();
      return api;
    }

    testWidgets('a staged FILE has no preview (no pixels to show)', (t) async {
      await pumpSession(t);
      selector.files = [tmpFile('notes.txt', utf8.encode('x'))];

      await attachMenu(t, 'attach-file-btn');
      await t.pumpAndSettle();
      expect(find.byKey(const Key('attachment-0')), findsOneWidget);

      // Clicking it is inert: a document has no pixels, so there is nothing to
      // preview and the viewer must not open on an empty list.
      await t.tap(find.byKey(const Key('attachment-0')));
      await t.pumpAndSettle();
      expect(find.byType(ImageViewerPage), findsNothing);
    });

    testWidgets('attach uploads to the host and the turn text carries the '
        'path-reference block', (t) async {
      final api = await pumpSession(t);
      selector.files = [tmpFile('notes.txt', utf8.encode('sentinel-content'))];

      await attachMenu(t, 'attach-file-btn');
      await t.pumpAndSettle();
      // Uploaded chip shows the filename.
      expect(find.byKey(const Key('attachment-0')), findsOneWidget);
      expect(find.text('notes.txt'), findsOneWidget);
      expect(api.lastUploadName, 'notes.txt');
      expect(utf8.decode(api.lastUploadBytes!), 'sentinel-content');

      await t.enterText(find.byType(TextField), 'check this');
      await t.pump();
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pumpAndSettle();

      // The wire text = typed text + the reference block with the HOST path.
      expect(
        api.lastTurnText,
        appendFileRefs('check this', ['/host/uploads/123/notes.txt']),
      );
      expect(api.lastTurnImages, isEmpty);
      // The bubble renders a chip + the typed text; the raw block is hidden.
      // (Two 'check this': the bubble and the top bar, which titles the
      // conversation with its preview.)
      expect(find.text('notes.txt'), findsOneWidget);
      expect(find.text('check this'), findsNWidgets(2));
      expect(find.textContaining(kAttachedFilesHeader), findsNothing);
      // Composer strip cleared.
      expect(find.byKey(const Key('attachment-0')), findsNothing);
    });

    testWidgets('a file-only message can be sent', (t) async {
      final api = await pumpSession(t);
      selector.files = [
        tmpFile('data.bin', [1, 2, 3]),
      ];
      await attachMenu(t, 'attach-file-btn');
      await t.pumpAndSettle();
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pumpAndSettle();

      expect(
        api.lastTurnText,
        appendFileRefs('', ['/host/uploads/123/data.bin']),
      );
      expect(find.text('data.bin'), findsOneWidget); // bubble chip
    });

    testWidgets('a failed upload removes the chip and reports the error', (
      t,
    ) async {
      final api = await pumpSession(t);
      api.uploadError = StateError('relay down');
      selector.files = [
        tmpFile('x.log', [9]),
      ];
      await attachMenu(t, 'attach-file-btn');
      await t.pumpAndSettle();

      expect(find.byKey(const Key('attachment-0')), findsNothing);
      expect(find.textContaining('上传文件到主机失败'), findsOneWidget);
      // Nothing to send: the button stays disabled without text.
      final btn = t.widget<IconButton>(find.byKey(const Key('send-btn')));
      expect(btn.onPressed, isNull);
    });

    testWidgets('an image picked through the FILE picker routes to the image '
        'pipeline', (t) async {
      final api = await pumpSession(t);
      selector.files = [tmpFile('shot.png', tinyPng())];
      await attachMenu(t, 'attach-file-btn');
      await t.pumpAndSettle();
      await t.tap(find.byKey(const Key('send-btn')));
      await t.pumpAndSettle();

      expect(api.lastTurnImages, hasLength(1));
      expect(api.lastTurnImages.single, startsWith('data:image/png;base64,'));
      expect(api.lastUploadName, isNull, reason: 'images are not uploaded');
      expect(api.lastTurnText, isEmpty);
    });

    testWidgets('history restores document chips from the reference block', (
      t,
    ) async {
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect('pcx:lb7666:app:default', 28080);
      api.readResult = ThreadHistory(
        items: [
          ThreadItem(
            id: 'u1',
            itemType: 'userMessage',
            title: '',
            text: appendFileRefs('review it', ['/host/uploads/9/report.pdf']),
          ),
        ],
        running: false,
      );
      t.view.devicePixelRatio = 1.0;
      t.view.physicalSize = const Size(400, 800);
      addTearDown(t.view.reset);
      await t.pumpWidget(
        host(
          const AppSessionScreen(
            serviceKey: 'pcx:lb7666:app:default',
            threadId: 'thread-7',
          ),
          api,
        ),
      );
      await t.pumpAndSettle();

      expect(find.text('report.pdf'), findsOneWidget); // chip label
      expect(find.text('review it'), findsOneWidget); // typed text
      expect(find.textContaining(kAttachedFilesHeader), findsNothing);
    });
  });

  testWidgets('Messages are copyable (copy button puts text on clipboard)', (
    t,
  ) async {
    final copied = <String>[];
    // Intercept the clipboard channel to capture what gets copied.
    t.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      (call) async {
        if (call.method == 'Clipboard.setData') {
          copied.add((call.arguments as Map)['text'] as String);
        }
        return null;
      },
    );
    addTearDown(
      () => t.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        null,
      ),
    );

    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();
    await t.enterText(find.byType(TextField), 'hello');
    await t.pump(); // let the send button enable for the non-empty input
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();

    // The copy action appears on hover (desktop). Hover the agent message,
    // then tap its copy icon.
    final gesture = await t.createGesture(kind: PointerDeviceKind.mouse);
    await gesture.addPointer(location: Offset.zero);
    addTearDown(() => gesture.removePointer());
    await gesture.moveTo(t.getCenter(find.byType(MarkdownBody)));
    await t.pumpAndSettle();
    await t.tap(find.byIcon(Icons.content_copy_outlined).last);
    await t.pump();
    expect(copied, isNotEmpty);
  });

  testWidgets('One turn is one block, however many items it arrives in', (
    t,
  ) async {
    final copied = <String>[];
    t.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      (call) async {
        if (call.method == 'Clipboard.setData') {
          copied.add((call.arguments as Map)['text'] as String);
        }
        return null;
      },
    );
    addTearDown(
      () => t.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        null,
      ),
    );

    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // One reply the server split across three items, the way it does when a
    // preamble precedes a tool batch and the answer follows it.
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(id: 'u1', itemType: 'userMessage', title: '', text: 'ask'),
        ThreadItem(
          id: 'a1',
          itemType: 'agentMessage',
          title: '',
          text: 'part one',
        ),
        ThreadItem(
          id: 'a2',
          itemType: 'agentMessage',
          title: '',
          text: 'part two',
        ),
        ThreadItem(
          id: 'a3',
          itemType: 'agentMessage',
          title: '',
          text: 'part three',
        ),
      ],
      running: false,
    );
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1400, 900);
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'th-merge',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // All three parts are on screen, but as ONE rendered block — not three,
    // each with its own hover actions.
    expect(find.textContaining('part one', findRichText: true), findsWidgets);
    expect(find.textContaining('part three', findRichText: true), findsWidgets);
    expect(find.byType(MarkdownBody), findsOneWidget);

    // And copying takes the whole turn, not just the part under the pointer.
    final gesture = await t.createGesture(kind: PointerDeviceKind.mouse);
    await gesture.addPointer(location: Offset.zero);
    addTearDown(() => gesture.removePointer());
    await gesture.moveTo(t.getCenter(find.byType(MarkdownBody)));
    await t.pumpAndSettle();
    await t.tap(find.byIcon(Icons.content_copy_outlined).last);
    await t.pump();
    expect(copied.single, contains('part one'));
    expect(copied.single, contains('part three'));
  });

  testWidgets('Agent replies render as Markdown (headings, not a bubble)', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/agentMessage/delta',
        threadId: 't1',
        itemId: 'a1',
        itemType: 'agentMessage',
        text: '# Title\n\nsome **bold** body',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();
    // Markdown produces RichText spans, not a Text bubble.
    expect(find.byType(MarkdownBody), findsOneWidget);
    expect(find.textContaining('Title', findRichText: true), findsWidgets);
  });

  testWidgets("A new conversation ignores another thread's events", (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // A brand-new conversation: no thread id until the first turn starts.
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // The app session is shared and another thread's turn may still be
    // streaming; its events must not be absorbed into the blank conversation.
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/agentMessage/delta',
        threadId: 'other-thread',
        itemId: 'x1',
        itemType: 'agentMessage',
        text: 'not mine',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();
    // Still the new-session guidance (the foreign event was dropped, no items).
    expect(find.text('我们该构建什么?'), findsOneWidget);
    expect(find.textContaining('not mine', findRichText: true), findsNothing);
  });

  testWidgets('A new conversation inherits the last-picked permission mode', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // The default permission mode is "自动" (auto). Switch it to "只读".
    await t.tap(find.text('自动'));
    await t.pumpAndSettle();
    await t.tap(find.text('只读'));
    await t.pumpAndSettle();
    expect(find.text('只读'), findsOneWidget); // the pill now reads read-only

    // Start a brand-new conversation: it inherits the read-only mode the user
    // last chose instead of resetting to the "自动" default.
    await t.tap(find.byKey(const Key('new-conversation-btn')));
    await t.pumpAndSettle();
    expect(find.text('只读'), findsOneWidget);
    expect(find.text('自动'), findsNothing);
  });

  testWidgets('A new session appears in the sessions pane after first send', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // No conversations in the pane yet.
    expect(find.text('暂无会话'), findsOneWidget); // noThreads (zh)

    // Sending the first message surfaces the new session in the left pane.
    await t.enterText(find.byType(TextField), 'hello there');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();

    expect(find.text('暂无会话'), findsNothing);
    // The new session shows in the pane as a conversation tile, with its message
    // preserved as the preview (not "(未命名)" — the server preview is still
    // empty for a just-started thread, so the optimistic one must win).
    expect(convTiles(), findsOneWidget);
    expect(find.text('(未命名)'), findsNothing);
  });

  testWidgets('Empty-state title is the project switcher', (t) async {
    final nowS = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.addAll([
      ThreadMeta(
        id: 'a1',
        preview: 'a',
        cwd: '/work/alpha',
        updatedAt: nowS - 60,
      ),
      ThreadMeta(
        id: 'b1',
        preview: 'b',
        cwd: '/work/beta',
        updatedAt: nowS - 600,
      ),
    ]);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900);
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          cwd: '/work/alpha',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // The headline names the project inline, and that name IS the trigger.
    expect(find.text('alpha'), findsWidgets);
    await t.tap(find.byKey(const Key('project-switcher-btn')));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('project-menu-search')), findsOneWidget);
    expect(find.text('不在项目中工作'), findsOneWidget); // workOutsideProject (zh)
    expect(find.byKey(const Key('project-menu-new')), findsOneWidget);

    // Switching project retargets the conversation that hasn't started yet.
    await t.tap(find.byKey(const Key('project-menu-item-/work/beta')));
    await t.pumpAndSettle();
    expect(find.text('beta'), findsWidgets);
  });

  testWidgets('Model chip opens a desktop popover with an effort slider', (
    t,
  ) async {
    // The popover is the desktop treatment; a phone keeps the sheet flow.
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900);
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    await t.tap(find.byKey(const Key('model-chip')));
    await t.pumpAndSettle();
    // Everything on one surface: models, the effort scale, and plan mode —
    // rather than three levels of sheet.
    expect(find.byKey(const Key('model-menu-item-gpt-5.5')), findsOneWidget);
    expect(find.byKey(const Key('model-menu-item-gpt-5')), findsOneWidget);
    expect(find.byKey(const Key('effort-steps')), findsOneWidget);
    expect(find.byKey(const Key('plan-toggle-row')), findsOneWidget);

    // Tapping the right end of the stepped selector sets the top level (gpt-5.5
    // advertises low/medium/high/xhigh → xhigh). The label reflects it.
    final steps = t.getRect(find.byKey(const Key('effort-steps')));
    await t.tapAt(Offset(steps.right - 4, steps.center.dy));
    await t.pumpAndSettle();
    expect(find.text('极高'), findsWidgets); // effortXhigh (zh)

    // Picking a model updates the chip without leaving the popover flow.
    await t.tap(find.byKey(const Key('model-menu-item-gpt-5')));
    await t.pumpAndSettle();
    expect(find.textContaining('GPT-5'), findsWidgets);
    // Must be cleared inside the body: the framework's end-of-test invariant
    // check runs before addTearDown callbacks.
    debugDefaultTargetPlatformOverride = null;
  });

  testWidgets('Home pane groups conversations under their project', (t) async {
    final nowS = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // Two projects, interleaved in time — grouping must beat pure recency.
    api.appThreads.addAll([
      ThreadMeta(
        id: 'a1',
        preview: 'alpha newest',
        cwd: '/work/alpha',
        updatedAt: nowS - 60,
      ),
      ThreadMeta(
        id: 'b1',
        preview: 'beta middle',
        cwd: '/work/beta',
        updatedAt: nowS - 3600,
      ),
      ThreadMeta(
        id: 'a2',
        preview: 'alpha oldest',
        cwd: '/work/alpha',
        updatedAt: nowS - 5 * 86400,
      ),
    ]);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          home: true,
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // A heading per project, not per time bucket.
    expect(find.text('alpha'), findsOneWidget);
    expect(find.text('beta'), findsOneWidget);
    expect(find.text('今天'), findsNothing); // groupToday (zh)
    expect(find.text('更早'), findsNothing); // groupEarlier (zh)

    // Both of alpha's conversations sit under the alpha heading — including the
    // 5-day-old one, which recency bucketing would have split off.
    final alphaY = t.getTopLeft(find.text('alpha')).dy;
    final betaY = t.getTopLeft(find.text('beta')).dy;
    expect(alphaY, lessThan(betaY)); // newest project first
    expect(t.getTopLeft(find.text('alpha newest')).dy, greaterThan(alphaY));
    final oldestY = t.getTopLeft(find.text('alpha oldest')).dy;
    expect(oldestY, greaterThan(alphaY));
    expect(oldestY, lessThan(betaY));
  });

  testWidgets('A project heading outranks the conversation rows under it', (
    t,
  ) async {
    final nowS = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.add(
      ThreadMeta(
        id: 'a1',
        preview: 'a conversation',
        cwd: '/work/alpha',
        updatedAt: nowS - 60,
      ),
    );
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          home: true,
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // The project is the tree's top level, so its label is genuinely larger
    // than the conversation titles — not merely bolder.
    final heading = t.widget<Text>(find.text('alpha'));
    final row = t.widget<Text>(find.text('a conversation'));
    expect(heading.style!.fontSize!, greaterThan(row.style!.fontSize!));

    // Rows carry no leading glyph of their own; the heading's chevron is the
    // tree's only icon.
    expect(find.byIcon(Icons.chat_bubble_outline), findsNothing);
    expect(find.byIcon(Icons.keyboard_arrow_down), findsOneWidget);
  });

  testWidgets('Clicking a project folder collapses its conversations', (
    t,
  ) async {
    final nowS = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.addAll([
      ThreadMeta(
        id: 'a1',
        preview: 'alpha one',
        cwd: '/work/alpha',
        updatedAt: nowS - 60,
      ),
      ThreadMeta(
        id: 'b1',
        preview: 'beta one',
        cwd: '/work/beta',
        updatedAt: nowS - 3600,
      ),
    ]);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          home: true,
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    expect(find.text('alpha one'), findsOneWidget);

    // Collapsing hides only that project's rows; the sibling project stays.
    await t.tap(find.byKey(const Key('project-header-/work/alpha')));
    await t.pumpAndSettle();
    expect(find.text('alpha one'), findsNothing);
    expect(find.text('beta one'), findsOneWidget);
    expect(find.text('alpha'), findsOneWidget); // the heading itself remains

    // And clicking again brings them back.
    await t.tap(find.byKey(const Key('project-header-/work/alpha')));
    await t.pumpAndSettle();
    expect(find.text('alpha one'), findsOneWidget);
  });

  testWidgets(
    'A failed startup listing retries instead of stranding the pane',
    (t) async {
      final nowS = DateTime.now().millisecondsSinceEpoch ~/ 1000;
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect('pcx:lb7666:app:default', 28080);
      api.appThreads.add(
        ThreadMeta(
          id: 'a1',
          preview: 'alpha one',
          cwd: '/work/alpha',
          updatedAt: nowS - 300,
        ),
      );
      // The very first listing fails — the app opened before the host was
      // reachable, or the host restarted underneath it.
      api.failNextThreadList = true;
      t.view.devicePixelRatio = 1.0;
      t.view.physicalSize = const Size(1200, 900);
      addTearDown(t.view.reset);
      await t.pumpWidget(
        host(
          const AppSessionScreen(
            serviceKey: 'pcx:lb7666:app:default',
            home: true,
          ),
          api,
        ),
      );
      await t.pumpAndSettle();
      // Nothing yet: the one shot at listing failed and was swallowed.
      expect(convTiles(), findsNothing);

      // The retry lands and the pane fills itself in. Without it the sidebar
      // stayed empty for the whole session — nothing else re-lists unless the
      // user sends a message.
      await t.pump(const Duration(seconds: 2));
      await t.pumpAndSettle();
      expect(find.byKey(const Key('conv-tile-a1')), findsOneWidget);
      expect(find.text('alpha one'), findsWidgets);
    },
  );

  testWidgets('A failed default-folder seed retries instead of rooting a new '
      'conversation in the wrong folder', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.projectConfigs['pcx:lb7666:app:default'] = const ProjectConfig(
      projectRoots: ['/work'],
      defaultProject: '/work/alpha',
    );
    // The meta tunnel isn't up on the first attempt — the cold-open race.
    api.failNextProjectConfig = true;
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900);
    addTearDown(t.view.reset);
    // No threadId and no cwd: a brand-new conversation, which is the only case
    // that seeds a default folder.
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          home: true,
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    // The seed failed, so the folder is still unset ("default folder").
    expect(find.text('alpha'), findsNothing);

    // The retry lands and the conversation adopts the host's default. Without
    // it the chat would silently run in the wrong directory for the whole
    // session — the agent reading and editing files the user never chose.
    await t.pump(const Duration(seconds: 2));
    await t.pumpAndSettle();
    expect(find.text('alpha'), findsWidgets);
  });

  testWidgets('A settled default-folder seed stops retrying', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // A host with NO default configured: answering is settled, even though
    // nothing was seeded. Retrying would just ask the same question five times.
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900);
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          home: true,
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    final after = api.projectConfigCalls;
    await t.pump(const Duration(seconds: 20));
    await t.pumpAndSettle();
    expect(api.projectConfigCalls, after, reason: 'no retry once answered');
  });

  testWidgets('A reconnect re-lists the sidebar, not just the transcript', (
    t,
  ) async {
    final nowS = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900);
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          home: true,
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    expect(convTiles(), findsNothing, reason: 'host had nothing to list yet');

    // The host restarts and now HAS history — exactly the case that left the
    // pane empty: the app listed once at startup against a host with no data
    // (or no connection), and nothing re-listed afterwards.
    await api.appDisconnect('pcx:lb7666:app:default');
    api.appThreads.add(
      ThreadMeta(
        id: 'a1',
        preview: 'alpha one',
        cwd: '/work/alpha',
        updatedAt: nowS - 300,
      ),
    );

    // The health timer notices the dead socket and reconnects.
    await t.pump(const Duration(seconds: 13));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('conv-tile-a1')), findsOneWidget);
  });

  testWidgets('The activity view groups by day and summarizes each row', (
    t,
  ) async {
    final now = DateTime.now();
    int at(Duration ago) => now.subtract(ago).millisecondsSinceEpoch ~/ 1000;
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.addAll([
      ThreadMeta(
        id: 'a1',
        preview: 'alpha one',
        cwd: '/work/alpha',
        updatedAt: at(const Duration(minutes: 5)),
      ),
      // Comfortably inside yesterday whatever time the test runs at.
      ThreadMeta(
        id: 'b1',
        preview: 'beta one',
        cwd: '/work/beta',
        updatedAt: at(Duration(hours: 24 + now.hour, minutes: now.minute)),
      ),
    ]);
    api.summaries['a1'] = 'Rewrote the retry loop.';
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          home: true,
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // The project tree is the default, so no summaries are fetched for it —
    // one thread/read per row is far too much to spend on a sidebar nobody
    // asked to see this way.
    expect(api.summaryCalls, isEmpty);
    expect(find.byKey(const Key('project-header-/work/alpha')), findsOneWidget);

    await t.tap(find.byKey(const Key('activity-view-btn')));
    await t.pumpAndSettle();

    // Day headings replace the project headings.
    expect(find.byKey(const Key('project-header-/work/alpha')), findsNothing);
    expect(find.text('今天'), findsOneWidget);
    expect(find.text('昨天'), findsOneWidget);
    // Both rows are present, each under its own day.
    expect(find.byKey(const Key('activity-tile-a1')), findsOneWidget);
    expect(find.byKey(const Key('activity-tile-b1')), findsOneWidget);
    // The summary is the agent's own last line, not the user's first message.
    expect(find.text('Rewrote the retry loop.'), findsOneWidget);
    expect(api.summaryCalls, containsAll(<String>['a1', 'b1']));

    // And the toggle goes back to the project tree.
    await t.tap(find.byKey(const Key('activity-view-btn')));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('project-header-/work/alpha')), findsOneWidget);
    expect(find.byKey(const Key('activity-tile-a1')), findsNothing);
  });

  testWidgets('The activity view fetches summaries for visible rows only', (
    t,
  ) async {
    final nowS = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // Far more rows than one viewport holds. The earlier test used two, which
    // both fit on screen — so "only the visible ones" and "all of them" looked
    // identical and an eager fetch passed it.
    const total = 60;
    for (var i = 0; i < total; i++) {
      api.appThreads.add(
        ThreadMeta(
          id: 't$i',
          preview: 'row $i',
          cwd: '/work/alpha',
          updatedAt: nowS - 60 * i,
        ),
      );
      api.summaries['t$i'] = 'gist $i';
    }
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900);
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          home: true,
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('activity-view-btn')));
    await t.pumpAndSettle();

    // Each summary costs a full `thread/read` server-side, so the toggle must
    // not fan out over the whole history to fill one screen.
    expect(
      api.summaryCalls.length,
      lessThan(total),
      reason:
          'a pre-built ListView would fetch all $total transcripts at once; '
          'got ${api.summaryCalls.length}',
    );
    expect(api.summaryCalls, isNotEmpty, reason: 'visible rows still load');

    // Scrolling brings more rows in, and those fetch on arrival.
    final before = api.summaryCalls.length;
    await t.drag(
      find.byKey(const Key('activity-tile-t0')),
      const Offset(0, -900),
    );
    await t.pumpAndSettle();
    expect(
      api.summaryCalls.length,
      greaterThan(before),
      reason: 'rows scrolled into view fetch their summary',
    );
  });

  testWidgets('The activity view keeps one row shape, Active group included', (
    t,
  ) async {
    final nowS = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.addAll([
      ThreadMeta(
        id: 'a1',
        preview: 'alpha one',
        cwd: '/work/alpha',
        updatedAt: nowS - 300,
      ),
      ThreadMeta(
        id: 'a2',
        preview: 'alpha two',
        cwd: '/work/alpha',
        updatedAt: nowS - 600,
      ),
    ]);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900);
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          home: true,
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('activity-view-btn')));
    await t.pumpAndSettle();

    // a1 starts a turn, so it moves up into the "Active" group.
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(kind: 'turn/started', threadId: 'a1', raw: '{}'),
    );
    await t.pump();
    await t.pump();
    expect(find.text('进行中'), findsOneWidget); // groupActive (zh)

    // Both rows are activity tiles: the Active group must not fall back to the
    // project tree's compact row while the rest of the list is summarized.
    expect(find.byKey(const Key('activity-tile-a1')), findsOneWidget);
    expect(find.byKey(const Key('activity-tile-a2')), findsOneWidget);
    expect(convTiles(), findsNothing);
  });

  testWidgets('A finished turn refreshes that row\'s cached summary', (
    t,
  ) async {
    final nowS = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.add(
      ThreadMeta(
        id: 'a1',
        preview: 'alpha one',
        cwd: '/work/alpha',
        updatedAt: nowS - 300,
      ),
    );
    api.summaries['a1'] = 'Looked at the retry loop.';
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900);
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'a1',
          home: true,
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('activity-view-btn')));
    await t.pumpAndSettle();
    expect(find.text('Looked at the retry loop.'), findsOneWidget);
    expect(api.summaryCalls, ['a1']);

    // A turn runs and produces a newer reply. The cached line now describes the
    // previous turn, so it has to be re-read rather than kept forever.
    api.summaries['a1'] = 'Rewrote the retry loop.';
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(kind: 'turn/started', threadId: 'a1', raw: '{}'),
    );
    await t.pump();
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(kind: 'turn/completed', threadId: 'a1', raw: '{}'),
    );
    await t.pumpAndSettle();

    expect(find.text('Rewrote the retry loop.'), findsOneWidget);
    expect(find.text('Looked at the retry loop.'), findsNothing);
    expect(api.summaryCalls, ['a1', 'a1']); // re-read exactly once
  });

  testWidgets('A turn finishing on a BACKGROUND thread refreshes its row', (
    t,
  ) async {
    final nowS = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.addAll([
      ThreadMeta(
        id: 'a1',
        preview: 'alpha one',
        cwd: '/work/alpha',
        updatedAt: nowS - 300,
      ),
      ThreadMeta(
        id: 'b1',
        preview: 'beta one',
        cwd: '/work/alpha',
        updatedAt: nowS - 600,
      ),
    ]);
    api.summaries['b1'] = 'First pass at the parser.';
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900);
    addTearDown(t.view.reset);
    // a1 is the OPEN conversation; b1 runs in the background.
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'a1',
          home: true,
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('activity-view-btn')));
    await t.pumpAndSettle();
    expect(find.text('First pass at the parser.'), findsOneWidget);

    // b1 finishes a turn while a1 stays selected. `_onEvent` drops events for
    // other threads — but the summary IS that thread's newest reply, so the
    // cache has to be dropped anyway or b1's row is stale for the session.
    api.summaries['b1'] = 'Parser handles nested groups now.';
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(kind: 'turn/completed', threadId: 'b1', raw: '{}'),
    );
    await t.pumpAndSettle();

    expect(find.text('Parser handles nested groups now.'), findsOneWidget);
    expect(find.text('First pass at the parser.'), findsNothing);
  });

  testWidgets('A finished turn re-lists so the day grouping stays current', (
    t,
  ) async {
    final now = DateTime.now();
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // A conversation last touched two days ago: it opens under a weekday
    // heading, not "today".
    api.appThreads.add(
      ThreadMeta(
        id: 'a1',
        preview: 'alpha one',
        cwd: '/work/alpha',
        updatedAt:
            now
                .subtract(Duration(days: 2, hours: now.hour))
                .millisecondsSinceEpoch ~/
            1000,
      ),
    );
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900);
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'a1',
          home: true,
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('activity-view-btn')));
    await t.pumpAndSettle();
    expect(find.text('今天'), findsNothing, reason: 'starts under an old day');

    // The user resumes it and a turn completes: the server moves `updatedAt` to
    // now, so the row belongs under "today". Nothing used to re-list on turn
    // end, leaving it filed under the old day with a stale relative time.
    final i = api.appThreads.indexWhere((x) => x.id == 'a1');
    api.appThreads[i] = ThreadMeta(
      id: 'a1',
      preview: 'alpha one',
      cwd: '/work/alpha',
      updatedAt: now.millisecondsSinceEpoch ~/ 1000,
    );
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(kind: 'turn/completed', threadId: 'a1', raw: '{}'),
    );
    // Past the re-list debounce.
    await t.pump(const Duration(milliseconds: 500));
    await t.pumpAndSettle();

    expect(find.text('今天'), findsOneWidget); // groupToday (zh)
  });

  testWidgets('An activity row stays readable before its summary arrives', (
    t,
  ) async {
    final nowS = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.add(
      ThreadMeta(
        id: 'a1',
        preview: 'alpha one',
        cwd: '/work/alpha',
        name: 'Retry work',
        updatedAt: nowS - 300,
      ),
    );
    api.summaries['a1'] = 'Rewrote the retry loop.';
    // Hold the summary in flight so the pre-load row is observable.
    final gate = Completer<void>();
    api.summaryGate = gate;
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900);
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          home: true,
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('activity-view-btn')));
    await t.pumpAndSettle();

    // Loading: the name leads and the preview stands in for the summary, so
    // the row reads immediately and the layout won't jump when the real one
    // lands.
    expect(find.text('Retry work'), findsOneWidget);
    expect(find.text('alpha one'), findsOneWidget);

    gate.complete();
    await t.pumpAndSettle();
    expect(find.text('Rewrote the retry loop.'), findsOneWidget);
    expect(find.text('alpha one'), findsNothing);
  });

  testWidgets('A project shows only its newest few rows until expanded', (
    t,
  ) async {
    final nowS = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // Seven conversations in one project — two past the 5-row peek.
    for (var i = 0; i < 7; i++) {
      api.appThreads.add(
        ThreadMeta(
          id: 't$i',
          preview: 'row $i',
          cwd: '/work/alpha',
          updatedAt: nowS - 60 * (i + 1),
        ),
      );
    }
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          home: true,
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // The newest five show; the rest sit behind "show more" (2 hidden).
    expect(find.text('row 0'), findsOneWidget);
    expect(find.text('row 4'), findsOneWidget);
    expect(find.text('row 5'), findsNothing);
    expect(find.text('row 6'), findsNothing);
    expect(find.text('展开显示（2）'), findsOneWidget); // showMoreCount (zh)

    await t.tap(find.byKey(const Key('project-peek-/work/alpha')));
    await t.pumpAndSettle();
    expect(find.text('row 5'), findsOneWidget);
    expect(find.text('row 6'), findsOneWidget);
    expect(find.text('收起'), findsOneWidget); // showLess (zh)
  });

  testWidgets('The composer card takes a text cursor and focuses on click', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → desktop composer
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // The padding around the field is part of the input, not dead chrome: it
    // shows a text cursor and clicking it focuses the field.
    final field = find.byKey(const Key('composer-input'));
    final card = find
        .ancestor(of: field, matching: find.byType(MouseRegion))
        .last;
    expect(t.widget<MouseRegion>(card).cursor, SystemMouseCursors.text);

    expect(t.widget<TextField>(field).focusNode!.hasFocus, isFalse);
    // Tap the card's own padding, clear of the field and the button row.
    final box = t.getRect(
      find.ancestor(of: field, matching: find.byType(Container)).first,
    );
    await t.tapAt(Offset(box.right - 4, box.top + 4));
    await t.pumpAndSettle();
    expect(t.widget<TextField>(field).focusNode!.hasFocus, isTrue);
  });

  testWidgets('The top-bar title hovers as an editable target', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.add(
      const ThreadMeta(
        id: 't1',
        preview: 'original preview',
        cwd: '',
        updatedAt: 0,
      ),
    );
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → desktop top bar
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();
    await t.tap(find.text('original preview').first);
    await t.pumpAndSettle();

    // Hovering paints a highlight behind the title and shows a text cursor, so
    // the label reads as clickable rather than as window-drag chrome.
    final target = find.byKey(const Key('bar-title-tap'));
    final tapTarget = t.widget<InkWell>(target);
    expect(tapTarget.hoverColor, isNotNull);
    expect(tapTarget.mouseCursor, SystemMouseCursors.text);

    final gesture = await t.createGesture(kind: PointerDeviceKind.mouse);
    await gesture.addPointer(location: Offset.zero);
    addTearDown(() => gesture.removePointer());
    await gesture.moveTo(t.getCenter(target));
    await t.pumpAndSettle();

    // And one click on the label starts editing — no double-click needed.
    await t.tap(target);
    await t.pumpAndSettle();
    expect(find.byKey(const Key('bar-title-field')), findsOneWidget);
  });

  testWidgets('Only an overflowing title fades at its trailing edge', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.addAll([
      const ThreadMeta(id: 'short', preview: 'brief', cwd: '', updatedAt: 0),
      ThreadMeta(
        id: 'long',
        preview: 'a very long conversation title ${'that keeps going ' * 6}',
        cwd: '',
        updatedAt: 0,
      ),
    ]);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → desktop top bar
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // A title that fits is painted plainly — no fade to explain away.
    await t.tap(find.text('brief').first);
    await t.pumpAndSettle();
    final bar = find.byKey(const Key('bar-title'));
    expect(
      find.descendant(of: bar, matching: find.byType(ShaderMask)),
      findsNothing,
    );

    // One that doesn't fit gets the fade, so it reads as text running past the
    // edge rather than as a short label.
    await t.tap(find.textContaining('a very long conversation').first);
    await t.pumpAndSettle();
    expect(
      find.descendant(of: bar, matching: find.byType(ShaderMask)),
      findsOneWidget,
    );
  });

  testWidgets('Clicking the top-bar title renames the conversation', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.add(
      const ThreadMeta(
        id: 't1',
        preview: 'original preview',
        cwd: '',
        updatedAt: 0,
      ),
    );
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → desktop top bar
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();
    await t.tap(find.text('original preview').first);
    await t.pumpAndSettle();

    // The title is a label until clicked, then a text field.
    expect(find.byKey(const Key('bar-title-field')), findsNothing);
    await t.tap(find.byKey(const Key('bar-title-tap')));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('bar-title-field')), findsOneWidget);

    // Submitting persists via the bridge and both the bar and the sidebar row
    // switch to the new name.
    await t.enterText(
      find.byKey(const Key('bar-title-field')),
      'renamed thread',
    );
    await t.testTextInput.receiveAction(TextInputAction.done);
    await t.pumpAndSettle();
    expect(api.setNames['t1'], 'renamed thread');
    expect(find.text('renamed thread'), findsWidgets);
    expect(find.text('original preview'), findsNothing);
  });

  testWidgets('A failed rename restores the previous title', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.add(
      const ThreadMeta(
        id: 't1',
        preview: 'original preview',
        cwd: '',
        updatedAt: 0,
      ),
    );
    api.failSetThreadName = true;
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → desktop top bar
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();
    await t.tap(find.text('original preview').first);
    await t.pumpAndSettle();

    await t.tap(find.byKey(const Key('bar-title-tap')));
    await t.pumpAndSettle();
    await t.enterText(find.byKey(const Key('bar-title-field')), 'nope');
    await t.testTextInput.receiveAction(TextInputAction.done);
    await t.pumpAndSettle();

    // The optimistic rename is rolled back rather than left claiming success.
    expect(find.text('nope'), findsNothing);
    expect(find.text('original preview'), findsWidgets);
    expect(find.textContaining('重命名失败'), findsOneWidget); // renameFailed (zh)
  });

  testWidgets('A stale rename failure does not clobber a newer rename', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.add(
      const ThreadMeta(
        id: 't1',
        preview: 'original preview',
        cwd: '',
        updatedAt: 0,
      ),
    );
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → desktop top bar
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();
    await t.tap(find.text('original preview').first);
    await t.pumpAndSettle();

    // Rename twice, with the first request still in flight so its failure
    // lands AFTER the second rename was already applied.
    final gate = Completer<void>();
    api.renameGate = gate;
    api.failNextSetThreadName = true;
    await t.tap(find.byKey(const Key('bar-title-tap')));
    await t.pumpAndSettle();
    await t.enterText(find.byKey(const Key('bar-title-field')), 'first try');
    await t.testTextInput.receiveAction(TextInputAction.done);
    await t.pump(); // dispatched, parked on the gate

    await t.tap(find.byKey(const Key('bar-title-tap')));
    await t.pumpAndSettle();
    await t.enterText(find.byKey(const Key('bar-title-field')), 'second try');
    await t.testTextInput.receiveAction(TextInputAction.done);
    await t.pumpAndSettle();

    gate.complete(); // now let the FIRST request fail
    await t.pumpAndSettle();

    // The newer rename owns the outcome: rolling back to the pre-first-request
    // title would leave the UI permanently disagreeing with the server, which
    // stored "second try".
    expect(api.setNames['t1'], 'second try');
    expect(find.text('second try'), findsWidgets);
    expect(find.text('original preview'), findsNothing);
  });

  testWidgets('A search reaches conversations inside a collapsed project', (
    t,
  ) async {
    final nowS = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    for (var i = 0; i < 8; i++) {
      api.appThreads.add(
        ThreadMeta(
          id: 't$i',
          preview: i == 7 ? 'findme needle' : 'row $i',
          cwd: '/work/alpha',
          updatedAt: nowS - 60 * (i + 1),
        ),
      );
    }
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          home: true,
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    await t.tap(find.byKey(const Key('project-header-/work/alpha')));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('conv-tile-t0')), findsNothing); // collapsed

    // A collapsed project must not swallow search hits: the filter found this
    // row, so hiding it would answer "nothing found" to a search that did.
    await t.enterText(find.byKey(const Key('conv-search')), 'findme');
    await t.pumpAndSettle();
    expect(find.text('findme needle'), findsOneWidget);

    // Clearing the query returns the project to its collapsed state.
    await t.enterText(find.byKey(const Key('conv-search')), '');
    await t.pumpAndSettle();
    expect(find.byKey(const Key('conv-tile-t0')), findsNothing);
  });

  testWidgets('The open conversation stays visible past the project peek', (
    t,
  ) async {
    final nowS = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // 8 rows in one project; the open one is the OLDEST, so the newest-5 peek
    // would otherwise bury it and the sidebar would show no selection at all.
    for (var i = 0; i < 8; i++) {
      api.appThreads.add(
        ThreadMeta(
          id: 't$i',
          preview: 'row $i',
          cwd: '/work/alpha',
          updatedAt: nowS - 60 * (i + 1),
        ),
      );
    }
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't7',
          home: true,
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    expect(find.byKey(const Key('conv-tile-t7')), findsOneWidget);
    // The peek still previews the newest rows, and still offers the rest.
    expect(find.byKey(const Key('conv-tile-t0')), findsOneWidget);
    expect(find.byKey(const Key('project-peek-/work/alpha')), findsOneWidget);
  });

  testWidgets('A conversation is searchable by the name the user gave it', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.addAll([
      const ThreadMeta(
        id: 'n1',
        preview: 'aaa original preview',
        name: 'zzz custom name',
        cwd: '',
        updatedAt: 10,
      ),
      // Enough rows for the search box to appear (>6).
      for (var i = 0; i < 7; i++)
        ThreadMeta(id: 'f$i', preview: 'filler $i', cwd: '', updatedAt: 9 - i),
    ]);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // The row shows the user's name, so that's the text a search must match —
    // filtering on the preview alone would miss it entirely.
    await t.enterText(find.byKey(const Key('conv-search')), 'zzz custom');
    await t.pumpAndSettle();
    expect(find.text('zzz custom name'), findsOneWidget);
    expect(find.byKey(const Key('conv-tile-f0')), findsNothing);
  });

  testWidgets('A rename from another device updates the list', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.add(
      const ThreadMeta(
        id: 't1',
        preview: 'original preview',
        cwd: '',
        updatedAt: 0,
      ),
    );
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // Titles live on the server, so a rename elsewhere arrives as a
    // notification. Nothing polls the thread list, so ignoring it would leave
    // the old title on screen indefinitely.
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'thread/name/updated',
        threadId: 't1',
        raw: '{"threadId":"t1","name":"renamed elsewhere"}',
      ),
    );
    await t.pumpAndSettle();
    expect(find.text('renamed elsewhere'), findsWidgets);
    expect(find.text('original preview'), findsNothing);
  });

  testWidgets('A renamed conversation shows its name in the sidebar', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.add(
      const ThreadMeta(
        id: 't1',
        preview: 'raw first message',
        name: 'My tidy name',
        cwd: '',
        updatedAt: 0,
      ),
    );
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // The server-set name replaces the preview everywhere it's shown.
    expect(find.text('My tidy name'), findsWidgets);
    expect(find.text('raw first message'), findsNothing);
  });

  testWidgets('Committing the title untouched does not pin a name', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.add(
      const ThreadMeta(
        id: 't1',
        preview: 'original preview',
        cwd: '',
        updatedAt: 0,
      ),
    );
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → desktop top bar
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();
    await t.tap(find.text('original preview').first);
    await t.pumpAndSettle();

    await t.tap(find.byKey(const Key('bar-title-tap')));
    await t.pumpAndSettle();
    // Submit the seeded text as-is.
    await t.testTextInput.receiveAction(TextInputAction.done);
    await t.pumpAndSettle();

    // Nothing was written, so the title keeps tracking the preview.
    expect(api.setNames, isEmpty);
    expect(find.byKey(const Key('bar-title-field')), findsNothing);
    expect(find.text('original preview'), findsWidgets);
  });

  testWidgets(
    'Conversations pane groups by time with relative-time subtitles',
    (t) async {
      final nowS = DateTime.now().millisecondsSinceEpoch ~/ 1000;
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect('pcx:lb7666:app:default', 28080);
      api.appThreads.addAll([
        ThreadMeta(
          id: 'tRecent',
          preview: 'recent chat',
          cwd: '',
          updatedAt: nowS - 120,
        ),
        ThreadMeta(
          id: 'tOld',
          preview: 'ancient chat',
          cwd: '',
          updatedAt: nowS - 5 * 86400,
        ),
      ]);
      t.view.devicePixelRatio = 1.0;
      t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
      addTearDown(t.view.reset);
      await t.pumpWidget(
        host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
      );
      await t.pumpAndSettle();

      // Each conversation shows its preview + a relative-time subtitle, and the
      // older one is bucketed under "Earlier".
      expect(find.text('recent chat'), findsOneWidget);
      expect(find.text('ancient chat'), findsOneWidget);
      expect(find.text('2 分钟前'), findsOneWidget); // timeMinutesAgo (zh)
      expect(find.text('5 天前'), findsOneWidget); // timeDaysAgo (zh)
      expect(find.text('更早'), findsOneWidget); // groupEarlier (zh)
    },
  );

  testWidgets('Conversations search box filters the list', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // The search box only appears once there are enough conversations (>6).
    api.appThreads.addAll(const [
      ThreadMeta(id: 't1', preview: 'alpha', cwd: '', updatedAt: 0),
      ThreadMeta(id: 't2', preview: 'beta', cwd: '', updatedAt: 0),
      ThreadMeta(id: 't3', preview: 'gamma', cwd: '', updatedAt: 0),
      ThreadMeta(id: 't4', preview: 'delta', cwd: '', updatedAt: 0),
      ThreadMeta(id: 't5', preview: 'epsilon', cwd: '', updatedAt: 0),
      ThreadMeta(id: 't6', preview: 'zeta', cwd: '', updatedAt: 0),
      ThreadMeta(id: 't7', preview: 'needle', cwd: '', updatedAt: 0),
    ]);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    expect(find.text('alpha'), findsOneWidget);
    expect(find.text('needle'), findsOneWidget);

    // Typing a query filters the list to matching previews only — non-matches
    // disappear and exactly one conversation tile remains.
    await t.enterText(find.byKey(const Key('conv-search')), 'needle');
    await t.pumpAndSettle();
    expect(find.text('alpha'), findsNothing);
    expect(find.text('beta'), findsNothing);
    expect(convTiles(), findsOneWidget);
  });

  testWidgets('Tapping a guidance card prefills the composer', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // The prompt shows once on the guidance card before a tap.
    const prompt = '介绍一下这个项目的结构、主要模块和技术栈。';
    expect(find.text(prompt), findsOneWidget);

    // Tapping the "了解项目" card prefills the composer (review-then-send).
    await t.tap(find.text('了解项目'));
    await t.pumpAndSettle();
    // The prompt now appears twice: the card subtitle + the composer field.
    expect(find.text(prompt), findsNWidgets(2));
    // The send button is enabled now that the composer is non-empty.
    final sendBtn = t.widget<IconButton>(find.byKey(const Key('send-btn')));
    expect(sendBtn.onPressed, isNotNull);
  });

  group("a turn's work folds behind one row", () {
    /// Mount a thread whose history is one turn: a question, [work] items, and
    /// an answer — the shape a real turn arrives in.
    Future<FakeBridgeApi> openThread(
      WidgetTester t, {
      required List<ThreadItem> work,
      int? durationMs = 151000,
    }) async {
      ThreadItem stamp(ThreadItem i) => ThreadItem(
        id: i.id,
        itemType: i.itemType,
        title: i.title,
        text: i.text,
        turnId: 'turn-1',
        turnDurationMs: durationMs,
      );
      final api =
          FakeBridgeApi(
              config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
            )
            ..readResult = ThreadHistory(
              items: [
                stamp(
                  const ThreadItem(
                    id: 'u1',
                    itemType: 'userMessage',
                    title: '',
                    text: 'why does edge fail?',
                  ),
                ),
                ...work.map(stamp),
                stamp(
                  const ThreadItem(
                    id: 'a1',
                    itemType: 'agentMessage',
                    title: '',
                    text: 'It is the taskbar shortcut, not Edge.',
                  ),
                ),
              ],
              running: false,
            );
      await api.appConnect('pcx:lb7666:app:default', 28080);
      await t.pumpWidget(
        host(
          const AppSessionScreen(
            serviceKey: 'pcx:lb7666:app:default',
            threadId: 't1',
          ),
          api,
        ),
      );
      await t.pumpAndSettle();
      return api;
    }

    const alternating = [
      ThreadItem(
        id: 'c1',
        itemType: 'commandExecution',
        title: 'pwsh -Command x',
        text: 'out',
      ),
      ThreadItem(id: 'r1', itemType: 'reasoning', title: 'checking', text: ''),
      ThreadItem(
        id: 'c2',
        itemType: 'commandExecution',
        title: 'pwsh -Command y',
        text: 'out',
      ),
      ThreadItem(id: 's1', itemType: 'webSearch', title: 'taskbar', text: ''),
    ];

    testWidgets('an alternating run folds into ONE row, not four', (t) async {
      // This is the bug the fold exists for. Grouping used to merge only
      // same-type neighbours, and a real turn alternates — so four rows of tool
      // machinery sat between the question and its answer, and nothing collapsed.
      await openThread(t, work: alternating);

      expect(find.byKey(const Key('turn-work-toggle')), findsOneWidget);
      for (final hidden in [
        'pwsh -Command x',
        'pwsh -Command y',
        'checking',
        'taskbar',
      ]) {
        expect(find.text(hidden), findsNothing, reason: '$hidden must fold');
      }
    });

    testWidgets('the answer stays visible while the work is folded', (t) async {
      // The point is to surface the answer, not to hide the turn.
      await openThread(t, work: alternating);
      expect(
        find.textContaining('taskbar shortcut', findRichText: true),
        findsOneWidget,
      );
    });

    testWidgets("the row names the turn's duration", (t) async {
      // From the server's own turn stamp, so it survives a reload — the locally
      // timed footnote only exists for a turn this app watched run.
      await openThread(t, work: alternating);
      expect(find.textContaining('已处理'), findsOneWidget);
      expect(find.textContaining('2:31'), findsOneWidget);
    });

    testWidgets('a turn with no reported duration says how much it did', (
      t,
    ) async {
      // A rollout read from disk carries no turn stamp. Better to count the steps
      // than to invent a time, which would read as fact.
      await openThread(t, work: alternating, durationMs: null);
      expect(find.textContaining('已处理'), findsNothing);
      expect(find.textContaining('4'), findsWidgets);
    });

    testWidgets('expanding reveals the work, grouped by type', (t) async {
      // Two levels: the turn opens to sub-runs, and a sub-run opens to its calls.
      await openThread(t, work: alternating);
      await openTurnWork(t);

      // The two commands are adjacent in neither position, so they stay separate
      // rows; each type is present with its own label.
      expect(find.text('pwsh -Command x'), findsOneWidget);
      expect(find.text('taskbar'), findsOneWidget);
    });

    testWidgets('same-type neighbours stay grouped inside the fold', (t) async {
      await openThread(
        t,
        work: const [
          ThreadItem(id: 'r1', itemType: 'reasoning', title: 'first', text: ''),
          ThreadItem(
            id: 'r2',
            itemType: 'reasoning',
            title: 'second',
            text: '',
          ),
        ],
      );
      await openTurnWork(t);
      // "思考 ×2", not two loose rows — the inner level survives.
      expect(find.text('思考 ×2'), findsOneWidget);
      expect(find.text('first'), findsNothing);
    });

    testWidgets('one turn is ONE row, however it was interleaved', (t) async {
      // The reported bug: a turn whose work was broken up by intermediate prose
      // and a compaction rendered as three 已处理 rows that each reported the
      // SAME duration — visibly one turn, presented as several.
      await openThread(
        t,
        work: const [
          ThreadItem(
            id: 'c1',
            itemType: 'commandExecution',
            title: 'first batch',
            text: '',
          ),
          ThreadItem(
            id: 'p1',
            itemType: 'agentMessage',
            title: '',
            text: 'now let me check the other layer',
          ),
          ThreadItem(
            id: 'cc1',
            itemType: 'contextCompaction',
            title: '',
            text: '',
          ),
          ThreadItem(
            id: 'c2',
            itemType: 'commandExecution',
            title: 'second batch',
            text: '',
          ),
        ],
      );

      expect(
        find.byKey(const Key('turn-work-toggle')),
        findsOneWidget,
        reason: 'one turn, one fold',
      );
      // And the answer is still the thing on screen.
      expect(
        find.textContaining('taskbar shortcut', findRichText: true),
        findsOneWidget,
      );
      // Everything in between is inside it, prose and compaction included.
      await openTurnWork(t);
      expect(find.text('first batch'), findsOneWidget);
      expect(find.text('second batch'), findsOneWidget);
      expect(find.text('对话已压缩'), findsOneWidget);
      expect(
        find.textContaining('check the other layer', findRichText: true),
        findsWidgets,
      );
    });

    testWidgets('a running turn is already open, and folds when it ends', (
      t,
    ) async {
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect('pcx:lb7666:app:default', 28080);
      await t.pumpWidget(
        host(
          const AppSessionScreen(
            serviceKey: 'pcx:lb7666:app:default',
            threadId: 't1',
          ),
          api,
        ),
      );
      await t.pumpAndSettle();

      // An in-flight command: visible without anyone opening anything, so the
      // user can watch the agent work.
      api.pushEvent(
        'pcx:lb7666:app:default',
        const AppEvent(
          kind: 'item/started',
          threadId: 't1',
          itemId: 'c1',
          itemType: 'commandExecution',
          title: 'cargo build',
          text: '',
          raw: '{}',
        ),
      );
      await t.pump(const Duration(milliseconds: 100));
      expect(find.text('cargo build'), findsOneWidget);

      // Settling hands control back to the fold, and the work goes away.
      api.pushEvent(
        'pcx:lb7666:app:default',
        const AppEvent(
          kind: 'item/completed',
          threadId: 't1',
          itemId: 'c1',
          itemType: 'commandExecution',
          title: 'cargo build',
          text: 'done',
          raw: '{}',
        ),
      );
      await t.pumpAndSettle();
      expect(find.text('cargo build'), findsNothing);
      expect(find.byKey(const Key('turn-work-toggle')), findsOneWidget);
    });
  });

  testWidgets('Tool calls render as expandable activity cards', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // A web-search tool item arrives as an item event.
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/completed',
        threadId: 't1',
        itemId: 's1',
        itemType: 'webSearch',
        title: 'rust async',
        text: '',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();
    // The turn's work is folded by default; open it to see the calls.
    await openTurnWork(t);
    // Localized label + the query are shown.
    expect(find.text('联网搜索'), findsOneWidget);
    expect(find.text('rust async'), findsOneWidget);

    // A command with output is expandable.
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/completed',
        threadId: 't1',
        itemId: 'c1',
        itemType: 'commandExecution',
        title: 'ls -la',
        text: 'total 0\n[exit 0]',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();
    expect(find.text('执行命令'), findsOneWidget);
    // Detail hidden until expanded.
    expect(find.textContaining('total 0', findRichText: true), findsNothing);
    await t.tap(find.text('ls -la'));
    await t.pumpAndSettle();
    expect(find.textContaining('total 0', findRichText: true), findsOneWidget);
  });

  testWidgets('Extended thread-item types keep distinct activity visuals', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    const items = [
      ('collabAgentToolCall', 'spawnAgent'),
      ('subAgentActivity', 'started'),
      ('imageView', r'C:\tmp\screen.png'),
      ('imageGeneration', 'A blue square'),
      ('sleep', '1250 ms'),
      ('hookPrompt', 'run-1'),
      ('enteredReviewMode', 'Review the patch'),
      ('exitedReviewMode', 'Review complete'),
    ];
    for (var i = 0; i < items.length; i++) {
      final (type, title) = items[i];
      api.pushEvent(
        'pcx:lb7666:app:default',
        AppEvent(
          kind: 'item/completed',
          threadId: 't1',
          itemId: 'extended-$i',
          itemType: type,
          title: title,
          text: '',
          raw: '{}',
        ),
      );
    }
    await t.pumpAndSettle();
    await openTurnWork(t);

    for (final label in [
      '协作智能体',
      '子智能体动态',
      '查看图片',
      '生成图片',
      '等待',
      '钩子提示',
      '进入审查',
      '退出审查',
    ]) {
      expect(find.text(label), findsOneWidget);
    }
    expect(find.byIcon(Icons.groups_outlined), findsOneWidget);
    expect(find.byIcon(Icons.image_search_outlined), findsOneWidget);
  });

  testWidgets('Camel-case outputDelta keeps a command visibly running', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/started',
        threadId: 't1',
        itemId: 'c1',
        itemType: 'commandExecution',
        title: 'cargo test',
        text: '',
        raw: '{}',
      ),
    );
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/commandExecution/outputDelta',
        threadId: 't1',
        itemId: 'c1',
        itemType: 'commandExecution',
        title: '',
        text: 'building...',
        raw: '{}',
      ),
    );
    await t.pump(const Duration(milliseconds: 100));
    await openTurnWork(t);
    expect(find.text('cargo test'), findsOneWidget);
    // Scoped to the card: the turn-work fold above it spins too while the turn
    // runs, so a bare type count would pass on either one.
    expect(
      find.descendant(
        of: find.byType(ActivityCard),
        matching: find.byType(CircularProgressIndicator),
      ),
      findsOneWidget,
    );

    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/completed',
        threadId: 't1',
        itemId: 'c1',
        itemType: 'commandExecution',
        title: 'cargo test',
        text: 'ok',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();
    expect(
      find.descendant(
        of: find.byType(ActivityCard),
        matching: find.byType(CircularProgressIndicator),
      ),
      findsNothing,
    );
  });

  testWidgets('a finished turn drops a 用时 duration footnote', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // No footnote before a turn runs.
    expect(find.textContaining('用时'), findsNothing);

    // A turn starts (elapsed clock begins) then completes.
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'turn/started',
        threadId: 't1',
        itemId: '',
        itemType: '',
        title: '',
        text: '',
        raw: '{}',
      ),
    );
    await t.pump();
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'turn/completed',
        threadId: 't1',
        itemId: '',
        itemType: '',
        title: '',
        text: '',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();

    // The per-turn footnote is dropped into the transcript (用时 0:00 for an
    // instant test turn).
    expect(find.textContaining('用时'), findsOneWidget);
  });

  testWidgets('a long model name ellipsizes instead of overflowing the row', (
    t,
  ) async {
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(360, 760); // a phone-ish viewport
    addTearDown(t.view.reset);
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // Turn on plan so the chip carries the longest label it ever shows
    // (model · effort · plan) on the narrowest phone.
    await turnSetting(t, 'plan');
    expect(
      t.getRect(find.byKey(const Key('model-chip'))).right,
      lessThanOrEqualTo(360.0),
    );
    expect(t.takeException(), isNull);

    // Every setting the old pill row held is still reachable, one tap in.
    await t.tap(find.byKey(const Key('model-chip')));
    await t.pumpAndSettle();
    expect(find.byKey(const ValueKey('opt-model')), findsOneWidget);
    expect(find.byKey(const ValueKey('opt-effort')), findsOneWidget);
    expect(find.byKey(const ValueKey('opt-plan')), findsOneWidget);
  });

  testWidgets('Opening an existing thread resumes it before reading', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'thread-42',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    // Must resume (load into session) before read/turn, else "thread not found".
    expect(api.lastResumed, 'thread-42');
  });

  testWidgets('Re-opening an in-flight thread restores history + thinking', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // History recovered from disk + a turn still running.
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(
          id: 'u1',
          itemType: 'userMessage',
          title: '',
          text: 'earlier question',
        ),
      ],
      running: true,
    );
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'thread-7',
        ),
        api,
      ),
    );
    // Not pumpAndSettle: the restored typing indicator animates forever.
    await t.pump();
    await t.pump(const Duration(milliseconds: 50));
    // Past message recovered, and the running state restored (composer shows
    // the stop button instead of send).
    expect(find.text('earlier question'), findsOneWidget);
    expect(find.byKey(const Key('stop-btn')), findsOneWidget);
    expect(find.byKey(const Key('send-btn')), findsNothing);
  });

  testWidgets('#2: a thread restores its persisted config on open', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(items: [], running: false);
    // The host persisted a non-default model for this thread; the server does
    // not restore the model, so without persistence it would reset to default.
    api.threadConfigs['thread-cfg'] = const ThreadConfig(model: 'gpt-5');
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'thread-cfg',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    // The screen fetched this thread's persisted config and applied the stored
    // model to the composer chip (instead of the default).
    expect(api.lastConfigGetThread, 'thread-cfg');
    expect(find.text('GPT-5'), findsOneWidget);
  });

  testWidgets('App session answers an approval prompt interactively', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // Server pushes an approval request (carries a request id).
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'execCommandApproval',
        requestId: '7',
        raw: '{"command":["ls"]}',
      ),
    );
    await t.pumpAndSettle();
    expect(find.byKey(const Key('approval-card')), findsOneWidget);

    await t.tap(find.byKey(const Key('approve-btn')));
    await t.pumpAndSettle();
    expect(api.lastApprovalDecision, 'accept');
    expect(find.byKey(const Key('approval-card')), findsNothing);
  });

  testWidgets('request_user_input renders an interactive question card, not an '
      'approval prompt', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // The model asks a structured multiple-choice question (NOT a command to
    // approve). It must render as an answerable question, not 智能体请求执行命令.
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/tool/requestUserInput',
        requestId: '9',
        raw:
            '{"questions":[{"id":"theme","header":"题旨","question":"主题落在哪一类？",'
            '"isOther":false,"isSecret":false,"options":['
            '{"label":"山水抒怀","description":"d1"},'
            '{"label":"怀古咏史","description":"d2"}]}]}',
      ),
    );
    await t.pumpAndSettle();

    expect(find.byKey(const Key('user-input-card')), findsOneWidget);
    expect(find.byKey(const Key('approval-card')), findsNothing);
    expect(find.text('主题落在哪一类？'), findsOneWidget);
    expect(find.text('山水抒怀'), findsOneWidget);

    // Pick an option and submit → the answer is sent in the protocol shape.
    await t.tap(find.text('山水抒怀'));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const Key('user-input-submit')));
    await t.pumpAndSettle();
    expect(api.lastUserInputAnswers, '{"theme":["山水抒怀"]}');
    expect(find.byKey(const Key('user-input-card')), findsNothing);
  });

  testWidgets('request_user_input with no options renders an answerable '
      'free-text field', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // A question with no options must stay answerable as free text (not a card
    // with a permanently-disabled Submit).
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/tool/requestUserInput',
        requestId: '9',
        raw:
            '{"questions":[{"id":"title","header":"标题","question":"取个标题？",'
            '"isOther":false,"isSecret":false,"options":[]}]}',
      ),
    );
    await t.pumpAndSettle();
    expect(find.byKey(const Key('user-input-card')), findsOneWidget);

    final field = find.descendant(
      of: find.byKey(const Key('user-input-card')),
      matching: find.byType(TextField),
    );
    expect(field, findsOneWidget);
    await t.enterText(field, '春日');
    await t.pump();
    await t.tap(find.byKey(const Key('user-input-submit')));
    await t.pumpAndSettle();
    expect(api.lastUserInputAnswers, '{"title":["春日"]}');
  });

  testWidgets('Plan-mode turn ending on prose still offers to implement', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // A plan-mode turn: the plan checklist is followed by the model's prose plan
    // summary (目标/约束/假设) — so the plan is NOT the last item. The implement
    // choice must still appear (the bug was keying on "last item is a plan").
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(
          id: 'u1',
          itemType: 'userMessage',
          title: '',
          text: '规划写一篇古诗文',
        ),
        ThreadItem(id: 'p1', itemType: 'plan', title: '', text: '# Step 1'),
        ThreadItem(
          id: 'a1',
          itemType: 'agentMessage',
          title: '',
          text: '目标：……',
        ),
      ],
      running: false,
      collaborationMode: 'plan',
    );
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'thread-plan-prose',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    expect(find.byKey(const Key('implement-btn')), findsOneWidget);
  });

  testWidgets('A re-plan after "keep planning" still offers to implement, even '
      'as a plain message', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // The user got a plan, tapped "keep planning", steered ("in English"), and
    // the re-plan arrived as a PLAIN agent message (no typed `plan` item — codex
    // sometimes surfaces literal <proposed_plan> tags). The earlier `plan` item
    // now has the steering message after it; the implement choice must still
    // appear (the bug keyed on the last `plan` item).
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(id: 'u1', itemType: 'userMessage', title: '', text: 'plan'),
        ThreadItem(id: 'p1', itemType: 'plan', title: '', text: '# v1'),
        ThreadItem(
          id: 'u2',
          itemType: 'userMessage',
          title: '',
          text: 'in English',
        ),
        ThreadItem(
          id: 'a1',
          itemType: 'agentMessage',
          title: '',
          text: '<proposed_plan> English plan … </proposed_plan>',
        ),
      ],
      running: false,
      collaborationMode: 'plan',
    );
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'thread-replan',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    expect(find.byKey(const Key('implement-btn')), findsOneWidget);
  });

  testWidgets('A <proposed_plan> message renders without the wrapper tags', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(
          id: 'a1',
          itemType: 'agentMessage',
          title: '',
          text: '<proposed_plan>\n# English Plan\nStep one.\n</proposed_plan>',
        ),
      ],
      running: false,
    );
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    // The rendered markdown shows the plan content but not the wrapper tags.
    final md = t
        .widgetList<MarkdownBody>(find.byType(MarkdownBody))
        .firstWhere((w) => w.data.contains('English Plan'));
    expect(md.data.contains('proposed_plan'), isFalse);
  });

  testWidgets('App session surfaces a turn failure with retry', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'turn/failed',
        threadId: 't1',
        text: 'model overloaded',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();
    expect(find.byKey(const Key('session-error')), findsOneWidget);
    expect(find.text('model overloaded'), findsOneWidget);
  });

  testWidgets('Plan mode toggle is sent with the turn', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // Toggle the plan pill on, then send.
    await turnSetting(t, 'plan');
    await t.pump();
    await t.enterText(find.byType(TextField), 'build a feature');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastCollaborationMode, 'plan');
  });

  testWidgets('A finished plan turn offers to implement, and implementing '
      'leaves plan mode', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // History ends on a plan item with the turn finished — the signature of a
    // completed plan-mode turn. This is what a resumed (restarted) thread looks
    // like, so the implement choice must persist from it.
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(
          id: 'u1',
          itemType: 'userMessage',
          title: '',
          text: 'plan a feature',
        ),
        ThreadItem(
          id: 'p1',
          itemType: 'plan',
          title: '',
          text: '# Step 1\n# Step 2',
        ),
      ],
      running: false,
      // The thread is genuinely in plan mode (sticky server setting), so
      // implementing must send "default" to leave it.
      collaborationMode: 'plan',
    );
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'thread-9',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    // The implement choice is shown (derived from the trailing plan item).
    expect(find.byKey(const Key('implement-btn')), findsOneWidget);

    // Implementing leaves plan mode (sends "default", since it's sticky) and
    // starts a normal turn with the implement prompt.
    await t.tap(find.byKey(const Key('implement-btn')));
    await t.pumpAndSettle();
    expect(api.lastCollaborationMode, 'default');
    expect(api.lastTurnText, '请按上面的计划开始实现。');
    // Once a new turn runs, the plan is no longer trailing → choice goes away.
    expect(find.byKey(const Key('implement-btn')), findsNothing);
  });

  testWidgets('Implement bar survives a screen rebuild via in-memory cache '
      'when the persisted config is unavailable', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);

    // Screen A: a new plan-mode conversation. Sending creates thread-0 and
    // records its plan mode in the process-wide static cache.
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();
    await turnSetting(t, 'plan');
    await t.pump();
    await t.enterText(find.byType(TextField), 'plan it');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();

    // Tear the screen down (like going back to the session list) so reopening
    // builds a fresh State, and drop the persisted config to simulate the PUT
    // not having landed yet (the race behind "switching session hides the bar").
    await t.pumpWidget(const SizedBox());
    await t.pumpAndSettle();
    api.threadConfigs.clear();

    // Reopen thread-0. The server exposes no collaborationMode and there is no
    // persisted config, so the implement bar can only come from the static
    // cache populated by the send above.
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(
          id: 'u1',
          itemType: 'userMessage',
          title: '',
          text: 'plan it',
        ),
        ThreadItem(id: 'p1', itemType: 'plan', title: '', text: '# Step 1'),
      ],
      running: false,
    );
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'thread-0',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    expect(find.byKey(const Key('implement-btn')), findsOneWidget);
  });

  testWidgets('Plan mode read from the server can be turned off', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // A resumed plan-mode thread whose LAST item is a normal reply (not a plan)
    // — the old "last item is plan" heuristic wrongly concluded plan mode was
    // off, so toggling it off never sent "default" and it stayed stuck on.
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(id: 'u1', itemType: 'userMessage', title: '', text: 'hi'),
        ThreadItem(
          id: 'a1',
          itemType: 'agentMessage',
          title: '',
          text: 'Yes, I am in plan mode.',
        ),
      ],
      running: false,
      collaborationMode: 'plan',
    );
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // The toggle is synced ON from the server mode; tap it OFF and send → the
    // turn carries "default", actually leaving plan mode.
    await turnSetting(t, 'plan'); // currently active
    await t.pump();
    await t.enterText(find.byType(TextField), 'continue');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastCollaborationMode, 'default');
  });

  testWidgets('Plan mode is remembered per thread across switching', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.addAll(const [
      ThreadMeta(id: 'tA', preview: 'chat A', cwd: '', updatedAt: 0),
      ThreadMeta(id: 'tB', preview: 'chat B', cwd: '', updatedAt: 0),
    ]);
    api.readResult = const ThreadHistory(items: [], running: false);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);

    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'tA',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // Enter plan mode in tA and send.
    await turnSetting(t, 'plan');
    await t.pump();
    await t.enterText(find.byType(TextField), 'plan it');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastCollaborationMode, 'plan');

    // Switch to tB and back to tA via the pane; tA's plan mode is remembered.
    await t.tap(find.text('chat B'));
    await t.pumpAndSettle();
    await t.tap(find.text('chat A'));
    await t.pumpAndSettle();

    // Turning plan OFF in tA now sends "default" (proving it was restored ON).
    await turnSetting(t, 'plan');
    await t.pump();
    await t.enterText(find.byType(TextField), 'stop planning');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastCollaborationMode, 'default');
  });

  testWidgets('Picking a reasoning effort sends it on the next turn', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(items: [], running: false);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // A fresh turn with no effort picked sends nothing (Auto = model default).
    await t.enterText(find.byType(TextField), 'hi');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastReasoningEffort, isNull);

    // Open the effort picker (the chip shows the localized "Effort" label) and
    // choose High; the next turn carries "high". The pills scroll horizontally,
    // so scroll the chip into view before tapping.
    await turnSetting(t, 'effort');
    await t.tap(find.text('高'));
    await t.pumpAndSettle();
    await t.enterText(find.byType(TextField), 'think hard');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastReasoningEffort, 'high');
  });

  testWidgets('Effort picker offers the model-supported levels incl. xhigh', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(items: [], running: false);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // The default model (gpt-5.5 in the fake) supports low/medium/high/xhigh.
    await turnSetting(t, 'effort');
    expect(find.text('极高'), findsOneWidget); // xhigh is offered
    expect(find.text('最低'), findsNothing); // minimal: not supported by gpt-5.5

    // Pick Extra-high and send → "xhigh" goes on the wire.
    await t.tap(find.text('极高'));
    await t.pumpAndSettle();
    await t.enterText(find.byType(TextField), 'think harder');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastReasoningEffort, 'xhigh');
  });

  testWidgets('Reasoning effort is restored from the thread on resume', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(
      items: [],
      running: false,
      reasoningEffort: 'high',
    );
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    // The chip reflects the thread's current effort ("Effort · High").
    expect(find.textContaining('· 高'), findsOneWidget);
  });

  testWidgets('Toggling plan re-asserts the effort instead of wiping it', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(items: [], running: false);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // Set High and send.
    await turnSetting(t, 'effort');
    await t.tap(find.text('高'));
    await t.pumpAndSettle();
    await t.enterText(find.byType(TextField), 'one');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastReasoningEffort, 'high');

    // Now toggle plan ON (no new effort pick) and send: the collaborationMode
    // turn must still carry "high", not wipe the thread's effort to null.
    await turnSetting(t, 'plan');
    await t.pump();
    await t.enterText(find.byType(TextField), 'two');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastCollaborationMode, 'plan');
    expect(api.lastReasoningEffort, 'high');
  });

  testWidgets('A reasoning-effort pick made mid-turn is not reverted (R4)', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(items: [], running: false);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // Pick xhigh, then send a turn that we hold in-flight via the gate.
    await turnSetting(t, 'effort');
    await t.tap(find.text('极高'));
    await t.pumpAndSettle();
    api.turnStartGate = Completer<void>();
    await t.enterText(find.byType(TextField), 'one');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pump(); // _send is now suspended awaiting the gate (turn in flight)
    expect(api.lastReasoningEffort, 'xhigh');

    // While the turn is in flight, change effort to High.
    await turnSetting(t, 'effort');
    await t.tap(find.text('高'));
    await t.pumpAndSettle();

    // Let the in-flight turn finish. Post-turn reconciliation must NOT revert the
    // mid-turn pick (the R4 guard keeps the pending effort = High, not xhigh).
    api.turnStartGate!.complete();
    await t.pumpAndSettle();
    api.turnStartGate = null;

    // The next turn carries the mid-turn pick, proving it survived.
    await t.enterText(find.byType(TextField), 'two');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastReasoningEffort, 'high');
  });

  testWidgets('Resume collapses a back-to-back duplicate user message', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // The artifact of a dropped-but-committed send recorded twice.
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(id: 'u1', itemType: 'userMessage', title: '', text: 'do it'),
        ThreadItem(id: 'u2', itemType: 'userMessage', title: '', text: 'do it'),
      ],
      running: false,
    );
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    expect(find.text('do it'), findsOneWidget);
  });

  testWidgets('Resume keeps a genuine re-ask (a reply sits between)', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(id: 'u1', itemType: 'userMessage', title: '', text: 'do it'),
        ThreadItem(id: 'a1', itemType: 'agentMessage', title: '', text: 'ok'),
        ThreadItem(id: 'u2', itemType: 'userMessage', title: '', text: 'do it'),
      ],
      running: false,
    );
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    expect(find.text('do it'), findsNWidgets(2));
  });

  testWidgets('An unsent effort pick does not leak across threads', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.addAll(const [
      ThreadMeta(id: 'tA', preview: 'chat A', cwd: '', updatedAt: 0),
      ThreadMeta(id: 'tB', preview: 'chat B', cwd: '', updatedAt: 0),
    ]);
    api.readResult = const ThreadHistory(items: [], running: false);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);

    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'tA',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // Pick High on tA but DON'T send.
    await turnSetting(t, 'effort');
    await t.tap(find.text('高'));
    await t.pumpAndSettle();
    expect(find.textContaining('· 高'), findsOneWidget);

    // Switch to tB and send: the unsent High pick must NOT carry over.
    await t.tap(find.text('chat B'));
    await t.pumpAndSettle();
    expect(find.textContaining('· 高'), findsNothing);
    await t.enterText(find.byType(TextField), 'hi from B');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastReasoningEffort, isNull);
  });

  testWidgets('URLs in a message render as tappable links', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(
          id: 'u1',
          itemType: 'userMessage',
          title: '',
          text: 'see https://example.com for details',
        ),
      ],
      running: false,
    );
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    // The user bubble linkifies the URL (highlighted + tappable) rather than
    // rendering it as inert text. flutter_linkify parses the bare URL into a
    // UrlElement, so the rendered Linkify carries the link.
    final linkifyWidgets = t.widgetList<Linkify>(find.byType(Linkify));
    expect(linkifyWidgets, isNotEmpty);
    final hasUrl = linkifyWidgets.any(
      (w) => w.text.contains('https://example.com'),
    );
    expect(hasUrl, isTrue);
  });

  testWidgets('A normal multi-step turn does not offer to implement', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // A plan item appears mid-turn but the turn ends on a message — not a
    // plan-mode turn, so no implement choice.
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(id: 'u1', itemType: 'userMessage', title: '', text: 'do it'),
        ThreadItem(id: 'p1', itemType: 'plan', title: '', text: '# Step 1'),
        ThreadItem(id: 'a1', itemType: 'agentMessage', title: '', text: 'done'),
      ],
      running: false,
    );
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'thread-10',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    expect(find.byKey(const Key('implement-btn')), findsNothing);
  });

  testWidgets('Leaving plan mode sends "default" once, then null thereafter', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // Turn 1: plan mode on → "plan".
    await turnSetting(t, 'plan');
    await t.pump();
    await t.enterText(find.byType(TextField), 'plan it');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastCollaborationMode, 'plan');

    // Turn 2: plan mode off → must send "default" to leave sticky plan mode.
    await turnSetting(t, 'plan');
    await t.pump();
    await t.enterText(find.byType(TextField), 'now normally');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastCollaborationMode, 'default');

    // Turn 3: still off → null, NOT "default" forever (the _planActive reset).
    await t.enterText(find.byType(TextField), 'and again');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastCollaborationMode, isNull);
  });

  testWidgets(
    '"Keep planning" dismisses the implement choice, keeping history',
    (t) async {
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect('pcx:lb7666:app:default', 28080);
      api.readResult = const ThreadHistory(
        items: [
          ThreadItem(
            id: 'u1',
            itemType: 'userMessage',
            title: '',
            text: 'plan a feature',
          ),
          ThreadItem(id: 'p1', itemType: 'plan', title: '', text: '# Step 1'),
        ],
        running: false,
        // The implement choice only shows for a genuine plan-mode thread (a
        // plan checklist in a normal turn must not trigger it).
        collaborationMode: 'plan',
      );
      await t.pumpWidget(
        host(
          const AppSessionScreen(
            serviceKey: 'pcx:lb7666:app:default',
            threadId: 'thread-11',
          ),
          api,
        ),
      );
      await t.pumpAndSettle();
      expect(find.byKey(const Key('implement-btn')), findsOneWidget);

      // Dismiss → the whole bar (implement + keep-planning) hides, but the
      // conversation timeline is untouched.
      await t.tap(find.text('继续规划'));
      await t.pumpAndSettle();
      expect(find.byKey(const Key('implement-btn')), findsNothing);
      expect(find.text('继续规划'), findsNothing);
      expect(find.text('plan a feature'), findsOneWidget);
    },
  );

  testWidgets('Plan mode with no available model surfaces an error, no turn', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    )..emptyModelList = true;
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    await turnSetting(t, 'plan');
    await t.pump();
    await t.enterText(find.byType(TextField), 'plan it');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();

    // Collaboration mode can't apply without a model, so the turn is refused
    // (nothing sent) and an error is shown instead of silently dropping it.
    expect(find.byKey(const Key('session-error')), findsOneWidget);
    expect(api.lastCollaborationMode, isNull);
    expect(api.lastTurnText, isNull);
  });

  testWidgets('Context gauge appears on token usage and opens a detail sheet', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // No gauge until a token-usage event arrives.
    expect(find.text('10'), findsNothing);
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'thread/tokenUsage/updated',
        threadId: 't1',
        raw:
            '{"tokenUsage":{"last":{"totalTokens":20000},"modelContextWindow":200000}}',
      ),
    );
    await t.pumpAndSettle();
    // 20000 / 200000 = 10%.
    expect(find.text('10'), findsOneWidget);

    // Tapping the gauge opens the context/quota detail sheet.
    api.rateLimitsJson =
        '{"rateLimits":{"primary":{"usedPercent":42,"windowDurationMins":300}}}';
    await t.tap(find.text('10'));
    await t.pumpAndSettle();
    expect(find.text('上下文与用量'), findsOneWidget); // contextUsageTitle (zh)
    // Scoped to the sheet: the same window label also names the always-visible
    // sidebar quota strip.
    expect(
      find.descendant(
        of: find.byType(Dialog),
        matching: find.text('5 小时额度'), // quota5h (zh)
      ),
      findsOneWidget,
    );
  });

  testWidgets('A huge file-change card is capped instead of laid out whole', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(id: 'u1', itemType: 'userMessage', title: '', text: 'hi'),
      ],
      running: false,
      branch: 'dev',
      cwd: '/proj',
    );
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't-diff',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // A 900-line file-change card — the inline card's `_DiffHunks` bounds it so
    // one huge expansion can't make the transcript drag.
    final buf = StringBuffer()
      ..writeln('diff --git a/Cargo.lock b/Cargo.lock')
      ..writeln('--- a/Cargo.lock')
      ..writeln('+++ b/Cargo.lock')
      ..writeln('@@ -1 +1,900 @@');
    for (var i = 0; i < 900; i++) {
      buf.writeln('+line $i');
    }
    api.pushEvent(
      'pcx:lb7666:app:default',
      AppEvent(
        kind: 'item/completed',
        threadId: 't-diff',
        itemId: 'f1',
        itemType: 'fileChange',
        title: 'Cargo.lock',
        text: buf.toString(),
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();
    await openTurnWork(t);
    await t.tap(find.text('Cargo.lock'));
    await t.pumpAndSettle();

    // Capped at 200 rendered rows, with the remainder accounted for rather
    // than silently dropped. The `@@` hunk header takes one of the 200, so the
    // last body line rendered is 198 out of 901 parsed rows. The `+` marker
    // lives in the gutter, so the highlighted row (a RichText) is the code
    // alone — textContaining skips RichText unless asked to include it.
    expect(find.textContaining('line 0', findRichText: true), findsOneWidget);
    expect(find.textContaining('line 198', findRichText: true), findsOneWidget);
    expect(find.textContaining('line 199', findRichText: true), findsNothing);
    expect(find.textContaining('另有 701 行未显示'), findsOneWidget);
  });

  testWidgets('Quota is warm in the sidebar without opening anything', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // 88% of the weekly window used → 12% left, and the weekly window is the
    // tighter of the two so it is the one the strip shows.
    api.rateLimitsJson =
        '{"rateLimits":{'
        '"primary":{"usedPercent":10,"windowDurationMins":300},'
        '"secondary":{"usedPercent":88,"windowDurationMins":10080}}}';
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // No tap, no sheet — the numbers are already on screen because the quota
    // is fetched with the subscription rather than on demand.
    expect(find.byKey(const Key('sidebar-quota')), findsOneWidget);
    expect(find.text('剩余用量'), findsOneWidget); // quotaRemaining (zh)
    expect(find.text('12%'), findsOneWidget);
  });

  testWidgets('Theme button flips between light and dark', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);
    // Its own host rather than `host`: the button reads the brightness in
    // EFFECT, so the app has to actually carry a light and a dark theme (and
    // honour the stored mode) the way main.dart does.
    await t.pumpWidget(
      ProviderScope(
        overrides: [bridgeApiProvider.overrideWithValue(api)],
        child: Consumer(
          builder: (context, ref, _) {
            final pref = ref.watch(uiPrefsProvider).valueOrNull?.themeMode;
            return MaterialApp(
              locale: const Locale('zh'),
              theme: ThemeData(brightness: Brightness.light),
              darkTheme: ThemeData(brightness: Brightness.dark),
              themeMode: switch (pref) {
                'light' => ThemeMode.light,
                'dark' => ThemeMode.dark,
                _ => ThemeMode.system,
              },
              localizationsDelegates: AppLocalizations.localizationsDelegates,
              supportedLocales: AppLocalizations.supportedLocales,
              home: const AppSessionScreen(
                serviceKey: 'pcx:lb7666:app:default',
                home: true,
              ),
            );
          },
        ),
      ),
    );
    await t.pumpAndSettle();

    // Two states only — no third "auto" icon to land on.
    final btn = find.byKey(const Key('theme-toggle-btn'));
    expect(btn, findsOneWidget);
    expect(find.byIcon(Icons.brightness_auto_outlined), findsNothing);

    // The test platform reports light, so the button offers dark and the icon
    // shows what is currently in effect.
    expect(find.byIcon(Icons.light_mode_outlined), findsOneWidget);
    await t.tap(btn);
    await t.pumpAndSettle();
    expect(find.byIcon(Icons.dark_mode_outlined), findsOneWidget);
    expect(find.byIcon(Icons.light_mode_outlined), findsNothing);

    // And back — tapping twice returns to where it started rather than
    // advancing into a third state.
    await t.tap(btn);
    await t.pumpAndSettle();
    expect(find.byIcon(Icons.light_mode_outlined), findsOneWidget);
    expect(find.byIcon(Icons.brightness_auto_outlined), findsNothing);

    // It lives with the window's controls, not in the sessions pane, so
    // collapsing the sidebar must not take appearance away with it.
    await t.tap(find.byIcon(Icons.menu_open));
    await t.pumpAndSettle();
    expect(btn, findsOneWidget);
  });

  testWidgets('The composer drops the project chip once the thread exists', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → desktop composer
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          cwd: '/work/alpha',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // Before the first turn the project is still switchable, so the chip earns
    // its place above the field.
    expect(find.byKey(const Key('composer-project-chip')), findsOneWidget);

    await t.enterText(find.byKey(const Key('composer-input')), 'hello');
    await t.pump();
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();

    // Once the thread exists the cwd is fixed, so the chip would be a label you
    // can't act on — and the sidebar already heads the project.
    expect(find.byKey(const Key('composer-project-chip')), findsNothing);
    // The host chip stays: where the turn RUNS is the one fact only the
    // composer reports, and it isn't repeated in the sidebar.
    expect(find.byIcon(Icons.computer), findsOneWidget);
  });

  testWidgets('Git branch badge shows changes and opens the review split', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // Desktop width so the branch chip opens the inline review split (not the
    // mobile bottom sheet).
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1400, 900);
    addTearDown(t.view.reset);
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(id: 'u1', itemType: 'userMessage', title: '', text: 'hi'),
      ],
      running: false,
      branch: 'feature/x',
      cwd: '/proj', // needed so _loadGit fetches the diff
      tokensUsed: 5000,
      contextWindow: 100000,
    );
    api.gitDiffText =
        'diff --git a/lib/x.dart b/lib/x.dart\n'
        '--- a/lib/x.dart\n'
        '+++ b/lib/x.dart\n'
        '@@ -1 +1,2 @@\n'
        '-old\n'
        '+new\n'
        '+more\n';
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'thread-g',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // Branch + change counts in the unified status bar, and again on the
    // composer's context line (desktop width).
    expect(find.text('feature/x'), findsNWidgets(2));
    expect(find.byKey(const Key('composer-branch-chip')), findsOneWidget);
    expect(find.text('+2'), findsWidgets); // 2 added
    expect(find.text('−1'), findsWidgets); // 1 removed

    // Tapping the branch chip opens the review split: the file tree names the
    // changed file, and its diff is shown highlighted (the single changed file
    // is selected by default).
    await t.tap(find.byKey(const Key('status-branch-chip')));
    await t.pumpAndSettle();
    expect(find.text('审阅'), findsOneWidget); // reviewTitle (zh)
    expect(find.byKey(const Key('review-file-lib/x.dart')), findsOneWidget);
    // The diff pane renders the changed lines highlighted (RichText).
    expect(find.textContaining('new', findRichText: true), findsWidgets);
    expect(find.textContaining('old', findRichText: true), findsWidgets);

    // Closing the review hides it.
    await t.tap(find.byKey(const Key('review-close')));
    await t.pumpAndSettle();
    expect(find.text('审阅'), findsNothing);
  });

  testWidgets('A slow diff load spins on the badge and can be cancelled', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1400, 900);
    addTearDown(t.view.reset);
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(id: 'u1', itemType: 'userMessage', title: '', text: 'hi'),
      ],
      running: false,
      branch: 'feature/x',
      cwd: '/proj',
      tokensUsed: 5000,
      contextWindow: 100000,
    );
    api.gitDiffText =
        'diff --git a/lib/x.dart b/lib/x.dart\n'
        '--- a/lib/x.dart\n'
        '+++ b/lib/x.dart\n'
        '@@ -1 +1,2 @@\n'
        '-old\n'
        '+new\n';
    // Gate the diff from the very start, so the screen reaches the badge with
    // NO diff read yet — the only case that has to block on a fetch.
    final gate = Completer<void>();
    api.gitDiffGate = gate;
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'thread-cancel',
        ),
        api,
      ),
    );
    await t.pump();
    await t.pump(const Duration(milliseconds: 100));

    await t.tap(find.byKey(const Key('status-branch-chip')));
    await t.pump();

    // The press is acknowledged immediately: the badge spins instead of sitting
    // inert until the diff lands.
    expect(find.byType(CircularProgressIndicator), findsWidgets);
    expect(find.text('审阅'), findsNothing); // review not open yet

    // Tapping again while it's in flight cancels: the spinner goes away and the
    // review never opens, even once the underlying call finally returns.
    await t.tap(find.byKey(const Key('status-branch-chip')));
    await t.pump();
    expect(find.byType(CircularProgressIndicator), findsNothing);

    gate.complete();
    await t.pumpAndSettle();
    expect(find.text('审阅'), findsNothing);

    // A fresh press still works — cancelling doesn't wedge the badge.
    api.gitDiffGate = null;
    await t.tap(find.byKey(const Key('status-branch-chip')));
    await t.pumpAndSettle();
    expect(find.text('审阅'), findsOneWidget);
  });

  testWidgets('Reopening the review does not wait on another diff fetch', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1400, 900);
    addTearDown(t.view.reset);
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(id: 'u1', itemType: 'userMessage', title: '', text: 'hi'),
      ],
      running: false,
      branch: 'feature/x',
      cwd: '/proj',
      tokensUsed: 5000,
      contextWindow: 100000,
    );
    api.gitDiffText =
        'diff --git a/lib/x.dart b/lib/x.dart\n'
        '--- a/lib/x.dart\n'
        '+++ b/lib/x.dart\n'
        '@@ -1 +1,2 @@\n'
        '-old\n'
        '+new\n';
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'thread-warm',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // A diff is already in hand, so hanging every subsequent fetch must not stop
    // the review from opening: it shows what it has and refreshes behind itself.
    final stuck = Completer<void>();
    api.gitDiffGate = stuck;
    await t.tap(find.byKey(const Key('status-branch-chip')));
    await t.pump();
    expect(find.text('审阅'), findsOneWidget);
    expect(find.byKey(const Key('review-file-lib/x.dart')), findsOneWidget);

    stuck.complete();
    await t.pumpAndSettle();
  });

  testWidgets('Compact menu action calls the bridge after confirm', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(id: 'u1', itemType: 'userMessage', title: '', text: 'hi'),
      ],
      running: false,
    );
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'thread-c',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    await t.tap(find.byType(PopupMenuButton<String>));
    await t.pumpAndSettle();
    await t.tap(find.text('压缩对话').last); // compact (zh)
    await t.pumpAndSettle();
    // Confirm dialog → tap the confirm button (the FilledButton).
    await t.tap(find.widgetWithText(FilledButton, '压缩对话'));
    await t.pumpAndSettle();
    expect(api.compacted, isTrue);
  });

  testWidgets('Consecutive same-type tool calls collapse into one group', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    for (final id in ['c1', 'c2', 'c3']) {
      api.pushEvent(
        'pcx:lb7666:app:default',
        AppEvent(
          kind: 'item/completed',
          threadId: 't1',
          itemId: id,
          itemType: 'commandExecution',
          title: 'cmd-$id',
          text: 'out-$id',
          raw: '{}',
        ),
      );
    }
    await t.pumpAndSettle();

    // The turn's work folds first, so nothing about the commands is on screen —
    // not even the group header.
    expect(find.text('执行命令 ×3'), findsNothing);
    expect(find.text('cmd-c1'), findsNothing);

    // First level: the turn's work. Reveals the group, still collapsed.
    await openTurnWork(t);
    expect(find.text('执行命令 ×3'), findsOneWidget);
    expect(find.text('cmd-c1'), findsNothing);

    // Second level: the group. Reveals the individual activity cards.
    await t.tap(find.text('执行命令 ×3'));
    await t.pumpAndSettle();
    expect(find.text('cmd-c1'), findsOneWidget);
    expect(find.text('cmd-c3'), findsOneWidget);
  });

  testWidgets('Sessions pane shows inline when wide, hidden when narrow', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.add(
      const ThreadMeta(id: 't9', preview: 'past chat', cwd: '', updatedAt: 0),
    );
    t.view.devicePixelRatio = 1.0;
    addTearDown(t.view.reset);

    // Wide: the left sessions pane is inline (header + thread visible).
    t.view.physicalSize = const Size(1200, 900);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();
    expect(find.text('会话'), findsOneWidget); // conversationsSection (zh)
    expect(find.text('past chat'), findsOneWidget);

    // Narrow: no inline pane (it moves into a closed drawer).
    t.view.physicalSize = const Size(400, 900);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();
    expect(find.text('past chat'), findsNothing);
  });

  testWidgets('Sessions pane buttons switch threads without crashing', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.add(
      const ThreadMeta(id: 't9', preview: 'old chat', cwd: '', updatedAt: 0),
    );
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(
          id: 'm1',
          itemType: 'agentMessage',
          title: '',
          text: 'hello',
        ),
      ],
      running: false,
    );
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900);
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // Tap an existing thread in the pane → resumes it (no Scaffold.of crash).
    await t.tap(find.text('old chat'));
    await t.pumpAndSettle();
    expect(t.takeException(), isNull);
    expect(api.lastResumed, 't9');

    // Tap "new conversation" (+) → clears to an empty conversation.
    await t.tap(find.byKey(const Key('new-conversation-btn')));
    await t.pumpAndSettle();
    expect(t.takeException(), isNull);
    // Tapping "new conversation" shows the new-session guidance.
    expect(find.text('我们该构建什么?'), findsOneWidget); // guidance (zh)
  });

  testWidgets('Plan renders as a status-iconed checklist', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/completed',
        threadId: 't1',
        itemId: 'p1',
        itemType: 'plan',
        title: '',
        // Summarizer format: explanation + `- [x|~| ] step` lines.
        text: 'Add a feature\n- [x] research\n- [~] implement\n- [ ] test',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();
    // Checklist is shown expanded by default: header + progress + each step.
    expect(find.text('计划'), findsWidgets); // toolPlan label (pill + card)
    expect(find.text('1/3'), findsOneWidget); // 1 of 3 completed
    expect(find.text('research'), findsOneWidget);
    expect(find.text('implement'), findsOneWidget);
    expect(find.text('test'), findsOneWidget);
    // Completed step shows a filled check; pending shows an empty circle.
    expect(find.byIcon(Icons.check_circle_rounded), findsOneWidget);
    expect(find.byIcon(Icons.timelapse_rounded), findsOneWidget);
    expect(find.byIcon(Icons.radio_button_unchecked), findsOneWidget);
    // The explanation renders as Markdown above the steps.
    expect(
      find.textContaining('Add a feature', findRichText: true),
      findsWidgets,
    );
  });

  testWidgets('Running plan tracker fits phone width and can collapse', (
    t,
  ) async {
    const key = 'pcx:lb7666:app:default';
    final api =
        FakeBridgeApi(
            config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
          )
          ..appThreadResumeError = StateError(
            'thread t-mobile already has an active writer',
          );
    await api.appConnect(key, 28080);
    api.transcripts['t-mobile'] = const [
      ThreadItem(id: 'u1', itemType: 'userMessage', title: '', text: 'ship it'),
      ThreadItem(
        id: 'p1',
        itemType: 'plan',
        title: '',
        text: '- [x] inspect\n- [~] implement\n- [ ] verify',
      ),
    ];
    api.liveness['t-mobile'] = const SessionLiveness(
      threadId: 't-mobile',
      turnState: 'incomplete',
      heldOpen: true,
      safety: 'ownedRunning',
      allowsResume: false,
      requiresTakeover: false,
      holders: [],
    );
    t.view.devicePixelRatio = 1;
    t.view.physicalSize = const Size(360, 700);
    addTearDown(t.view.reset);

    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: key, threadId: 't-mobile'), api),
    );
    for (var i = 0; i < 6; i++) {
      await t.pump(const Duration(milliseconds: 100));
    }
    expect(t.takeException(), isNull);
    expect(find.text('第 2 / 3 步'), findsOneWidget);
    expect(find.byKey(const Key('turn-progress-panel')), findsOneWidget);
    expect(
      find.ancestor(
        of: find.byKey(const Key('turn-progress-summary')),
        matching: find.byKey(const Key('chat-conversation-layer')),
      ),
      findsOneWidget,
      reason: 'the live tracker floats over the conversation layer',
    );

    await t.tap(find.byKey(const Key('turn-progress-summary')));
    await t.pump(const Duration(milliseconds: 250));
    expect(find.byKey(const Key('turn-progress-panel')), findsNothing);

    await t.pumpWidget(const SizedBox());
  });

  testWidgets('New conversation applies the chosen permission mode', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(const AppSessionScreen(serviceKey: 'pcx:lb7666:app:default'), api),
    );
    await t.pumpAndSettle();

    // Default mode = Auto → on-request / workspace-write.
    await t.enterText(find.byType(TextField), 'hi');
    await t.pump(); // let the send button enable for the non-empty input
    await t.tap(find.byKey(const Key('send-btn')));
    await t.pumpAndSettle();
    expect(api.lastApproval, 'on-request');
    expect(api.lastSandbox, 'workspace-write');
  });

  testWidgets('Stop button interrupts the running turn with its turn id', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // The server starts a turn carrying a turn id; the stop button appears.
    // A running turn animates the typing indicator forever, so pump one frame
    // rather than settling.
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'turn/started',
        threadId: 't1',
        raw: '{"turnId":"turn-42"}',
      ),
    );
    await t.pump(); // deliver the broadcast event
    await t.pump(); // build the resulting frame
    expect(find.byKey(const Key('stop-btn')), findsOneWidget);

    // Tapping stop sends turn/interrupt with the captured turn id (the server
    // rejects an interrupt that omits it).
    await t.tap(find.byKey(const Key('stop-btn')));
    await t.pump();
    expect(api.interrupted, isTrue);
    expect(api.lastInterruptTurnId, 'turn-42');
  });

  testWidgets('Stop works for a thread already running when opened', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // Resumed mid-turn: history.running=true, but the UI never saw turn/started
    // so it has no turn id. Stop must still fire (the engine supplies the id).
    api.readResult = const ThreadHistory(items: [], running: true);

    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pump(); // run the resume future
    await t
        .pump(); // build with _streaming=true (don't settle: typing animates)

    expect(find.byKey(const Key('stop-btn')), findsOneWidget);
    await t.tap(find.byKey(const Key('stop-btn')));
    await t.pump();
    expect(api.interrupted, isTrue);
    expect(api.lastInterruptTurnId, isNull); // UI had none; engine falls back
  });

  testWidgets('Status bar reflects ready then working state', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // Narrow so the sessions pane is a hidden drawer — the open thread's sidebar
    // "running" subtitle then can't collide with the status bar's working text.
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(400, 800);
    addTearDown(t.view.reset);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    // Idle → the status bar reads "Ready".
    expect(find.text('就绪'), findsOneWidget); // stateReady (zh)

    // A running turn flips it to "Working…" (pump one frame — the typing
    // indicator animates forever, so the tree never fully settles).
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'turn/started',
        threadId: 't1',
        raw: '{"turnId":"x"}',
      ),
    );
    await t.pump(); // deliver the broadcast event
    await t.pump(); // build the resulting frame
    expect(find.text('运行中…'), findsOneWidget); // stateWorking (zh)
    expect(find.text('就绪'), findsNothing);
  });

  testWidgets('File change shows ± counts and expands to the diff', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/completed',
        threadId: 't1',
        itemId: 'f1',
        itemType: 'fileChange',
        title: 'lib/x.dart',
        text:
            'diff --git a/lib/x.dart b/lib/x.dart\n'
            '--- a/lib/x.dart\n'
            '+++ b/lib/x.dart\n'
            '@@ -1 +1,2 @@\n'
            '-old\n'
            '+new\n'
            '+more\n',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();
    await openTurnWork(t);

    // Collapsed: an "Edited files" header with the path and the ± counts.
    expect(find.text('修改文件'), findsOneWidget); // toolEdited (zh)
    expect(find.text('lib/x.dart'), findsOneWidget);
    expect(find.text('+2'), findsWidgets);
    expect(find.text('−1'), findsWidgets);

    // Expanding reveals the highlighted diff lines for review — the +/−
    // markers moved to the gutter, so each row (a RichText) carries just code.
    await t.tap(find.text('lib/x.dart'));
    await t.pumpAndSettle();
    expect(find.textContaining('new', findRichText: true), findsOneWidget);
    expect(find.textContaining('old', findRichText: true), findsOneWidget);
  });

  testWidgets('Active writer stays in chat read-only, then can be taken over', (
    t,
  ) async {
    const key = 'pcx:lb7666:app:default';
    final api =
        FakeBridgeApi(
            config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
          )
          ..appThreadResumeError = StateError(
            'thread t-active already has an active writer',
          )
          ..gitDiffText = '''--- a/lib/live.dart
+++ b/lib/live.dart
@@ -1 +1 @@
-old
+new
''';
    await api.appConnect(key, 28080);
    api.transcripts['t-active'] = const [
      ThreadItem(
        id: 'remote-progress',
        itemType: 'agentMessage',
        title: '',
        text: 'work from the other client',
      ),
      ThreadItem(
        id: 'remote-compaction',
        itemType: 'contextCompaction',
        title: 'inProgress',
        text: '',
      ),
      ThreadItem(
        id: 'remote-plan',
        itemType: 'plan',
        title: '',
        text: '- [x] inspect\n- [~] implement\n- [ ] test',
      ),
    ];
    api.liveness['t-active'] = const SessionLiveness(
      threadId: 't-active',
      turnState: 'incomplete',
      heldOpen: true,
      safety: 'ownedRunning',
      allowsResume: false,
      requiresTakeover: false,
      holders: [],
    );

    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: key,
          threadId: 't-active',
          cwd: r'E:\project',
        ),
        api,
      ),
    );
    // External-writer mode deliberately keeps several animations alive. Use
    // bounded pumps rather than pumpAndSettle, which can never settle them.
    for (var i = 0; i < 6; i++) {
      await t.pump(const Duration(milliseconds: 100));
    }

    // This is still AppSessionScreen: its chat status, transcript and sessions
    // chrome remain mounted, while only the composer becomes read-only.
    expect(find.byType(AppSessionScreen), findsOneWidget);
    expect(find.text('其他进程运行中'), findsOneWidget);
    expect(find.text('work from the other client'), findsOneWidget);
    // Compaction happens during a turn, so it folds with the rest of the work.
    await openTurnWork(t);
    expect(find.text('正在自动压缩上下文'), findsOneWidget);
    expect(find.byKey(const Key('chat-compaction-progress')), findsOneWidget);
    expect(find.byKey(const Key('turn-progress-panel')), findsOneWidget);
    expect(find.byKey(const Key('turn-progress-summary')), findsOneWidget);
    expect(find.text('第 2 / 3 步'), findsOneWidget);
    expect(find.text('inspect'), findsWidgets);
    expect(find.text('implement'), findsWidgets);
    expect(find.text('test'), findsWidgets);
    expect(find.text('只读 — 其他客户端正在使用此会话'), findsOneWidget);
    expect(find.byKey(const Key('composer-input')), findsNothing);
    expect(find.byKey(const Key('chat-read-only-action')), findsOneWidget);
    expect(find.byKey(const Key('chat-external-diff-action')), findsOneWidget);
    expect(
      find.byKey(const Key('chat-external-output-indicator')),
      findsOneWidget,
    );
    expect(find.byKey(const Key('chat-status-running-pulse')), findsOneWidget);
    final selectedConversation = find.byKey(const Key('conv-tile-t-active'));
    expect(
      find.descendant(
        of: selectedConversation,
        matching: find.byType(PulsingDot),
      ),
      findsOneWidget,
    );
    expect(find.text('进行中'), findsOneWidget);
    expect(find.text('会话'), findsOneWidget);
    expect(api.metaSessionEventSubscriptions, 1);

    // The working-tree diff remains directly reviewable without taking over.
    await t.tap(find.byKey(const Key('chat-external-diff-action')));
    for (var i = 0; i < 4; i++) {
      await t.pump(const Duration(milliseconds: 100));
    }
    expect(find.textContaining('live.dart'), findsWidgets);
    Navigator.of(t.element(find.textContaining('live.dart').first)).pop();
    for (var i = 0; i < 4; i++) {
      await t.pump(const Duration(milliseconds: 100));
    }

    const running = SessionLiveness(
      threadId: 't-active',
      turnState: 'incomplete',
      heldOpen: true,
      safety: 'ownedRunning',
      allowsResume: false,
      requiresTakeover: false,
      holders: [],
    );
    const liveItems = [
      ThreadItem(
        id: 'remote-progress',
        itemType: 'agentMessage',
        title: '',
        text: 'work from the other client',
      ),
      ThreadItem(
        id: 'remote-progress-2',
        itemType: 'agentMessage',
        title: '',
        text: 'new progress from subscription',
      ),
      ThreadItem(
        id: 'remote-command',
        itemType: 'commandExecution',
        title: 'cargo test',
        text: 'all tests passed',
      ),
      ThreadItem(
        id: 'remote-edit',
        itemType: 'fileChange',
        title: 'lib/live.dart',
        text: '''--- a/lib/live.dart
+++ b/lib/live.dart
@@ -1 +1 @@
-old
+new
''',
      ),
      ThreadItem(
        id: 'remote-plan',
        itemType: 'plan',
        title: '',
        text: '- [x] inspect\n- [x] implement\n- [~] test',
      ),
    ];
    api.pushMetaSessionUpdate(
      't-active',
      const SessionFollowUpdate(liveness: running, items: liveItems),
    );
    await t.pump();
    await t.pump(const Duration(milliseconds: 100));
    // This prose has a command and a file change AFTER it, so it is the agent
    // narrating what it is about to do — a preamble, which folds with the work
    // rather than standing above it as the answer.
    await openTurnWork(t);
    // Rendered as markdown inside the fold, hence `findRichText`.
    expect(
      find.textContaining('new progress from subscription', findRichText: true),
      findsWidgets,
    );
    expect(find.text('cargo test'), findsOneWidget);
    expect(find.textContaining('live.dart'), findsWidgets);
    expect(find.text('第 3 / 3 步'), findsOneWidget);
    expect(find.byKey(const Key('turn-progress-panel')), findsOneWidget);
    expect(find.byKey(const Key('turn-progress-diff')), findsOneWidget);
    expect(find.text('1 个文件有变更'), findsWidgets);

    // A stream update flips the action immediately when the other turn ends.
    api.pushMetaSessionUpdate(
      't-active',
      const SessionFollowUpdate(
        liveness: SessionLiveness(
          threadId: 't-active',
          turnState: 'completed',
          heldOpen: true,
          safety: 'ownedIdle',
          allowsResume: true,
          requiresTakeover: true,
          holders: [],
        ),
        items: liveItems,
      ),
    );
    await t.pumpAndSettle();
    expect(find.byType(AppSessionScreen), findsOneWidget);
    expect(
      find.byKey(const Key('chat-external-output-indicator')),
      findsNothing,
    );
    expect(
      find.descendant(
        of: selectedConversation,
        matching: find.byType(PulsingDot),
      ),
      findsNothing,
    );
    expect(find.byKey(const Key('chat-takeover-action')), findsOneWidget);
    expect(find.text('强制接管'), findsOneWidget);
    expect(find.text('被其他进程占用'), findsOneWidget);
    expect(find.byKey(const Key('turn-progress-summary')), findsNothing);

    // Confirming takeover resumes in place; it must not navigate away from the
    // chat route or leave the user stranded in a separate viewer.
    api.appThreadResumeError = null;
    await t.tap(find.byKey(const Key('chat-takeover-action')));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('takeover-dialog')), findsOneWidget);
    await t.tap(find.byKey(const Key('takeover-confirm')));
    await t.pumpAndSettle();
    expect(api.lastMetaResumedKey, key);
    expect(api.lastMetaResumedThread, 't-active');
    expect(find.byType(AppSessionScreen), findsOneWidget);
    expect(find.byKey(const Key('composer-input')), findsOneWidget);

    await t.pumpWidget(const SizedBox());
  });

  testWidgets('A compaction item shows its live lifecycle in the transcript', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/started',
        threadId: 't1',
        itemId: 'cc1',
        itemType: 'contextCompaction',
        title: '',
        text: '',
        raw: '{}',
      ),
    );
    await t.pump();
    await t.pump(const Duration(milliseconds: 100));
    // In flight, so the turn's work is already open.
    expect(find.text('正在自动压缩上下文'), findsOneWidget);
    expect(find.byKey(const Key('chat-compaction-progress')), findsOneWidget);

    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(
        kind: 'item/completed',
        threadId: 't1',
        itemId: 'cc1',
        itemType: 'contextCompaction',
        title: '',
        text: '',
        raw: '{}',
      ),
    );
    await t.pumpAndSettle();
    // Settled, so the work folded away with it. Re-open to see the outcome.
    await openTurnWork(t);
    expect(find.text('对话已压缩'), findsOneWidget); // compacted (zh)
    expect(find.byKey(const Key('chat-compaction-progress')), findsNothing);
  });

  testWidgets('Running sessions show a pulsing badge in the sessions pane', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    api.appThreads.addAll(const [
      ThreadMeta(id: 'tOpen', preview: 'open chat', cwd: '', updatedAt: 0),
      ThreadMeta(id: 't9', preview: 'other chat', cwd: '', updatedAt: 0),
    ]);
    t.view.devicePixelRatio = 1.0;
    t.view.physicalSize = const Size(1200, 900); // wide → left pane inline
    addTearDown(t.view.reset);

    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 'tOpen',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();
    expect(find.byType(PulsingDot), findsNothing);

    // A turn starts on the OTHER thread (t9), not the open one. The session's
    // own handler ignores cross-thread events, but the shared provider tracks
    // it, so the pane shows a running badge + header count.
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(kind: 'turn/started', threadId: 't9', raw: '{}'),
    );
    await t.pump(); // deliver the broadcast event
    await t.pump(); // build the resulting frame
    // The running thread moves into the "Active" group with a pulsing dot.
    expect(find.text('进行中'), findsOneWidget); // groupActive (zh)
    expect(find.byType(PulsingDot), findsAtLeastNWidgets(1));

    // Turn completes → the badge clears.
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(kind: 'turn/completed', threadId: 't9', raw: '{}'),
    );
    await t.pump();
    await t.pump();
    expect(find.byType(PulsingDot), findsNothing);
  });

  testWidgets('Stopping a turn shows a "stopped" marker in the transcript', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    // Opened mid-turn: streaming, stop button available.
    api.readResult = const ThreadHistory(items: [], running: true);

    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pump(); // run resume
    await t.pump(); // build with _streaming=true

    await t.tap(find.byKey(const Key('stop-btn')));
    await t.pump();
    expect(api.interrupted, isTrue);

    // The server ends the aborted turn; a "stopped" marker appears (and no
    // error banner).
    api.pushEvent(
      'pcx:lb7666:app:default',
      const AppEvent(kind: 'turn/completed', threadId: 't1', raw: '{}'),
    );
    await t.pump();
    await t.pump();
    expect(find.text('已停止'), findsOneWidget); // turnStopped (zh)
    expect(find.byKey(const Key('session-error')), findsNothing);
  });

  testWidgets('Compactions fold with the work, and are not counted as one', (
    t,
  ) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    await api.appConnect('pcx:lb7666:app:default', 28080);
    await t.pumpWidget(
      host(
        const AppSessionScreen(
          serviceKey: 'pcx:lb7666:app:default',
          threadId: 't1',
        ),
        api,
      ),
    );
    await t.pumpAndSettle();

    for (final id in ['c1', 'c2']) {
      api.pushEvent(
        'pcx:lb7666:app:default',
        AppEvent(
          kind: 'item/completed',
          threadId: 't1',
          itemId: id,
          itemType: 'contextCompaction',
          title: '',
          text: '',
          raw: '{}',
        ),
      );
    }
    await t.pumpAndSettle();
    // Compaction happens during a turn, so it folds with the work rather than
    // interrupting it — leaving it out split one turn into several 已处理 rows
    // that each reported the same duration.
    expect(find.text('对话已压缩'), findsNothing);

    // Opened, each still renders as its own notice rather than collapsing into a
    // single "×2" row: a compaction is an event, and two of them mean the context
    // was squeezed twice.
    await openTurnWork(t);
    expect(find.text('对话已压缩'), findsNWidgets(2));
  });

  group('Esc state machine + message queue (codex-cli parity)', () {
    const svc = 'pcx:lb7666:app:default';

    // Run a test body with a desktop target forced (registers the Esc/paste key
    // handler). Reset inside the body — not addTearDown — so the framework's
    // debug-var invariant check passes.
    Future<void> onDesktop(Future<void> Function() body) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.windows;
      try {
        await body();
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    }

    // Mount a desktop session sitting on a running turn with NO output yet:
    // sends "first", and with autoCompleteTurn=false the fake emits only
    // turn/started — the exact state Esc "undoes".
    Future<FakeBridgeApi> pumpStreaming(WidgetTester t) async {
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      )..autoCompleteTurn = false;
      await api.appConnect(svc, 28080);
      await t.pumpWidget(
        host(const AppSessionScreen(serviceKey: svc, threadId: 't1'), api),
      );
      await t.pumpAndSettle();
      await t.enterText(find.byKey(const Key('composer-input')), 'first');
      await t.testTextInput.receiveAction(TextInputAction.send);
      await t.pump();
      return api;
    }

    String composerText(WidgetTester t) => t
        .widget<TextField>(find.byKey(const Key('composer-input')))
        .controller!
        .text;

    // Ensure the composer is focused (the key handler requires it), then Esc.
    Future<void> pressEsc(WidgetTester t) async {
      await t.tap(find.byKey(const Key('composer-input')));
      await t.pump();
      await t.sendKeyEvent(LogicalKeyboardKey.escape);
      await t.pump();
    }

    Future<void> queue(WidgetTester t, String text) async {
      await t.enterText(find.byKey(const Key('composer-input')), text);
      await t.testTextInput.receiveAction(TextInputAction.send);
      await t.pump();
    }

    void pushDelta(FakeBridgeApi api) => api.pushEvent(
      svc,
      const AppEvent(
        kind: 'item/agentMessage/delta',
        threadId: 't1',
        itemId: 'a1',
        itemType: 'agentMessage',
        text: 'thinking…',
        raw: '{}',
      ),
    );

    void pushCompleted(FakeBridgeApi api) => api.pushEvent(
      svc,
      const AppEvent(kind: 'turn/completed', threadId: 't1', raw: '{}'),
    );

    testWidgets('Esc before any output undoes the send (restore, no marker)', (
      t,
    ) async {
      await onDesktop(() async {
        final api = await pumpStreaming(t);
        expect(find.byKey(const Key('stop-btn')), findsOneWidget);
        expect(composerText(t), '');

        await pressEsc(t);

        // Interrupted, the message is back in the box, and it was NOT resent.
        expect(api.interrupted, isTrue);
        expect(composerText(t), 'first');
        expect(api.turnStartCount, 1);

        // The aborted turn ends without a "stopped" marker — this was an undo.
        pushCompleted(api);
        await t.pumpAndSettle();
        expect(find.text('已停止'), findsNothing);
      });
    });

    testWidgets('Esc after output interrupts the turn (no restore, marker)', (
      t,
    ) async {
      await onDesktop(() async {
        final api = await pumpStreaming(t);
        pushDelta(api); // the model began replying → output has started
        await t.pump();

        await pressEsc(t);

        expect(api.interrupted, isTrue);
        expect(composerText(t), ''); // nothing restored
        expect(api.turnStartCount, 1);

        pushCompleted(api);
        await t.pumpAndSettle();
        expect(find.text('已停止'), findsOneWidget); // stopped marker shown
      });
    });

    testWidgets('a message sent mid-turn queues, then flushes on turn end', (
      t,
    ) async {
      await onDesktop(() async {
        final api = await pumpStreaming(t);
        pushDelta(api);
        await t.pump();

        await queue(t, 'second');
        expect(find.byKey(const Key('queued-0')), findsOneWidget);
        expect(find.text('second'), findsOneWidget);
        expect(api.lastTurnText, 'first'); // not sent yet

        // The running turn ends → the queued message flushes as the next turn.
        pushCompleted(api);
        await t.pump();
        expect(api.lastTurnText, 'second');
        expect(api.turnStartCount, 2);
        expect(find.byKey(const Key('queued-0')), findsNothing);
      });
    });

    testWidgets('Esc with a queued message pops it back before the turn', (
      t,
    ) async {
      await onDesktop(() async {
        final api = await pumpStreaming(t);
        pushDelta(api); // output started: a fall-through Esc would interrupt
        await t.pump();
        await queue(t, 'second');
        expect(find.byKey(const Key('queued-0')), findsOneWidget);

        // First Esc dequeues 'second' back to the composer — turn untouched.
        await pressEsc(t);
        expect(composerText(t), 'second');
        expect(find.byKey(const Key('queued-0')), findsNothing);
        expect(api.interrupted, isFalse);

        // Second Esc (queue now empty) interrupts the still-running turn.
        await pressEsc(t);
        expect(api.interrupted, isTrue);
      });
    });

    testWidgets('the ✕ on a queued chip discards it (not restored)', (t) async {
      await onDesktop(() async {
        final api = await pumpStreaming(t);
        await queue(t, 'second');
        expect(find.byKey(const Key('queued-0')), findsOneWidget);

        await t.tap(find.byKey(const Key('queued-remove-0')));
        await t.pump();
        expect(find.byKey(const Key('queued-0')), findsNothing);
        expect(composerText(t), ''); // discarded, not put back in the box

        // Nothing to flush when the turn ends.
        pushCompleted(api);
        await t.pump();
        expect(api.lastTurnText, 'first');
        expect(api.turnStartCount, 1);
      });
    });
  });

  group('a long thread loads its history a page at a time', () {
    // The rail is desktop-only by construction, and the test harness forces
    // android. Restored in a finally so the debug-var invariant check passes.
    Future<void> onDesktop(Future<void> Function() body) async {
      debugDefaultTargetPlatformOverride = TargetPlatform.windows;
      try {
        await body();
      } finally {
        debugDefaultTargetPlatformOverride = null;
      }
    }

    ThreadItem user(String id, String text, String turn) => ThreadItem(
      id: id,
      itemType: 'userMessage',
      title: '',
      text: text,
      turnId: turn,
    );
    ThreadItem agent(String id, String text, String turn) => ThreadItem(
      id: id,
      itemType: 'agentMessage',
      title: '',
      text: text,
      turnId: turn,
    );

    /// A thread whose newest turn is loaded and whose earlier turns are not.
    Future<FakeBridgeApi> openPaginated(WidgetTester t) async {
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect('pcx:lb7666:app:default', 28080);
      api.readResult = ThreadHistory(
        items: [
          user('u5', 'newest question', 't5'),
          agent('a5', 'newest answer', 't5'),
        ],
        running: false,
        hasOlder: true,
        // The server enumerated every turn, including the four not loaded.
        // Five of them, so the count clears kTurnMinimapMinItems and the rail
        // is the affordance rather than the corner arrows.
        turns: const [
          TurnSummary(
            turnId: 't1',
            userText: 'first question',
            assistantText: 'first answer',
          ),
          TurnSummary(
            turnId: 't2',
            userText: 'second question',
            assistantText: 'second answer',
          ),
          TurnSummary(
            turnId: 't3',
            userText: 'third question',
            assistantText: 'third answer',
          ),
          TurnSummary(
            turnId: 't4',
            userText: 'fourth question',
            assistantText: 'fourth answer',
          ),
          TurnSummary(
            turnId: 't5',
            userText: 'newest question',
            assistantText: 'newest answer',
            loaded: true,
          ),
        ],
      );
      await t.pumpWidget(
        host(
          const AppSessionScreen(
            serviceKey: 'pcx:lb7666:app:default',
            threadId: 'thread-long',
          ),
          api,
        ),
      );
      await t.pumpAndSettle();
      return api;
    }

    testWidgets('opening it shows the newest turn, not the whole history', (
      t,
    ) async {
      await openPaginated(t);
      expect(find.text('newest answer'), findsOneWidget);
      expect(find.text('first answer'), findsNothing);
      // And it says so, rather than looking like the conversation starts here.
      expect(find.byKey(const Key('chat-older-history')), findsOneWidget);
    });

    testWidgets('the rail has a tick per turn of the WHOLE thread', (t) async {
      await onDesktop(() async {
        await t.binding.setSurfaceSize(const Size(1600, 900));
        addTearDown(() => t.binding.setSurfaceSize(null));
        await openPaginated(t);
        // Three turns exist; only one is loaded. A rail derived from loaded rows
        // would show one tick and misreport the conversation's shape.
        final rail = t.widget<TurnMinimap>(find.byType(TurnMinimap));
        expect(rail.items, hasLength(5));
        expect(rail.items.map((i) => i.turnId), ['t1', 't2', 't3', 't4', 't5']);
        // Only the loaded turn resolves to a row; the rest await their items.
        expect(rail.items[0].rowIndex, -1);
        expect(rail.items[4].rowIndex, greaterThanOrEqualTo(0));
      });
    });

    testWidgets('an unloaded tick still previews, without fetching', (t) async {
      await onDesktop(() async {
        await t.binding.setSurfaceSize(const Size(1600, 900));
        addTearDown(() => t.binding.setSurfaceSize(null));
        final api = await openPaginated(t);
        final rail = t.widget<TurnMinimap>(find.byType(TurnMinimap));
        // The skeleton carries the text, so hovering costs no round trip.
        expect(rail.items[0].userText, 'first question');
        expect(rail.items[0].assistantText, 'first answer');
        expect(api.turnItemCalls, isEmpty);
      });
    });

    testWidgets('selecting an unloaded turn fetches just that turn', (t) async {
      await onDesktop(() async {
        await t.binding.setSurfaceSize(const Size(1600, 900));
        addTearDown(() => t.binding.setSurfaceSize(null));
        final api = await openPaginated(t);
        api.turnItems = {
          't1': [
            user('u1', 'first question', 't1'),
            agent('a1', 'first answer', 't1'),
          ],
        };
        final rail = t.widget<TurnMinimap>(find.byType(TurnMinimap));
        rail.onSelect(rail.items[0]);
        await t.pumpAndSettle();
        expect(api.turnItemCalls, ['t1']);
        expect(find.text('first answer'), findsOneWidget);
        // The newest turn is still there — a jump adds, it doesn't replace.
        expect(find.text('newest answer'), findsOneWidget);
      });
    });

    testWidgets('an older page prepends, keeping what was already shown', (
      t,
    ) async {
      final api = await openPaginated(t);
      api.olderPages = [
        [
          user('u2', 'second question', 't2'),
          agent('a2', 'second answer', 't2'),
        ],
      ];
      // Reaching back past the top. Tapped rather than scrolled because the
      // loaded page is shorter than the viewport here, so there is nothing to
      // scroll — which is exactly why the row is tappable.
      await t.tap(find.byKey(const Key('chat-older-history-load')));
      await t.pumpAndSettle();
      expect(api.olderPageCalls, 1);
      expect(find.text('second answer'), findsOneWidget);
      expect(find.text('newest answer'), findsOneWidget);
      // That was the last page, so the header retires.
      expect(find.byKey(const Key('chat-older-history')), findsNothing);
    });

    testWidgets('a thread that arrives whole pages nothing', (t) async {
      final api = FakeBridgeApi(
        config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      );
      await api.appConnect('pcx:lb7666:app:default', 28080);
      // A legacy thread: every item present, no turn enumeration, no older page.
      api.readResult = ThreadHistory(
        items: [
          user('u1', 'only question', 't1'),
          agent('a1', 'only answer', 't1'),
        ],
        running: false,
      );
      await t.pumpWidget(
        host(
          const AppSessionScreen(
            serviceKey: 'pcx:lb7666:app:default',
            threadId: 'thread-legacy',
          ),
          api,
        ),
      );
      await t.pumpAndSettle();
      expect(find.text('only answer'), findsOneWidget);
      expect(find.byKey(const Key('chat-older-history')), findsNothing);
      expect(api.olderPageCalls, 0);
    });
  });
}
