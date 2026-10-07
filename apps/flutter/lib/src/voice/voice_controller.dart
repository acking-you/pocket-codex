import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/voice/voice_transport.dart';

/// Transport state is separate from Codex's backing text turns: a reply that
/// finishes, or a task the voice delegated, says nothing about whether the
/// call itself is still up.
enum VoicePhase {
  /// No call has run, or the last one ended normally.
  idle,

  /// Microphone and media link are being set up.
  connecting,

  /// Audio is flowing both ways.
  active,

  /// The media link dropped and is trying to come back; the microphone is
  /// still held. Turns into [active] or [failed].
  reconnecting,

  /// Hanging up: microphone released, host being told.
  stopping,

  /// The call ended because something went wrong; see [VoiceController.error].
  failed,
}

/// Who holds the floor while a call is [VoicePhase.active].
enum VoiceTurn {
  /// Nobody is talking; the microphone is open.
  listening,

  /// The user is talking (their transcript is streaming).
  user,

  /// The assistant is talking.
  assistant,
}

/// One bounded live transcript segment. Durable history belongs to Codex.
class VoiceTranscript {
  const VoiceTranscript(this.role, this.text, {this.interrupted = false});
  final String role;
  final String text;

  /// The assistant was talked over, so this is what it had said when it was
  /// cut off, not a reply it finished.
  final bool interrupted;
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

  /// Why the last call ended, when the host said ("closed" with a reason).
  String? endedReason;

  /// The last call ended because the user hung up here.
  bool endedByUser = false;
  final List<VoiceTranscript> transcript = [];
  final Map<String, String> _partial = {};
  VoiceTransport? _transport;
  StreamSubscription<AppEvent>? _events;
  Timer? _timeout;
  int _generation = 0;
  bool _disposed = false;
  Future<void>? _stopping;
  Future<void>? _starting;

  // Barge-in bookkeeping, after the native Codex client: every time the user
  // starts a new utterance the input generation advances. The assistant reply
  // that was playing belongs to an older generation, so its audio is cut and
  // stays cut until a reply for the current generation starts.
  int _inputGeneration = 0;
  bool _userTalking = false;
  int? _replyGeneration;
  bool _speakerSuppressed = false;
  Timer? _speakingTail;
  Timer? _resumeFallback;
  bool _assistantAudible = false;
  // Assistant chunks arrived after the user finished their interjection.
  bool _spokeSinceUserDone = false;

  /// How long after its last transcript chunk the assistant is assumed to
  /// still be audible. Captions run slightly ahead of playback.
  static const speakingTail = Duration(milliseconds: 1200);

  /// When the server keeps one reply going across the user's interjection
  /// (no "done" between them), how long after the user finishes to wait
  /// before treating the continuing reply as the answer to them.
  static const resumeFallback = Duration(milliseconds: 2500);

  bool get busy =>
      phase == VoicePhase.connecting ||
      phase == VoicePhase.active ||
      phase == VoicePhase.reconnecting;

  /// Who holds the floor right now; [VoiceTurn.listening] outside a call.
  VoiceTurn get turn {
    if (phase != VoicePhase.active) return VoiceTurn.listening;
    if (_userTalking) return VoiceTurn.user;
    if (_assistantAudible && !_speakerSuppressed) return VoiceTurn.assistant;
    return VoiceTurn.listening;
  }

  /// Whether the assistant's audio is currently cut because the user talked
  /// over it.
  bool get interrupting => _speakerSuppressed;

  Map<String, String> get partial => Map.unmodifiable(_partial);

  /// Live sound levels while a call is up: the microphone and the assistant,
  /// each 0..1, smoothed. A separate notifier so a waveform redraws at audio
  /// rate without rebuilding everything that listens to the call state.
  final ValueNotifier<AudioLevels> levels = ValueNotifier((
    input: 0,
    output: 0,
  ));
  Timer? _levelPoll;

  /// How often the media link is asked for levels; ~16 Hz reads as live
  /// without flooding the platform channel.
  static const levelInterval = Duration(milliseconds: 60);

  void _startLevels(VoiceTransport transport, int generation) {
    _levelPoll?.cancel();
    var reading = false;
    _levelPoll = Timer.periodic(levelInterval, (_) async {
      if (reading || generation != _generation || _disposed) return;
      reading = true;
      try {
        final raw = await transport.levels();
        if (raw == null || generation != _generation || _disposed) return;
        // WebRTC's audioLevel is linear amplitude; speech sits around
        // 0.02-0.3. A square root spreads that across the bar height.
        double shape(double v) => math.sqrt(v.clamp(0.0, 1.0));
        final prev = levels.value;
        // Fast attack, slower release: bars jump with a syllable and settle
        // rather than flicker between frames.
        double ease(double from, double to) =>
            to > from ? from + (to - from) * 0.7 : from + (to - from) * 0.3;
        levels.value = (
          // A muted microphone sends nothing; show it flat, not noise.
          input: muted ? 0 : ease(prev.input, shape(raw.input)),
          output: _speakerSuppressed ? 0 : ease(prev.output, shape(raw.output)),
        );
      } finally {
        reading = false;
      }
    });
  }

  void _stopLevels() {
    _levelPoll?.cancel();
    _levelPoll = null;
    levels.value = (input: 0, output: 0);
  }

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

  Future<void> start(
    String id, {
    String? model,
    String? voice,
    String version = 'v3',
  }) async {
    if (_starting != null || _stopping != null || busy || _disposed) return;
    final operation = _start(id, model: model, voice: voice, version: version);
    _starting = operation;
    try {
      await operation;
    } finally {
      _starting = null;
    }
  }

  void _resetFloor() {
    _inputGeneration = 0;
    _userTalking = false;
    _replyGeneration = null;
    _speakerSuppressed = false;
    _assistantAudible = false;
    _spokeSinceUserDone = false;
    _speakingTail?.cancel();
    _resumeFallback?.cancel();
  }

  Future<void> _start(
    String id, {
    String? model,
    String? voice,
    String version = 'v3',
  }) async {
    if (_disposed) return;
    final generation = ++_generation;
    bool current() => !_disposed && generation == _generation;
    threadId = id;
    error = null;
    endedReason = null;
    endedByUser = false;
    muted = false;
    transcript.clear();
    _partial.clear();
    _resetFloor();
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
          _startLevels(transport, generation);
          _changed();
        },
        onInterrupted: () {
          if (!current() || phase != VoicePhase.active) return;
          phase = VoicePhase.reconnecting;
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
        'version': version,
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
        // The host ended it. Keep its reason, so the user sees why the call
        // is gone rather than a bare "ended".
        final reason = data['reason']?.toString();
        endedReason = reason == null || reason.isEmpty ? null : reason;
        unawaited(stop());
      case 'thread/realtime/itemAdded':
        // Server-side voice activity (V2 transports) says the user started
        // talking before any transcript exists; that is the earliest moment
        // a stale reply can be cut.
        final item = data['item'];
        final type = item is Map ? item['type'] : null;
        if (type == 'input_audio_buffer.speech_started') _userStarted();
        if (type == 'response.cancelled') _replyEnded();
      case 'thread/realtime/transcript/delta':
        final role = data['role']?.toString() ?? 'assistant';
        final delta = data['delta']?.toString() ?? '';
        if (role == 'user') {
          if (delta.trim().isNotEmpty) _userStarted();
        } else if (delta.trim().isNotEmpty) {
          _assistantSpoke();
        }
        final value = '${_partial[role] ?? ''}$delta';
        _partial[role] = value.length > 16000
            ? value.substring(value.length - 16000)
            : value;
        _changed();
      case 'thread/realtime/transcript/done':
        final role = data['role']?.toString() ?? 'assistant';
        final text = data['text']?.toString() ?? _partial[role] ?? '';
        _partial.remove(role);
        // A reply that was talked over is recorded as cut off, so the log
        // never presents half an answer as a complete one.
        final cut =
            role != 'user' &&
            _replyGeneration != null &&
            _replyGeneration != _inputGeneration;
        if (text.isNotEmpty) {
          transcript.add(
            VoiceTranscript(
              role,
              text.length > 16000 ? text.substring(0, 16000) : text,
              interrupted: cut,
            ),
          );
          if (transcript.length > 12) transcript.removeAt(0);
        }
        if (role == 'user') {
          _userFinished();
        } else {
          _replyEnded();
        }
        _changed();
    }
  }

  /// The user began a new utterance. If the assistant was talking, that is a
  /// barge-in: its audio is cut now, and stays cut until a reply to *this*
  /// utterance starts. Ignored while muted, and once per utterance — a pause
  /// mid-sentence keeps the same utterance, so it does not count again.
  void _userStarted() {
    if (muted || phase != VoicePhase.active || _userTalking) return;
    _userTalking = true;
    _inputGeneration++;
    _resumeFallback?.cancel();
    final replyPlaying =
        _assistantAudible || (_partial['assistant']?.isNotEmpty ?? false);
    if (replyPlaying && !_speakerSuppressed) {
      _speakerSuppressed = true;
      _transport?.setSpeakerSuppressed(true);
    }
    _changed();
  }

  void _userFinished() {
    _userTalking = false;
    _spokeSinceUserDone = false;
    if (_speakerSuppressed) {
      // The usual case: the server ends the old reply and starts a new one,
      // which un-mutes. Some servers instead keep one reply going; if its
      // chunks are still arriving once the user is done, it is the answer.
      _resumeFallback?.cancel();
      _resumeFallback = Timer(resumeFallback, () {
        if (_speakerSuppressed && _spokeSinceUserDone) {
          _replyGeneration = _inputGeneration;
          _releaseSpeaker();
        }
      });
    }
  }

  void _assistantSpoke() {
    // The first chunk of a reply stamps it with the utterance it answers.
    _replyGeneration ??= _inputGeneration;
    _assistantAudible = true;
    if (!_userTalking) _spokeSinceUserDone = true;
    _speakingTail?.cancel();
    _speakingTail = Timer(speakingTail, () {
      _assistantAudible = false;
      _changed();
    });
    if (_speakerSuppressed && _replyGeneration == _inputGeneration) {
      _releaseSpeaker();
    }
  }

  void _replyEnded() {
    _replyGeneration = null;
  }

  void _releaseSpeaker() {
    _resumeFallback?.cancel();
    if (!_speakerSuppressed) return;
    _speakerSuppressed = false;
    _transport?.setSpeakerSuppressed(false);
    _changed();
  }

  void toggleMuted() {
    if (phase != VoicePhase.active && phase != VoicePhase.reconnecting) return;
    muted = !muted;
    _transport?.setMuted(muted);
    if (muted) _userTalking = false;
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

  /// Hang up from the UI: same as [stop], but remembered as the user's
  /// choice so the ended state can say so.
  Future<void> hangUp() {
    if (busy) endedByUser = true;
    return stop();
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
    _stopLevels();
    _resetFloor();
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
    _levelPoll?.cancel();
    _resetFloor();
    unawaited(stop());
    levels.dispose();
    super.dispose();
  }
}
