// The one attachment admission rule shared by image picking, file picking,
// drag-and-drop and clipboard paste.

import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/screens/app_session/attachment_gate.dart';

void main() {
  test('a text-only provider takes documents but refuses images', () {
    const gate = AttachmentGate(editable: true, imageInput: false);
    expect(gate.acceptsAny, isTrue);
    expect(gate.acceptsImages, isFalse);
    expect(gate.admit('notes.md'), AttachmentAdmission.document);
    expect(gate.admit('shot.PNG'), AttachmentAdmission.refusedImage);
    expect(gate.admit('photo.jpeg'), AttachmentAdmission.refusedImage);
  });

  test('a provider with image prompts takes both', () {
    const gate = AttachmentGate(editable: true, imageInput: true);
    expect(gate.admit('shot.png'), AttachmentAdmission.image);
    expect(gate.admit('report.pdf'), AttachmentAdmission.document);
    expect(gate.admit('no-extension'), AttachmentAdmission.document);
  });

  test('a read-only or busy view takes nothing at all', () {
    for (final images in [true, false]) {
      final gate = AttachmentGate(editable: false, imageInput: images);
      expect(gate.acceptsAny, isFalse);
      expect(gate.acceptsImages, isFalse);
      expect(gate.admit('notes.md'), AttachmentAdmission.refused);
      expect(gate.admit('shot.png'), AttachmentAdmission.refused);
    }
  });

  group('a late result', () {
    const both = AttachmentGate(editable: true, imageInput: true);
    const textOnly = AttachmentGate(editable: true, imageInput: false);
    const ticket = AttachmentTicket(service: 's', revision: 4, granted: both);

    test('brings nothing once revoked or for another service', () {
      for (final onScreen in [both, null]) {
        expect(
          ticket.now(service: 's', revision: 5, onScreen: onScreen).acceptsAny,
          isFalse,
        );
        expect(
          ticket.now(service: 'x', revision: 4, onScreen: onScreen).acceptsAny,
          isFalse,
        );
      }
    });

    test('on screen, no more than the view allows now', () {
      expect(
        ticket
            .now(service: 's', revision: 4, onScreen: AttachmentGate.closed)
            .acceptsAny,
        isFalse,
        reason: 'the view turned read-only',
      );
      final now = ticket.now(service: 's', revision: 4, onScreen: textOnly);
      expect(now.acceptsAny, isTrue);
      expect(now.acceptsImages, isFalse, reason: 'image support went away');
      expect(
        ticket.now(service: 's', revision: 4, onScreen: both).acceptsImages,
        isTrue,
      );
    });

    test('after navigating away, what was granted — never more', () {
      expect(ticket.now(service: 's', revision: 4).acceptsImages, isTrue);
      const narrow = AttachmentTicket(
        service: 's',
        revision: 4,
        granted: textOnly,
      );
      expect(
        narrow.now(service: 's', revision: 4, onScreen: both).acceptsImages,
        isFalse,
        reason: 'a grant is never widened',
      );
      const none = AttachmentTicket(
        service: 's',
        revision: 4,
        granted: AttachmentGate.closed,
      );
      expect(none.now(service: 's', revision: 4).acceptsAny, isFalse);
    });
  });
}
