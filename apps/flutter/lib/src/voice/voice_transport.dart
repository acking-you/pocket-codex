import 'dart:async';

import 'package:flutter_webrtc/flutter_webrtc.dart';

/// Client-owned media connection; credentials and Codex remain on the host.
abstract interface class VoiceTransport {
  /// Capture the microphone and return a complete WebRTC offer.
  Future<String> offer({
    required void Function() onConnected,
    required void Function(String) onFailure,
  });

  /// Apply the answer delivered by thread/realtime/sdp.
  Future<void> answer(String sdp);

  /// Enable or disable the existing microphone track without reconnecting.
  void setMuted(bool muted);

  /// Release microphone, data channel and peer connection, including startup.
  Future<void> close();
}

/// WebRTC supplies audio encoding, playback and echo cancellation natively.
class WebRtcVoiceTransport implements VoiceTransport {
  RTCPeerConnection? _peer;
  MediaStream? _microphone;
  RTCDataChannel? _channel;
  Completer<void>? _gathered;
  bool _closed = false;

  void _checkOpen() {
    if (_closed) throw StateError('Voice connection was cancelled');
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
      if (state == RTCPeerConnectionState.RTCPeerConnectionStateConnected) {
        onConnected();
      } else if (state == RTCPeerConnectionState.RTCPeerConnectionStateFailed ||
          state == RTCPeerConnectionState.RTCPeerConnectionStateDisconnected) {
        onFailure('Audio connection interrupted');
      }
    };
    final media = await navigator.mediaDevices.getUserMedia({
      'audio': {'echoCancellation': true, 'noiseSuppression': true},
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
  }

  @override
  void setMuted(bool muted) {
    for (final track in _microphone?.getAudioTracks() ?? <MediaStreamTrack>[]) {
      track.enabled = !muted;
    }
  }

  @override
  Future<void> close() async {
    _closed = true;
    final gathered = _gathered;
    if (gathered != null && !gathered.isCompleted) gathered.complete();
    final media = _microphone;
    final peer = _peer;
    final channel = _channel;
    _microphone = null;
    _peer = null;
    _channel = null;
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
