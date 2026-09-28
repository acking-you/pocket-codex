import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/service_key.dart';

/// A compact pill naming the service provider (Codex / OpenCode) of a host,
/// shown next to host names so two providers on one device stay apart.
class ProviderBadge extends StatelessWidget {
  /// A badge for [provider] (`codex` or `opencode`).
  const ProviderBadge({super.key, required this.provider});

  /// A badge for the provider behind the relay service [key].
  factory ProviderBadge.forKey(String key, {Key? badgeKey}) => ProviderBadge(
    key: badgeKey,
    provider: isOpenCodeKey(key) ? 'opencode' : 'codex',
  );

  /// `codex` or `opencode`.
  final String provider;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final openCode = provider == 'opencode';
    final bg = openCode ? scheme.tertiaryContainer : scheme.secondaryContainer;
    final fg = openCode
        ? scheme.onTertiaryContainer
        : scheme.onSecondaryContainer;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1.5),
      decoration: BoxDecoration(
        color: bg,
        borderRadius: BorderRadius.circular(999),
      ),
      child: Text(
        openCode ? l10n.providerOpenCode : l10n.providerCodex,
        maxLines: 1,
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
