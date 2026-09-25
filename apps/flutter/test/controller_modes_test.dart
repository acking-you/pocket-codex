import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/widgets/turn_minimap.dart';
import 'package:flutter/foundation.dart';
import 'package:image_picker_platform_interface/image_picker_platform_interface.dart';
import 'package:pocket_codex/src/image_attachments.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';

import 'fake_bridge_api.dart';
import 'support/screen_harness.dart';

const service = 'pcx:mode-test:app:default';
const fastModel = ModelInfo(
  id: 'fast-capable',
  displayName: 'Fast capable',
  description: '',
  supportedServiceTiers: ['priority'],
  isDefault: true,
);

Future<void> frames(WidgetTester t) async {
  for (var i = 0; i < 8; i++) {
    await t.pump(const Duration(milliseconds: 50));
  }
}

class ModesApi extends FakeBridgeApi {
  List<ModelInfo> catalog = [fastModel];
  int modelFailures = 0;
  int modelCalls = 0;
  @override
  Future<List<ModelInfo>> appModelList(String serviceKey) async {
    modelCalls++;
    if (modelFailures-- > 0) throw StateError('connection not ready');
    return catalog;
  }
}

Future<void> open(WidgetTester t, ModesApi api, {String? thread}) async {
  await api.appConnect(service, 28080);
  await t.pumpWidget(
    host(AppSessionScreen(serviceKey: service, threadId: thread), api),
  );
  await frames(t);
}

Future<void> send(WidgetTester t, String text) async {
  await t.enterText(find.byKey(const Key('composer-input')), text);
  await t.pump();
  await t.tap(find.byKey(const Key('send-btn')));
  await frames(t);
}

Future<void> running(WidgetTester t, ModesApi api) async {
  api.autoCompleteTurn = false;
  await open(t, api);
  await send(t, 'Original goal');
  api.pushEvent(
    service,
    const AppEvent(
      kind: 'turn/started',
      threadId: 'thread-0',
      raw: '{"turn":{"id":"turn-1"}}',
    ),
  );
  await frames(t);
}

String draft(WidgetTester t) => t
    .widget<TextField>(find.byKey(const Key('composer-input')))
    .controller!
    .text;

FakeImagePicker imagePicker() {
  final original = ImagePickerPlatform.instance;
  final picker = FakeImagePicker();
  ImagePickerPlatform.instance = picker;
  processImageImpl = (bytes) async => processImageBytes(bytes);
  addTearDown(() {
    ImagePickerPlatform.instance = original;
    processImageImpl = (bytes) => compute(processImageBytes, bytes);
  });
  return picker;
}

Future<void> attach(WidgetTester t, FakeImagePicker picker, String name) async {
  picker.files = [MemXFile(tinyPng(), name)];
  await t.tap(find.byKey(const Key('attach-menu-btn')));
  await frames(t);
  await t.tap(find.byKey(const Key('attach-btn')));
  await frames(t);
}

void complete(ModesApi api) => api.pushEvent(
  service,
  const AppEvent(kind: 'turn/completed', threadId: 'thread-0', raw: '{}'),
);

void main() {
  setUp(AppSessionScreen.debugResetThreadMemory);

  testWidgets(
    'new sessions use auto review and manual mode resets the reviewer',
    (t) async {
      final api = ModesApi();
      await open(t, api);
      await send(t, 'First');
      expect(api.lastApprovalsReviewer, 'auto_review');
      expect(api.lastApproval, 'on-request');
      expect(api.lastSandbox, 'workspace-write');
      await t.tap(find.byKey(const Key('permission-chip')));
      await frames(t);
      await t.tap(find.byKey(const Key('opt-auto')));
      await frames(t);
      await send(t, 'Second');
      expect(api.lastApprovalsReviewer, 'user');
    },
  );

  testWidgets(
    'Fast follows catalog capability and toggles priority to explicit default',
    (t) async {
      final api = ModesApi();
      await open(t, api);
      expect(find.byKey(const Key('fast-mode-btn')), findsOneWidget);
      await t.tap(find.byKey(const Key('fast-mode-btn')));
      await send(t, 'Fast turn');
      expect(api.lastServiceTier, 'priority');
      expect(api.threadConfigs['thread-0']?.serviceTier, 'priority');
      await t.tap(find.byKey(const Key('fast-mode-btn')));
      await send(t, 'Standard turn');
      expect(api.lastServiceTier, 'default');
    },
  );

  testWidgets('unsupported models do not offer Fast', (t) async {
    final api = ModesApi()
      ..catalog = [
        const ModelInfo(
          id: 'ordinary',
          displayName: 'Ordinary',
          description: '',
          isDefault: true,
        ),
      ];
    await open(t, api);
    expect(find.byKey(const Key('fast-mode-btn')), findsNothing);
  });

  testWidgets('resumed Auto and Fast preserve the server reported settings', (
    t,
  ) async {
    final api = ModesApi()
      ..readResult = const ThreadHistory(
        items: [],
        running: false,
        model: 'fast-capable',
        approvalPolicy: 'on-request',
        sandboxMode: 'workspace-write',
        approvalsReviewer: 'auto_review',
        serviceTier: 'priority',
      );
    await open(t, api, thread: 'resumed');
    expect(
      t.widget<FilterChip>(find.byKey(const Key('fast-mode-btn'))).selected,
      isTrue,
    );
    await send(t, 'Continue');
    expect(api.lastApprovalsReviewer, 'auto_review');
    expect(api.lastServiceTier, 'priority');
  });

  testWidgets('running send queues a new turn and preserves a newer draft', (
    t,
  ) async {
    final api = ModesApi();
    await running(t, api);
    await send(t, 'Next goal');
    expect(api.turnStartCount, 1);
    expect(api.lastSteerText, isNull);
    await t.enterText(
      find.byKey(const Key('composer-input')),
      'Still drafting',
    );
    api.pushEvent(
      service,
      const AppEvent(kind: 'turn/completed', threadId: 'thread-0', raw: '{}'),
    );
    await frames(t);
    expect(api.turnStartCount, 2);
    expect(api.lastTurnText, 'Next goal');
    expect(draft(t), 'Still drafting');
  });

  testWidgets(
    'supplement targets the current turn without starting another turn',
    (t) async {
      final api = ModesApi();
      await running(t, api);
      await t.tap(find.byKey(const Key('supplement-toggle')));
      await send(t, 'Extra constraint');
      expect(api.lastSteerText, 'Extra constraint');
      expect(api.lastSteerTurnId, 'turn-1');
      expect(api.turnStartCount, 1);
      expect(draft(t), isEmpty);
      expect(
        t
            .widget<FilterChip>(find.byKey(const Key('supplement-toggle')))
            .selected,
        isFalse,
      );
      expect(find.byKey(const Key('stop-btn')), findsOneWidget);
    },
  );

  testWidgets(
    'failed supplement keeps the draft and never falls back to a new turn',
    (t) async {
      final api = ModesApi()..steerError = 'expectedTurnId mismatch';
      await running(t, api);
      await t.tap(find.byKey(const Key('supplement-toggle')));
      await send(t, 'Keep this draft');
      expect(draft(t), 'Keep this draft');
      expect(api.turnStartCount, 1);
    },
  );

  testWidgets(
    'late supplement acknowledgement preserves a changed draft and finished turn',
    (t) async {
      final api = ModesApi()..steerGate = Completer<void>();
      await running(t, api);
      await t.tap(find.byKey(const Key('supplement-toggle')));
      await send(t, 'Supplement');
      await t.enterText(find.byKey(const Key('composer-input')), 'New draft');
      api.pushEvent(
        service,
        const AppEvent(kind: 'turn/completed', threadId: 'thread-0', raw: '{}'),
      );
      await frames(t);
      api.steerGate!.complete();
      await frames(t);
      expect(draft(t), 'New draft');
      expect(find.byKey(const Key('stop-btn')), findsNothing);
      expect(api.turnStartCount, 1);
    },
  );
  testWidgets(
    'changing to a model without Fast clears the requested tier',
    (t) async {
      final api = ModesApi()
        ..catalog = [
          fastModel,
          const ModelInfo(
            id: 'ordinary',
            displayName: 'Ordinary',
            description: '',
          ),
        ];
      await open(t, api);
      await t.tap(find.byKey(const Key('fast-mode-btn')));
      await t.tap(find.byKey(const Key('model-chip')));
      await frames(t);
      await t.tap(find.byKey(const Key('model-menu-item-ordinary')));
      await frames(t);
      expect(find.byKey(const Key('fast-mode-btn')), findsNothing);
      await send(t, 'Use ordinary model');
      expect(api.lastTurnModel, 'ordinary');
      expect(api.lastServiceTier, 'default');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.linux),
  );

  testWidgets('supplements carry image attachments', (t) async {
    final originalPicker = ImagePickerPlatform.instance;
    final picker = FakeImagePicker()
      ..files = [MemXFile(tinyPng(), 'supplement.png')];
    ImagePickerPlatform.instance = picker;
    processImageImpl = (bytes) async => processImageBytes(bytes);
    addTearDown(() {
      ImagePickerPlatform.instance = originalPicker;
      processImageImpl = (bytes) => compute(processImageBytes, bytes);
    });
    final api = ModesApi();
    await running(t, api);
    await t.tap(find.byKey(const Key('attach-menu-btn')));
    await frames(t);
    await t.tap(find.byKey(const Key('attach-btn')));
    await frames(t);
    await t.tap(find.byKey(const Key('supplement-toggle')));
    await send(t, 'Use this image');
    expect(api.lastSteerImages, hasLength(1));
    expect(api.lastSteerImages.single, startsWith('data:image/'));
    expect(api.turnStartCount, 1);
  });

  for (final width in [360.0, 800.0, 1280.0]) {
    for (final dark in [false, true]) {
      testWidgets(
        'turn controls fit $width px in ${dark ? 'dark' : 'light'} theme',
        (t) async {
          t.view.devicePixelRatio = 1;
          t.view.physicalSize = Size(width, 900);
          addTearDown(t.view.reset);
          final api = ModesApi()..autoCompleteTurn = false;
          await api.appConnect(service, 28080);
          await t.pumpWidget(
            host(
              Theme(
                data: dark ? darkTheme() : lightTheme(),
                child: const AppSessionScreen(serviceKey: service),
              ),
              api,
            ),
          );
          await frames(t);
          await send(t, 'Start');
          await t.enterText(find.byKey(const Key('composer-input')), 'Extra');
          await frames(t);
          for (final key in [
            'fast-mode-btn',
            'supplement-toggle',
            'send-btn',
            'stop-btn',
          ]) {
            expect(find.byKey(Key(key)).hitTestable(), findsOneWidget);
          }
          expect(t.takeException(), isNull);
        },
      );
    }
  }
  testWidgets('a history refresh during a supplement releases the send lock', (
    t,
  ) async {
    final api = ModesApi()..steerGate = Completer<void>();
    await running(t, api);
    await t.tap(find.byKey(const Key('supplement-toggle')));
    await send(t, 'Accepted supplement');
    api.readResult = const ThreadHistory(
      items: [
        ThreadItem(
          id: 'server-supplement',
          itemType: 'userMessage',
          title: '',
          text: 'Accepted supplement',
          turnId: 'turn-1',
        ),
      ],
      running: true,
    );
    api.pushEvent(
      service,
      const AppEvent(kind: 'thread/compacted', threadId: 'thread-0', raw: '{}'),
    );
    await frames(t);
    api.steerGate!.complete();
    await frames(t);
    expect(draft(t), isEmpty);
    expect(find.text('Accepted supplement'), findsOneWidget);
    await send(t, 'Next queued goal');
    expect(draft(t), isEmpty);
    expect(api.turnStartCount, 1);
  });
  testWidgets(
    'send preflight retains queued text and attachments during setup',
    (t) async {
      final picker = imagePicker();
      final api = ModesApi()
        ..autoCompleteTurn = false
        ..codexStatus = const CodexSetupStatus(
          codexHome: '/fake',
          hasConfig: true,
          hasAuth: true,
          hasCustomProvider: false,
          needsSetup: false,
          promptVariant: 'default',
        );
      api.serveHosts.add(
        const AppServeStatus(
          name: 'local',
          device: 'test',
          alive: true,
          appServiceKey: service,
        ),
      );
      await api.appConnect(service, 28080);
      await t.pumpWidget(
        routerHost(
          api,
          initial: '/',
          routes: [
            GoRoute(
              path: '/',
              builder: (_, _) => const AppSessionScreen(serviceKey: service),
            ),
            GoRoute(
              path: '/setup/codex',
              builder: (context, _) => Scaffold(
                body: TextButton(
                  onPressed: () => context.pop(),
                  child: const Text('Return to chat'),
                ),
              ),
            ),
          ],
        ),
      );
      await frames(t);
      await send(t, 'Original goal');
      await attach(t, picker, 'queued.png');
      await send(t, 'Queued goal');
      expect(find.byKey(const Key('queued-0')), findsOneWidget);
      api.codexStatus = const CodexSetupStatus(
        codexHome: '/fake',
        hasConfig: true,
        hasAuth: false,
        hasCustomProvider: false,
        needsSetup: true,
        promptVariant: 'default',
      );
      ProviderScope.containerOf(
        t.element(find.byType(AppSessionScreen)),
      ).invalidate(codexSetupStatusProvider);
      await frames(t);
      complete(api);
      await frames(t);
      expect(find.text('Return to chat'), findsOneWidget);
      expect(api.turnStartCount, 1);
      api.codexStatus = const CodexSetupStatus(
        codexHome: '/fake',
        hasConfig: true,
        hasAuth: true,
        hasCustomProvider: false,
        needsSetup: false,
        promptVariant: 'default',
      );
      await t.tap(find.text('Return to chat'));
      await t.pumpAndSettle();
      expect(find.byKey(const Key('queued-0')), findsOneWidget);
      await t.tap(find.byKey(const Key('queued-0')));
      await frames(t);
      expect(draft(t), 'Queued goal');
      await send(t, draft(t));
      expect(api.turnStartCount, 2);
      expect(api.lastTurnText, 'Queued goal');
      expect(api.lastTurnImages, hasLength(1));
    },
  );

  testWidgets(
    'undo of a drained turn preserves both drafts and both attachments',
    (t) async {
      final picker = imagePicker();
      final api = ModesApi();
      await running(t, api);
      await attach(t, picker, 'queued.png');
      await send(t, 'Queued goal');
      await attach(t, picker, 'new-draft.png');
      await t.enterText(find.byKey(const Key('composer-input')), 'New draft');
      complete(api);
      await frames(t);
      expect(api.turnStartCount, 2);
      expect(draft(t), 'New draft');
      await t.tap(find.byKey(const Key('composer-input')));
      await t.sendKeyEvent(LogicalKeyboardKey.escape);
      await frames(t);
      expect(api.interrupted, isTrue);
      expect(draft(t), 'Queued goal\n\nNew draft');
      complete(api);
      await frames(t);
      expect(api.turnStartCount, 2);
      await send(t, draft(t));
      expect(api.lastTurnText, 'Queued goal\n\nNew draft');
      expect(api.lastTurnImages, hasLength(2));
    },
    variant: TargetPlatformVariant.only(TargetPlatform.linux),
  );

  for (final summaries in [true, false]) {
    testWidgets(
      'resumed supplements keep one entry per physical turn (summaries: $summaries)',
      (t) async {
        t.view.devicePixelRatio = 1;
        t.view.physicalSize = const Size(1600, 1000);
        addTearDown(t.view.reset);
        final api = ModesApi()
          ..resolvedSteerTurnId = 'turn-5'
          ..readResult = ThreadHistory(
            items: [
              for (var i = 1; i <= 5; i++)
                ThreadItem(
                  id: 'user-$i',
                  itemType: 'userMessage',
                  title: '',
                  text: 'Goal $i',
                  turnId: 'turn-$i',
                ),
            ],
            turns: summaries
                ? [
                    for (var i = 1; i <= 5; i++)
                      TurnSummary(
                        turnId: 'turn-$i',
                        userText: 'Goal $i',
                        loaded: true,
                      ),
                  ]
                : [],
            running: true,
          );
        await open(t, api, thread: 'resumed');
        expect(
          t.widget<TurnMinimap>(find.byType(TurnMinimap)).items,
          hasLength(5),
        );
        await t.tap(find.byKey(const Key('supplement-toggle')));
        await send(t, 'More detail');
        expect(api.lastSteerTurnId, isNull);
        expect(api.lastSteerText, 'More detail');
        expect(api.turnStartCount, 0);
        final rail = t.widget<TurnMinimap>(find.byType(TurnMinimap));
        expect(rail.items, hasLength(5));
        expect(rail.items.last.turnId, 'turn-5');
        await t.tap(find.byKey(const Key('stop-btn')));
        await frames(t);
        expect(api.lastInterruptTurnId, 'turn-5');
      },
      variant: TargetPlatformVariant.only(TargetPlatform.linux),
    );
  }

  testWidgets(
    'mobile default model clears a previously active Fast tier',
    (t) async {
      final api = ModesApi()
        ..catalog = [
          const ModelInfo(
            id: 'ordinary',
            displayName: 'Ordinary',
            description: '',
            isDefault: true,
          ),
          const ModelInfo(
            id: 'fast-capable',
            displayName: 'Fast capable',
            description: '',
            supportedServiceTiers: ['priority'],
          ),
        ]
        ..readResult = const ThreadHistory(
          items: [],
          running: false,
          model: 'fast-capable',
          serviceTier: 'priority',
        );
      await open(t, api, thread: 'resumed');
      expect(
        t.widget<FilterChip>(find.byKey(const Key('fast-mode-btn'))).selected,
        isTrue,
      );
      await turnSetting(t, 'model');
      await t.tap(find.text('默认模型'));
      await frames(t);
      expect(find.byKey(const Key('fast-mode-btn')), findsNothing);
      expect(api.threadConfigs['resumed']?.serviceTier, 'default');
      await turnSetting(t, 'model');
      await t.tap(find.byKey(const ValueKey('opt-Fast capable')));
      await frames(t);
      expect(
        t.widget<FilterChip>(find.byKey(const Key('fast-mode-btn'))).selected,
        isFalse,
      );
    },
    variant: TargetPlatformVariant.only(TargetPlatform.android),
  );

  testWidgets('Fast appears after a failed cold-open catalog request retries', (
    t,
  ) async {
    final api = ModesApi()..modelFailures = 1;
    await open(t, api);
    expect(api.modelCalls, 1);
    expect(find.byKey(const Key('fast-mode-btn')), findsNothing);
    await t.pump(const Duration(seconds: 1));
    await frames(t);
    expect(api.modelCalls, 2);
    expect(find.byKey(const Key('fast-mode-btn')), findsOneWidget);
    await t.pump(const Duration(seconds: 32));
    expect(api.modelCalls, 2);
  });

  testWidgets('cold-open catalog retries are bounded', (t) async {
    final api = ModesApi()..modelFailures = 100;
    await open(t, api);
    for (final seconds in [1, 2, 4, 8, 16]) {
      await t.pump(Duration(seconds: seconds));
      await frames(t);
    }
    expect(api.modelCalls, 6);
    await t.pump(const Duration(seconds: 60));
    await frames(t);
    expect(api.modelCalls, 6);
  });
}
