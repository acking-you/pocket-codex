/// The in-transcript diff surface: one changed file as a collapsible tile over
/// its syntax-highlighted hunks.
///
/// Distinct from `widgets/diff_review.dart`, which is the *desktop review split*
/// — a file tree beside a full-height diff. This is the compact form that sits
/// inline in a `fileChange` row, so it is height-bounded and collapsed by
/// default. The shared diff parsing and hunk-header helpers are in
/// `src/git_diff.dart`.
library;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/code_highlight.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/git_diff.dart';
import 'package:pocket_codex/src/theme.dart';

/// Most diff lines rendered for one expanded file. Everything inside a
/// [SingleChildScrollView] is laid out whether or not it is on screen, so this
/// is the cap that keeps one enormous file from making the whole panel drag,
/// and bounds the one-time highlight cost when a file is expanded.
const int _maxDiffLines = 200;

/// Directory prefix of a diff path, keeping its trailing separator. Empty for
/// a file at the repo root.
String _dirOf(String path) {
  final i = path.lastIndexOf(RegExp(r'[\\/]'));
  return i < 0 ? '' : path.substring(0, i + 1);
}

/// File name of a diff path.
String _baseOf(String path) {
  final i = path.lastIndexOf(RegExp(r'[\\/]'));
  return i < 0 ? path : path.substring(i + 1);
}

class DiffFileTile extends StatelessWidget {
  const DiffFileTile({
    super.key,
    required this.file,
    this.initiallyExpanded = true,
  });
  final DiffFile file;

  /// Inline review cards open expanded (the diff IS the point); the side
  /// panel's file list stays collapsed so a wide change stays scannable.
  final bool initiallyExpanded;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final l10n = AppLocalizations.of(context);
    return ExpansionTile(
      initiallyExpanded: initiallyExpanded,
      tilePadding: const EdgeInsets.symmetric(horizontal: 12),
      title: Row(
        children: [
          // The file NAME is what identifies a row, so it never ellipsizes —
          // the leading directories give way instead. A plain one-line path
          // clips from the tail, which in a side panel eats exactly the part
          // the user is scanning for.
          Expanded(
            child: Row(
              children: [
                if (_dirOf(file.path).isNotEmpty)
                  Flexible(
                    child: Text(
                      _dirOf(file.path),
                      style: TextStyle(
                        fontFamily: monoFontFamily,
                        fontFamilyFallback: monoCjkFallback,
                        fontSize: 12.5,
                        color: scheme.onSurfaceVariant,
                      ),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                Text(
                  _baseOf(file.path),
                  style: const TextStyle(
                    fontFamily: monoFontFamily,
                    fontFamilyFallback: monoCjkFallback,
                    fontSize: 12.5,
                  ),
                  maxLines: 1,
                ),
              ],
            ),
          ),
          const SizedBox(width: 8),
          Text(
            '+${file.added}',
            style: TextStyle(fontSize: 11, color: additionColor(scheme)),
          ),
          const SizedBox(width: 4),
          Text(
            '−${file.removed}',
            style: TextStyle(fontSize: 11, color: scheme.error),
          ),
          IconButton(
            tooltip: l10n.copy,
            visualDensity: VisualDensity.compact,
            icon: const Icon(Icons.copy_outlined, size: 15),
            onPressed: () => Clipboard.setData(ClipboardData(text: file.path)),
          ),
        ],
      ),
      children: [_DiffHunks(file: file)],
    );
  }
}

// The hunk-header and language-hint helpers this file used to define now live
// with the diff parser in `git_diff.dart`, shared with the desktop review pane.

/// One file's diff, rendered as a real code view: syntax-highlighted lines with
/// a line-number + add/remove gutter, on tinted rows.
///
/// The height is BOUNDED (its own scroll box) for a reason beyond looks: when
/// this lived as one tall child of the panel's outer `ListView.builder`, that
/// list estimated its total scroll extent from the heights of laid-out
/// children — and a single ~5,000px expanded `Cargo.lock` made the estimate
/// balloon and swing as items entered and left, so dragging the scrollbar thumb
/// (mapped through that estimate) flew across the whole file. Capping the box
/// keeps every outer item a similar size, so the thumb tracks the pointer. A
/// box that fits its content has no scroll extent of its own and lets the wheel
/// fall through to the panel; only a genuinely long diff scrolls internally.
class _DiffHunks extends StatefulWidget {
  const _DiffHunks({required this.file});
  final DiffFile file;

  @override
  State<_DiffHunks> createState() => _DiffHunksState();
}

class _DiffHunksState extends State<_DiffHunks> {
  final ScrollController _v = ScrollController();

  @override
  void dispose() {
    _v.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final l10n = AppLocalizations.of(context);
    final brightness = Theme.of(context).brightness;
    final lang = languageHintForPath(widget.file.path);
    final shown = widget.file.lines.take(_maxDiffLines).toList();

    final rows = <Widget>[];
    int? newNo;
    for (final line in shown) {
      switch (line.kind) {
        case DiffLineKind.hunk:
          newNo = hunkNewStart(line.text);
          rows.add(_hunkRow(scheme, line.text));
        case DiffLineKind.added:
          rows.add(_codeRow(scheme, brightness, lang, line, newNo));
          if (newNo != null) newNo++;
        case DiffLineKind.context:
          rows.add(_codeRow(scheme, brightness, lang, line, newNo));
          if (newNo != null) newNo++;
        case DiffLineKind.removed:
          rows.add(_codeRow(scheme, brightness, lang, line, null));
      }
    }

    // ~18px per single-line row. Fit-to-content for a short diff (no inner
    // scroll, wheel falls through); capped for a long one (scrolls in place).
    final boxH = (shown.length * 18.0 + 6).clamp(0.0, 460.0);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SizedBox(
          height: boxH,
          child: Scrollbar(
            controller: _v,
            thumbVisibility: true,
            child: SingleChildScrollView(
              // Distinct PageStorage identity, per file. Without one each inner
              // scrollable shares the enclosing ExpansionTile's bucket entry
              // with its expanded flag, and restoring the offset reads that
              // bool as a double: "type 'bool' is not a subtype of 'double?'".
              key: PageStorageKey<String>('diff-v-${widget.file.path}'),
              controller: _v,
              child: SingleChildScrollView(
                key: PageStorageKey<String>('diff-h-${widget.file.path}'),
                scrollDirection: Axis.horizontal,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: rows,
                ),
              ),
            ),
          ),
        ),
        if (widget.file.lines.length > _maxDiffLines)
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 8, 12, 10),
            child: Text(
              l10n.diffTruncated(widget.file.lines.length - _maxDiffLines),
              style: TextStyle(fontSize: 11.5, color: scheme.outline),
            ),
          ),
      ],
    );
  }

  static const _mono = TextStyle(
    fontFamily: monoFontFamily,
    fontFamilyFallback: monoCjkFallback,
    fontSize: 12,
    height: 1.35,
  );

  Widget _codeRow(
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
    // The code keeps its syntax colours whatever the row tint — the way a real
    // diff viewer reads. Removed lines dim slightly so the eye lands on adds.
    final base = _mono.copyWith(
      color: line.kind == DiffLineKind.removed
          ? scheme.onSurfaceVariant
          : scheme.onSurface,
    );
    final span = highlightCode(
      code: line.text,
      language: lang,
      base: base,
      brightness: brightness,
    );
    return Container(
      color: bg,
      padding: const EdgeInsets.symmetric(vertical: 1),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 42,
            child: Text(
              lineNo?.toString() ?? '',
              textAlign: TextAlign.right,
              style: _mono.copyWith(fontSize: 11, color: scheme.outline),
            ),
          ),
          SizedBox(
            width: 16,
            child: Text(
              marker,
              textAlign: TextAlign.center,
              style: _mono.copyWith(color: markerColor),
            ),
          ),
          Padding(
            padding: const EdgeInsets.only(right: 12),
            child: Text.rich(span),
          ),
        ],
      ),
    );
  }

  // No explicit width: this sits inside a horizontal scroll view, which offers
  // unbounded width — `double.infinity` there is an error, so the row sizes to
  // its `@@…@@` text.
  Widget _hunkRow(ColorScheme scheme, String text) => Container(
    color: scheme.primary.withValues(alpha: 0.07),
    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 3),
    child: Text(
      text,
      style: _mono.copyWith(fontSize: 11.5, color: scheme.primary),
    ),
  );
}
