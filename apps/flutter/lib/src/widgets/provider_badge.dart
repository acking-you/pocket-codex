import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/service_key.dart';

/// A compact pill naming the service provider of a host (Codex or
/// an ACP agent), shown next to host names so providers on one device stay
/// apart. ACP agents show their own name plus an "ACP" tag, so the protocol
/// is visible without guessing it from the name.
class ProviderBadge extends StatelessWidget {
  /// A badge for [provider] (`codex` or `acp`); [name] labels an
  /// ACP agent.
  const ProviderBadge({super.key, required this.provider, this.name});

  /// A badge for the provider behind the relay service [key].
  factory ProviderBadge.forKey(String key, {Key? badgeKey, String? name}) =>
      ProviderBadge(
        key: badgeKey,
        provider: switch (sessionProtocolOf(key)) {
          SessionProtocol.acp => 'acp',
          SessionProtocol.codexAppServer => 'codex',
        },
        name: name,
      );

  /// `codex` or `acp`.
  final String provider;

  /// Agent name for ACP badges (empty or null falls back to "ACP agent").
  final String? name;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final native = provider == 'codex';
    final acp = provider == 'acp';
    final bg = native ? scheme.secondaryContainer : scheme.tertiaryContainer;
    final fg = native
        ? scheme.onSecondaryContainer
        : scheme.onTertiaryContainer;
    final agentName = name?.trim() ?? '';
    final label = switch (provider) {
      'acp' => agentName.isNotEmpty ? agentName : l10n.providerAcpAgent,
      _ => l10n.providerCodex,
    };
    final style = TextStyle(
      fontSize: 10.5,
      height: 1.3,
      color: fg,
      fontWeight: FontWeight.w600,
      letterSpacing: 0.2,
    );
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1.5),
      decoration: BoxDecoration(
        color: bg,
        borderRadius: BorderRadius.circular(999),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Flexible(
            child: Text(
              label,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: style,
            ),
          ),
          if (acp) ...[
            const SizedBox(width: 4),
            Container(
              key: const Key('provider-badge-acp-tag'),
              padding: const EdgeInsets.symmetric(horizontal: 3),
              decoration: BoxDecoration(
                border: Border.all(
                  color: fg.withValues(alpha: 0.6),
                  width: 0.8,
                ),
                borderRadius: BorderRadius.circular(4),
              ),
              child: Text(
                l10n.protocolAcpTag,
                style: style.copyWith(fontSize: 9, letterSpacing: 0.4),
              ),
            ),
          ],
        ],
      ),
    );
  }
}
