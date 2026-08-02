/// Display-side handling of codex's IDE-context prompt wrapper.
///
/// A client with editor context (the IDE extension, the ChatGPT desktop app,
/// codex's own TUI) does not send the user's text alone: it serializes the
/// context — active file, selection, mentioned files — ahead of the request and
/// separates the two with a marker. Upstream states the contract at
/// `deps/codex/codex-rs/tui/src/ide_context/prompt.rs:12`:
///
/// > Match the desktop app and IDE extension delimiter exactly. IDE context is
/// > serialized into the raw prompt before this marker, then transcript
/// > rendering strips back to the request after the last marker.
///
/// So every surface that renders a transcript is expected to strip. Without it
/// a message that was typed as "为什么仍然黑屏" displays as a wall of wire text.
///
/// [kUserMessageBegin] is the protocol constant (`USER_MESSAGE_BEGIN`,
/// `deps/codex/codex-rs/protocol/src/protocol.rs:120`) and is the ONLY thing
/// this module treats as contract. The shape of the context ahead of it varies
/// by client — the vendored codex renders `## Active file:` while the desktop
/// app emits `# Files mentioned by the user:` — so [splitIdeContext] reads
/// mentions out of that prefix on a best-effort basis. A prefix it cannot
/// parse costs thumbnails, never the message text.
library;

/// The marker separating serialized IDE context from the user's own request.
/// Wire format: changing it orphans every transcript written by another client.
const String kUserMessageBegin = '## My request for Codex:';

/// A file the sending client named in its IDE context.
class IdeMentionedFile {
  /// Creates a mention of [path], labelled [name].
  const IdeMentionedFile({required this.name, required this.path});

  /// Display label the client used (usually the basename).
  final String name;

  /// The path as written — absolute on the sending machine, or workspace
  /// relative. Not resolved here; the widget layer decides what it can read.
  final String path;
}

// `## <label>: <path>` — the shape both known context renderers use for a
// file line. The label may contain spaces; the path runs to end of line.
final _mentionLine = RegExp(r'^##[ \t]+(.+?):[ \t]+(\S.*)$', multiLine: true);

// A mention worth showing: an absolute path (POSIX or Windows), or one that at
// least carries a directory separator. Prose like `## Active selection range:`
// has no value after the colon and never reaches here; a bare word would.
final _looksLikePath = RegExp(r'^(/|[A-Za-z]:[/\\]|\\\\)|[/\\]');

/// Split a raw user message into what the user actually typed and the files the
/// sending client mentioned around it.
///
/// Mirrors upstream's `extract_prompt_request_with_offset`: the request is
/// everything after the LAST [kUserMessageBegin]. A message without the marker
/// is returned unchanged with no mentions — the common case, since our own
/// composer never writes one.
({String text, List<IdeMentionedFile> files}) splitIdeContext(String raw) {
  final at = raw.lastIndexOf(kUserMessageBegin);
  if (at < 0) return (text: raw, files: const []);
  final text = raw.substring(at + kUserMessageBegin.length).trim();
  final files = <IdeMentionedFile>[];
  for (final m in _mentionLine.allMatches(raw.substring(0, at))) {
    final path = m[2]!.trim();
    if (!_looksLikePath.hasMatch(path)) continue;
    files.add(IdeMentionedFile(name: m[1]!.trim(), path: path));
  }
  return (text: text, files: files);
}

// Section headers the two known context renderers open with. Only used to
// recognise a preview the SERVER truncated before the marker — an unknown
// header degrades to showing the raw preview rather than guessing.
const _contextHeaders = [
  '# Files mentioned by the user:',
  '# Context from my IDE setup:',
];

/// Whether [preview] is the start of an IDE-context block with no request in
/// it yet — i.e. a server-truncated preview whose visible part is all wire.
bool isTruncatedIdeContext(String preview) {
  final t = preview.trimLeft();
  if (t.contains(kUserMessageBegin)) return false;
  return _contextHeaders.any(t.startsWith);
}

// Marker pairs codex wraps an injected contextual user fragment in — a message
// that carries the user role but that codex wrote. Mirrors the wire constants
// in `codex-rs/protocol/src/protocol.rs` and the fragments declaring their own
// `type_markers()` under `codex-rs/core/src/context/`. The same list lives in
// `crates/pocket-codex-codex/src/rollout.rs`, which filters rollouts we read
// ourselves; this copy covers previews that come from codex's app-server.
//
// Most fragments are `<tag>…</tag>`; the project's AGENTS.md is marked by prose
// instead (`UserInstructions::type_markers`), so it needs an explicit pair.
const _fragmentPairs = [('# AGENTS.md instructions', '</INSTRUCTIONS>')];

const _fragmentTags = [
  'user_instructions',
  'environment_context',
  'apps_instructions',
  'skills_instructions',
  'plugins_instructions',
  'collaboration_mode',
  'multi_agent_mode',
  'realtime_conversation',
  'context_window',
  'context_window_guidance',
  'recommended_plugins',
  'model_switch',
  'personality_spec',
  'subagent_notification',
  'turn_aborted',
  'user_shell_command',
];

/// Whether [text] is entirely one injected context fragment, and so is
/// machinery rather than anything the user said.
///
/// Mirrors upstream `matches_marked_text`
/// (`codex-rs/context-fragments/src/fragment.rs:116`): the trimmed text must
/// both open and close with the pair, case-insensitively. Text that merely
/// *contains* a marker is left alone — someone asking about `<turn_aborted>`
/// is still writing their own message.
/// A preview the server cut short counts too: it opened a fragment and the
/// close marker fell off the end, which still leaves nothing but wire.
bool isContextFragment(String text) {
  final trimmed = text.trim().toLowerCase();
  bool bounded(String open, String close) {
    if (!trimmed.startsWith(open)) return false;
    return trimmed.endsWith(close)
        ? trimmed.length >= open.length + close.length
        : !trimmed.contains(close);
  }

  return _fragmentTags.any((tag) => bounded('<$tag>', '</$tag>')) ||
      _fragmentPairs.any(
        (p) => bounded(p.$1.toLowerCase(), p.$2.toLowerCase()),
      );
}

/// What the user actually said inside a `<realtime_delegation>` wrapper.
///
/// Voice handoff wraps the spoken turn in `<input>…</input>`. Unlike the
/// fragments above these ARE the person's words, so they are unwrapped for
/// display rather than hidden. Null when [text] isn't such a wrapper.
String? realtimeDelegationInput(String text) {
  final trimmed = text.trim();
  if (!trimmed.startsWith('<realtime_delegation>') ||
      !trimmed.endsWith('</realtime_delegation>')) {
    return null;
  }
  final open = trimmed.indexOf('<input>');
  if (open < 0) return null;
  final close = trimmed.indexOf('</input>', open);
  if (close < 0) return null;
  final input = trimmed.substring(open + '<input>'.length, close).trim();
  return input.isEmpty ? null : input;
}

/// Extensions the image pipeline can decode, for deciding whether a mention
/// renders as a thumbnail or as a filename chip.
const _imageExtensions = {'png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'};

/// Whether [path] names a file we could show as a picture.
bool looksLikeImagePath(String path) {
  final dot = path.lastIndexOf('.');
  if (dot < 0 || dot == path.length - 1) return false;
  return _imageExtensions.contains(path.substring(dot + 1).toLowerCase());
}
