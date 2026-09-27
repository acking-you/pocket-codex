import 'dart:async';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/opencode_api.dart';
import 'package:pocket_codex/src/opencode_controller.dart';

class FakeOpenCodeApi extends OpenCodeApi {
  final sent = <String>[];
  OpenCodeSubmission submission = OpenCodeSubmission.accepted;
  Completer<OpenCodeSubmission>? sending;
  OpenCodeSnapshot current = const OpenCodeSnapshot(sessionId: 'a');
  final opened = <String, Completer<OpenCodeSnapshot>>{};
  final stream = StreamController<OpenCodeSnapshot>.broadcast(sync: true);
  final decisions = <String>[];
  List<List<String>>? answers;
  Map<String, dynamic>? formAnswers;
  String? search;
  Completer<String>? connecting;
  final disconnected = <String>[];
  final connections = <Map<String, Object?>>[];
  Object? connectionError;
  @override
  Future<String> connect({
    String? baseUrl,
    String? serviceKey,
    required String directory,
    String username = 'opencode',
    String? password,
  }) async {
    connections.add({
      'baseUrl': baseUrl,
      'serviceKey': serviceKey,
      'directory': directory,
      'username': username,
      'password': password,
    });
    if (connectionError case final error?) throw error;
    return connecting?.future ?? 'connection';
  }

  @override
  Future<List<OpenCodeSession>> sessions(
    String connectionId, {
    String? search,
  }) async {
    this.search = search;
    return [const OpenCodeSession(id: 'a', title: 'Session A')];
  }

  @override
  Future<OpenCodeSnapshot> older(String connectionId, String sessionId) async =>
      const OpenCodeSnapshot(
        sessionId: 'a',
        messages: [
          {
            'info': {'id': 'older'},
            'parts': [
              {'type': 'text', 'text': 'Older message'},
            ],
          },
        ],
      );
  @override
  Future<OpenCodeSnapshot> create(String connectionId) async =>
      const OpenCodeSnapshot(sessionId: 'new');
  @override
  Future<OpenCodeSnapshot> openSession(
    String connectionId,
    String sessionId,
  ) async => opened[sessionId]?.future ?? current;
  @override
  Future<OpenCodeSubmission> send(
    String connectionId,
    String sessionId,
    String text,
  ) async {
    sent.add(text);
    return sending?.future ?? submission;
  }

  @override
  Stream<OpenCodeSnapshot> events(String connectionId) => stream.stream;
  @override
  Future<void> disconnect(String connectionId) async {
    disconnected.add(connectionId);
  }

  @override
  Future<void> permissionReply(
    String connectionId,
    String requestId,
    String reply,
  ) async {
    decisions.add('$requestId:$reply');
  }

  @override
  Future<void> questionReply(
    String connectionId,
    String requestId,
    List<List<String>> answers,
  ) async {
    this.answers = answers;
  }

  @override
  Future<void> formReply(
    String connectionId,
    String requestId,
    Map<String, dynamic> answers,
  ) async {
    formAnswers = answers;
  }

  @override
  Future<void> questionReject(String connectionId, String requestId) async {
    decisions.add('$requestId:reject');
  }

  @override
  Future<void> abort(String connectionId, String sessionId) async {
    decisions.add('$sessionId:abort');
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  test('a late submission cannot erase a different session draft', () async {
    final api = FakeOpenCodeApi()..sending = Completer();
    final controller = OpenCodeController(api);
    await controller.connect(directory: '/work');
    await controller.select('a');
    controller.draft = 'Same text';
    final pending = controller.send();
    api.current = const OpenCodeSnapshot(sessionId: 'b');
    await controller.select('b');
    controller.draft = 'Same text';
    api.sending!.complete(OpenCodeSubmission.accepted);
    await pending;
    expect(controller.draft, 'Same text');
    controller.dispose();
  });
  test('disconnect releases its connection without abort', () async {
    final api = FakeOpenCodeApi();
    final controller = OpenCodeController(api);
    await controller.connect(directory: '/work');
    await controller.select('a');
    await controller.disconnect();
    expect(api.disconnected, ['connection']);
    expect(api.decisions, isEmpty);
    controller.dispose();
  });
  test('leaving while connecting releases the eventual connection', () async {
    final api = FakeOpenCodeApi()..connecting = Completer();
    final controller = OpenCodeController(api);
    final pending = controller.connect(directory: '/work');
    controller.dispose();
    api.connecting!.complete('late');
    await pending;
    expect(api.disconnected, ['late']);
  });
  test('lost event stream preserves history but disables mutations', () async {
    final api = FakeOpenCodeApi();
    final controller = OpenCodeController(api);
    await controller.connect(directory: '/work');
    await controller.select('a');
    api.stream.addError(StateError('connection lost'));
    await Future<void>.delayed(Duration.zero);
    expect(controller.snapshot?.writable, isFalse);
    expect(controller.snapshot?.sessionId, 'a');
    controller.dispose();
  });
  test(
    'draft and unknown submission remain attached to their session',
    () async {
      final api = FakeOpenCodeApi()..submission = OpenCodeSubmission.unknown;
      final controller = OpenCodeController(api);
      await controller.connect(directory: '/work');
      await controller.select('a');
      controller.draft = 'Keep A';
      await controller.send();
      api.current = const OpenCodeSnapshot(sessionId: 'b');
      await controller.select('b');
      expect(controller.draft, '');
      expect(controller.submissionUnknown, isFalse);
      controller.draft = 'Keep B';
      api.current = const OpenCodeSnapshot(sessionId: 'a');
      await controller.select('a');
      expect(controller.draft, 'Keep A');
      expect(controller.submissionUnknown, isTrue);
      controller.dispose();
    },
  );
  test(
    'calibrated events replace text and stale state preserves drafts read-only',
    () async {
      final api = FakeOpenCodeApi();
      final controller = OpenCodeController(api);
      await controller.connect(directory: '/work');
      await controller.select('a');
      controller.draft = 'Next';
      api.stream.add(
        const OpenCodeSnapshot(sessionId: 'a', status: 'busy', revision: 1),
      );
      expect(controller.snapshot?.busy, isTrue);
      await controller.send();
      expect(api.sent, isEmpty);
      api.stream.add(
        const OpenCodeSnapshot(sessionId: 'a', state: 'stale', revision: 2),
      );
      await controller.send();
      expect(controller.draft, 'Next');
      expect(api.sent, isEmpty);
      controller.dispose();
      await api.stream.close();
    },
  );
  test('a slow obsolete session cannot replace the newer selection', () async {
    final api = FakeOpenCodeApi();
    api.opened['a'] = Completer();
    api.opened['b'] = Completer();
    final controller = OpenCodeController(api);
    await controller.connect(directory: '/work');
    final a = controller.select('a');
    final b = controller.select('b');
    api.opened['b']!.complete(const OpenCodeSnapshot(sessionId: 'b'));
    await b;
    api.opened['a']!.complete(const OpenCodeSnapshot(sessionId: 'a'));
    await a;
    expect(controller.snapshot?.sessionId, 'b');
    controller.dispose();
  });
  test(
    'an unknown submission retains the draft and cannot be automatically resent',
    () async {
      final api = FakeOpenCodeApi()..submission = OpenCodeSubmission.unknown;
      final controller = OpenCodeController(api);
      await controller.connect(directory: '/work');
      await controller.select('a');
      controller.draft = 'Continue';
      await controller.send();
      await controller.send();
      expect(api.sent, ['Continue']);
      expect(controller.draft, 'Continue');
      expect(controller.submissionUnknown, isTrue);
      controller.dispose();
    },
  );
  test('a direct connection lists and opens a session without Codex', () async {
    final controller = OpenCodeController(FakeOpenCodeApi());
    await controller.connect(
      baseUrl: 'http://localhost:4096',
      directory: '/work',
    );
    expect(controller.sessions.single.title, 'Session A');
    await controller.select('a');
    expect(controller.snapshot?.sessionId, 'a');
    controller.dispose();
  });
}
