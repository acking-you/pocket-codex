import 'package:flutter/material.dart';

import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/realtime_delegation.dart';

/// A Live voice exchange, rendered as its own kind of turn.
///
/// A handoff is not an ordinary typed message: it carries a stretch of spoken
/// back-and-forth that happened in the realtime session before the text model
/// was brought in. Showing it as one grey user bubble (let alone as raw XML)
/// loses who said what, so it gets a card of its own — mic-badged, with the
/// spoken turns laid out as a small conversation.
class RealtimeHandoffCard extends StatelessWidget {
  /// Creates the card for [handoff].
  const RealtimeHandoffCard({super.key, required this.handoff});

  /// The parsed voice handoff.
  final RealtimeHandoff handoff;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    return Container(
      width: double.infinity,
      margin: const EdgeInsets.symmetric(vertical: 4),
      decoration: BoxDecoration(
        color: scheme.surfaceContainerLow,
        borderRadius: BorderRadius.circular(14),
        border: Border.all(color: scheme.outlineVariant, width: 0.5),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(14, 11, 14, 7),
            child: Row(
              children: [
                Icon(Icons.graphic_eq, size: 17, color: scheme.primary),
                const SizedBox(width: 8),
                Text(
                  handoff.isSessionTail
                      ? l10n.voiceSessionEnded
                      : l10n.voiceLive,
                  style: TextStyle(
                    fontSize: 12.5,
                    fontWeight: FontWeight.w500,
                    color: scheme.primary,
                  ),
                ),
              ],
            ),
          ),
          for (final turn in handoff.turns) _turn(context, turn),
          // The triggering turn, unless the transcript already ends with it.
          if (!handoff.inputIsRedundant)
            _turn(context, RealtimeTurn(isUser: true, text: handoff.input)),
          const SizedBox(height: 8),
        ],
      ),
    );
  }

  Widget _turn(BuildContext context, RealtimeTurn turn) {
    final scheme = Theme.of(context).colorScheme;
    final l10n = AppLocalizations.of(context);
    return Padding(
      padding: const EdgeInsets.fromLTRB(14, 3, 14, 3),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // A fixed-width speaker column keeps the utterances left-aligned
          // with each other, which reads as a transcript rather than as chat.
          SizedBox(
            width: 46,
            child: Text(
              turn.isUser ? l10n.voiceYou : l10n.voiceAgent,
              style: TextStyle(
                fontSize: 12,
                height: 1.5,
                fontWeight: FontWeight.w500,
                color: turn.isUser ? scheme.primary : scheme.onSurfaceVariant,
              ),
            ),
          ),
          Expanded(
            child: Text(
              turn.text,
              style: TextStyle(
                fontSize: 13.5,
                height: 1.5,
                color: scheme.onSurface,
              ),
            ),
          ),
        ],
      ),
    );
  }
}
