import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';

/// Parent/child affordances shared by live and on-disk session inventories.
class SessionTreeRow extends StatelessWidget {
  const SessionTreeRow({
    super.key,
    required this.id,
    required this.depth,
    required this.childCount,
    required this.expanded,
    required this.onToggle,
    required this.child,
    this.childSession = false,
    this.guardian = false,
  });

  final String id;
  final int depth;
  final int childCount;
  final bool expanded;
  final VoidCallback onToggle;
  final Widget child;
  final bool childSession;
  final bool guardian;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    if (childCount == 0 && !childSession && !guardian) return child;
    return Padding(
      key: ValueKey('session-tree-$id'),
      padding: EdgeInsets.only(left: math.min(depth, 4) * 12.0),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (childCount > 0)
            IconButton(
              key: ValueKey('session-children-$id'),
              tooltip: l10n.childSessions(childCount),
              isSelected: expanded,
              onPressed: onToggle,
              icon: Icon(expanded ? Icons.expand_more : Icons.chevron_right),
            )
          else
            const Padding(
              padding: EdgeInsets.fromLTRB(8, 12, 4, 0),
              child: Icon(Icons.subdirectory_arrow_right, size: 16),
            ),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                if (guardian || childSession)
                  Padding(
                    padding: const EdgeInsets.only(left: 12, top: 4),
                    child: Text(
                      guardian ? l10n.guardianSession : l10n.childSession,
                      style: Theme.of(context).textTheme.labelSmall,
                    ),
                  ),
                child,
              ],
            ),
          ),
        ],
      ),
    );
  }
}
