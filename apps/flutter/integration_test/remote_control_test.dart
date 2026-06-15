// Live end-to-end test of codex app-server REMOTE CONTROL through the real
// bridge → pb-mapper relay → remote app-server → model.
//
// This is an ONLINE test: it needs a reachable relay, a registered
// `pcx:*:app:*` service, and a logged-in codex on the host. It is gated on
// --dart-define so CI / offline runs skip it instead of failing:
//
//   fvm flutter test integration_test/remote_control_test.dart -d windows \
//     --dart-define=PCX_RELAY=lb7666.top:7666 \
//     --dart-define=PCX_KEY=<32-byte MSG_HEADER_KEY> \
//     [--dart-define=PCX_PORT=28092]
//
// It drives the exact bridge surface the Flutter UI calls, so a pass proves
// the remote-control UI path works against a real backend.

import 'dart:async';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:pocket_codex/src/rust/api/bridge.dart' as frb;
import 'package:pocket_codex/src/rust/frb_generated.dart';

const _relay = String.fromEnvironment('PCX_RELAY');
const _key = String.fromEnvironment('PCX_KEY');
const _port = int.fromEnvironment('PCX_PORT', defaultValue: 28092);

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async => RustLib.init());

  testWidgets('drives a remote app-server turn and gets a model reply', (
    tester,
  ) async {
    if (_relay.isEmpty || _key.isEmpty) {
      // No live backend configured — skip rather than fail.
      markTestSkipped('PCX_RELAY / PCX_KEY not provided; skipping online test');
      return;
    }

    // Isolate from the real app config: bridge writes config.toml under here.
    final dir = Directory.systemTemp.createTempSync('pcx-rc-it');
    await frb.initBridge(supportDir: dir.path);
    await frb.setRelay(relay: _relay);
    await frb.setKey(key: _key);

    // Discover the app-server service on the relay.
    final services = await frb.discoverServices();
    final app = services.where((s) => s.kind == 'app').toList();
    expect(app, isNotEmpty, reason: 'no app-server service on the relay');
    final key = app.first.key;

    // Connect (subscribe + ws + initialize), start a thread, send a turn.
    await frb.appConnect(serviceKey: key, localPort: _port);

    // model/list should return at least one model from the real app-server.
    final models = await frb.appModelList(serviceKey: key);
    expect(models, isNotEmpty, reason: 'model/list returned nothing');

    // Single subscription (the FRB stream is single-listen, as the UI uses it):
    // accumulate agent text and complete when the turn finishes.
    final reply = StringBuffer();
    final done = Completer<void>();
    final sub = frb.appEvents(serviceKey: key).listen((e) {
      if (e.itemType == 'agentMessage' && (e.text?.isNotEmpty ?? false)) {
        reply.write(e.text);
      }
      if ((e.kind == 'turn/completed' || e.kind == 'turn/failed') &&
          !done.isCompleted) {
        done.complete();
      }
    });

    // Start with explicit model + cwd + a non-blocking permission preset, to
    // prove those params reach the real app-server.
    final threadId = await frb.appThreadStart(
      serviceKey: key,
      model: models.first.id,
      cwd: null,
      approvalPolicy: 'never',
      sandbox: 'workspace-write',
    );
    expect(threadId, isNotEmpty);
    await frb.appTurnStart(
      serviceKey: key,
      threadId: threadId,
      text: 'Reply with exactly one word: remote-ui-ok',
      collaborationMode: null,
    );

    // Wait for the turn to complete (model streams back through the relay).
    await done.future.timeout(const Duration(seconds: 90));
    await sub.cancel();
    expect(
      reply.toString().trim(),
      isNotEmpty,
      reason: 'expected a streamed model reply',
    );

    await frb.appDisconnect(serviceKey: key);
    dir.deleteSync(recursive: true);
  });
}
