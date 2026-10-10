/// The permission card for an ACP agent's `session/request_permission`.
///
/// Unlike the Codex approval card it has no fixed decisions: it shows the
/// options the agent offered, in the agent's order and with the agent's own
/// labels, and answers with the chosen option id exactly as received.
/// Option kinds only pick a visual weight; an unknown kind is shown neutrally
/// and still answered with its own id.
library;

import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/fonts.dart';

/// Event kind of an ACP permission request.
const agentPermissionKind = 'acp/permission/requested';

/// One option an agent offered.
class AgentPermissionOption {
  /// Creates an option.
  const AgentPermissionOption({
    required this.id,
    required this.name,
    required this.kind,
  });

  /// Opaque option id, answered verbatim.
  final String id;

  /// The agent's label.
  final String name;

  /// `allow_once` / `allow_always` / `reject_once` / `reject_always`, or an
  /// unknown future kind.
  final String kind;

  /// Whether choosing it asks the agent to remember the decision.
  bool get remembers => kind == 'allow_always' || kind == 'reject_always';
}

/// A parsed ACP permission request.
class AgentPermissionRequest {
  /// Creates a request.
  const AgentPermissionRequest({
    required this.title,
    required this.detail,
    required this.options,
  });

  /// The tool call's title.
  final String title;

  /// Locations and input worth showing (display only).
  final String detail;

  /// Offered options, in the agent's order.
  final List<AgentPermissionOption> options;

  /// Parse an [agentPermissionKind] event; options that are not well-formed
  /// are left out, never repaired.
  static AgentPermissionRequest parse(AppEvent event) {
    Map<String, dynamic> raw = const {};
    try {
      final decoded = jsonDecode(event.raw);
      if (decoded is Map<String, dynamic>) raw = decoded;
    } catch (_) {}
    final tool = raw['toolCall'] is Map<String, dynamic>
        ? raw['toolCall'] as Map<String, dynamic>
        : const <String, dynamic>{};
    final options = <AgentPermissionOption>[];
    for (final option in (raw['options'] as List?) ?? const []) {
      if (option is! Map) continue;
      final id = option['optionId'];
      final name = option['name'];
      final kind = option['kind'];
      if (id is! String || name is! String || kind is! String) continue;
      options.add(AgentPermissionOption(id: id, name: name, kind: kind));
    }
    final lines = <String>[];
    for (final location in (tool['locations'] as List?) ?? const []) {
      if (location is Map && location['path'] is String) {
        lines.add(location['path'] as String);
      }
    }
    final input = tool['rawInput'];
    if (input is Map && input.isNotEmpty) {
      lines.add(const JsonEncoder.withIndent('  ').convert(input));
    }
    return AgentPermissionRequest(
      title: (tool['title'] as String?) ?? event.title ?? '',
      detail: lines.join('\n'),
      options: options,
    );
  }
}

/// The card; [onAnswer] receives the chosen option id.
class AgentPermissionCard extends StatefulWidget {
  /// Creates the card for [prompt].
  const AgentPermissionCard({
    super.key,
    required this.prompt,
    required this.agentName,
    required this.onAnswer,
  });

  /// The [agentPermissionKind] event.
  final AppEvent prompt;

  /// Agent name for the heading.
  final String agentName;

  /// Sends the answer.
  final Future<void> Function(AppEvent prompt, String optionId) onAnswer;

  @override
  State<AgentPermissionCard> createState() => _AgentPermissionCardState();
}

class _AgentPermissionCardState extends State<AgentPermissionCard> {
  late final AgentPermissionRequest _request = AgentPermissionRequest.parse(
    widget.prompt,
  );
  bool _sending = false;

  Future<void> _choose(AgentPermissionOption option) async {
    if (_sending) return;
    if (option.remembers) {
      final l10n = AppLocalizations.of(context);
      final ok = await showDialog<bool>(
        context: context,
        builder: (ctx) => AlertDialog(
          title: Text(l10n.permissionRememberTitle),
          content: Text(l10n.permissionRememberBody(option.name)),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(ctx).pop(false),
              child: Text(l10n.cancel),
            ),
            FilledButton(
              key: const Key('agent-permission-remember-confirm'),
              onPressed: () => Navigator.of(ctx).pop(true),
              child: Text(option.name),
            ),
          ],
        ),
      );
      if (ok != true || !mounted) return;
    }
    setState(() => _sending = true);
    try {
      await widget.onAnswer(widget.prompt, option.id);
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  Widget _button(AgentPermissionOption option, int index) {
    final key = Key('agent-permission-option-$index');
    final onPressed = _sending ? null : () => _choose(option);
    final label = Text(option.name, overflow: TextOverflow.ellipsis);
    return switch (option.kind) {
      'allow_once' => FilledButton(
        key: key,
        onPressed: onPressed,
        child: label,
      ),
      'allow_always' => FilledButton.tonal(
        key: key,
        onPressed: onPressed,
        child: label,
      ),
      _ => TextButton(key: key, onPressed: onPressed, child: label),
    };
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final request = _request;
    final agent = widget.agentName.trim().isEmpty
        ? l10n.providerAcpAgent
        : widget.agentName.trim();
    return Container(
      key: const Key('agent-permission-card'),
      margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      decoration: BoxDecoration(
        color: scheme.surfaceContainerHigh,
        border: Border.all(color: scheme.outlineVariant, width: 0.5),
        borderRadius: BorderRadius.circular(kPanelRadius),
      ),
      child: Padding(
        padding: const EdgeInsets.all(14),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(Icons.shield_outlined, size: 18, color: scheme.primary),
                const SizedBox(width: 9),
                Expanded(
                  child: Text(
                    l10n.permissionFromAgent(agent),
                    style: TextStyle(
                      color: scheme.onSurface,
                      fontWeight: FontWeight.w500,
                      fontSize: 14,
                    ),
                  ),
                ),
              ],
            ),
            if (request.title.isNotEmpty) ...[
              const SizedBox(height: 8),
              Text(
                request.title,
                key: const Key('agent-permission-title'),
                style: TextStyle(color: scheme.onSurface, fontSize: 13),
              ),
            ],
            if (request.detail.isNotEmpty) ...[
              const SizedBox(height: 8),
              Container(
                width: double.infinity,
                constraints: const BoxConstraints(maxHeight: 160),
                padding: const EdgeInsets.all(10),
                decoration: BoxDecoration(
                  color: scheme.surface.withValues(alpha: 0.6),
                  borderRadius: BorderRadius.circular(8),
                ),
                child: SingleChildScrollView(
                  child: SelectableText(
                    request.detail,
                    style: const TextStyle(
                      fontFamily: monoFontFamily,
                      fontFamilyFallback: monoCjkFallback,
                      fontSize: 12,
                    ),
                  ),
                ),
              ),
            ],
            const SizedBox(height: 8),
            Wrap(
              alignment: WrapAlignment.end,
              spacing: 8,
              runSpacing: 4,
              children: [
                for (var i = 0; i < request.options.length; i++)
                  _button(request.options[i], i),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
