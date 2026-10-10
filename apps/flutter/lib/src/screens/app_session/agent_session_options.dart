/// An ACP agent's own per-session controls in the composer toolbar: one chip
/// per select configuration option it reported (model, mode, thought level,
/// …), the legacy mode when it reports one, and — before the first send —
/// the project folder. An agent that reports nothing gets no chips: there is
/// no invented model catalog behind them.
library;

import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';

/// The chips; selecting a value calls back with the opaque ids verbatim.
class AgentSessionOptions extends StatelessWidget {
  /// Creates the chips from [settings].
  const AgentSessionOptions({
    super.key,
    required this.settings,
    required this.enabled,
    required this.onSelectConfig,
    required this.onSelectMode,
    this.projectLabel,
    this.onPickProject,
  });

  /// The agent's current session state, or null before a session exists.
  final SessionSettings? settings;

  /// Whether the chips can be used right now.
  final bool enabled;

  /// Sets select option `configId` to `value`.
  final Future<void> Function(String configId, String value) onSelectConfig;

  /// Switches the legacy mode.
  final Future<void> Function(String modeId) onSelectMode;

  /// The project chip label, before the first send.
  final String? projectLabel;

  /// Opens the project picker.
  final Future<void> Function()? onPickProject;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final settings = this.settings;
    final chips = <Widget>[
      if (projectLabel != null && onPickProject != null)
        ActionChip(
          key: const Key('agent-project-chip'),
          avatar: const Icon(Icons.folder_outlined, size: 16),
          label: Text(projectLabel!, overflow: TextOverflow.ellipsis),
          onPressed: () => onPickProject!(),
        ),
      if (settings != null)
        for (final option in settings.configOptions)
          _OptionChip(
            key: Key('agent-config-${option.id}'),
            tooltip: option.description ?? option.name,
            label: option.currentLabel,
            icon: switch (option.category) {
              'model' => Icons.auto_awesome,
              'thought_level' => Icons.psychology_outlined,
              'mode' => Icons.tune,
              _ => Icons.tune,
            },
            enabled: enabled,
            entries: [
              for (final value in option.values)
                (
                  id: value.value,
                  label: value.group == null
                      ? value.name
                      : '${value.group} · ${value.name}',
                  selected: value.value == option.currentValue,
                ),
            ],
            onSelected: (value) => onSelectConfig(option.id, value),
          ),
      if (settings != null && settings.modes.isNotEmpty)
        _OptionChip(
          key: const Key('agent-mode'),
          tooltip: l10n.agentMode,
          label:
              settings.modes
                  .where((m) => m.id == settings.currentMode)
                  .firstOrNull
                  ?.name ??
              l10n.agentMode,
          icon: Icons.swap_horiz,
          enabled: enabled,
          entries: [
            for (final mode in settings.modes)
              (
                id: mode.id,
                label: mode.name,
                selected: mode.id == settings.currentMode,
              ),
          ],
          onSelected: onSelectMode,
        ),
    ];
    if (chips.isEmpty) return const SizedBox.shrink();
    return Semantics(
      container: true,
      label: l10n.agentOptions,
      child: SingleChildScrollView(
        key: const Key('agent-session-options'),
        scrollDirection: Axis.horizontal,
        reverse: true,
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (var i = 0; i < chips.length; i++) ...[
              if (i > 0) const SizedBox(width: 4),
              chips[i],
            ],
          ],
        ),
      ),
    );
  }
}

class _OptionChip extends StatelessWidget {
  const _OptionChip({
    super.key,
    required this.tooltip,
    required this.label,
    required this.icon,
    required this.enabled,
    required this.entries,
    required this.onSelected,
  });

  final String tooltip;
  final String label;
  final IconData icon;
  final bool enabled;
  final List<({String id, String label, bool selected})> entries;
  final Future<void> Function(String id) onSelected;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return PopupMenuButton<String>(
      tooltip: tooltip,
      enabled: enabled && entries.isNotEmpty,
      onSelected: (id) => onSelected(id),
      itemBuilder: (_) => [
        for (final entry in entries)
          CheckedPopupMenuItem<String>(
            value: entry.id,
            checked: entry.selected,
            child: Text(entry.label, overflow: TextOverflow.ellipsis),
          ),
      ],
      child: ConstrainedBox(
        constraints: const BoxConstraints(minHeight: 40, maxWidth: 200),
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
          decoration: BoxDecoration(
            border: Border.all(color: scheme.outlineVariant),
            borderRadius: BorderRadius.circular(999),
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(icon, size: 15, color: scheme.onSurfaceVariant),
              const SizedBox(width: 4),
              Flexible(
                child: Text(
                  label,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(fontSize: 12.5, color: scheme.onSurface),
                ),
              ),
              Icon(Icons.expand_more, size: 15, color: scheme.onSurfaceVariant),
            ],
          ),
        ),
      ),
    );
  }
}
