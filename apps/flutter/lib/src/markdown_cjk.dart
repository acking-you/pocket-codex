/// CommonMark's emphasis rules assume space-delimited scripts, so `**bold**`
/// silently fails to render in Chinese prose whenever a delimiter sits between
/// a CJK glyph and full-width punctuation — the two configurations below.
/// Models write both constantly (`**重点是印度：**苹果…`), and the reader sees
/// literal asterisks instead of bold text.
///
/// The rule at fault is flanking (CommonMark §6.2): a `**` run may close only
/// if it is *right-flanking*, which — when the preceding character is
/// punctuation — additionally requires the following character to be
/// whitespace or punctuation. A Han glyph is neither, so the run can't close.
/// The mirror case blocks opening. ASCII prose never trips this because a
/// space always separates the word from the delimiter.
library;

import 'package:markdown/markdown.dart' as md;

// Han, kana and hangul: the letters that sit flush against a delimiter.
const _letter = r'぀-ヿ㐀-䶿一-鿿가-힯豈-﫿';

// ASCII punctuation plus the CJK / full-width punctuation that shows up in
// Chinese prose (：，。、！？；「」《》【】—…). Two deliberate exclusions:
// U+3000 IDEOGRAPHIC SPACE (it is whitespace, which CommonMark handles), and
// `*` itself — the ASCII run is split around it. An asterisk counting as
// punctuation would let a delimiter satisfy the "next to punctuation"
// condition, and `文字***强调***文字` would be claimed here and rendered as
// `<strong>*强调</strong>*` instead of the nested em+strong the stock
// delimiter machinery produces.
const _punct =
    r'!-)+-\/:-@\[-`\{-~'
    r'‐-‧‰-⁞'
    r'、-〿︐-︙︰-﹫'
    r'！-＠［-｀｛-･';

// Emphasis body: anything but a newline or a second asterisk, so a match can
// never swallow an adjacent delimiter run or span a block boundary.
const _body = r'(?:[^*\n]|\*(?!\*))';

// Branch 1 — closing `**` preceded by punctuation, followed by a CJK letter.
// Branch 2 — opening `**` preceded by a CJK letter, followed by punctuation.
const _pattern =
    r'\*\*(?![\s*])('
    '$_body*?[$_punct]'
    r')\*\*(?=['
    '$_letter'
    r'])'
    r'|'
    r'(?<=['
    '$_letter'
    r'])\*\*(?=['
    '$_punct'
    r'])('
    '$_body+?'
    r')\*\*';

/// Renders `**…**` as strong emphasis in the two CJK configurations where
/// CommonMark's flanking rules refuse to.
///
/// Registered ahead of the built-in syntaxes, so it only ever sees text the
/// stock parser has not already claimed: inline code and links are matched at
/// their own opening character earlier in the scan, and fenced code never
/// reaches the inline parser at all. Everything the standard rules already
/// handle — `这是**加粗**紧贴中文`, `***粗斜体***`, escaped `\*\*` — is left to
/// them, because neither branch matches it.
class CjkStrongSyntax extends md.InlineSyntax {
  /// Creates the syntax. `startCharacter` is `*`, a cheap pre-filter that skips
  /// the regex at every non-delimiter position.
  CjkStrongSyntax() : super(_pattern, startCharacter: 0x2a);

  @override
  bool onMatch(md.InlineParser parser, Match match) {
    final content = match[1] ?? match[2];
    if (content == null || content.isEmpty) {
      // Unreachable: both branches capture at least one character. Consume the
      // match as plain text anyway — `tryMatch` reports success whatever this
      // returns, and returning false skips the consume, so the parser would
      // sit on this position forever instead of falling through.
      parser.addNode(md.Text(match[0] ?? ''));
      return true;
    }
    // Re-parse the body so nested inline syntax (links, code, emphasis)
    // survives. The body excludes `**`, so this cannot recurse into itself.
    parser.addNode(
      md.Element('strong', md.InlineParser(content, parser.document).parse()),
    );
    return true;
  }
}

/// The extension set every `MarkdownBody` in the app renders with: GitHub
/// flavour (tables, strikethrough, fenced code) plus [CjkStrongSyntax].
///
/// `flutter_markdown_plus` defaults to `ExtensionSet.gitHubFlavored`; this
/// keeps that behaviour and only prepends the extra syntax.
final md.ExtensionSet markdownExtensionSet = md.ExtensionSet(
  md.ExtensionSet.gitHubFlavored.blockSyntaxes,
  <md.InlineSyntax>[
    CjkStrongSyntax(),
    ...md.ExtensionSet.gitHubFlavored.inlineSyntaxes,
  ],
);
