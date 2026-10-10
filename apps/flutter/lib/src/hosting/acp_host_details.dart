import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/widgets/project_folders_editor.dart';
import 'package:pocket_codex/src/widgets/provider_badge.dart';

/// A running ACP host: its agent's state (starting, running, not running and
/// why, sign-in needed), what the agent negotiated, the gateway and meta
/// service, a restart action and the project folders.
///
/// Feature rows are shown only from a live negotiation; before it they say
/// so instead of guessing.
class AcpHostDetails extends ConsumerStatefulWidget {
  /// Creates the details for [host].
  const AcpHostDetails({super.key, required this.host});

  /// The ACP host.
  final AppServeStatus host;

  @override
  ConsumerState<AcpHostDetails> createState() => _AcpHostDetailsState();
}

class _AcpHostDetailsState extends ConsumerState<AcpHostDetails> {
  bool _restarting = false;
  String? _error;

  Future<void> _restart() async {
    setState(() {
      _restarting = true;
      _error = null;
    });
    try {
      await ref.read(bridgeApiProvider).appServeRestartAcp(widget.host.name);
      ref.invalidate(localServeListProvider);
    } catch (e) {
      if (mounted) setState(() => _error = friendlyError(e));
    } finally {
      if (mounted) setState(() => _restarting = false);
    }
  }

  String _phaseLabel(AppLocalizations l10n, String? phase) => switch (phase) {
    'ready' => l10n.acpPhaseReady,
    'failed' => l10n.acpPhaseFailed,
    'stopped' => l10n.acpPhaseStopped,
    _ => l10n.acpPhaseStarting,
  };

  Widget _feature(String label, String value, TextStyle? style) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 2),
    child: Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Expanded(child: Text(label, style: style)),
        const SizedBox(width: 8),
        Flexible(
          child: Text(value, style: style, textAlign: TextAlign.end),
        ),
      ],
    ),
  );

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final small = Theme.of(context).textTheme.bodySmall;
    final muted = small?.copyWith(color: scheme.onSurfaceVariant);
    final host = widget.host;
    final phase = host.agentPhase;
    final ready = phase == 'ready';
    final caps = ref
        .read(bridgeApiProvider)
        .appCapabilities(host.appServiceKey);
    final negotiated = ready && caps.negotiated;
    final phaseColor = switch (phase) {
      'ready' => successColor(scheme),
      'failed' => scheme.error,
      _ => scheme.onSurfaceVariant,
    };
    String supported(bool value) =>
        value ? l10n.acpFeatureSupported : l10n.acpFeatureNotSupported;
    final reopen = switch (caps.sessionReopen) {
      'load' => l10n.acpFeatureReopenLoad,
      'resume' => l10n.acpFeatureReopenResume,
      _ => l10n.acpFeatureNotSupported,
    };
    return Column(
      key: const Key('acp-host-details'),
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Wrap(
          spacing: 8,
          runSpacing: 6,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            ProviderBadge(provider: 'acp', name: host.providerName),
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(Icons.circle, size: 10, color: phaseColor),
                const SizedBox(width: 4),
                Flexible(
                  child: Text(
                    _phaseLabel(l10n, phase),
                    key: const Key('acp-phase'),
                    style: small?.copyWith(color: phaseColor),
                  ),
                ),
              ],
            ),
          ],
        ),
        if (host.agentError != null && !ready) ...[
          const SizedBox(height: 6),
          SelectableText(
            host.agentError!,
            key: const Key('acp-agent-error'),
            style: small?.copyWith(color: scheme.error),
          ),
        ],
        if (host.authRequired) ...[
          const SizedBox(height: 6),
          Text(
            l10n.acpAuthRequired,
            key: const Key('acp-auth-required'),
            style: small?.copyWith(color: cautionColor(scheme)),
          ),
        ],
        const SizedBox(height: 8),
        Text(l10n.acpStopNote, style: small),
        const SizedBox(height: 4),
        Align(
          alignment: AlignmentDirectional.centerStart,
          child: OutlinedButton.icon(
            key: const Key('acp-restart'),
            onPressed: _restarting ? null : _restart,
            icon: const Icon(Icons.restart_alt),
            label: Text(l10n.acpRestart),
          ),
        ),
        if (_error != null)
          Text(_error!, style: small?.copyWith(color: scheme.error)),
        const Divider(height: 24),
        Text(
          l10n.acpFeatures,
          style: small?.copyWith(fontWeight: FontWeight.w600),
        ),
        const SizedBox(height: 4),
        if (!negotiated)
          Text(
            l10n.acpFeatureUnknown,
            key: const Key('acp-features-unknown'),
            style: muted,
          )
        else ...[
          _feature(l10n.acpFeatureReopen, reopen, small),
          _feature(
            l10n.acpFeatureList,
            supported(caps.sessionList == 'agent'),
            small,
          ),
          _feature(l10n.acpFeatureImages, supported(caps.imageInput), small),
        ],
        const Divider(height: 24),
        Text(l10n.acpGatewayLabel, style: small),
        Text(l10n.localHostListening(host.appListenAddr), style: small),
        SelectableText(host.appServiceKey, style: muted),
        const SizedBox(height: 8),
        Text(l10n.hostMetaLabel, style: small),
        Text(l10n.localHostListening(host.metaListenAddr), style: small),
        SelectableText(host.metaServiceKey, style: muted),
        if (host.codexBinary != null) ...[
          const SizedBox(height: 8),
          SelectableText(
            '${l10n.acpProgram}: ${host.codexBinary}',
            style: muted,
          ),
        ],
        const Divider(height: 24),
        ProjectFoldersEditor(serviceKey: host.appServiceKey),
      ],
    );
  }
}
