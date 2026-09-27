import 'dart:async';
import 'package:flutter/foundation.dart';

import 'opencode_api.dart';

/// Owns UI selections and drafts; Rust owns transport and message reconciliation.
class OpenCodeController extends ChangeNotifier {
  OpenCodeController(this.api);
  final OpenCodeApi api;
  String? connectionId;
  List<OpenCodeSession> sessions = [];
  OpenCodeSnapshot? snapshot;
  String draft = '';
  bool submitting = false;
  bool submissionUnknown = false;
  int _selection = 0;
  bool _disposed = false;
  StreamSubscription<OpenCodeSnapshot>? _events;
  final _drafts = <String, String>{};
  final _unknown = <String>{};
  Future<void> connect({
    String? baseUrl,
    String? serviceKey,
    required String directory,
    String username = 'opencode',
    String? password,
  }) async {
    final generation = ++_selection;
    final id = await api.connect(
      baseUrl: baseUrl,
      serviceKey: serviceKey,
      directory: directory,
      username: username,
      password: password,
    );
    if (_disposed || generation != _selection) {
      await api.disconnect(id);
      return;
    }
    connectionId = id;
    try {
      sessions = await api.sessions(id);
    } catch (_) {
      await disconnect();
      rethrow;
    }
    if (_disposed || generation != _selection) return;
    _events = api
        .events(connectionId!)
        .listen(
          (event) {
            if (event.sessionId != snapshot?.sessionId ||
                event.revision < snapshot!.revision) {
              return;
            }
            snapshot = event;
            notifyListeners();
          },
          onError: (Object _) => _stale(),
          onDone: _stale,
        );
    notifyListeners();
  }

  void _stale() {
    snapshot = snapshot?.withState('stale');
    notifyListeners();
  }

  Future<void> select(String sessionId) async {
    if (snapshot != null) _drafts[snapshot!.sessionId] = draft;
    final generation = ++_selection;
    final result = await api.openSession(connectionId!, sessionId);
    if (generation != _selection) return;
    if (snapshot?.sessionId != sessionId ||
        snapshot!.revision <= result.revision) {
      snapshot = result;
    }
    draft = _drafts[sessionId] ?? '';
    submissionUnknown = _unknown.contains(sessionId);
    notifyListeners();
  }

  Future<void> search(String query) async {
    final generation = _selection;
    final result = await api.sessions(connectionId!, search: query);
    if (generation != _selection) return;
    sessions = result;
    notifyListeners();
  }

  Future<void> create() async {
    if (snapshot != null) _drafts[snapshot!.sessionId] = draft;
    final generation = ++_selection;
    final result = await api.create(connectionId!);
    if (generation != _selection) return;
    snapshot = result;
    draft = '';
    submissionUnknown = false;
    final listed = await api.sessions(connectionId!);
    if (generation != _selection) return;
    sessions = listed;
    notifyListeners();
  }

  Future<void> older() async {
    final current = snapshot;
    if (current == null || current.nextCursor == null) return;
    final generation = _selection;
    final result = await api.older(connectionId!, current.sessionId);
    if (generation != _selection ||
        result.revision < (snapshot?.revision ?? 0)) {
      return;
    }
    snapshot = result;
    notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    _selection++;
    _events?.cancel();
    final id = connectionId;
    connectionId = null;
    if (id != null) unawaited(api.disconnect(id).catchError((Object _) {}));
    super.dispose();
  }

  Future<void> disconnect() async {
    _selection++;
    final events = _events;
    _events = null;
    if (events != null) unawaited(events.cancel().catchError((Object _) {}));
    final id = connectionId;
    connectionId = null;
    snapshot = null;
    sessions = [];
    _drafts.clear();
    _unknown.clear();
    draft = '';
    submissionUnknown = false;
    if (id != null) await api.disconnect(id);
    notifyListeners();
  }

  @override
  void notifyListeners() {
    if (!_disposed) super.notifyListeners();
  }

  Future<void> send() async {
    final current = snapshot;
    if (current == null ||
        !current.writable ||
        current.busy ||
        submitting ||
        submissionUnknown ||
        draft.trim().isEmpty) {
      return;
    }
    final text = draft;
    final generation = _selection;
    submitting = true;
    notifyListeners();
    try {
      final result = await api.send(connectionId!, current.sessionId, text);
      final unknown = result == OpenCodeSubmission.unknown;
      if (unknown) _unknown.add(current.sessionId);
      if (!unknown && _drafts[current.sessionId] == text) {
        _drafts[current.sessionId] = '';
      }
      if (generation == _selection) {
        submissionUnknown = unknown;
        if (!unknown && draft == text) draft = '';
      }
    } finally {
      submitting = false;
      notifyListeners();
    }
  }

  /// Unlocks manual sending after an explicit user review, without sending.
  void acknowledgeUnknown(String sessionId) {
    _unknown.remove(sessionId);
    if (snapshot?.sessionId == sessionId) submissionUnknown = false;
    notifyListeners();
  }
}
