import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:markdown/markdown.dart' as md;

import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/markdown_cjk.dart';
import 'package:pocket_codex/src/widgets/links.dart';

/// One Markdown renderer for every agent-authored surface (replies, plan
/// proposals, plan explanations), so they share the CJK fixes, the table
/// treatment and the link presentation.
///
/// Selection is deliberately not enabled here: both transcripts wrap their
/// list in a `SelectionArea`, and a nested `SelectableText` opts itself *out*
/// of that unified selection instead of joining it.
class MarkdownView extends StatefulWidget {
  /// Renders [data] as Markdown.
  const MarkdownView({super.key, required this.data});

  /// Raw Markdown source, as written by the model.
  final String data;

  @override
  State<MarkdownView> createState() => _MarkdownViewState();
}

class _MarkdownViewState extends State<MarkdownView> {
  // Stateful only to own the link builder's gesture recognizers: a streaming
  // reply rebuilds this widget on every token, and a recognizer allocated per
  // link per build is never disposed by anyone.
  late final _LinkBuilder _links = _LinkBuilder();

  @override
  void dispose() {
    _links.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => MarkdownBody(
    data: autolinkifyMarkdown(widget.data),
    selectable: false,
    extensionSet: markdownExtensionSet,
    styleSheet: markdownStyle(context),
    builders: <String, MarkdownElementBuilder>{'a': _links},
    // Unreachable while the 'a' builder above owns links, but MarkdownBody
    // also routes image taps here — keep the handler wired.
    onTapLink: (text, href, title) =>
        onTapMarkdownLink(context, text, href, title),
  );
}

// Last stylesheet handed out, keyed by the theme it came from. Building one
// costs ~40 TextStyles, and every message in the transcript asks for it on
// every frame of a streaming turn; the theme only changes on a light/dark or
// platform switch, so a one-entry memo removes essentially all of that work.
ThemeData? _styleTheme;
MarkdownStyleSheet? _styleCache;

/// Theme-derived Markdown styling: comfortable line height, tinted code
/// blocks, and tables that read as rows of data rather than a spreadsheet
/// grid — no vertical rules, a divider between rows, left-aligned headers.
MarkdownStyleSheet markdownStyle(BuildContext context) {
  final cached = _styleCache;
  if (cached != null && identical(_styleTheme, Theme.of(context))) {
    return cached;
  }
  final built = _buildMarkdownStyle(context);
  _styleTheme = Theme.of(context);
  _styleCache = built;
  return built;
}

MarkdownStyleSheet _buildMarkdownStyle(BuildContext context) {
  final theme = Theme.of(context);
  final scheme = theme.colorScheme;
  final body = theme.textTheme.bodyLarge?.copyWith(height: 1.65);
  // CJK text needs less weight than Latin to read as emphasised: at w700 a
  // bold Han glyph turns into a dark blob at body size.
  final strongWeight = isDesktop ? FontWeight.w600 : FontWeight.w700;
  final divider = scheme.outlineVariant;
  return MarkdownStyleSheet.fromTheme(theme).copyWith(
    p: body,
    listBullet: body,
    // Weight only: this style is *merged onto* the enclosing one, so carrying a
    // size or height here would blow up bold text inside a table cell or a
    // heading to body size.
    strong: TextStyle(fontWeight: strongWeight),
    a: linkStyleOf(context),
    pPadding: const EdgeInsets.only(bottom: 8),
    h1Padding: const EdgeInsets.only(top: 10, bottom: 4),
    h2Padding: const EdgeInsets.only(top: 10, bottom: 4),
    h3Padding: const EdgeInsets.only(top: 8, bottom: 4),
    listBulletPadding: const EdgeInsets.only(right: 6),
    blockSpacing: 10,
    // `IntrinsicColumnWidth` is load-bearing, not cosmetic: flutter_markdown_plus
    // only wraps a table in a horizontal scroll view for intrinsic/fixed column
    // widths. With the default `FlexColumnWidth` a wide table is squeezed into
    // the viewport and its last columns are clipped off-screen.
    tableColumnWidth: const IntrinsicColumnWidth(),
    tableBorder: TableBorder(
      horizontalInside: BorderSide(color: divider, width: 1),
      bottom: BorderSide(color: divider, width: 1),
    ),
    tableHead: theme.textTheme.bodyMedium?.copyWith(
      fontWeight: strongWeight,
      height: 1.4,
    ),
    tableBody: theme.textTheme.bodyMedium?.copyWith(height: 1.4),
    tableHeadAlign: TextAlign.left,
    tableCellsPadding: const EdgeInsets.fromLTRB(0, 10, 28, 10),
    tablePadding: const EdgeInsets.only(bottom: 10),
    code: theme.textTheme.bodyMedium?.copyWith(
      fontFamily: 'monospace',
      fontFamilyFallback: monoCjkFallback,
      backgroundColor: scheme.surfaceContainerHighest,
    ),
    codeblockDecoration: BoxDecoration(
      color: scheme.surfaceContainerHighest,
      borderRadius: BorderRadius.circular(10),
    ),
    codeblockPadding: const EdgeInsets.all(14),
    blockquoteDecoration: BoxDecoration(
      color: scheme.surfaceContainerHighest,
      borderRadius: BorderRadius.circular(8),
      border: Border(left: BorderSide(color: scheme.primary, width: 3)),
    ),
  );
}

/// Renders `<a>` as `favicon + underlined text`, tappable.
///
/// Registering a builder for `a` takes link handling away from MarkdownBody
/// (see its `builders.containsKey('a')` checks), so the tap recognizer is ours
/// to attach. Returning a `Text.rich` — rather than a bare widget — keeps the
/// link inline: the builder merges neighbouring text widgets into one span, so
/// the link still wraps mid-paragraph like ordinary text.
class _LinkBuilder extends MarkdownElementBuilder {
  // One recognizer per destination, reused across rebuilds. Allocating per
  // build would leak one per link per streamed token; the pool is bounded by
  // the number of distinct links in a single message.
  final Map<String, TapGestureRecognizer> _taps = {};

  /// Releases every pooled recognizer. Call from the owning State's `dispose`.
  void dispose() {
    for (final r in _taps.values) {
      r.dispose();
    }
    _taps.clear();
  }

  @override
  Widget? visitElementAfterWithContext(
    BuildContext context,
    md.Element element,
    TextStyle? preferredStyle,
    TextStyle? parentStyle,
  ) {
    final href = element.attributes['href'];
    final style = (parentStyle ?? const TextStyle()).merge(preferredStyle);
    final host = _faviconHost(href);
    final recognizer = _taps.putIfAbsent(href ?? '', TapGestureRecognizer.new)
      // Re-point every build: `context` is fresh each time, and the old
      // closure would hold the previous element alive.
      ..onTap = () => openUrl(context, href);
    return Text.rich(
      TextSpan(
        children: <InlineSpan>[
          if (host != null)
            WidgetSpan(
              alignment: PlaceholderAlignment.middle,
              child: Padding(
                padding: const EdgeInsets.only(right: 4),
                child: _Favicon(host: host),
              ),
            ),
          ..._linkLabel(element.children, style, recognizer),
        ],
      ),
    );
  }
}

/// Link label as a FLAT list of leaf spans. Building the child nodes ourselves
/// (instead of flattening to `textContent`) is what keeps `[**粗体**说明](url)`
/// bold — replacing the `a` builder discards the spans MarkdownBody had already
/// built for it. Flat is not a style choice: MarkdownBuilder re-creates every
/// child span as `TextSpan(text: …)` when it merges inline widgets, dropping
/// any `children`, so a nested span would render as nothing at all.
List<InlineSpan> _linkLabel(
  List<md.Node>? nodes,
  TextStyle style,
  GestureRecognizer recognizer,
) => [
  for (final node in nodes ?? const <md.Node>[])
    if (node is md.Element)
      ..._linkLabel(node.children, switch (node.tag) {
        'strong' => style.copyWith(fontWeight: FontWeight.w600),
        'em' => style.copyWith(fontStyle: FontStyle.italic),
        'del' => style.copyWith(decoration: TextDecoration.lineThrough),
        _ => style,
      }, recognizer)
    else
      TextSpan(text: node.textContent, style: style, recognizer: recognizer),
];

/// Host to fetch a favicon from, or null when the link isn't a plain web URL
/// (relative paths, `mailto:`, the `#anchor` links a model writes for its own
/// headings — none of which have a site to represent).
String? _faviconHost(String? href) {
  final uri = Uri.tryParse(href?.trim() ?? '');
  if (uri == null || !uri.hasAuthority) return null;
  if (uri.scheme != 'http' && uri.scheme != 'https') return null;
  return uri.host.isEmpty ? null : uri.host;
}

/// When a host's favicon fetch last failed. Flutter's image cache does not
/// remember failures, so without this a site that 404s its `/favicon.ico` is
/// re-requested on every rebuild — and transcripts rebuild constantly while a
/// turn streams. Entries expire because the app runs on phones and flaky
/// networks: one bad moment must not strand a host's icon for the session.
final Map<String, DateTime> _faviconMisses = {};
const _faviconRetryAfter = Duration(minutes: 2);

/// A site's favicon, sized to sit on a text line.
///
/// Fetched straight from the site (`https://<host>/favicon.ico`) rather than
/// through a favicon proxy: no third party learns which links the user reads,
/// and nothing depends on a service that may be unreachable. The generic globe
/// shows immediately and stays put if the fetch is slow or fails, so a dead or
/// blocked host never holds up the text.
class _Favicon extends StatelessWidget {
  const _Favicon({required this.host});

  final String host;

  static const double _size = 15;

  @override
  Widget build(BuildContext context) {
    final fallback = Icon(
      Icons.public,
      size: _size,
      color: Theme.of(context).colorScheme.onSurfaceVariant,
    );
    final missedAt = _faviconMisses[host];
    if (missedAt != null) {
      if (DateTime.now().difference(missedAt) < _faviconRetryAfter) {
        return fallback;
      }
      _faviconMisses.remove(host);
    }
    return SizedBox(
      width: _size,
      height: _size,
      child: Image.network(
        'https://$host/favicon.ico',
        width: _size,
        height: _size,
        fit: BoxFit.contain,
        // Sites routinely serve a 256 px (or larger) .ico. Without a decode
        // bound each one is held in the image cache at full size for a 15 px
        // slot — a transcript citing a dozen hosts would pin several MB.
        cacheWidth: (_size * MediaQuery.devicePixelRatioOf(context)).round(),
        errorBuilder: (context, error, stack) {
          _faviconMisses[host] = DateTime.now();
          return fallback;
        },
        frameBuilder: (context, child, frame, wasSynchronouslyLoaded) =>
            frame == null && !wasSynchronouslyLoaded ? fallback : child,
      ),
    );
  }
}
