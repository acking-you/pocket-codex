import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/code_highlight.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/git_diff.dart';
import 'package:pocket_codex/src/motion.dart';
import 'package:pocket_codex/src/widgets/code_block.dart';
import 'package:pocket_codex/src/widgets/markdown_view.dart';

/// Extensions shown as a picture rather than as bytes.
const previewImageExtensions = {'png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'};

/// Extensions with no useful text form; the preview offers download instead.
const _binaryExtensions = {'pdf', 'doc', 'docx', 'xlsx', 'pptx', 'zip'};

/// Rows of a delimited table rendered before the rest is left to the source
/// view; a table is for glancing, and every cell is a widget.
const _tableRowLimit = 300;

/// How a file's content is shown when it can be more than its text.
enum PreviewKind {
  /// A picture.
  image,

  /// Vector art, drawn.
  svg,

  /// Markdown, rendered.
  markdown,

  /// JSON, pretty-printed.
  json,

  /// Comma- or tab-separated values, as a table.
  table,

  /// Text with nothing to render: highlighted source only.
  source,

  /// Not text; nothing to show.
  unsupported,
}

/// The way to show [name]'s [bytes] (of which [truncated] says whether
/// there are more on the host).
PreviewKind previewKindFor(
  String name,
  Uint8List bytes, {
  required bool truncated,
}) {
  final ext = _extension(name);
  if (previewImageExtensions.contains(ext)) {
    // A partial picture does not decode.
    return truncated ? PreviewKind.unsupported : PreviewKind.image;
  }
  if (_binaryExtensions.contains(ext) || bytes.take(8192).contains(0)) {
    return PreviewKind.unsupported;
  }
  return switch (ext) {
    'svg' when !truncated => PreviewKind.svg,
    'md' || 'markdown' || 'mdx' => PreviewKind.markdown,
    'json' when !truncated => PreviewKind.json,
    'csv' || 'tsv' => PreviewKind.table,
    _ => PreviewKind.source,
  };
}

String _extension(String name) {
  final dot = name.lastIndexOf('.');
  return dot < 0 ? '' : name.substring(dot + 1).toLowerCase();
}

/// Parses delimited text (RFC 4180 quoting) into rows, stopping after
/// [limit] rows.
List<List<String>> parseDelimited(
  String text, {
  String separator = ',',
  int limit = _tableRowLimit,
}) {
  final rows = <List<String>>[];
  var row = <String>[];
  final cell = StringBuffer();
  var quoted = false;
  for (var i = 0; i < text.length && rows.length < limit; i++) {
    final c = text[i];
    if (quoted) {
      if (c == '"') {
        if (i + 1 < text.length && text[i + 1] == '"') {
          cell.write('"');
          i++;
        } else {
          quoted = false;
        }
      } else {
        cell.write(c);
      }
    } else if (c == '"' && cell.isEmpty) {
      quoted = true;
    } else if (c == separator) {
      row.add(cell.toString());
      cell.clear();
    } else if (c == '\n' || c == '\r') {
      if (c == '\r' && i + 1 < text.length && text[i + 1] == '\n') i++;
      row.add(cell.toString());
      cell.clear();
      rows.add(row);
      row = <String>[];
    } else {
      cell.write(c);
    }
  }
  if (rows.length < limit && (cell.isNotEmpty || row.isNotEmpty)) {
    row.add(cell.toString());
    rows.add(row);
  }
  return rows;
}

/// A file opened from a transcript link: rendered when its type has a
/// rendered form (Markdown, JSON, a table, an image, an SVG), with a switch
/// to its highlighted source and a copy button. All text stays selectable.
class FilePreview extends StatefulWidget {
  /// Previews [bytes] of the file called [name].
  const FilePreview({
    super.key,
    required this.name,
    required this.bytes,
    required this.text,
    required this.truncated,
  });

  /// File name, for its extension.
  final String name;

  /// The bytes read.
  final Uint8List bytes;

  /// [bytes] decoded as text, or null when they are not text.
  final String? text;

  /// Only the start of the file was read.
  final bool truncated;

  @override
  State<FilePreview> createState() => _FilePreviewState();
}

class _FilePreviewState extends State<FilePreview> {
  bool _source = false;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final kind = previewKindFor(
      widget.name,
      widget.bytes,
      truncated: widget.truncated,
    );
    final text = widget.text;
    if (kind == PreviewKind.image) {
      return InteractiveViewer(
        child: Image.memory(
          widget.bytes,
          fit: BoxFit.contain,
          errorBuilder: (_, _, _) => Text(l10n.fileLinkUnsupported),
        ),
      );
    }
    if (kind == PreviewKind.unsupported || text == null) {
      return Text(l10n.fileLinkUnsupported);
    }
    final renderable = kind != PreviewKind.source;
    final showSource = _source || !renderable;
    final body = AnimatedSwitcher(
      duration: Motion.of(context, Motion.fast),
      child: KeyedSubtree(
        key: ValueKey(showSource),
        child: showSource
            ? _SourceView(
                key: const Key('file-preview-source'),
                text: text,
                language: languageHintForPath(widget.name),
              )
            : _rendered(context, kind, text),
      ),
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            if (renderable)
              Flexible(
                child: SegmentedButton<bool>(
                  key: const Key('file-preview-mode'),
                  showSelectedIcon: false,
                  style: const ButtonStyle(
                    visualDensity: VisualDensity.compact,
                    tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                  ),
                  segments: [
                    ButtonSegment(
                      value: false,
                      label: Text(l10n.previewRendered),
                    ),
                    ButtonSegment(value: true, label: Text(l10n.previewSource)),
                  ],
                  selected: {_source},
                  onSelectionChanged: (s) => setState(() => _source = s.first),
                ),
              ),
            const Spacer(),
            CopyTextButton(text: text),
          ],
        ),
        if (widget.truncated)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: Text(
              l10n.fileLinkTruncated,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(context).textTheme.bodySmall,
            ),
          ),
        const SizedBox(height: 8),
        Expanded(child: SelectionArea(child: body)),
      ],
    );
  }

  Widget _rendered(BuildContext context, PreviewKind kind, String text) {
    final l10n = AppLocalizations.of(context);
    return switch (kind) {
      PreviewKind.markdown => SingleChildScrollView(
        key: const Key('file-preview-markdown'),
        child: MarkdownView(data: text),
      ),
      PreviewKind.json => _SourceView(
        key: const Key('file-preview-json'),
        text: _prettyJson(text) ?? text,
        language: 'json',
      ),
      PreviewKind.table => _DelimitedTable(
        key: const Key('file-preview-table'),
        rows: parseDelimited(
          text,
          separator: _extension(widget.name) == 'tsv' ? '\t' : ',',
        ),
      ),
      PreviewKind.svg => InteractiveViewer(
        key: const Key('file-preview-svg'),
        child: SvgPicture.memory(
          widget.bytes,
          fit: BoxFit.contain,
          errorBuilder: (_, _, _) =>
              Center(child: Text(l10n.fileLinkUnsupported)),
        ),
      ),
      _ => const SizedBox.shrink(),
    };
  }
}

String? _prettyJson(String text) {
  try {
    return const JsonEncoder.withIndent('  ').convert(jsonDecode(text));
  } catch (_) {
    return null;
  }
}

/// Highlighted source that stays responsive for a large file: highlighted in
/// one piece up to the highlighter's budget, and laid out line by line past
/// it (where highlighting would be skipped anyway).
class _SourceView extends StatelessWidget {
  const _SourceView({super.key, required this.text, required this.language});
  final String text;
  final String language;

  static const _wholeLimit = 20000;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final mono = TextStyle(
      fontFamily: monoFontFamily,
      fontFamilyFallback: monoCjkFallback,
      fontSize: 12.5,
      height: 1.5,
      color: scheme.onSurface,
    );
    final Widget body;
    if (text.length <= _wholeLimit) {
      body = SingleChildScrollView(
        padding: const EdgeInsets.all(12),
        child: SingleChildScrollView(
          scrollDirection: Axis.horizontal,
          child: Text.rich(
            highlightCode(
              code: text,
              language: language,
              base: mono,
              brightness: Theme.of(context).brightness,
              allowItalic: false,
            ),
          ),
        ),
      );
    } else {
      final lines = text.split('\n');
      body = ListView.builder(
        padding: const EdgeInsets.all(12),
        itemCount: lines.length,
        itemBuilder: (context, index) => Text(lines[index], style: mono),
      );
    }
    return DecoratedBox(
      decoration: BoxDecoration(
        color: scheme.surfaceContainerLow,
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: scheme.outlineVariant),
      ),
      child: ClipRRect(borderRadius: BorderRadius.circular(8), child: body),
    );
  }
}

/// A delimited file as a grid, the first row as its header.
class _DelimitedTable extends StatelessWidget {
  const _DelimitedTable({super.key, required this.rows});
  final List<List<String>> rows;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    if (rows.isEmpty) return const SizedBox.shrink();
    final columns = rows.fold<int>(0, (m, r) => r.length > m ? r.length : m);
    final base = Theme.of(context).textTheme.bodySmall;
    TableRow row(List<String> cells, {bool header = false}) => TableRow(
      decoration: BoxDecoration(
        color: header ? scheme.surfaceContainerHigh : null,
        border: Border(bottom: BorderSide(color: scheme.outlineVariant)),
      ),
      children: [
        for (var i = 0; i < columns; i++)
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
            child: Text(
              i < cells.length ? cells[i] : '',
              style: header
                  ? base?.copyWith(fontWeight: FontWeight.w600)
                  : base,
            ),
          ),
      ],
    );
    return DecoratedBox(
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: scheme.outlineVariant),
      ),
      child: ClipRRect(
        borderRadius: BorderRadius.circular(8),
        child: SingleChildScrollView(
          child: SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Table(
              defaultColumnWidth: const IntrinsicColumnWidth(),
              children: [
                row(rows.first, header: true),
                for (final r in rows.skip(1)) row(r),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
