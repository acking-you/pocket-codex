import 'package:flutter/material.dart';

/// A titled card whose rows are separated by hairlines — the app's standard way
/// to group related settings or capabilities.
///
/// Settings, the services hub and the Codex wizard each grew their own copy of
/// this, which is how they came to disagree on header height and inset for what
/// reads as one component. [hint] adds a line under the title for a group that
/// needs framing; [trailing] puts a count or status pill on the header row.
class GroupCard extends StatelessWidget {
  /// Creates a titled group of [children].
  const GroupCard({
    super.key,
    required this.title,
    required this.children,
    this.hint,
    this.trailing,
  });

  /// The group's name.
  final String title;

  /// A sentence of framing under the title, when the name alone is not enough.
  final String? hint;

  /// Header-row trailing widget — a count pill, a status chip.
  final Widget? trailing;

  /// The rows, each preceded by a hairline so the header reads as their parent.
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Card(
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Container(
            // A minimum rather than a fixed height, so a wrapped hint grows the
            // header instead of being clipped by it.
            constraints: const BoxConstraints(minHeight: 46),
            padding: const EdgeInsets.fromLTRB(14, 10, 14, 10),
            child: Row(
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      Text(
                        title,
                        style: const TextStyle(fontWeight: FontWeight.w600),
                      ),
                      if (hint != null) ...[
                        const SizedBox(height: 2),
                        Text(
                          hint!,
                          style: Theme.of(context).textTheme.bodySmall
                              ?.copyWith(color: scheme.onSurfaceVariant),
                        ),
                      ],
                    ],
                  ),
                ),
                ?trailing,
              ],
            ),
          ),
          for (final child in children) ...[
            Divider(height: 1, color: scheme.outlineVariant),
            child,
          ],
        ],
      ),
    );
  }
}
