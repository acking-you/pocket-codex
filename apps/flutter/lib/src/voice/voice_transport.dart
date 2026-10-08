import 'dart:async';

import 'package:flutter_webrtc/flutter_webrtc.dart';

/// Client-owned media connection; credentials and Codex remain on the host.
abstract interface class VoiceTransport {
  /// Capture the microphone and return a complete WebRTC offer.
  ///
  /// [onConnected] fires when audio first flows and again after a transient
  /// drop recovers; [onInterrupted] fires when the link drops but may still
  /// recover; [onFailure] fires once the connection is gone for good.
  Future<String> offer({
    required void Function() onConnected,
    required void Function(String) onFailure,
    void Function()? onInterrupted,
  });

  /// Apply the answer delivered by thread/realtime/sdp.
  Future<void> answer(String sdp);

  /// Enable or disable the existing microphone track without reconnecting.
  void setMuted(bool muted);

  /// Silence (or restore) the assistant's audio without touching the link.
  /// Used to cut a stale reply the moment the user talks over it.
  void setSpeakerSuppressed(bool suppressed);

  /// The current sound levels, each 0..1: what the microphone picks up and
  /// what the assistant is playing. Null before audio flows.
  Future<AudioLevels?> levels();

  /// Release microphone, data channel and peer connection, including startup.
  Future<void> close();
}

/// Sound levels read from the media link, linear 0..1 (WebRTC `audioLevel`).
typedef AudioLevels = ({double input, double output});

/// The levels in a WebRTC stats [reports]: `media-source` is the local
/// microphone after capture processing, `inbound-rtp` the remote audio.
AudioLevels? audioLevelsFromStats(Iterable<StatsReport> reports) {
  double? input, output;
  for (final r in reports) {
    final level = r.values['audioLevel'];
    if (level is! num) continue;
    final kind = r.values['kind'] ?? r.values['mediaType'];
    if (kind != null && kind != 'audio') continue;
    if (r.type == 'media-source') input = level.toDouble();
    if (r.type == 'inbound-rtp') output = level.toDouble();
  }
  if (input == null && output == null) return null;
  return (input: input ?? 0, output: output ?? 0);
}

/// WebRTC supplies audio encoding, playback and echo cancellation natively.
class WebRtcVoiceTransport implements VoiceTransport {
  RTCPeerConnection? _peer;
  MediaStream? _microphone;
  RTCDataChannel? _channel;
  Completer<void>? _gathered;
  final List<MediaStreamTrack> _remoteAudio = [];
  bool _speakerSuppressed = false;
  Timer? _recovery;
  bool _closed = false;

  /// How long a Disconnected peer may take to come back before the call is
  /// treated as lost. ICE often recovers from a network blip (Wi-Fi roam, VPN
  /// reconnect) within a few seconds; tearing down at once lost those calls.
  static const recoveryGrace = Duration(seconds: 8);

  void _checkOpen() {
    if (_closed) throw StateError('Voice connection was cancelled');
  }

  void _adoptRemote(MediaStreamTrack track) {
    if (track.kind != 'audio' || _remoteAudio.contains(track)) return;
    _remoteAudio.add(track);
    track.enabled = !_speakerSuppressed;
  }

  @override
  Future<String> offer({
    required void Function() onConnected,
    required void Function(String) onFailure,
    void Function()? onInterrupted,
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
          onInterrupted?.call();
          _recovery ??= Timer(recoveryGrace, () {
            if (!_closed) onFailure('Audio connection interrupted');
          });
        case RTCPeerConnectionState.RTCPeerConnectionStateFailed:
          _recovery?.cancel();
          onFailure('Audio connection failed');
        case RTCPeerConnectionState.RTCPeerConnectionStateClosed:
          _recovery?.cancel();
          onFailure('Audio connection closed');
        default:
          break;
      }
    };
    // Keep a handle on the assistant's audio so a barge-in can silence it.
    peer.onTrack = (event) => _adoptRemote(event.track);
    final media = await navigator.mediaDevices.getUserMedia({
      'audio': {
        // The platform's echo canceller is what keeps the assistant's own
        // voice, coming out of the speakers, from reading as the user
        // talking over it. Gain control keeps a quiet or distant voice above
        // the recogniser's threshold without boosting room noise into it.
        'echoCancellation': true,
        'noiseSuppression': true,
        'autoGainControl': true,
      },
      'video': false,
    });
    if (_closed) {
      for (final track in media.getTracks()) {
        await track.stop();
      }
      await media.dispose();
      _checkOpen();
    }
    _microphone = media;
    for (final track in media.getAudioTracks()) {
      await peer.addTrack(track, media);
      _checkOpen();
    }
    // The upstream offer expects both audio and a realtime event channel.
    // App-server sideband notifications own transcript/agent handling.
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
    final offer = await peer.createOffer({
      'mandatory': {'OfferToReceiveAudio': true, 'OfferToReceiveVideo': false},
      'optional': <Object>[],
    });
    _checkOpen();
    await peer.setLocalDescription(offer);
    // Signaling has no trickle-ICE RPC: send one complete offer.
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
    if (peer == null) throw StateError('Audio connection is not ready');
    await peer.setRemoteDescription(RTCSessionDescription(sdp, 'answer'));
    // Some platforms surface the remote track only through the receivers.
    for (final receiver in await peer.getReceivers()) {
      final track = receiver.track;
      if (track != null) _adoptRemote(track);
    }
  }

  @override
  void setMuted(bool muted) {
    for (final track in _microphone?.getAudioTracks() ?? <MediaStreamTrack>[]) {
      track.enabled = !muted;
    }
  }

  @override
  void setSpeakerSuppressed(bool suppressed) {
    _speakerSuppressed = suppressed;
    // A disabled remote track renders silence, so audio already in flight
    // for the old reply is dropped instead of queued behind the new one.
    for (final track in _remoteAudio) {
      track.enabled = !suppressed;
    }
  }

  @override
  Future<AudioLevels?> levels() async {
    final peer = _peer;
    if (peer == null || _closed) return null;
    try {
      return audioLevelsFromStats(await peer.getStats());
    } catch (_) {
      // A peer torn down mid-read has no stats; the next poll sees null.
      return null;
    }
  }

  @override
  Future<void> close() async {
    _closed = true;
    _recovery?.cancel();
    _recovery = null;
    final gathered = _gathered;
    if (gathered != null && !gathered.isCompleted) gathered.complete();
    final media = _microphone;
    final peer = _peer;
    final channel = _channel;
    _microphone = null;
    _peer = null;
    _channel = null;
    _remoteAudio.clear();
    Object? failure;
    Future<void> release(Future<void> Function() action) async {
      try {
        await action();
      } catch (e) {
        failure ??= e;
      }
    }

    // A failed track release must not skip the peer/channel cleanup.
    for (final track in media?.getTracks() ?? <MediaStreamTrack>[]) {
      await release(() async {
        track.enabled = false;
        await track.stop();
      });
    }
    if (media != null) await release(media.dispose);
    if (channel != null) await release(channel.close);
    if (peer != null) await release(peer.dispose);
    if (failure != null) throw StateError('Audio cleanup failed: $failure');
  }
}
