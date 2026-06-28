import 'package:flutter/material.dart';
import 'package:flutter_linkify/flutter_linkify.dart';
import 'package:url_launcher/url_launcher.dart';

import 'package:pocket_codex/l10n/gen/app_localizations.dart';

/// Open [url] in the system browser. Only `http`/`https` are followed — any
/// other scheme (a stray `file:` / `javascript:` / `mailto:` in model or tool
/// output) is ignored so a tapped link can't do anything but browse. Shows a
/// snackbar if the launch fails.
Future<void> openUrl(BuildContext context, String? url) async {
  if (url == null) return;
  final uri = Uri.tryParse(url.trim());
  if (uri == null || (uri.scheme != 'http' && uri.scheme != 'https')) return;
  // Capture context-derived values before the await (context may unmount).
  final messenger = ScaffoldMessenger.maybeOf(context);
  final failMessage = AppLocalizations.of(context).linkOpenFailed;
  var ok = false;
  try {
    ok = await launchUrl(uri, mode: LaunchMode.externalApplication);
  } catch (_) {
    ok = false;
  }
  if (!ok) {
    messenger?.showSnackBar(SnackBar(content: Text(failMessage)));
  }
}

/// The style applied to links: the theme's primary color, underlined, so URLs
/// read as tappable in both markdown and plain text.
TextStyle linkStyleOf(BuildContext context) {
  final primary = Theme.of(context).colorScheme.primary;
  return TextStyle(
    color: primary,
    decoration: TextDecoration.underline,
    decorationColor: primary,
  );
}

// A CJK ideograph or CJK/full-width punctuation glued directly to a bare URL.
// The GFM autolink parser only links a bare URL at a word boundary (space/start
// or ASCII delimiter), so `应用：https://x` or `仓库https://x` won't link. Models
// writing Chinese routinely glue URLs to a full-width colon, so insert a space.
final _cjkBeforeUrl = RegExp(r'([　-〿一-鿿＀-￯])(https?://)');

/// Make bare URLs glued to CJK text/punctuation autolinkable by the markdown
/// parser, without touching markdown links (`](http…`) which have ASCII
/// boundaries. Applied to agent/plan markdown before rendering.
String autolinkifyMarkdown(String data) =>
    data.replaceAllMapped(_cjkBeforeUrl, (m) => '${m[1]} ${m[2]}');

/// `onTapLink` handler for `MarkdownBody`: opens the tapped href in the browser.
/// (`text` and `title` are part of the callback signature but unused here.)
void onTapMarkdownLink(
  BuildContext context,
  String text,
  String? href,
  String title,
) {
  openUrl(context, href);
}

// URLs only — emails / phone numbers / @-tags are left as plain text so a
// `mailto:`/`tel:` never sneaks past [openUrl]'s http(s) guard.
const _linkifiers = [UrlLinkifier()];
const _linkifyOptions = LinkifyOptions(humanize: false, looseUrl: false);

/// Render [text] with any bare URLs highlighted (primary + underline) and
/// tappable (opens in the browser). [selectable] preserves text selection/copy.
/// [style] is the base text style for the non-link runs.
Widget linkifyText(
  BuildContext context,
  String text, {
  TextStyle? style,
  bool selectable = false,
  int? maxLines,
  TextOverflow overflow = TextOverflow.clip,
}) {
  final link = linkStyleOf(context);
  void open(LinkableElement e) => openUrl(context, e.url);
  return selectable
      ? SelectableLinkify(
          text: text,
          style: style,
          linkStyle: link,
          onOpen: open,
          linkifiers: _linkifiers,
          options: _linkifyOptions,
          maxLines: maxLines,
        )
      : Linkify(
          text: text,
          style: style,
          linkStyle: link,
          onOpen: open,
          linkifiers: _linkifiers,
          options: _linkifyOptions,
          maxLines: maxLines,
          overflow: overflow,
        );
}
