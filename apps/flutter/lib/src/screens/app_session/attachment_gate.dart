/// The one admission rule for composer attachments, shared by the image
/// picker, the file picker, drag-and-drop and clipboard paste.
///
/// A read-only view (a Guardian reviewer, a session another writer holds, a
/// child session) and a send in flight take nothing: no chip, no upload. An
/// image is taken only where prompts can carry images — a native Codex
/// view, and an ACP agent that advertised image prompts. Text paste
/// never passes through here, so it keeps working everywhere.
library;

/// Extensions the image pipeline decodes; anything else is a document.
const attachmentImageExtensions = {'png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'};

/// Whether a file named [name] goes through the image pipeline.
bool looksLikeImage(String name) {
  final dot = name.lastIndexOf('.');
  if (dot < 0 || dot == name.length - 1) return false;
  return attachmentImageExtensions.contains(
    name.substring(dot + 1).toLowerCase(),
  );
}

/// What the composer may take right now.
class AttachmentGate {
  /// A gate for a view that is [editable] (not read-only, not sending) and
  /// whose provider does or does not accept [imageInput].
  const AttachmentGate({required this.editable, required this.imageInput});

  /// A gate that takes nothing.
  static const closed = AttachmentGate(editable: false, imageInput: false);

  /// The view can take attachments at all.
  final bool editable;

  /// Prompts can carry images.
  final bool imageInput;

  /// Whether any attachment may enter.
  bool get acceptsAny => editable;

  /// Whether an image may enter.
  bool get acceptsImages => editable && imageInput;

  /// How a file named [name] may enter: through the image pipeline, as an
  /// uploaded document, or not at all.
  AttachmentAdmission admit(String name) {
    if (!editable) return AttachmentAdmission.refused;
    if (!looksLikeImage(name)) return AttachmentAdmission.document;
    return imageInput
        ? AttachmentAdmission.image
        : AttachmentAdmission.refusedImage;
  }
}

/// An admission that completes later: a picker, a clipboard read, or a
/// file read before its upload.
///
/// It is bound to the draft chosen when the user acted, through the
/// service and the draft's admission revision of that moment, and to the
/// gate [granted] then. When the result arrives, [now] decides what it may
/// still bring:
///
/// - nothing, once the draft's revision moved (its view turned read-only,
///   its host connection was lost or replaced, image support changed) or
///   the screen shows another service;
/// - for a draft still on screen, no more than both the grant and the gate
///   of this moment allow;
/// - for a draft the user navigated away from, no more than was granted:
///   navigating away neither revokes the grant nor widens it.
class AttachmentTicket {
  /// A ticket for [service] at draft revision [revision], granted [granted].
  const AttachmentTicket({
    required this.service,
    required this.revision,
    required this.granted,
  });

  /// The service the user acted in.
  final String service;

  /// The destination draft's admission revision when the user acted.
  final int revision;

  /// What the view allowed when the user acted.
  final AttachmentGate granted;

  /// What the result may bring now. [onScreen] is the current gate when the
  /// destination draft is the one on screen, otherwise `null`.
  AttachmentGate now({
    required String service,
    required int revision,
    AttachmentGate? onScreen,
  }) {
    if (service != this.service || revision != this.revision) {
      return AttachmentGate.closed;
    }
    final limit = onScreen ?? granted;
    return AttachmentGate(
      editable: granted.editable && limit.editable,
      imageInput: granted.imageInput && limit.imageInput,
    );
  }
}

/// The outcome of [AttachmentGate.admit].
enum AttachmentAdmission {
  /// Processed locally and sent inline with the prompt.
  image,

  /// Uploaded to the host and referenced by path in the prompt text.
  document,

  /// An image where prompts cannot carry images.
  refusedImage,

  /// Nothing enters a read-only or busy view.
  refused,
}
