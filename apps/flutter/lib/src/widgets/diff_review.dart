import 'package:flutter/material.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/code_highlight.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/git_diff.dart';
import 'package:pocket_codex/src/theme.dart';

/// The desktop diff-review surface: a changed-file tree beside a single file's
/// syntax-highlighted diff, focused on the changed hunks. Composed by the
/// session screen into a resizable split; these are the two panes.

/// One rendered row: either a code line or a "N unmodified lines" gap marker.
sealed class _Row {
  const _Row();
}

class _CodeRow extends _Row {
  const _CodeRow(this.line, this.newNo);
  final DiffLine line;
  final int? newNo;
}

/// A run of unchanged lines git elided between hunks. Carries the run's
/// new-file line range so, on click, the review can splice in exactly those
/// lines from the current file — they're unchanged, so the working tree holds
/// them verbatim.
class _GapRow extends _Row {
  const _GapRow(this.count, this.newStart, this.newEnd);
  final int count;
  final int newStart; // 1-indexed, inclusive, in the new file
  final int newEnd;
}

/// The diff pane: one file's changes, highlighted, hunk by hunk. Runs of
/// unchanged lines that git elided between hunks show as a clickable
/// "N unmodified lines" marker; clicking reads the file and reveals those lines
/// in place. One lazy vertical list — long lines wrap rather than opening a
/// second scroll axis.
class DiffReviewView extends StatefulWidget {
  const DiffReviewView({
    super.key,
    required this.file,
    this.branch,
    this.onLoadFile,
  });

  final DiffFile file;
  final String? branch;

  /// Reads the current file's lines so an elided gap can be expanded. Returns
  /// null when the host can't serve it (no meta tunnel / outside a root), in
  /// which case gaps stay collapsed. Called at most once per open file.
  final Future<List<String>?> Function(String relPath)? onLoadFile;

  @override
  State<DiffReviewView> createState() => _DiffReviewViewState();
}

class _DiffReviewViewState extends State<DiffReviewView> {
  final Set<int> _expanded = {}; // indices (into _rows) of expanded gaps
  List<String>? _lines; // the file's lines, fetched lazily on first expand
  bool _loading = false;
  bool _failed = false;
  late List<_Row> _rows = _computeRows();

  @override
  void didUpdateWidget(DiffReviewView old) {
    super.didUpdateWidget(old);
    if (old.file.path != widget.file.path) {
      _expanded.clear();
      _lines = null;
      _loading = false;
      _failed = false;
      _rows = _computeRows();
    }
  }

  List<_Row> _computeRows() {
    final rows = <_Row>[];
    int? oldNo, newNo; // next old-/new-file line numbers
    for (final line in widget.file.lines) {
      if (line.kind == DiffLineKind.hunk) {
        final ns = hunkNewStart(line.text);
        if (ns != null) {
          // The unchanged run git left out before this hunk, on the new side.
          final gapStart = newNo ?? 1;
          final gapEnd = ns - 1;
          if (gapEnd >= gapStart) {
            rows.add(_GapRow(gapEnd - gapStart + 1, gapStart, gapEnd));
          }
          newNo = ns;
        }
        final os = hunkOldStart(line.text);
        if (os != null) oldNo = os;
        continue;
      }
      switch (line.kind) {
        case DiffLineKind.removed:
          rows.add(_CodeRow(line, oldNo)); // removed lines carry the old number
          if (oldNo != null) oldNo++;
        case DiffLineKind.added:
          rows.add(_CodeRow(line, newNo));
          if (newNo != null) newNo++;
        case DiffLineKind.context:
          rows.add(_CodeRow(line, newNo));
          if (oldNo != null) oldNo++;
          if (newNo != null) newNo++;
        case DiffLineKind.hunk:
          break;
      }
    }
    return rows;
  }

  Future<void> _ensureLoaded() async {
    if (_lines != null || _loading || _failed || widget.onLoadFile == null) {
      return;
    }
    setState(() => _loading = true);
    final lines = await widget.onLoadFile!(widget.file.path);
    if (!mounted) return;
    setState(() {
      _loading = false;
      if (lines == null) {
        _failed = true;
      } else {
        _lines = lines;
      }
    });
  }

  void _toggleGap(int index) {
    setState(() {
      if (!_expanded.remove(index)) _expanded.add(index);
    });
    _ensureLoaded();
  }

  /// The rows to actually paint: an expanded gap (once the file is loaded) is
  /// replaced by its real unchanged lines; every other gap stays a marker.
  List<({_Row row, int? gapIndex})> _display() {
    final out = <({_Row row, int? gapIndex})>[];
    for (var i = 0; i < _rows.length; i++) {
      final r = _rows[i];
      if (r is _GapRow && _expanded.contains(i) && _lines != null) {
        for (var ln = r.newStart; ln <= r.newEnd; ln++) {
          final text = (ln - 1) < _lines!.length ? _lines![ln - 1] : '';
          out.add((
            row: _CodeRow(DiffLine(DiffLineKind.context, text), ln),
            gapIndex: null,
          ));
        }
      } else {
        out.add((row: r, gapIndex: r is _GapRow ? i : null));
      }
    }
    return out;
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final l10n = AppLocalizations.of(context);
    final file = widget.file;
    final lang = languageHintForPath(file.path);
    final display = _display();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // File header: path + counts, and the branch comparison it belongs to.
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 10, 12, 8),
          child: Row(
            children: [
              Icon(
                Icons.insert_drive_file_outlined,
                size: 15,
                color: scheme.onSurfaceVariant,
              ),
              const SizedBox(width: 8),
              Expanded(
                child: SelectableText(
                  file.path,
                  maxLines: 1,
                  style: TextStyle(
                    fontFamily: monoFontFamily,
                    fontFamilyFallback: monoCjkFallback,
                    fontSize: 12.5,
                    color: scheme.onSurface,
                  ),
                ),
              ),
              const SizedBox(width: 10),
              Text(
                '+${file.added}',
                style: TextStyle(
                  fontSize: 12,
                  fontWeight: FontWeight.w600,
                  color: additionColor(scheme),
                ),
              ),
              const SizedBox(width: 6),
              Text(
                '−${file.removed}',
                style: TextStyle(
                  fontSize: 12,
                  fontWeight: FontWeight.w600,
                  color: scheme.error,
                ),
              ),
            ],
          ),
        ),
        if (widget.branch != null)
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
            child: Row(
              children: [
                Icon(
                  Icons.account_tree_outlined,
                  size: 13,
                  color: scheme.onSurfaceVariant,
                ),
                const SizedBox(width: 6),
                Text(
                  '${widget.branch} → origin/${widget.branch}',
                  style: TextStyle(
                    fontSize: 11.5,
                    color: scheme.onSurfaceVariant,
                  ),
                ),
              ],
            ),
          ),
        const Divider(height: 1),
        Expanded(
          // Selectable so a reviewer can lift a snippet; lazy so a long file
          // doesn't lay out every line up front.
          child: SelectionArea(
            child: ListView.builder(
              itemCount: display.length,
              itemBuilder: (c, i) {
                final entry = display[i];
                final r = entry.row;
                if (r is _GapRow) {
                  return _gap(scheme, l10n, r.count, entry.gapIndex!);
                }
                final code = r as _CodeRow;
                return _code(
                  scheme,
                  Theme.of(context).brightness,
                  lang,
                  code.line,
                  code.newNo,
                );
              },
            ),
          ),
        ),
      ],
    );
  }

  Widget _gap(ColorScheme scheme, AppLocalizations l10n, int count, int index) {
    // Expandable only when a file reader is wired AND the last read didn't
    // fail; otherwise a plain marker (nothing to reveal).
    final canExpand = widget.onLoadFile != null && !_failed;
    final busy = _loading && _expanded.contains(index);
    return InkWell(
      mouseCursor: clickable,
      key: Key('gap-$index'),
      onTap: canExpand ? () => _toggleGap(index) : null,
      child: Container(
        color: scheme.surfaceContainerLow,
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
        child: Row(
          children: [
            if (busy)
              const SizedBox(
                width: 15,
                height: 15,
                child: CircularProgressIndicator(strokeWidth: 2),
              )
            else
              Icon(
                canExpand ? Icons.unfold_more : Icons.more_horiz,
                size: 15,
                color: canExpand ? scheme.primary : scheme.onSurfaceVariant,
              ),
            const SizedBox(width: 8),
            Text(
              _failed
                  ? l10n.diffExpandFailed(count)
                  : l10n.diffUnmodified(count),
              style: TextStyle(
                fontSize: 11.5,
                color: canExpand ? scheme.primary : scheme.onSurfaceVariant,
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _code(
    ColorScheme scheme,
    Brightness brightness,
    String lang,
    DiffLine line,
    int? lineNo,
  ) {
    final (Color bg, String marker, Color markerColor) = switch (line.kind) {
      DiffLineKind.added => (
        additionColor(scheme).withValues(alpha: 0.13),
        '+',
        additionColor(scheme),
      ),
      DiffLineKind.removed => (
        scheme.error.withValues(alpha: 0.10),
        '−',
        scheme.error,
      ),
      _ => (Colors.transparent, ' ', scheme.onSurfaceVariant),
    };
    final base = TextStyle(
      fontFamily: monoFontFamily,
      fontFamilyFallback: monoCjkFallback,
      fontSize: 12.5,
      height: 1.4,
      color: line.kind == DiffLineKind.removed
          ? scheme.onSurfaceVariant
          : scheme.onSurface,
    );
    final span = highlightCode(
      code: line.text,
      language: lang,
      base: base,
      brightness: brightness,
      // Upright: italic comments over a CJK fallback read as distorted.
      allowItalic: false,
    );
    return Container(
      color: bg,
      padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 1),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 44,
            child: Text(
              lineNo?.toString() ?? '',
              textAlign: TextAlign.right,
              style: base.copyWith(fontSize: 11, color: scheme.outline),
            ),
          ),
          SizedBox(
            width: 18,
            child: Text(
              marker,
              textAlign: TextAlign.center,
              style: base.copyWith(color: markerColor),
            ),
          ),
          // Wrap long lines rather than open a horizontal scroll — the review
          // pane is already the wide one, and a second scroll axis is exactly
          // the nested-scroll trap we avoid elsewhere.
          Expanded(child: Text.rich(span)),
        ],
      ),
    );
  }
}

/// The changed-file tree: paths folded into a collapsible folder hierarchy,
/// each leaf a changed file with its ± counts. The selected file is marked.
class ChangedFileTree extends StatelessWidget {
  const ChangedFileTree({
    super.key,
    required this.files,
    required this.selected,
    required this.onSelect,
  });

  final List<DiffFile> files;
  final String? selected;
  final ValueChanged<String> onSelect;

  @override
  Widget build(BuildContext context) {
    final root = _Node('');
    for (final f in files) {
      root.insert(f.path.split(RegExp(r'[\\/]')), f);
    }
    return ListView(
      padding: const EdgeInsets.symmetric(vertical: 4),
      children: root.children.values.map((n) => _node(context, n, 0)).toList(),
    );
  }

  Widget _node(BuildContext context, _Node node, int depth) {
    final scheme = Theme.of(context).colorScheme;
    final pad = EdgeInsets.only(left: 8.0 + depth * 14, right: 8);
    if (node.file != null) {
      final f = node.file!;
      final isSel = f.path == selected;
      return Material(
        color: isSel ? scheme.primary.withValues(alpha: 0.12) : null,
        child: InkWell(
          mouseCursor: clickable,
          key: Key('review-file-${f.path}'),
          onTap: () => onSelect(f.path),
          child: Padding(
            padding: pad.copyWith(top: 5, bottom: 5),
            child: Row(
              children: [
                Icon(
                  Icons.description_outlined,
                  size: 14,
                  color: isSel ? scheme.primary : scheme.onSurfaceVariant,
                ),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(
                    node.name,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 12.5,
                      color: isSel ? scheme.primary : scheme.onSurface,
                      fontWeight: isSel ? FontWeight.w600 : FontWeight.w400,
                    ),
                  ),
                ),
                Text(
                  '+${f.added}',
                  style: TextStyle(
                    fontSize: 10.5,
                    color: additionColor(scheme),
                  ),
                ),
                const SizedBox(width: 3),
                Text(
                  '−${f.removed}',
                  style: TextStyle(fontSize: 10.5, color: scheme.error),
                ),
              ],
            ),
          ),
        ),
      );
    }
    // A folder: collapse one-child chains (a/b/c) into a single row so the tree
    // doesn't waste a level per empty directory.
    var display = node;
    final segs = [node.name];
    while (display.file == null &&
        display.children.length == 1 &&
        display.children.values.first.file == null) {
      display = display.children.values.first;
      segs.add(display.name);
    }
    return Theme(
      data: Theme.of(context).copyWith(dividerColor: Colors.transparent),
      child: ExpansionTile(
        key: PageStorageKey<String>('review-dir-${node.path}'),
        initiallyExpanded: true,
        dense: true,
        tilePadding: pad,
        childrenPadding: EdgeInsets.zero,
        visualDensity: VisualDensity.compact,
        leading: Icon(
          Icons.folder_outlined,
          size: 15,
          color: scheme.onSurfaceVariant,
        ),
        title: Text(
          segs.join('/'),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: const TextStyle(fontSize: 12.5),
        ),
        children: display.children.values
            .map((c) => _node(context, c, depth + 1))
            .toList(),
      ),
    );
  }
}

/// One node of the file tree — an internal folder (children) or a leaf (file).
class _Node {
  _Node(this.name, [this.path = '']);
  final String name;
  final String path;
  final Map<String, _Node> children = {};
  DiffFile? file;

  void insert(List<String> segs, DiffFile f) {
    if (segs.isEmpty) return;
    if (segs.length == 1) {
      children[segs.first] = _Node(segs.first, f.path)..file = f;
      return;
    }
    final head = segs.first;
    final child = children.putIfAbsent(
      head,
      () => _Node(head, path.isEmpty ? head : '$path/$head'),
    );
    child.insert(segs.sublist(1), f);
  }
}
