/// OpenCode controller contracts, independent of generated bridge bindings.
library;

enum OpenCodeSubmission { accepted, unknown }

class OpenCodeSession {
  const OpenCodeSession({required this.id, required this.title});
  final String id;
  final String title;
}

/// An authoritative, calibrated window. Partial SSE deltas never reach the UI.
class OpenCodeSnapshot {
  const OpenCodeSnapshot({
    required this.sessionId,
    this.messages = const [],
    this.status = 'idle',
    this.permissions = const [],
    this.questions = const [],
    this.nextCursor,
    this.state = 'ready',
    this.revision = 0,
  });

  final String sessionId;
  final List<Map<String, dynamic>> messages;
  final String status;
  final List<Map<String, dynamic>> permissions;
  final List<Map<String, dynamic>> questions;
  final String? nextCursor;
  final String state;
  final int revision;
  bool get writable => state == 'ready';
  bool get busy => status != 'idle';
  OpenCodeSnapshot withState(String state) => OpenCodeSnapshot(
    sessionId: sessionId,
    messages: messages,
    status: status,
    permissions: permissions,
    questions: questions,
    nextCursor: nextCursor,
    state: state,
    revision: revision,
  );
}

/// The FRB seam. Passwords live only during the connect call and in Rust memory.
abstract class OpenCodeApi {
  Future<String> connect({
    String? baseUrl,
    String? serviceKey,
    required String directory,
    String username = 'opencode',
    String? password,
  });
  Future<List<OpenCodeSession>> sessions(String connectionId, {String? search});
  Future<OpenCodeSnapshot> openSession(String connectionId, String sessionId);
  Future<OpenCodeSnapshot> older(String connectionId, String sessionId);
  Future<OpenCodeSnapshot> create(String connectionId);
  Future<OpenCodeSubmission> send(
    String connectionId,
    String sessionId,
    String text,
  );
  Future<void> permissionReply(
    String connectionId,
    String requestId,
    String reply,
  );
  Future<void> questionReply(
    String connectionId,
    String requestId,
    List<List<String>> answers,
  );

  /// Replies to a native v2 form without coercing its typed values.
  Future<void> formReply(
    String connectionId,
    String requestId,
    Map<String, dynamic> answers,
  );
  Future<void> questionReject(String connectionId, String requestId);
  Future<void> abort(String connectionId, String sessionId);
  Future<void> disconnect(String connectionId);
  Stream<OpenCodeSnapshot> events(String connectionId);
}
