import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/hosting/editor_block.dart';
import 'package:pocket_codex/src/managed/default_project_tile.dart';
import 'package:pocket_codex/src/managed/managed_add_service.dart';
import 'package:pocket_codex/src/hosting/model_service_section.dart';
import 'package:pocket_codex/src/managed/managed_host_editor.dart';
import 'package:pocket_codex/src/managed/managed_providers.dart';
import 'package:pocket_codex/src/managed/management_failure.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/widgets/agent_icon.dart';
import 'package:pocket_codex/src/widgets/app_toast.dart';
import 'package:pocket_codex/src/widgets/group_card.dart';
import 'package:pocket_codex/src/app_icons.dart';

/// Management of another computer through its `host:` service: its services
/// (running and stopped), adding one from what it has installed, start /
/// stop, its default project folder and folded diagnostics.
///
/// The entry stays while the computer is online, even with no services. An
/// older Pocket without management never publishes `host:`, so this panel is
/// never offered for it.
class ManagedDevicePanel extends ConsumerStatefulWidget {
  /// Creates the panel for the computer behind [serviceKey].
  const ManagedDevicePanel({
    super.key,
    required this.serviceKey,
    required this.device,
    this.onOpen,
  });

  /// The computer's management key.
  final String serviceKey;

  /// Its device name.
  final String device;

  /// Open a running service's conversations (its session key).
  final ValueChanged<String>? onOpen;

  @override
  ConsumerState<ManagedDevicePanel> createState() => _ManagedDevicePanelState();
}

class _ManagedDevicePanelState extends ConsumerState<ManagedDevicePanel> {
  /// Hosts with a mutation in flight on this computer.
  final Set<String> _pending = {};

  /// Each host's last operation result: (text, failed).
  final Map<String, (String, bool)> _results = {};

  /// Moves whenever the panel shows another computer: an answer for the
  /// computer it was asked on never lands on another one.
  int _generation = 0;

  @override
  void didUpdateWidget(ManagedDevicePanel old) {
    super.didUpdateWidget(old);
    // Results belong to the computer they were reported for.
    if (old.serviceKey != widget.serviceKey) {
      _generation++;
      _pending.clear();
      _results.clear();
    }
  }

  void _refresh() => ref.invalidate(managedDeviceProvider(widget.serviceKey));

  /// Run one row operation for [host] on computer [key] (captured before
  /// any confirmation). A lost answer is reported as unknown and the
  /// computer is re-read; the request is never sent again by itself.
  Future<void> _run(
    String key,
    ManagedHost host,
    Future<void> Function(BridgeApi api, String key) operation,
    String success,
  ) async {
    // The computer moved while a confirmation was open, or the row is
    // already busy: nothing is sent.
    if (!mounted || key != widget.serviceKey) return;
    if (_pending.contains(host.name)) return;
    final generation = _generation;
    final l10n = AppLocalizations.of(context);
    // The container outlives this panel: the re-read still happens after
    // the page is left.
    final container = ProviderScope.containerOf(context, listen: false);
    final api = ref.read(bridgeApiProvider);
    setState(() {
      _pending.add(host.name);
      _results.remove(host.name);
    });
    (String, bool) result;
    try {
      await operation(api, key);
      result = (success, false);
    } catch (e) {
      result = (managedFailureText(l10n, ManagementFailure.of(e)), true);
    }
    container.invalidate(managedDeviceProvider(key));
    if (!mounted || generation != _generation) return;
    setState(() {
      _pending.remove(host.name);
      _results[host.name] = result;
    });
  }

  Future<void> _start(ManagedHost host) => _run(
    widget.serviceKey,
    host,
    (api, key) => api.managedHostStartSaved(
      key,
      host.name,
      expectedRevision: host.config.revision,
    ),
    AppLocalizations.of(context).managedStarted(host.name),
  );

  Future<void> _stop(ManagedHost host) async {
    final incarnation = host.config.active?.incarnation;
    if (incarnation == null) return;
    final key = widget.serviceKey;
    final l10n = AppLocalizations.of(context);
    if (!await confirmManaged(
      context,
      l10n.managedStopConfirmTitle(host.name),
      l10n.managedStopConfirmBody,
      l10n.stopHosting,
    )) {
      return;
    }
    await _run(
      key,
      host,
      (api, key) =>
          api.managedHostStop(key, host.name, expectedIncarnation: incarnation),
      l10n.managedStopped(host.name),
    );
  }

  Future<void> _edit(ManagedHost host) async {
    await showManagedHostEditor(
      context,
      serviceKey: widget.serviceKey,
      device: widget.device,
      host: host,
    );
    if (mounted) _refresh();
  }

  Future<void> _add(List<ManagedHost> hosts) async {
    await showManagedAddService(
      context,
      serviceKey: widget.serviceKey,
      device: widget.device,
      hosts: hosts,
    );
    if (mounted) _refresh();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final async = ref.watch(managedDeviceProvider(widget.serviceKey));
    final device = async.valueOrNull;
    final children = <Widget>[];
    if (device?.restoreProblem case final problem?) {
      children.add(
        EditorBlock(
          child: EditorNote(
            l10n.managedRestoreProblem(problem),
            key: const Key('managed-device-restore-problem'),
            color: cautionColor(scheme),
          ),
        ),
      );
    }
    if (device == null && async.isLoading) {
      children.add(const LinearProgressIndicator(minHeight: 2));
    } else if (device == null && async.hasError) {
      children.add(
        EditorBlock(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                managedFailureText(l10n, ManagementFailure.of(async.error!)),
                key: const Key('managed-device-error'),
                style: TextStyle(color: scheme.error),
              ),
              TextButton(
                key: const Key('managed-device-retry'),
                onPressed: _refresh,
                child: Text(l10n.retry),
              ),
            ],
          ),
        ),
      );
    }
    if (device != null) {
      final hosts = [...device.hosts]..sort((a, b) => a.name.compareTo(b.name));
      if (hosts.isEmpty) {
        children.add(
          EditorBlock(
            child: Text(
              l10n.managedNoServices,
              key: const Key('managed-no-services'),
              style: TextStyle(color: scheme.onSurfaceVariant),
            ),
          ),
        );
      }
      for (final host in hosts) {
        children.add(
          _HostRow(
            key: Key('managed-host-${host.name}'),
            host: host,
            pending: _pending.contains(host.name),
            result: _results[host.name],
            onOpen: widget.onOpen,
            onStart: () => _start(host),
            onStop: () => _stop(host),
            onEdit: () => _edit(host),
          ),
        );
      }
      children.add(
        EditorBlock(
          child: Align(
            alignment: AlignmentDirectional.centerStart,
            child: FilledButton.tonalIcon(
              key: const Key('managed-add-service'),
              onPressed: () => _add(hosts),
              icon: const Icon(AppIcons.add, size: 18),
              label: Text(l10n.managedAddService),
            ),
          ),
        ),
      );
    }
    children
      ..add(
        EditorBlock(
          child: DefaultProjectTile(
            key: ValueKey('managed-default-${widget.serviceKey}'),
            serviceKey: widget.serviceKey,
          ),
        ),
      )
      ..add(_diagnostics(l10n, device));
    return GroupCard(
      key: Key('managed-device-${widget.device}'),
      title: l10n.managedComputerTitle,
      hint: device == null
          ? null
          : l10n.managedComputerSummary(
              device.metadata?.instanceLabel ?? device.os,
              device.appVersion,
            ),
      trailing: IconButton(
        key: const Key('managed-device-refresh'),
        tooltip: l10n.managedRefreshComputer,
        icon: async.isLoading && device != null
            ? const SizedBox(
                width: 18,
                height: 18,
                child: CircularProgressIndicator(strokeWidth: 2),
              )
            : const Icon(AppIcons.refresh, size: 20),
        onPressed: _refresh,
      ),
      children: children,
    );
  }

  Widget _diagnostics(AppLocalizations l10n, ManagedDevice? device) => Material(
    // An own ink layer: a coloured ancestor would hide the tile's.
    type: MaterialType.transparency,
    child: Theme(
      data: Theme.of(context).copyWith(dividerColor: Colors.transparent),
      child: ExpansionTile(
        key: const Key('managed-diagnostics'),
        title: Text(l10n.managedDiagnostics),
        childrenPadding: const EdgeInsets.fromLTRB(18, 0, 18, 12),
        expandedCrossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          DiagnosticLine(label: l10n.managedKey, value: widget.serviceKey),
          if (device != null)
            DiagnosticLine(label: l10n.managedDevice, value: device.device),
          if (device?.metadata case final metadata?)
            if (metadata.hostId case final id?)
              DiagnosticLine(label: l10n.servicesHostInstance, value: id),
          if (device?.metadata case final metadata?)
            DiagnosticLine(
              label: l10n.servicesPublisherInstance,
              value: metadata.instanceId,
            ),
          if (device != null)
            for (final host in device.hosts)
              if (host.status case final status?)
                DiagnosticLine(label: host.name, value: status.appServiceKey),
        ],
      ),
    ),
  );
}

/// One label / value of a diagnostics section: the value wraps, is
/// selectable and can be copied whole.
class DiagnosticLine extends StatelessWidget {
  /// Creates a line.
  const DiagnosticLine({super.key, required this.label, required this.value});

  /// What the value is.
  final String label;

  /// The full value.
  final String value;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final muted = Theme.of(context).textTheme.bodySmall?.copyWith(
      color: Theme.of(context).colorScheme.onSurfaceVariant,
    );
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(label, style: muted),
                SelectableText(value, style: const TextStyle(fontSize: 13)),
              ],
            ),
          ),
          IconButton(
            tooltip: l10n.copy,
            icon: const Icon(AppIcons.copy, size: 16),
            onPressed: () {
              Clipboard.setData(ClipboardData(text: value));
              showToast(context, l10n.copied);
            },
          ),
        ],
      ),
    );
  }
}

class _HostRow extends StatelessWidget {
  const _HostRow({
    super.key,
    required this.host,
    required this.pending,
    required this.result,
    required this.onStart,
    required this.onStop,
    required this.onEdit,
    this.onOpen,
  });

  final ManagedHost host;
  final bool pending;
  final (String, bool)? result;
  final ValueChanged<String>? onOpen;
  final VoidCallback onStart;
  final VoidCallback onStop;
  final VoidCallback onEdit;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final status = host.status;
    final unreadable = host.config.problem != null;
    final muted = theme.textTheme.bodySmall?.copyWith(
      color: scheme.onSurfaceVariant,
    );
    final engine = host.engine;
    final profile =
        status?.profileId ??
        host.config.runtime?.agent?.profileId ??
        host.config.active?.runtime.agent?.profileId;
    return EditorBlock(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              AgentIcon.of(
                provider: engine.isEmpty ? 'acp' : engine,
                profileId: profile,
                name: host.name,
                size: 20,
              ),
              const SizedBox(width: 10),
              Expanded(
                child: Tooltip(
                  message: host.name,
                  child: Text(
                    host.name,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: theme.textTheme.titleSmall,
                  ),
                ),
              ),
              const SizedBox(width: 8),
              managedStateChip(context, host),
            ],
          ),
          const SizedBox(height: 4),
          Text(managedModelSummary(l10n, host), style: muted),
          if (unreadable)
            Padding(
              padding: const EdgeInsets.only(top: 4),
              child: Text(
                l10n.managedConfigUnreadable(host.config.problem!),
                style: TextStyle(color: scheme.error, fontSize: 12),
              ),
            ),
          if (status != null && host.config.pendingApply)
            Padding(
              padding: const EdgeInsets.only(top: 4),
              child: Text(
                l10n.hostConfigPending,
                style: TextStyle(color: cautionColor(scheme), fontSize: 12),
              ),
            ),
          if (host.restoreProblem case final problem?)
            Padding(
              padding: const EdgeInsets.only(top: 4),
              child: Text(
                l10n.managedRestoreProblem(problem),
                style: TextStyle(color: cautionColor(scheme), fontSize: 12),
              ),
            ),
          const SizedBox(height: 8),
          Wrap(
            spacing: 8,
            runSpacing: 4,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              if (pending)
                const SizedBox(
                  key: Key('managed-row-pending'),
                  width: 18,
                  height: 18,
                  child: CircularProgressIndicator(strokeWidth: 2),
                ),
              if (status != null && onOpen != null && !unreadable)
                FilledButton.tonal(
                  key: Key('managed-open-${host.name}'),
                  onPressed: pending
                      ? null
                      : () => onOpen!(status.appServiceKey),
                  child: Text(l10n.servicesOpen),
                ),
              if (status == null && !unreadable)
                FilledButton.tonal(
                  key: Key('managed-start-${host.name}'),
                  onPressed: pending ? null : onStart,
                  child: Text(l10n.managedStart),
                ),
              OutlinedButton(
                key: Key('managed-edit-${host.name}'),
                onPressed: pending ? null : onEdit,
                child: Text(l10n.managedEdit),
              ),
              if (status != null)
                TextButton(
                  key: Key('managed-stop-${host.name}'),
                  style: TextButton.styleFrom(foregroundColor: scheme.error),
                  onPressed: pending || host.config.active == null
                      ? null
                      : onStop,
                  child: Text(l10n.stopHosting),
                ),
            ],
          ),
          if (result case (final text, final failed))
            Padding(
              padding: const EdgeInsets.only(top: 6),
              child: Text(
                text,
                key: Key('managed-result-${host.name}'),
                style: TextStyle(
                  color: failed ? scheme.error : successColor(scheme),
                  fontSize: 12,
                ),
              ),
            ),
        ],
      ),
    );
  }
}

/// One line on where [host] gets its model.
String managedModelSummary(AppLocalizations l10n, ManagedHost host) {
  final c = host.config;
  final kind = managedKindLabel(
    l10n,
    managedHostKind(host),
    agentName: managedAgent(host)?.displayName,
  );
  return [
    kind,
    modelSourceLabel(l10n, c.source),
    if (c.source == 'custom')
      if (c.model case final model? when model.isNotEmpty) model,
  ].join(' · ');
}
