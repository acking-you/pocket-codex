import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/widgets/middle_click_scroll.dart';

import '../fake_bridge_api.dart';
import '../support/screen_harness.dart';

void main() {
  testWidgets('middle-click reads back through the real transcript viewport', (
    t,
  ) async {
    AppSessionScreen.debugResetThreadMemory();
    t.view.devicePixelRatio = 1;
    t.view.physicalSize = const Size(1200, 900);
    addTearDown(t.view.reset);
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
    );
    const service = 'pcx:lb7666:app:default';
    await api.appConnect(service, 28080);
    api.readResult = ThreadHistory(
      items: [
        for (var i = 0; i < 80; i++)
          ThreadItem(
            id: 'item-$i',
            turnId: 'turn-${i ~/ 2}',
            itemType: i.isEven ? 'userMessage' : 'agentMessage',
            title: '',
            text: 'Message $i\n\n${'Conversation history for reading. ' * 15}',
          ),
      ],
      running: false,
    );
    await t.pumpWidget(
      host(
        const AppSessionScreen(serviceKey: service, threadId: 'scroll-history'),
        api,
      ),
    );
    await t.pumpAndSettle();
    final viewport = t.getRect(
      find.byKey(const Key('chat-conversation-layer')),
    );
    final controller = t
        .widget<MiddleClickScroll>(find.byType(MiddleClickScroll))
        .controller;
    final before = controller.offset;
    expect(before, greaterThan(500));
    final mouse = await t.createGesture(
      kind: PointerDeviceKind.mouse,
      buttons: kMiddleMouseButton,
    );
    await mouse.addPointer(location: viewport.center);
    addTearDown(mouse.removePointer);
    await mouse.down(viewport.center);
    await mouse.up();
    await t.pump();
    expect(find.byKey(const Key('middle-click-scroll-anchor')), findsOneWidget);
    await mouse.moveTo(viewport.center - const Offset(0, 100));
    for (var i = 0; i < 15; i++) {
      await t.pump(const Duration(milliseconds: 16));
    }
    expect(controller.offset, lessThan(before));
    await t.sendKeyEvent(LogicalKeyboardKey.escape);
    await t.pumpAndSettle();
    expect(find.byKey(const Key('middle-click-scroll-anchor')), findsNothing);
    expect(t.takeException(), isNull);
  });
}
