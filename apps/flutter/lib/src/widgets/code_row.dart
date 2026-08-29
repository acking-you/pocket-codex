import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/widgets/app_toast.dart';

/// A machine-facing value — a relay key, a path, an address — shown as
/// selectable mono text with a copy button.
///
/// These values exist to be pasted somewhere else, so every one of them wants
/// the same three things: a mono face (so a path's separators and an `l`/`1` are
/// unambiguous), selectability, and one tap to the clipboard. Each site used to
/// spell that out again, which is how they drifted apart on icon size and inset.
class CodeRow extends StatelessWidget {
  /// Creates a copyable code row for [value].
  const CodeRow({
    super.key,
    required this.value,
    this.label,
    this.filled = true,
    this.copyKey,
  });

  /// The value itself, copied verbatim.
  final String value;

  /// Optional leading label, for a row that needs saying what the value IS.
  /// Omit it where the surrounding context already answers that.
  final String? label;

  /// Whether to sit on a tinted ground. False for a row already inside a panel
  /// that provides its own, where a second fill would read as a box in a box.
  final bool filled;

  /// Identifies the copy button, for a test that reaches for a specific row's.
  final Key? copyKey;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    return Container(
      padding: filled
          ? const EdgeInsets.only(left: 12, right: 2)
          : EdgeInsets.zero,
      decoration: filled
          ? BoxDecoration(
              color: scheme.surfaceContainerLow,
              borderRadius: BorderRadius.circular(kControlRadius),
            )
          : null,
      child: Row(
        children: [
          if (label != null) ...[
            Text(
              label!,
              style: theme.textTheme.bodySmall?.copyWith(
                color: onSurfaceMuted(scheme),
              ),
            ),
            const SizedBox(width: 10),
          ],
          Expanded(
            child: SelectableText(
              value,
              maxLines: 1,
              style: TextStyle(
                fontFamily: monoFontFamily,
                fontFamilyFallback: monoCjkFallback,
                fontSize: 12,
                color: scheme.onSurfaceVariant,
              ),
            ),
          ),
          IconButton(
            key: copyKey,
            tooltip: l10n.copy,
            icon: const Icon(Icons.copy, size: 15),
            visualDensity: VisualDensity.compact,
            onPressed: () async {
              final messenger = ToastMessenger.of(context);
              await Clipboard.setData(ClipboardData(text: value));
              messenger.ok(l10n.copied);
            },
          ),
        ],
      ),
    );
  }
}
