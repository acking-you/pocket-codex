import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';

/// The app's one failed-and-retryable surface: what went wrong, and a way to
/// try again.
///
/// Seven screens used to hand-roll this same centered card, which is how they
/// drifted — some showed a loading guard, some didn't, none of them told the
/// user that a retry was already happening underneath. One widget means one
/// answer to "what does a failure look like here".
///
/// Automatic retries are shown, not hidden: while the bridge is re-issuing a
/// request the card reports which attempt it is on, so a wait reads as progress
/// rather than a freeze. The [onRetry] button stays available throughout — a
/// user who doesn't want to wait out the backoff shouldn't have to.
class ErrorRetry extends ConsumerWidget {
  /// Creates a failure card.
  const ErrorRetry({
    super.key,
    required this.message,
    required this.onRetry,
    this.errorKey,
    this.busy = false,
    this.title,
  });

  /// What failed, in the user's language where possible.
  final String message;

  /// Try the failed operation again.
  final VoidCallback onRetry;

  /// Key for the detail text, so a screen's own test can assert on it.
  final Key? errorKey;

  /// True while a retry this card triggered is still running — swaps the button
  /// for a spinner so the tap visibly took effect.
  final bool busy;

  /// Optional headline above the detail (e.g. "couldn't connect").
  final String? title;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final scheme = Theme.of(context).colorScheme;
    // A live automatic retry, if one is in flight for a host meta request.
    final retrying = ref.watch(metaRetryProvider).valueOrNull;
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (title != null) ...[
              Text(
                title!,
                textAlign: TextAlign.center,
                style: Theme.of(context).textTheme.titleMedium,
              ),
              const SizedBox(height: 8),
            ],
            Text(
              message,
              key: errorKey,
              textAlign: TextAlign.center,
              style: TextStyle(color: scheme.error),
            ),
            const SizedBox(height: 12),
            if (retrying != null) ...[
              _RetryLine(progress: retrying),
              const SizedBox(height: 12),
            ],
            if (busy)
              const SizedBox(
                width: 20,
                height: 20,
                child: CircularProgressIndicator(strokeWidth: 2),
              )
            else
              FilledButton(
                onPressed: onRetry,
                child: Text(AppLocalizations.of(context).retry),
              ),
          ],
        ),
      ),
    );
  }
}

/// The "retrying n/10" line: a small spinner plus the attempt count.
class _RetryLine extends StatelessWidget {
  const _RetryLine({required this.progress});

  final RetryProgress progress;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Row(
      key: const Key('retry-progress'),
      mainAxisSize: MainAxisSize.min,
      children: [
        SizedBox(
          width: 13,
          height: 13,
          child: CircularProgressIndicator(
            strokeWidth: 2,
            color: scheme.onSurfaceVariant,
          ),
        ),
        const SizedBox(width: 8),
        Text(
          AppLocalizations.of(
            context,
          ).retryingAttempt(progress.attempt, progress.maxAttempts),
          style: TextStyle(fontSize: 12.5, color: scheme.onSurfaceVariant),
        ),
      ],
    );
  }
}

/// The spinner shown while a host request is in flight — and the reason it is a
/// widget rather than a bare `CircularProgressIndicator`.
///
/// Retries happen DURING the load, but the bridge broadcasts progress live and
/// does not replay it. A screen that only mounts [ErrorRetry] in its failure
/// branch therefore has no subscriber while the retries are actually running,
/// so every tick is missed — the user waits out the whole budget with no
/// explanation and then sees a bare error. Watching from the loading state is
/// what makes the progress reachable at all.
class LoadingWithRetry extends ConsumerWidget {
  /// Creates the loading indicator.
  const LoadingWithRetry({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final retrying = ref.watch(metaRetryProvider).valueOrNull;
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const CircularProgressIndicator(),
          if (retrying != null) ...[
            const SizedBox(height: 14),
            _RetryLine(progress: retrying),
          ],
        ],
      ),
    );
  }
}
