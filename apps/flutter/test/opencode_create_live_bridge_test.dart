// Opt-in acceptance of session creation through a packaged native bridge.
// The official service, Downloads directory and state are all test-owned.
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart'
    show ExternalLibrary;
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api_rust.dart';
import 'package:pocket_codex/src/opencode_controller.dart';
import 'package:pocket_codex/src/rust/api/bridge.dart' as frb;
import 'package:pocket_codex/src/rust/frb_generated.dart';

void main() {
  test(
    'packaged bridge creates and opens a session outside the server cwd',
    () async {
      final library = Platform.environment['PCX_OPENCODE_BRIDGE_LIBRARY'];
      final binary = Platform.environment['PCX_TEST_OPENCODE_BINARY'];
      expect(library != null && binary != null, isTrue);
      final root = await Directory.systemTemp.createTemp('pcx-create-test-');
      addTearDown(() => root.delete(recursive: true));
      for (final name in [
        'home/Downloads',
        'config',
        'data',
        'cache',
        'state',
        'support',
      ]) {
        await Directory('${root.path}/$name').create(recursive: true);
      }
      await RustLib.init(externalLibrary: ExternalLibrary.open(library!));
      addTearDown(RustLib.dispose);
      await frb.initBridge(supportDir: '${root.path}/support');

      final reservation = await ServerSocket.bind(
        InternetAddress.loopbackIPv4,
        0,
      );
      final port = reservation.port;
      await reservation.close();
      final random = Random.secure();
      final password = base64Url.encode(
        List.generate(32, (_) => random.nextInt(256)),
      );
      final process = await Process.start(
        binary!,
        ['serve', '--hostname', '127.0.0.1', '--port', '$port'],
        workingDirectory: '${root.path}/home',
        environment: {
          'HOME': '${root.path}/home',
          'XDG_CONFIG_HOME': '${root.path}/config',
          'XDG_DATA_HOME': '${root.path}/data',
          'XDG_CACHE_HOME': '${root.path}/cache',
          'XDG_STATE_HOME': '${root.path}/state',
          'OPENCODE_CONFIG_DIR': '${root.path}/config',
          'OPENCODE_SERVER_PASSWORD': password,
          'OPENCODE_DISABLE_AUTOUPDATE': 'true',
          'OPENCODE_DISABLE_MODELS_FETCH': 'true',
          'OPENCODE_CONFIG_CONTENT': '{"plugin":[]}',
        },
      );
      final stdoutDone = process.stdout.drain<void>();
      final stderrDone = process.stderr.drain<void>();
      addTearDown(() async {
        process.kill();
        await process.exitCode.timeout(
          const Duration(seconds: 5),
          onTimeout: () {
            process.kill(ProcessSignal.sigkill);
            return process.exitCode;
          },
        );
        await Future.wait([stdoutDone, stderrDone]);
      });

      final controller = OpenCodeController(const RustBridgeApi());
      addTearDown(() async {
        await controller.disconnect();
        controller.dispose();
      });
      final deadline = DateTime.now().add(const Duration(seconds: 20));
      while (true) {
        try {
          await controller.connect(
            baseUrl: 'http://127.0.0.1:$port',
            directory: '${root.path}/home/Downloads',
            password: password,
          );
          break;
        } catch (_) {
          if (DateTime.now().isAfter(deadline)) rethrow;
          await Future<void>.delayed(const Duration(milliseconds: 100));
        }
      }
      expect(controller.sessions, isEmpty);
      await controller.create();
      final created = controller.snapshot!;
      expect(created.state, 'ready');
      expect(created.messages, isEmpty);
      expect(created.writable, isTrue);
      expect(controller.sessions.single.id, created.sessionId);
      await controller.select(created.sessionId);
      expect(controller.snapshot!.sessionId, created.sessionId);
      expect(controller.snapshot!.state, 'ready');
    },
    skip: Platform.environment['PCX_RUN_REAL_OPENCODE'] != '1',
    timeout: const Timeout(Duration(minutes: 2)),
  );
}
