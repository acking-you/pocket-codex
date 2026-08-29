import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';

/// Confirms a force takeover and names the holder processes that will be
/// terminated.
class TakeoverDialog extends StatelessWidget {
  /// Creates a takeover confirmation for [holders].
  const TakeoverDialog({
    super.key,
    required this.holders,
    required this.hasTarget,
  });

  /// Processes that the takeover will attempt to terminate.
  final List<Holder> holders;

  /// Whether an app-server is available to receive the resumed session.
  final bool hasTarget;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return AlertDialog(
      key: const Key('takeover-dialog'),
      title: Text(l10n.takeoverTitle),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(l10n.takeoverBody(holders.length)),
          if (holders.isNotEmpty) ...[
            const SizedBox(height: 12),
            Text(
              l10n.takeoverWillTerminate,
              style: Theme.of(context).textTheme.labelMedium,
            ),
            const SizedBox(height: 4),
            ...holders.map(
              (holder) => Padding(
                padding: const EdgeInsets.symmetric(vertical: 2),
                child: Text(
                  l10n.holderRow(holder.name, holder.pid),
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ),
            ),
          ],
          if (!hasTarget) ...[
            const SizedBox(height: 12),
            Text(
              l10n.takeoverNoTarget,
              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                color: Theme.of(context).colorScheme.error,
              ),
            ),
          ],
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(false),
          child: Text(l10n.cancel),
        ),
        FilledButton(
          key: const Key('takeover-confirm'),
          onPressed: hasTarget ? () => Navigator.of(context).pop(true) : null,
          child: Text(l10n.takeoverConfirm),
        ),
      ],
    );
  }
}
