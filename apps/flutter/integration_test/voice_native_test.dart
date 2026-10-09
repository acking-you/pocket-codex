import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';
import 'package:integration_test/integration_test.dart';

/// Opt-in native smoke test: negotiates real WebRTC without microphone access,
/// credentials, or a paid realtime call. Device calls remain a separate check.
void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  testWidgets(
    'native audio SDP and ordered control channel negotiate locally',
    (tester) async {
      await tester.pumpWidget(
        const MaterialApp(home: Scaffold(body: Text('WebRTC smoke test'))),
      );
      final left = await createPeerConnection({'iceServers': <Object>[]});
      final right = await createPeerConnection({'iceServers': <Object>[]});
      try {
        final received = Completer<String>();
        right.onDataChannel = (channel) {
          channel.onMessage = (message) {
            if (!received.isCompleted) received.complete(message.text);
          };
        };
        await left.addTransceiver(
          kind: RTCRtpMediaType.RTCRtpMediaTypeAudio,
          init: RTCRtpTransceiverInit(direction: TransceiverDirection.RecvOnly),
        );
        final channel = await left.createDataChannel(
          'oai-events',
          RTCDataChannelInit(),
        );
        channel.onDataChannelState = (state) {
          if (state == RTCDataChannelState.RTCDataChannelOpen) {
            channel.send(RTCDataChannelMessage('native-ready'));
          }
        };
        Future<RTCSessionDescription> local(
          RTCPeerConnection peer,
          RTCSessionDescription description,
        ) async {
          final gathered = Completer<void>();
          peer.onIceGatheringState = (state) {
            if (state == RTCIceGatheringState.RTCIceGatheringStateComplete &&
                !gathered.isCompleted) {
              gathered.complete();
            }
          };
          await peer.setLocalDescription(description);
          await gathered.future.timeout(const Duration(seconds: 10));
          return (await peer.getLocalDescription())!;
        }

        final offer = await local(
          left,
          await left.createOffer({
            'mandatory': {
              'OfferToReceiveAudio': true,
              'OfferToReceiveVideo': false,
            },
            'optional': <Object>[],
          }),
        );
        expect(offer.sdp, contains('m=audio'));
        expect(offer.sdp, isNot(contains('m=video')));
        expect(offer.sdp, contains('opus/48000'));
        expect(offer.sdp, contains('m=application'));
        await right.setRemoteDescription(offer);
        final answer = await local(right, await right.createAnswer());
        await left.setRemoteDescription(answer);
        expect(
          await received.future.timeout(const Duration(seconds: 15)),
          'native-ready',
        );
        await channel.close();
      } finally {
        await left.dispose();
        await right.dispose();
      }
    },
    skip: !const bool.fromEnvironment('PCX_NATIVE_VOICE'),
  );
}
