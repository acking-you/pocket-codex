import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/voice/voice_transport.dart';

/// Whether the dictation line (its realtime session on the host) is up.
enum DictationLink {
  /// No session; the next take (or a warm-up) opens one.
  off,

  /// Opening the session: thread, media link, host realtime start.
  warming,

  /// Connected and paused: no microphone attached, nothing is heard.
  ready,

  /// The last attempt failed; see [DictationLine.error]. Retried on demand.
  failed,
}

/// Where the current take is.
enum DictationTake {
  /// Not dictating.
  idle,

  /// Asked to dictate; waiting for the line and the microphone.
  opening,

  /// The microphone is attached and words are streaming in.
  listening,

  /// The microphone is detached; the last words are still arriving.
  finishing,
}

/// Why a take produced less than it should have.
enum DictationFailure {
  /// The OS refused the microphone.
  permission,

  /// The line could not be opened.
  unavailable,

  /// The line dropped mid-take; what was heard so far stays.
  interrupted,
}

/// The media half of the dictation line, behind an interface so the line can
/// be tested without WebRTC.
abstract interface class DictationTransport {
  /// Build a complete offer with an audio slot that has no microphone yet.
  ///
  /// [onConnected] fires when the media link is up; [onFailure] once it is
  /// gone for good.
  Future<String> offer({
    required void Function() onConnected,
    required void Function(String) onFailure,
  });

  /// Apply the answer delivered by `thread/realtime/sdp`.
  Future<void> answer(String sdp);

  /// Attach the microphone to the link, acquiring it if it is not held.
  Future<void> openMicrophone();

  /// Detach the microphone. The device may stay open a short while so a
  /// follow-up take starts at once; nothing is sent while detached.
  Future<void> closeMicrophone();

  /// The microphone level, 0..1 (linear), while it is attached.
  Future<double?> inputLevel();

  /// Release the microphone and the link.
  Future<void> close();
}

/// [DictationTransport] over WebRTC. The audio slot is a send-receive
/// transceiver whose sender starts empty: the host and the model see a
/// connected call with no input until [openMicrophone] puts the microphone
/// on it, so a paused line hears nothing and costs no upload.
class WebRtcDictationTransport implements DictationTransport {
  RTCPeerConnection? _peer;
  RTCRtpSender? _sender;
  RTCDataChannel? _channel;
  MediaStream? _microphone;
  Completer<void>? _gathered;
  Timer? _recovery;
  Timer? _release;
  bool _attached = false;
  bool _closed = false;

  /// How long the microphone stays open after a take, detached, so the next
  /// take starts without waiting for the device. The OS shows the
  /// microphone in use for this long.
  static const microphoneGrace = Duration(seconds: 10);

  void _checkOpen() {
    if (_closed) throw StateError('Dictation was closed');
  }

  @override
  Future<String> offer({
    required void Function() onConnected,
    required void Function(String) onFailure,
  }) async {
    final peer = await createPeerConnection({'iceServers': <Object>[]});
    if (_closed) {
      await peer.dispose();
      _checkOpen();
    }
    _peer = peer;
    peer.onConnectionState = (state) {
      if (_closed) return;
      switch (state) {
        case RTCPeerConnectionState.RTCPeerConnectionStateConnected:
          _recovery?.cancel();
          _recovery = null;
          onConnected();
        case RTCPeerConnectionState.RTCPeerConnectionStateDisconnected:
          // Same grace as the call: a network blip often recovers.
          _recovery ??= Timer(WebRtcVoiceTransport.recoveryGrace, () {
            if (!_closed) onFailure('Dictation connection interrupted');
          });
        case RTCPeerConnectionState.RTCPeerConnectionStateFailed:
          _recovery?.cancel();
          onFailure('Dictation connection failed');
        case RTCPeerConnectionState.RTCPeerConnectionStateClosed:
          _recovery?.cancel();
          onFailure('Dictation connection closed');
        default:
          break;
      }
    };
    // The model is told to stay silent; should it speak anyway, nothing is
    // played. Dictation has no speaker.
    peer.onTrack = (event) {
      if (event.track.kind == 'audio') event.track.enabled = false;
    };
    final transceiver = await peer.addTransceiver(
      kind: RTCRtpMediaType.RTCRtpMediaTypeAudio,
      init: RTCRtpTransceiverInit(direction: TransceiverDirection.SendRecv),
    );
    _checkOpen();
    _sender = transceiver.sender;
    final channel = await peer.createDataChannel(
      'oai-events',
      RTCDataChannelInit(),
    );
    if (_closed) {
      await channel.close();
      _checkOpen();
    }
    _channel = channel;
    final gathered = Completer<void>();
    _gathered = gathered;
    peer.onIceGatheringState = (state) {
      if (state == RTCIceGatheringState.RTCIceGatheringStateComplete &&
          !gathered.isCompleted) {
        gathered.complete();
      }
    };
    await peer.setLocalDescription(await peer.createOffer());
    _checkOpen();
    // No trickle-ICE RPC: send one complete offer.
    await gathered.future.timeout(const Duration(seconds: 10));
    _checkOpen();
    final sdp = (await peer.getLocalDescription())?.sdp;
    if (sdp == null || sdp.isEmpty) throw StateError('Missing audio offer');
    return sdp;
  }

  @override
  Future<void> answer(String sdp) async {
    _checkOpen();
    final peer = _peer;
    if (peer == null) throw StateError('Dictation is not ready');
    await peer.setRemoteDescription(RTCSessionDescription(sdp, 'answer'));
    for (final receiver in await peer.getReceivers()) {
      final track = receiver.track;
      if (track != null && track.kind == 'audio') track.enabled = false;
    }
  }

  @override
  Future<void> openMicrophone() async {
    _checkOpen();
    _release?.cancel();
    _release = null;
    final sender = _sender;
    if (sender == null) throw StateError('Dictation is not ready');
    var media = _microphone;
    if (media == null) {
      media = await navigator.mediaDevices.getUserMedia({
        'audio': {
          'echoCancellation': true,
          'noiseSuppression': true,
          'autoGainControl': true,
        },
        'video': false,
      });
      if (_closed) {
        await _stopMedia(media);
        _checkOpen();
      }
      _microphone = media;
    }
    final track = media.getAudioTracks().firstOrNull;
    if (track == null) throw StateError('No microphone track');
    track.enabled = true;
    await sender.replaceTrack(track);
    _attached = true;
  }

  @override
  Future<void> closeMicrophone() async {
    if (_closed) return;
    _attached = false;
    await _sender?.replaceTrack(null);
    _release?.cancel();
    _release = Timer(microphoneGrace, () {
      final media = _microphone;
      _microphone = null;
      if (media != null) unawaited(_stopMedia(media).catchError((_) {}));
    });
  }

  @override
  Future<double?> inputLevel() async {
    final peer = _peer;
    if (peer == null || _closed || !_attached) return null;
    try {
      return audioLevelsFromStats(await peer.getStats())?.input;
    } catch (_) {
      return null;
    }
  }

  static Future<void> _stopMedia(MediaStream media) async {
    for (final track in media.getTracks()) {
      track.enabled = false;
      await track.stop();
    }
    await media.dispose();
  }

  @override
  Future<void> close() async {
    _closed = true;
    _attached = false;
    _recovery?.cancel();
    _release?.cancel();
    final gathered = _gathered;
    if (gathered != null && !gathered.isCompleted) gathered.complete();
    final media = _microphone;
    final channel = _channel;
    final peer = _peer;
    _microphone = null;
    _channel = null;
    _peer = null;
    _sender = null;
    Object? failure;
    Future<void> release(Future<void> Function() action) async {
      try {
        await action();
      } catch (e) {
        failure ??= e;
      }
    }

    if (media != null) await release(() => _stopMedia(media));
    if (channel != null) await release(channel.close);
    if (peer != null) await release(peer.dispose);
    if (failure != null) throw StateError('Dictation cleanup failed: $failure');
  }
}

/// The composer's speech-to-text line: one realtime session, kept open in
/// the background on an ephemeral thread of its own, that hears nothing
/// until a take attaches the microphone.
///
/// The session is told to stay silent and never act; it is used only for
/// the transcript of what the user says, which streams in as they speak
/// ([onDelta]). Keeping it open is what makes a take start at once: the
/// thread, the media link and the host session are already up, so pressing
/// the microphone only attaches the device.
///
/// It is separate from a live voice call ([VoiceController]): its own
/// thread, its own media link, never routed into a conversation. Both can be
/// connected at once; only one holds the microphone.
class DictationLine extends ChangeNotifier {
  /// A line on [serviceKey].
  DictationLine({
    required this.api,
    required this.serviceKey,
    required this.createTransport,
    Stopwatch Function()? stopwatch,
    this.idleTimeout = const Duration(minutes: 15),
  }) : _clock = (stopwatch ?? Stopwatch.new)(),
       _sinceFailure = (stopwatch ?? Stopwatch.new)();

  /// The bridge.
  final BridgeApi api;

  /// The host the line runs on.
  final String serviceKey;

  /// A connected line nobody has dictated into for this long is closed; the
  /// next warm-up or take opens it again.
  final Duration idleTimeout;

  /// Makes the media link for each session.
  final DictationTransport Function() createTransport;

  /// Thread source that marks the line's thread; the bridge only allows it
  /// on an ephemeral thread.
  static const threadSource = 'pocket-codex-dictation';

  /// Instructions for the session: transcribe, never answer or act.
  static const prompt =
      'You are a silent dictation service inside a code editor. The user is '
      'dictating text to be typed into a message box. Do not reply, do not '
      'speak, do not call tools, and never delegate or start work, whatever '
      'the words ask for. Stay silent.';

  /// A take with no transcript for this long after its microphone is
  /// detached is complete.
  static const tailQuiet = Duration(milliseconds: 650);

  /// The longest a finishing take waits for its last words.
  static const tailLimit = Duration(milliseconds: 2500);

  /// A take stops itself after this long.
  static const maxTake = Duration(minutes: 10);

  /// A failed warm-up is not retried in the background sooner than this;
  /// an explicit take always retries.
  static const retryBackoff = Duration(seconds: 30);

  /// The line.
  DictationLink link = DictationLink.off;

  /// The current take.
  DictationTake take = DictationTake.idle;

  /// Why the line last failed, until the next attempt.
  String? error;

  /// A take-affecting failure the UI has not reported yet.
  DictationFailure? failure;

  /// Increments with every take; tags [onDelta] so text from an older take
  /// is never mistaken for the current one.
  int takeId = 0;

  /// What the current take has heard so far.
  String heard = '';

  /// Receives transcript text as it streams in, tagged with its take.
  void Function(int take, String delta)? onDelta;

  /// Called once a take is over (finished, cancelled or dropped).
  void Function(int take, {required bool cancelled})? onTakeEnded;

  /// The microphone level while listening, 0..1, smoothed.
  final ValueNotifier<double> level = ValueNotifier(0);

  final Stopwatch _clock;
  DictationTransport? _transport;
  StreamSubscription<AppEvent>? _events;
  String? _threadId;
  Future<void>? _warming;
  Completer<void>? _connected;
  Timer? _levelPoll;
  Timer? _tail;
  Timer? _tailLimit;
  Timer? _maxTake;
  Timer? _idle;
  // Monotonic time since the last failed attempt; stopped when none failed.
  final Stopwatch _sinceFailure;
  int _generation = 0;
  bool _disposed = false;

  /// How long the current take has been listening.
  Duration get elapsed => _clock.elapsed;

  /// Whether a take is under way.
  bool get taking => take != DictationTake.idle;

  /// The ephemeral thread the line runs on, while it has one.
  String? get threadId => _threadId;

  void _changed() {
    if (!_disposed) notifyListeners();
  }

  Future<Map<String, dynamic>> _request(
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

  /// Open the line in the background, so a take starts at once. Does nothing
  /// when it is already up or opening, or (unless [force]) shortly after a
  /// failure.
  Future<void> warm({bool force = false}) {
    if (_disposed) return Future.value();
    if (link == DictationLink.ready) {
      _armIdle();
      return Future.value();
    }
    if (_warming case final pending?) return pending;
    if (!force &&
        _sinceFailure.isRunning &&
        _sinceFailure.elapsed < retryBackoff) {
      return Future.value();
    }
    final operation = _warm();
    _warming = operation;
    return operation.whenComplete(() => _warming = null);
  }

  Future<void> _warm() async {
    final generation = ++_generation;
    bool current() => !_disposed && generation == _generation;
    link = DictationLink.warming;
    error = null;
    _changed();
    try {
      final started = await _request('thread/start', {
        'threadSource': threadSource,
        'ephemeral': true,
        // The session never acts; should a turn start anyway it is
        // interrupted, and could not write or ask for anything meanwhile.
        'sandbox': 'read-only',
        'approvalPolicy': 'never',
      });
      final thread = started['thread'];
      final id = thread is Map ? thread['id'] : null;
      if (id is! String || id.isEmpty) {
        throw StateError('The host did not open a dictation thread');
      }
      if (!current()) {
        unawaited(_release(id, null, null));
        return;
      }
      _threadId = id;
      final connected = Completer<void>();
      // Failures can land before anything awaits it (the host refuses the
      // start first); the await below still sees them.
      connected.future.ignore();
      _connected = connected;
      _events = api
          .appEvents(serviceKey)
          .listen(
            (e) {
              if (current()) _event(e, generation);
            },
            onError: (Object e) {
              if (current()) _drop('$e');
            },
            onDone: () {
              if (current()) _drop('Host connection closed');
            },
          );
      final transport = createTransport();
      _transport = transport;
      final sdp = await transport.offer(
        onConnected: () {
          if (!connected.isCompleted) connected.complete();
        },
        onFailure: (message) {
          if (!current()) return;
          if (!connected.isCompleted) {
            connected.completeError(StateError(message));
          } else {
            _drop(message);
          }
        },
      );
      if (!current()) return;
      await _request('thread/realtime/start', {
        'threadId': id,
        // V3 is what a ChatGPT login is allowed over WebRTC, and it only
        // speaks audio; the prompt keeps it silent and nothing is played.
        'outputModality': 'audio',
        'version': 'v3',
        'transport': {'type': 'webrtc', 'sdp': sdp},
        'clientManagedHandoffs': true,
        'delegationAckFiller': false,
        'includeStartupContext': false,
        'prompt': prompt,
      });
      await connected.future.timeout(const Duration(seconds: 20));
      if (!current()) return;
      link = DictationLink.ready;
      _sinceFailure
        ..stop()
        ..reset();
      _armIdle();
      _changed();
    } catch (e) {
      if (current()) _fail('$e');
    }
  }

  void _event(AppEvent e, int generation) {
    final id = _threadId;
    if (id == null || e.threadId != id) return;
    if (e.kind == 'turn/started') {
      // The session is told never to delegate; if it does anyway, no work
      // runs on the user's behalf from dictated words.
      unawaited(api.appTurnInterrupt(serviceKey, id).catchError((Object _) {}));
      return;
    }
    if (!e.kind.startsWith('thread/realtime/')) return;
    Map<String, dynamic> data;
    try {
      data = jsonDecode(e.raw) as Map<String, dynamic>;
    } catch (_) {
      return;
    }
    switch (e.kind) {
      case 'thread/realtime/sdp':
        final sdp = data['sdp'];
        if (sdp is String) {
          unawaited(
            _transport?.answer(sdp).catchError((Object err) {
              if (generation == _generation) _drop('$err');
            }),
          );
        }
      case 'thread/realtime/error':
        final message = data['message']?.toString() ?? 'Dictation failed';
        if (link == DictationLink.warming) {
          final connected = _connected;
          if (connected != null && !connected.isCompleted) {
            connected.completeError(StateError(message));
          }
        } else {
          _drop(message);
        }
      case 'thread/realtime/closed':
        _drop(data['reason']?.toString());
      case 'thread/realtime/transcript/delta':
        if (data['role']?.toString() != 'user') return;
        final delta = data['delta']?.toString() ?? '';
        if (delta.isEmpty) return;
        if (take != DictationTake.listening &&
            take != DictationTake.finishing) {
          return;
        }
        heard += delta;
        onDelta?.call(takeId, delta);
        if (take == DictationTake.finishing) _armTail();
        _changed();
    }
  }

  /// Start a take: open the line if needed, then attach the microphone.
  Future<void> startTake() async {
    if (_disposed || taking) return;
    final id = ++takeId;
    heard = '';
    failure = null;
    take = DictationTake.opening;
    _idle?.cancel();
    _changed();
    if (link != DictationLink.ready) {
      await warm(force: true);
      if (id != takeId || take != DictationTake.opening) return;
      if (link != DictationLink.ready) {
        take = DictationTake.idle;
        failure = DictationFailure.unavailable;
        _changed();
        onTakeEnded?.call(id, cancelled: true);
        return;
      }
    }
    final transport = _transport;
    try {
      if (transport == null) throw StateError('Dictation is not ready');
      await transport.openMicrophone();
    } catch (e) {
      if (id != takeId || take != DictationTake.opening) return;
      take = DictationTake.idle;
      failure = DictationFailure.permission;
      error = '$e';
      _armIdle();
      _changed();
      onTakeEnded?.call(id, cancelled: true);
      return;
    }
    if (id != takeId || take != DictationTake.opening) {
      // Cancelled while the microphone was opening.
      unawaited(transport.closeMicrophone().catchError((Object _) {}));
      return;
    }
    take = DictationTake.listening;
    _clock
      ..reset()
      ..start();
    _startLevels(transport, id);
    _maxTake = Timer(maxTake, () {
      if (id == takeId && take == DictationTake.listening) {
        unawaited(finishTake());
      }
    });
    _changed();
  }

  void _startLevels(DictationTransport transport, int id) {
    _levelPoll?.cancel();
    var reading = false;
    _levelPoll = Timer.periodic(const Duration(milliseconds: 60), (_) async {
      if (reading || id != takeId || take != DictationTake.listening) return;
      reading = true;
      try {
        final raw = await transport.inputLevel();
        if (raw == null || id != takeId || _disposed) return;
        // Same shaping as the call's waveform: speech's linear level sits
        // around 0.02-0.3; a square root spreads it over the bar height,
        // with a fast attack and a slower release.
        final v = math.sqrt(raw.clamp(0.0, 1.0));
        final prev = level.value;
        level.value = v > prev
            ? prev + (v - prev) * 0.7
            : prev + (v - prev) * 0.3;
      } finally {
        reading = false;
      }
    });
  }

  void _stopTakeTimers() {
    _levelPoll?.cancel();
    _levelPoll = null;
    _maxTake?.cancel();
    _maxTake = null;
    _tail?.cancel();
    _tail = null;
    _tailLimit?.cancel();
    _tailLimit = null;
    if (!_disposed) level.value = 0;
  }

  void _armTail() {
    _tail?.cancel();
    _tail = Timer(tailQuiet, _endTake);
  }

  /// Stop listening. The microphone is detached at once; words still in
  /// flight keep arriving for a moment and the take then ends by itself.
  Future<void> finishTake() async {
    if (take == DictationTake.opening) return cancelTake();
    if (take != DictationTake.listening) return;
    take = DictationTake.finishing;
    _clock.stop();
    _levelPoll?.cancel();
    _maxTake?.cancel();
    level.value = 0;
    _armTail();
    _tailLimit = Timer(tailLimit, _endTake);
    _changed();
    try {
      await _transport?.closeMicrophone();
    } catch (_) {
      /* The take still ends; the line is dropped if the link is gone. */
    }
  }

  void _endTake() {
    if (take != DictationTake.finishing) return;
    final id = takeId;
    _stopTakeTimers();
    take = DictationTake.idle;
    _clock
      ..stop()
      ..reset();
    _armIdle();
    _changed();
    onTakeEnded?.call(id, cancelled: false);
  }

  /// Drop the take; whatever it inserted is the caller's to remove.
  Future<void> cancelTake() async {
    if (!taking) return;
    final id = takeId;
    final wasAttached =
        take == DictationTake.listening || take == DictationTake.finishing;
    // A new id, so text still in flight for this take is ignored.
    ++takeId;
    _stopTakeTimers();
    take = DictationTake.idle;
    _clock
      ..stop()
      ..reset();
    _armIdle();
    _changed();
    onTakeEnded?.call(id, cancelled: true);
    if (wasAttached) {
      try {
        await _transport?.closeMicrophone();
      } catch (_) {
        /* The microphone is released with the line either way. */
      }
    }
  }

  void _armIdle() {
    _idle?.cancel();
    if (link != DictationLink.ready || taking) return;
    _idle = Timer(idleTimeout, () {
      if (!taking) unawaited(close());
    });
  }

  void _fail(String message) {
    _sinceFailure
      ..reset()
      ..start();
    final connected = _connected;
    if (connected != null && !connected.isCompleted) {
      connected.completeError(StateError(message));
    }
    unawaited(_shutdown(DictationLink.failed, message));
  }

  /// The line went away after it was up.
  void _drop(String? reason) {
    if (link == DictationLink.off || link == DictationLink.failed) return;
    if (link == DictationLink.warming) {
      _fail(reason ?? 'Dictation closed');
      return;
    }
    if (take == DictationTake.listening || take == DictationTake.opening) {
      failure = DictationFailure.interrupted;
    }
    unawaited(_shutdown(DictationLink.off, reason));
  }

  /// Close the line (and any take). The next take opens it again.
  Future<void> close() => _shutdown(DictationLink.off, null);

  Future<void> _shutdown(DictationLink next, String? message) async {
    ++_generation;
    _idle?.cancel();
    final wasTaking = taking;
    final id = takeId;
    // A take waiting on a line that could not open says why it got nothing.
    if (take == DictationTake.opening && next == DictationLink.failed) {
      failure = DictationFailure.unavailable;
    }
    if (wasTaking) ++takeId;
    _stopTakeTimers();
    take = DictationTake.idle;
    _clock
      ..stop()
      ..reset();
    final transport = _transport;
    final events = _events;
    final thread = _threadId;
    _transport = null;
    _events = null;
    _threadId = null;
    _connected = null;
    link = next;
    error = next == DictationLink.failed ? message : null;
    _changed();
    if (wasTaking) onTakeEnded?.call(id, cancelled: false);
    await _release(thread, transport, events);
  }

  Future<void> _release(
    String? thread,
    DictationTransport? transport,
    StreamSubscription<AppEvent>? events,
  ) async {
    try {
      await transport?.close();
    } catch (_) {
      /* Local cleanup must not depend on the platform's error paths. */
    }
    // Nothing reads the feed past this point; the stop does not wait on it.
    if (events != null) {
      unawaited(events.cancel().then((_) {}, onError: (Object _) {}));
    }
    if (thread == null) return;
    // The host ends its session and lets the ephemeral thread go; a host
    // that is unreachable has already dropped both.
    try {
      await _request('thread/realtime/stop', {
        'threadId': thread,
      }).timeout(const Duration(seconds: 5));
    } catch (_) {}
    try {
      await _request('thread/unsubscribe', {
        'threadId': thread,
      }).timeout(const Duration(seconds: 5));
    } catch (_) {}
  }

  @override
  void dispose() {
    _disposed = true;
    unawaited(close());
    level.dispose();
    super.dispose();
  }
}
