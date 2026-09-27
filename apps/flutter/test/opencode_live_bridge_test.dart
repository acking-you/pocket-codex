// Opt-in, read-only acceptance of a packaged native bridge and existing sessions.
// No prompts, approvals, session creation, or process management are performed.
import 'dart:io';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart'
    show ExternalLibrary;
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api_rust.dart';
import 'package:pocket_codex/src/opencode_controller.dart';
import 'package:pocket_codex/src/rust/api/bridge.dart' as frb;
import 'package:pocket_codex/src/rust/frb_generated.dart';

void main() {
  test(
    'packaged bridge opens existing sessions and their pending interactions',
    () async {
      final library = Platform.environment['PCX_OPENCODE_BRIDGE_LIBRARY'];
      final directory = Platform.environment['PCX_OPENCODE_DIRECTORY'];
      expect(
        library != null && directory != null,
        isTrue,
        reason:
            'a packaged bridge and an existing project directory are required',
      );
      final support = await Directory.systemTemp.createTemp('pcx-bridge-test-');
      addTearDown(() => support.delete(recursive: true));
      await RustLib.init(externalLibrary: ExternalLibrary.open(library!));
      addTearDown(RustLib.dispose);
      await frb.initBridge(supportDir: support.path);
      final controller = OpenCodeController(const RustBridgeApi());
      addTearDown(() async {
        await controller.disconnect();
        controller.dispose();
      });

      await controller.connect(directory: directory!);
      expect(
        controller.sessions.isNotEmpty,
        isTrue,
        reason: 'the selected project must contain existing sessions',
      );
      final sessions = controller.sessions.take(3).toList();
      var messageCount = 0;
      for (final session in sessions) {
        await controller.select(session.id);
        final snapshot = controller.snapshot!;
        expect(snapshot.sessionId == session.id, isTrue);
        expect(snapshot.state, 'ready');
        messageCount += snapshot.messages.length;
        if (snapshot.nextCursor != null) {
          await controller.older();
          expect(
            controller.snapshot!.messages.length >= snapshot.messages.length,
            isTrue,
          );
        }
      }
      expect(
        messageCount > 0,
        isTrue,
        reason: 'acceptance must read real history, not just empty sessions',
      );
      await controller.select(sessions.first.id);
      expect(controller.snapshot!.sessionId == sessions.first.id, isTrue);
    },
    skip: Platform.environment['PCX_LIVE_OPENCODE'] != '1',
    timeout: const Timeout(Duration(minutes: 2)),
  );
}
