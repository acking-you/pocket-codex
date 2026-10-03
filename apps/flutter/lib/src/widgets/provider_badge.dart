import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/service_key.dart';

/// A compact pill naming the service provider (Codex / OpenCode / an ACP
/// agent) of a host, shown next to host names so providers stay apart.
class ProviderBadge extends StatelessWidget {
  /// A badge for [provider] (`codex`, `opencode` or `acp`); ACP badges show
  /// [label] (the agent name) when given.
  const ProviderBadge({super.key, required this.provider, this.label});

  /// A badge for the provider behind the relay service [key].
  factory ProviderBadge.forKey(String key, {Key? badgeKey, String? label}) =>
      ProviderBadge(
        key: badgeKey,
        provider: isAcpKey(key)
            ? 'acp'
            : isOpenCodeKey(key)
            ? 'opencode'
            : 'codex',
        label: label,
      );

  /// `codex`, `opencode` or `acp`.
  final String provider;

  /// Agent name of an ACP badge.
  final String? label;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final openCode = provider == 'opencode';
    final acp = provider == 'acp';
    final bg = acp
        ? scheme.primaryContainer
        : openCode
        ? scheme.tertiaryContainer
        : scheme.secondaryContainer;
    final fg = acp
        ? scheme.onPrimaryContainer
        : openCode
        ? scheme.onTertiaryContainer
        : scheme.onSecondaryContainer;
    final text = acp
        ? ((label ?? '').trim().isEmpty ? l10n.providerAcp : label!.trim())
        : openCode
        ? l10n.providerOpenCode
        : l10n.providerCodex;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1.5),
      decoration: BoxDecoration(
        color: bg,
        borderRadius: BorderRadius.circular(999),
      ),
      child: Text(
        text,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: TextStyle(
          fontSize: 10.5,
          height: 1.3,
          color: fg,
          fontWeight: FontWeight.w600,
          letterSpacing: 0.2,
        ),
      ),
    );
  }
}
