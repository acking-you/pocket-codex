import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/code_highlight.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/widgets/app_toast.dart';

/// A framed, syntax-highlighted block of code with its language and a copy
/// button: one look for a fenced block in a reply, a tool call's command or
/// JSON in a step, and a file opened from the host.
///
/// Lines never wrap — code read sideways is code; wrapped, it is a different
/// program — so the body scrolls horizontally. [maxHeight] bounds a long body
/// with its own vertical scroll; null lays it all out (the caller scrolls).
class CodeBlock extends StatelessWidget {
  /// Shows [code] highlighted as [language] (a fence info string, a file
  /// extension, or empty for plain text).
  const CodeBlock({
    super.key,
    required this.code,
    this.language = '',
    this.label,
    this.maxHeight,
    this.fontSize = 12.5,
    this.margin = const EdgeInsets.symmetric(vertical: 6),
    this.trailing,
  });

  /// The source, shown as-is.
  final String code;

  /// Grammar to highlight with; unknown or empty renders plain.
  final String language;

  /// Header text; defaults to [language], or "text".
  final String? label;

  /// Bounds the body, which then scrolls vertically.
  final double? maxHeight;

  /// Body text size.
  final double fontSize;

  /// Space around the frame.
  final EdgeInsetsGeometry margin;

  /// Extra header controls, before the copy button.
  final List<Widget>? trailing;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final mono = TextStyle(
      fontFamily: monoFontFamily,
      fontFamilyFallback: monoCjkFallback,
      fontSize: fontSize,
      height: 1.5,
      color: scheme.onSurface,
    );
    final body = SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      padding: const EdgeInsets.fromLTRB(12, 10, 12, 12),
      child: Text.rich(
        highlightCode(
          code: code,
          language: language,
          base: mono,
          brightness: Theme.of(context).brightness,
          // Upright: italic comments over a CJK fallback look distorted.
          allowItalic: false,
        ),
      ),
    );
    return Container(
      width: double.infinity,
      margin: margin,
      decoration: BoxDecoration(
        color: scheme.surfaceContainerLow,
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: scheme.outlineVariant),
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const SizedBox(width: 12),
              Expanded(
                child: Text(
                  label ?? (language.isEmpty ? 'text' : language),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontFamily: monoFontFamily,
                    fontFamilyFallback: monoCjkFallback,
                    fontSize: 11,
                    color: scheme.onSurfaceVariant,
                  ),
                ),
              ),
              ...?trailing,
              CopyTextButton(text: code),
            ],
          ),
          Container(
            width: double.infinity,
            decoration: BoxDecoration(
              border: Border(top: BorderSide(color: scheme.outlineVariant)),
            ),
            child: maxHeight == null
                ? body
                : ConstrainedBox(
                    constraints: BoxConstraints(maxHeight: maxHeight!),
                    child: Scrollbar(
                      child: SingleChildScrollView(primary: false, child: body),
                    ),
                  ),
          ),
        ],
      ),
    );
  }
}

/// Copies [text] and confirms it, without stealing focus.
class CopyTextButton extends StatelessWidget {
  /// Copies [text] when pressed.
  const CopyTextButton({super.key, required this.text});

  /// What to copy.
  final String text;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return IconButton(
      icon: const Icon(Icons.content_copy_outlined, size: 14),
      iconSize: 14,
      visualDensity: VisualDensity.compact,
      tooltip: l10n.copy,
      color: Theme.of(context).colorScheme.onSurfaceVariant,
      onPressed: () {
        Clipboard.setData(ClipboardData(text: text));
        showToastOk(context, l10n.copied);
      },
    );
  }
}
