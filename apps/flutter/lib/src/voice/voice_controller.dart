import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/voice/voice_transport.dart';

/// Transport state is separate from Codex's backing text turns.
enum VoicePhase { idle, connecting, active, stopping, failed }

/// One bounded live transcript segment. Durable history belongs to Codex.
class VoiceTranscript {
  const VoiceTranscript(this.role, this.text);
  final String role;
  final String text;
}

/// Owns one foreground voice call and never restarts a microphone implicitly.
class VoiceController extends ChangeNotifier {
  VoiceController({
    required this.api,
    required this.serviceKey,
    required this.createTransport,
  });
  final BridgeApi api;
  final String serviceKey;
  final VoiceTransport Function() createTransport;
  VoicePhase phase = VoicePhase.idle;
  String? error;
  String? threadId;
  bool muted = false;
  final List<VoiceTranscript> transcript = [];
  final Map<String, String> _partial = {};
  VoiceTransport? _transport;
  StreamSubscription<AppEvent>? _events;
  Timer? _timeout;
  int _generation = 0;
  bool _disposed = false;
  Future<void>? _stopping;
  Future<void>? _starting;

  bool get busy => phase == VoicePhase.connecting || phase == VoicePhase.active;
  Map<String, String> get partial => Map.unmodifiable(_partial);
  void _changed() {
    if (!_disposed) notifyListeners();
  }

  Future<Map<String, dynamic>> request(
    String method,
    Map<String, dynamic> params,
  ) async {
    final raw = await api.appRealtimeRequest(
      serviceKey,
      method,
      jsonEncode(params),
    );
    return jsonDecode(raw) as Map<String, dynamic>;
  }

  Future<void> start(String id, {String? model, String? voice}) async {
    if (_starting != null || _stopping != null || busy || _disposed) return;
    final operation = _start(id, model: model, voice: voice);
    _starting = operation;
    try {
      await operation;
    } finally {
      _starting = null;
    }
  }

  Future<void> _start(String id, {String? model, String? voice}) async {
    if (_disposed) return;
    final generation = ++_generation;
    bool current() => !_disposed && generation == _generation;
    threadId = id;
    error = null;
    muted = false;
    transcript.clear();
    _partial.clear();
    phase = VoicePhase.connecting;
    final transport = createTransport();
    _transport = transport;
    _events = api
        .appEvents(serviceKey)
        .listen(
          (event) {
            if (!current()) return;
            try {
              _event(event, generation);
            } catch (e) {
              _fail(e.toString());
            }
          },
          onError: (Object e) {
            if (current()) _fail(e.toString());
          },
          onDone: () {
            if (current()) _fail('Host connection closed');
          },
        );
    _timeout = Timer(const Duration(seconds: 45), () {
      if (current()) _fail('Voice connection timed out');
    });
    _changed();
    try {
      final sdp = await transport.offer(
        onConnected: () {
          if (!current()) return;
          _timeout?.cancel();
          phase = VoicePhase.active;
          _changed();
        },
        onFailure: (message) {
          if (current()) _fail(message);
        },
      );
      if (!current()) return;
      await request('thread/realtime/start', {
        'threadId': id,
        'outputModality': 'audio',
        'version': 'v3',
        'transport': {'type': 'webrtc', 'sdp': sdp},
        'clientManagedHandoffs': false,
        if (model != null && model.trim().isNotEmpty) 'model': model.trim(),
        'voice': ?voice,
      });
      // A cancelled startup can finish after stop; close that late host call.
      if (!current()) await _stopHost(id);
    } catch (e) {
      if (current()) _fail(e.toString());
    }
  }

  void _event(AppEvent event, int generation) {
    if (event.threadId != threadId ||
        !event.kind.startsWith('thread/realtime/')) {
      return;
    }
    final data = jsonDecode(event.raw) as Map<String, dynamic>;
    switch (event.kind) {
      case 'thread/realtime/sdp':
        final sdp = data['sdp'];
        if (sdp is String) {
          unawaited(
            _transport?.answer(sdp).catchError((Object e) {
              if (generation == _generation) _fail(e.toString());
            }),
          );
        }
      case 'thread/realtime/error':
        _fail(data['message']?.toString() ?? 'Voice session failed');
      case 'thread/realtime/closed':
        unawaited(stop());
      case 'thread/realtime/transcript/delta':
        final role = data['role']?.toString() ?? 'assistant';
        final value = '${_partial[role] ?? ''}${data['delta'] ?? ''}';
        _partial[role] = value.length > 16000
            ? value.substring(value.length - 16000)
            : value;
        _changed();
      case 'thread/realtime/transcript/done':
        final role = data['role']?.toString() ?? 'assistant';
        final text = data['text']?.toString() ?? _partial[role] ?? '';
        _partial.remove(role);
        if (text.isNotEmpty) {
          transcript.add(
            VoiceTranscript(
              role,
              text.length > 16000 ? text.substring(0, 16000) : text,
            ),
          );
          if (transcript.length > 12) transcript.removeAt(0);
        }
        _changed();
    }
  }

  void toggleMuted() {
    if (phase != VoicePhase.active) return;
    muted = !muted;
    _transport?.setMuted(muted);
    _changed();
  }

  void _fail(String message) {
    error = message;
    unawaited(stop(failed: true));
  }

  Future<void> _stopHost(String id) async {
    try {
      await request('thread/realtime/stop', {
        'threadId': id,
      }).timeout(const Duration(seconds: 5));
    } catch (_) {
      /* Local audio cleanup must not depend on host reachability. */
    }
  }

  Future<void> stop({bool failed = false}) {
    if (_stopping case final pending?) return pending;
    if (_transport == null && _events == null) return Future.value();
    final operation = _stop(failed);
    _stopping = operation;
    return operation.whenComplete(() => _stopping = null);
  }

  Future<void> _stop(bool failed) async {
    ++_generation;
    _timeout?.cancel();
    final transport = _transport;
    final events = _events;
    final id = threadId;
    _transport = null;
    _events = null;
    phase = VoicePhase.stopping;
    _changed();
    try {
      await transport?.close();
    } catch (e) {
      error ??= e.toString();
    }
    try {
      await events?.cancel();
    } catch (e) {
      error ??= e.toString();
    }
    if (id != null) await _stopHost(id);
    phase = failed ? VoicePhase.failed : VoicePhase.idle;
    _changed();
  }

  @override
  void dispose() {
    _disposed = true;
    unawaited(stop());
    super.dispose();
  }
}
