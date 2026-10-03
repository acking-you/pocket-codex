/// The ACP agent's own session options (TRD §4.7.5): modes and any other
/// select or boolean option the agent reports, except model and reasoning
/// effort, which the regular pickers already cover.
library;

import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/providers.dart';

/// Lists the `mode` / `other` options of [threadId] and sets them through
/// `appSetConfigOption`. Re-reads whenever [revision] changes
/// (`acp/config/updated`).
class AcpConfigOptionsPanel extends ConsumerStatefulWidget {
  /// Creates the panel.
  const AcpConfigOptionsPanel({
    super.key,
    required this.serviceKey,
    required this.threadId,
    this.revision,
  });

  /// ACP service.
  final String serviceKey;

  /// Session whose options are shown.
  final String threadId;

  /// Bumped when the agent reports changed options.
  final ValueListenable<int>? revision;

  @override
  ConsumerState<AcpConfigOptionsPanel> createState() =>
      _AcpConfigOptionsPanelState();
}

class _AcpConfigOptionsPanelState extends ConsumerState<AcpConfigOptionsPanel> {
  List<AcpConfigOption>? _options;
  String? _error;

  @override
  void initState() {
    super.initState();
    widget.revision?.addListener(_load);
    _load();
  }

  @override
  void didUpdateWidget(AcpConfigOptionsPanel old) {
    super.didUpdateWidget(old);
    if (old.revision != widget.revision) {
      old.revision?.removeListener(_load);
      widget.revision?.addListener(_load);
    }
    if (old.threadId != widget.threadId) _load();
  }

  @override
  void dispose() {
    widget.revision?.removeListener(_load);
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final options = await ref
          .read(bridgeApiProvider)
          .appConfigOptions(widget.serviceKey, widget.threadId);
      if (mounted) setState(() => _options = options);
    } catch (e) {
      if (mounted) setState(() => _error = friendlyError(e));
    }
  }

  Future<void> _set(AcpConfigOption option, String value) async {
    setState(() {
      _error = null;
      _options = [
        for (final o in _options ?? const <AcpConfigOption>[])
          o.id == option.id
              ? AcpConfigOption(
                  id: o.id,
                  name: o.name,
                  description: o.description,
                  category: o.category,
                  role: o.role,
                  kind: o.kind,
                  currentValue: value,
                  options: o.options,
                )
              : o,
      ];
    });
    try {
      await ref
          .read(bridgeApiProvider)
          .appSetConfigOption(
            widget.serviceKey,
            widget.threadId,
            option.id,
            value,
            boolean: option.isBoolean,
          );
    } catch (e) {
      if (mounted) setState(() => _error = friendlyError(e));
      await _load();
    }
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final shown = (_options ?? const <AcpConfigOption>[])
        .where((o) => o.role == 'mode' || o.role == 'other')
        .toList();
    if (shown.isEmpty && _error == null) return const SizedBox.shrink();
    return Padding(
      key: const Key('acp-config-panel'),
      padding: const EdgeInsets.fromLTRB(14, 2, 14, 8),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (final o in shown)
            if (o.isBoolean)
              SwitchListTile(
                key: Key('acp-config-${o.id}'),
                dense: true,
                contentPadding: EdgeInsets.zero,
                title: Text(o.name, style: const TextStyle(fontSize: 13)),
                subtitle: o.description.isEmpty ? null : Text(o.description),
                value: o.currentValue == 'true',
                onChanged: (v) => _set(o, '$v'),
              )
            else
              DropdownButtonFormField<String>(
                key: Key('acp-config-${o.id}'),
                initialValue: o.options.any((v) => v.value == o.currentValue)
                    ? o.currentValue
                    : null,
                isExpanded: true,
                isDense: true,
                decoration: InputDecoration(
                  labelText: o.name,
                  helperText: o.description.isEmpty ? null : o.description,
                ),
                items: [
                  for (final v in o.options)
                    DropdownMenuItem(
                      value: v.value,
                      child: Text(
                        v.group == null ? v.name : '${v.group} · ${v.name}',
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                ],
                onChanged: (v) {
                  if (v != null && v != o.currentValue) _set(o, v);
                },
              ),
          if (_error != null)
            Text(_error!, style: TextStyle(color: scheme.error, fontSize: 12)),
        ],
      ),
    );
  }
}
