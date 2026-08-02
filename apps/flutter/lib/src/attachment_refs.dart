/// The text convention for document/file attachments.
///
/// codex's input protocol carries only text and images inline; its native
/// document workflow is a FILE PATH in the prompt text that the agent reads
/// with its own tools (exactly what the TUI's `@file` mention inserts — a bare
/// path, double-quoted only when it contains whitespace). Pocket-Codex uploads
/// the controller-side file to the HOST first (over the meta tunnel), then
/// appends a reference block in that same convention:
///
/// ```text
/// <the user's message>
///
/// ## Attached files (read them from this machine's filesystem):
/// - "C:\Users\u\.codex\pocket-codex-uploads\...-report with space.pdf"
/// - /home/u/.codex/pocket-codex-uploads/...-notes.md
/// ```
///
/// The block is part of the sent text — it echoes back verbatim in history and
/// on other devices, so [splitFileRefs] can always rebuild the chips. The
/// header line is wire format: changing it orphans chips in old transcripts.
library;

import 'package:pocket_codex/src/ide_context.dart';

/// Header line introducing the attached-files block (English on purpose — it
/// is model-facing wire text, like codex's own `<image>` markers).
const String kAttachedFilesHeader =
    "## Attached files (read them from this machine's filesystem):";

/// The most files one message may carry (UI guard).
const int kMaxFilesPerMessage = 4;

/// Per-file upload cap. Below the host's 64 MB request-body limit with
/// headroom, and tunnel-friendly on a mobile uplink.
const int kMaxFileBytes = 32 * 1024 * 1024;

/// Quote [path] like the codex TUI quotes an `@file` mention: double quotes
/// only when the path contains whitespace (and no quote of its own).
String quotePathLikeCodex(String path) {
  final hasWhitespace = path.contains(RegExp(r'\s'));
  if (hasWhitespace && !path.contains('"')) return '"$path"';
  return path;
}

/// Append the attached-files block for [hostPaths] to [text]. Returns [text]
/// unchanged when there is nothing to attach; the block alone when [text] is
/// empty (a files-only message).
String appendFileRefs(String text, List<String> hostPaths) {
  if (hostPaths.isEmpty) return text;
  final block = StringBuffer(kAttachedFilesHeader);
  for (final p in hostPaths) {
    block.write('\n- ${quotePathLikeCodex(p)}');
  }
  if (text.isEmpty) return block.toString();
  return '$text\n\n$block';
}

/// Split a message's text back into the display text and the attached host
/// paths. Returns the input unchanged (no paths) unless the text ends in a
/// well-formed block: the header on its own line followed only by `- ` lines —
/// anything else (user prose that merely resembles the header, trailing extra
/// text) is left alone rather than mis-parsed into chips.
({String text, List<String> paths}) splitFileRefs(String text) {
  final at = text.lastIndexOf(kAttachedFilesHeader);
  if (at < 0) return (text: text, paths: const []);
  // The header must start its own line.
  if (at > 0 && text[at - 1] != '\n') return (text: text, paths: const []);
  final after = text.substring(at + kAttachedFilesHeader.length);
  final lines = after.split('\n');
  // First split element is the remainder of the header line — must be empty.
  if (lines.first.trim().isNotEmpty) return (text: text, paths: const []);
  final paths = <String>[];
  for (final line in lines.skip(1)) {
    final t = line.trim();
    if (t.isEmpty) continue;
    if (!t.startsWith('- ')) return (text: text, paths: const []);
    var p = t.substring(2).trim();
    if (p.length >= 2 && p.startsWith('"') && p.endsWith('"')) {
      p = p.substring(1, p.length - 1);
    }
    if (p.isNotEmpty) paths.add(p);
  }
  if (paths.isEmpty) return (text: text, paths: const []);
  return (text: text.substring(0, at).trimRight(), paths: paths);
}

/// Basename of a host path (either separator style), for chip labels.
String hostPathBasename(String path) {
  final cut = path.lastIndexOf(RegExp(r'[/\\]'));
  return cut < 0 ? path : path.substring(cut + 1);
}

/// Display form of a session/thread PREVIEW that may carry wire text. Server
/// previews are the first user message verbatim (possibly truncated), so
/// without this a list title reads as the raw attached-files header or the
/// serialized IDE context instead of what the user typed.
///
/// A preview that is nothing but an attachment block shows [placeholder];
/// otherwise the wrappers are stripped the same way the transcript strips
/// them. `startsWith` (not a full parse) is what catches server-truncated
/// blocks.
///
/// Returns EMPTY when the preview is an injected context fragment: a session
/// opened from an IDE begins with codex's own `<recommended_plugins>` message,
/// and calling that a title labelled every such session identically. There is
/// no title to derive from machinery, so callers fall back to "untitled".
String previewWithoutFileRefs(String preview, String placeholder) {
  if (preview.trimLeft().startsWith(kAttachedFilesHeader)) return placeholder;
  if (isContextFragment(preview)) return '';
  if (isTruncatedIdeContext(preview)) return placeholder;
  // A voice handoff is the user's own turn, wrapped — show what they said.
  final spoken = realtimeDelegationInput(preview);
  if (spoken != null) return spoken;
  final text = splitIdeContext(preview).text;
  final split = splitFileRefs(text);
  return split.paths.isEmpty ? text : split.text;
}
