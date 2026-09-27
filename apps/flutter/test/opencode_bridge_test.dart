import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api_rust.dart';
import 'package:pocket_codex/src/rust/api/bridge.dart';
import 'package:pocket_codex/src/rust/frb_generated.dart';

class _OpenCodeWireApi extends RustLibApi {
  String? connectionId;
  String? requestId;
  String? answersJson;

  @override
  Future<OpenCodeSnapshotDto> crateApiBridgeOpencodeReplyForm({
    required String connectionId,
    required String requestId,
    required String answersJson,
  }) async {
    this.connectionId = connectionId;
    this.requestId = requestId;
    this.answersJson = answersJson;
    return OpenCodeSnapshotDto(
      sessionId: 'form',
      messagesJson: '[]',
      status: 'idle',
      permissionsJson: '[]',
      questionsJson: '[]',
      revision: BigInt.zero,
      state: 'ready',
    );
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  test(
    'RustBridgeApi retains native form values across the generated boundary',
    () async {
      final wire = _OpenCodeWireApi();
      RustLib.initMock(api: wire);
      addTearDown(RustLib.dispose);
      const answers = <String, dynamic>{
        'name': 'Pocket',
        'count': 2,
        'ratio': 0.5,
        'enabled': false,
        'targets': ['mac', 'linux'],
      };
      await const RustBridgeApi().formReply('connection', 'form', answers);
      expect(wire.connectionId, 'connection');
      expect(wire.requestId, 'form');
      expect(jsonDecode(wire.answersJson!), answers);
    },
  );
}
