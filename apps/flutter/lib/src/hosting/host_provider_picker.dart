import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';

/// Hosting choices: native Codex, the attached OpenCode HTTP service, or any
/// ACP agent (OpenCode's `opencode acp` is one of its presets).
///
/// Chips in a [Wrap] rather than a segmented control, so three choices stay
/// readable and reachable (padded touch targets) on a 320 px phone.
class HostProviderPicker extends StatelessWidget {
  /// Creates the picker with the selected [value]: `codex`, `opencode` or
  /// `acp`.
  const HostProviderPicker({
    super.key,
    required this.value,
    required this.onChanged,
  });

  /// The selected provider.
  final String value;

  /// Called with the new choice; `null` disables the picker.
  final ValueChanged<String>? onChanged;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    Widget chip(String id, String label) => ChoiceChip(
      label: Text(label, key: Key('provider-$id')),
      selected: value == id,
      materialTapTargetSize: MaterialTapTargetSize.padded,
      onSelected: onChanged == null ? null : (_) => onChanged!(id),
    );
    return Semantics(
      container: true,
      label: l10n.providerLabel,
      child: Wrap(
        key: const Key('host-provider-picker'),
        spacing: 8,
        runSpacing: 4,
        children: [
          chip('codex', l10n.providerCodex),
          chip('opencode', l10n.providerOpenCodeService),
          chip('acp', l10n.providerAcpAgent),
        ],
      ),
    );
  }
}
