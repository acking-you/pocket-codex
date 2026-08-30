/// The transcript's data model and the few values every view of it shares.
///
/// Kept separate from the widgets so the session screen and its extracted cards
/// can both depend on this without depending on each other: `TranscriptItem` is
/// built by the screen's event handling and consumed by nearly every card, so
/// putting it in either place would make the split circular.
///
/// Belongs here: the shape of a transcript row, and constants two or more views
/// must agree on. NOT here: anything that builds a widget.
library;

import 'package:pocket_codex/src/widgets/message_images.dart';

/// Width at which the transcript switches from mobile chat bubbles to the
/// desktop document layout (role labels, accent rails, compact tool rows).
///
/// Shared because the composer, the transcript, and the activity cards must
/// flip together — one of them using a different threshold would leave a
/// half-mobile, half-desktop transcript at some widths.
const double docLayoutWidth = 720;

/// One row of the transcript: a message, a tool call, or a synthesized marker.
///
/// Mutable on purpose. A streaming reply arrives as deltas that append to
/// [text], and the screen keeps an id -> index map so each delta finds its row
/// without rebuilding the list.
class TranscriptItem {
  /// Creates a transcript row.
  TranscriptItem({
    required this.id,
    required this.type,
    this.title = '',
    this.text = '',
    this.images = const [],
    this.imageUrls = const [],
    this.streaming = false,
    this.model,
    this.effortWire,
    this.modelConfirmed = false,
    this.modelRerouted = false,
    this.turnId = '',
    this.turnCompletedAt,
  });

  /// Stable row id: the server's item id, or a locally minted one for an
  /// optimistic user message.
  final String id;

  /// `userMessage` | `agentMessage` | `commandExecution` | `webSearch` | …
  String type;

  /// Short header for a tool call (the command, the query, the file).
  String title;

  /// The row's body. Appended to in place while [streaming].
  String text;

  /// Image attachments of a user message, resolved once (data URLs decoded to
  /// bytes; host-only paths kept as chips) so rebuilds never re-decode base64.
  List<ResolvedImage> images;

  /// The same attachments' raw wire URLs, kept for CONTENT comparisons (the
  /// duplicate-collapse guard): image-only messages all have empty text, so
  /// only the URLs distinguish two different photos.
  List<String> imageUrls;

  /// Whether more deltas are still arriving for this row.
  bool streaming;

  /// Model the closing turn actually ran with, for a `turnDuration` footnote.
  String? model;

  /// Reasoning effort the closing turn ran with, on the wire.
  String? effortWire;

  /// Whether the server confirmed the model stamp (vs. our local guess).
  bool modelConfirmed;

  /// Whether the server rerouted the model mid-turn.
  bool modelRerouted;

  /// The turn this item belongs to, per the server (`thread/read` nests items
  /// under their turn). Empty when the source carried no turn envelope — a
  /// locally synthesized marker, or a rollout file read from disk.
  String turnId;

  /// Unix seconds when this item's turn completed, when the server said.
  int? turnCompletedAt;

  /// Whether this row is the user's own message.
  bool get isUser => type == 'userMessage';

  /// Whether this row is a reply from the agent.
  bool get isAgent => type == 'agentMessage';

  /// Whether this row is a message from either side (vs. a tool call).
  bool get isMessage => isUser || isAgent;

  /// Standalone system notices (compaction / stopped) that render as a centered
  /// divider and must never be folded into a tool-call group.
  bool get isNotice => type == 'contextCompaction' || type == 'interrupted';
}

// `ActivityGroup` and `AgentTurn` — the two ways consecutive rows collapse into
// one card — live in `activity_cards.dart`, with the widgets that render them.
