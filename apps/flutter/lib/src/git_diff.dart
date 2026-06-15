/// A lightweight unified-diff model + parser for the in-app diff viewer.
/// Avoids a heavy dependency — codex returns a standard `git diff`.
library;

/// Kind of a single diff line, used to colour it.
enum DiffLineKind {
  /// Unchanged context line.
  context,

  /// Added line (`+`).
  added,

  /// Removed line (`-`).
  removed,

  /// A `@@ ... @@` hunk header.
  hunk,
}

/// One line within a file's diff.
class DiffLine {
  /// Creates a diff line.
  const DiffLine(this.kind, this.text);

  /// What kind of change this line represents.
  final DiffLineKind kind;

  /// The line text (without the leading +/-/space marker).
  final String text;
}

/// One file's changes within a diff.
class DiffFile {
  /// Creates a file diff.
  DiffFile({required this.path, required this.lines})
    : added = lines.where((l) => l.kind == DiffLineKind.added).length,
      removed = lines.where((l) => l.kind == DiffLineKind.removed).length;

  /// File path (the `b/` side, i.e. the new path).
  final String path;

  /// Ordered diff lines (hunks + context + changes).
  final List<DiffLine> lines;

  /// Added line count.
  final int added;

  /// Removed line count.
  final int removed;
}

/// A parsed unified diff: a list of changed files + roll-up counts.
class DiffModel {
  /// Creates a diff model.
  const DiffModel(this.files);

  /// Changed files, in diff order.
  final List<DiffFile> files;

  /// Total added lines across all files.
  int get added => files.fold(0, (s, f) => s + f.added);

  /// Total removed lines across all files.
  int get removed => files.fold(0, (s, f) => s + f.removed);

  /// Whether the diff has no files.
  bool get isEmpty => files.isEmpty;

  /// Parse a unified diff string. Tolerant of missing `diff --git` headers
  /// (falls back to `+++`/`---` file markers) and of an empty input.
  static DiffModel parse(String raw) {
    final files = <DiffFile>[];
    String? path;
    String? aPath; // the `--- a/…` path, used as fallback for deletions
    var lines = <DiffLine>[];

    void flush() {
      final p = path;
      if (p != null && lines.isNotEmpty) {
        files.add(DiffFile(path: p, lines: lines));
      }
      lines = <DiffLine>[];
    }

    // Split on \r?\n so diffs produced with CRLF endings don't leave a trailing
    // \r on each line (which would break the prefix checks below / rendering).
    for (final line in raw.split(RegExp(r'\r?\n'))) {
      if (line.startsWith('diff --git')) {
        // Don't parse the path from this header — it splits on spaces and
        // would truncate paths containing spaces. The authoritative path comes
        // from the `+++`/`---` markers below.
        flush();
        path = null;
        aPath = null;
        continue;
      }
      if (line.startsWith('--- ')) {
        aPath = _pathOf(line.substring(4));
        continue;
      }
      if (line.startsWith('+++ ')) {
        // New path is authoritative; for deletions (`+++ /dev/null`) fall back
        // to the old path. Starts a new file in header-less diffs too.
        final p = _pathOf(line.substring(4)) ?? aPath;
        if (p != null) {
          if (lines.isNotEmpty) flush();
          path = p;
        }
        continue;
      }
      if (line.startsWith('index ') ||
          line.startsWith('new file') ||
          line.startsWith('deleted file') ||
          line.startsWith('similarity ') ||
          line.startsWith('rename ') ||
          line.startsWith('old mode') ||
          line.startsWith('new mode')) {
        continue; // metadata, not shown
      }
      if (path == null) continue;
      if (line.startsWith('@@')) {
        lines.add(DiffLine(DiffLineKind.hunk, line));
      } else if (line.startsWith('+')) {
        lines.add(DiffLine(DiffLineKind.added, line.substring(1)));
      } else if (line.startsWith('-')) {
        lines.add(DiffLine(DiffLineKind.removed, line.substring(1)));
      } else if (line.startsWith(' ')) {
        lines.add(DiffLine(DiffLineKind.context, line.substring(1)));
      }
    }
    flush();
    return DiffModel(files);
  }
}

/// Resolve a `--- `/`+++ ` marker payload to a clean path, or null for
/// `/dev/null`. Cuts a trailing tab (some diffs append a timestamp) and strips
/// the `a/`/`b/` prefix + quotes. Tolerant of spaces in the filename.
String? _pathOf(String raw) {
  var s = raw.trim();
  final tab = s.indexOf('\t');
  if (tab >= 0) s = s.substring(0, tab).trim();
  if (s == '/dev/null' || s.isEmpty) return null;
  return _strip(s);
}

/// Strip a `a/` or `b/` prefix and surrounding quotes from a diff path.
String _strip(String p) {
  var s = p;
  if (s.startsWith('a/') || s.startsWith('b/')) s = s.substring(2);
  if (s.startsWith('"') && s.endsWith('"') && s.length >= 2) {
    s = s.substring(1, s.length - 1);
  }
  return s;
}
