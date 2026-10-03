import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/acp_errors.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/widgets/app_toast.dart';
import 'package:pocket_codex/src/widgets/group_card.dart';
import 'package:pocket_codex/src/widgets/utility_page.dart';

/// ACP agent management (TRD §4.7.3). Without [serviceKey] it manages this
/// desktop (install, upgrade, uninstall, settings, gateways, custom agents);
/// with one it manages the host behind that service through its meta service
/// (install pinned versions and start hosting, when the host allows it).
class AcpAgentsScreen extends ConsumerStatefulWidget {
  /// Creates the screen; [gatewayAgentId] opens that agent's gateway editor.
  const AcpAgentsScreen({super.key, this.serviceKey, this.gatewayAgentId});

  /// A session service of the remote host to manage, or null for this device.
  final String? serviceKey;

  /// Agent whose gateway dialog opens once the page has loaded.
  final String? gatewayAgentId;

  @override
  ConsumerState<AcpAgentsScreen> createState() => _AcpAgentsScreenState();
}

class _AcpAgentsScreenState extends ConsumerState<AcpAgentsScreen> {
  List<AcpAgent>? _agents;
  bool _remoteManagement = true;
  AcpSettings? _settings;
  List<AcpCustomAgent> _custom = const [];
  List<AppServeStatus> _hosts = const [];
  final Map<String, AcpJob> _jobs = {};
  Timer? _poll;
  String? _error;
  bool _busy = false;

  // Editable settings.
  bool _remoteToggle = true;
  final _npmRegistry = TextEditingController();
  final _codexBinary = TextEditingController();
  final _claudeEngine = TextEditingController();
  final Map<String, String> _dataModes = {};
  final Map<String, TextEditingController> _binaries = {};

  bool get _remote => widget.serviceKey != null;

  @override
  void initState() {
    super.initState();
    Future.microtask(() async {
      await _load();
      final gateway = widget.gatewayAgentId;
      if (gateway != null && mounted && !_remote) {
        final agent = _agents?.where((a) => a.id == gateway).firstOrNull;
        if (agent != null) await _editGateway(agent);
      }
    });
  }

  @override
  void dispose() {
    _poll?.cancel();
    _npmRegistry.dispose();
    _codexBinary.dispose();
    _claudeEngine.dispose();
    for (final c in _binaries.values) {
      c.dispose();
    }
    super.dispose();
  }

  BridgeApi get _api => ref.read(bridgeApiProvider);

  Future<void> _load() async {
    try {
      if (_remote) {
        final remote = await _api.metaAcpAgents(widget.serviceKey!);
        if (!mounted) return;
        setState(() {
          _agents = remote.agents;
          _remoteManagement = remote.remoteManagement;
        });
        return;
      }
      final agents = await _api.acpAgents();
      final settings = await _api.acpSettings();
      final custom = await _api.acpCustomAgents();
      var hosts = const <AppServeStatus>[];
      try {
        hosts = await _api.appServeStatus();
      } catch (_) {
        // Hosting status only adds the gateway method list.
      }
      if (!mounted) return;
      setState(() {
        _agents = agents.where((a) => a.source != 'custom').toList();
        _custom = custom;
        _hosts = hosts.where((h) => h.isAcp).toList();
        _applySettings(settings);
      });
    } catch (e) {
      if (mounted) {
        setState(() {
          _agents ??= const [];
          _error = acpErrorMessage(AppLocalizations.of(context), e);
        });
      }
    }
  }

  void _applySettings(AcpSettings settings) {
    _settings = settings;
    _remoteToggle = settings.remoteManagement;
    _npmRegistry.text = settings.npmRegistry ?? '';
    _codexBinary.text = settings.codexBinary ?? '';
    _claudeEngine.text = settings.claudeEnginePath ?? '';
    _dataModes
      ..clear()
      ..addEntries(
        settings.opencodeData.map((d) => MapEntry(d.family, d.mode)),
      );
    for (final o in settings.binaryOverrides) {
      _binaryController(o.agentId).text = o.path;
    }
  }

  TextEditingController _binaryController(String agentId) =>
      _binaries.putIfAbsent(agentId, TextEditingController.new);

  String? _blankToNull(String text) => text.trim().isEmpty ? null : text.trim();

  /// The settings as edited on this page.
  AcpSettings _edited({List<AcpGateway>? gateways, List<AcpAgentFlag>? flags}) {
    final base = _settings ?? const AcpSettings();
    return AcpSettings(
      remoteManagement: _remoteToggle,
      npmRegistry: _blankToNull(_npmRegistry.text),
      codexBinary: _blankToNull(_codexBinary.text),
      claudeEnginePath: _blankToNull(_claudeEngine.text),
      binaryOverrides: [
        for (final e in _binaries.entries)
          if (e.value.text.trim().isNotEmpty)
            AcpBinaryOverride(agentId: e.key, path: e.value.text.trim()),
      ],
      opencodeData: [
        for (final e in _dataModes.entries)
          AcpDataMode(family: e.key, mode: e.value),
      ],
      gateways: gateways ?? base.gateways,
      flags: flags ?? base.flags,
    );
  }

  Future<bool> _confirm(
    String body, {
    String? title,
    required Key key,
    String? okLabel,
  }) async {
    final l10n = AppLocalizations.of(context);
    final ok = await showDialog<bool>(
      context: context,
      builder: (dialog) => AlertDialog(
        key: key,
        title: title == null ? null : Text(title),
        content: Text(body),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(dialog).pop(false),
            child: Text(l10n.cancel),
          ),
          FilledButton(
            key: const Key('acp-confirm-ok'),
            onPressed: () => Navigator.of(dialog).pop(true),
            child: Text(okLabel ?? l10n.save),
          ),
        ],
      ),
    );
    return ok == true;
  }

  /// Save [settings]; D16 warnings are confirmed before forcing the save.
  Future<bool> _save(AcpSettings settings) async {
    final l10n = AppLocalizations.of(context);
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      var result = await _api.acpSettingsSet(settings);
      if (!result.saved && result.warnings.isNotEmpty) {
        if (!mounted) return false;
        final force = await _confirm(
          result.warnings.join('\n\n'),
          key: const Key('acp-save-warning-confirm'),
        );
        if (!force) return false;
        result = await _api.acpSettingsSet(settings, force: true);
      }
      if (!mounted) return result.saved;
      if (result.saved) {
        setState(() => _applySettings(settings));
        showToastOk(context, l10n.acpSettingsSaved);
      }
      return result.saved;
    } catch (e) {
      if (mounted) setState(() => _error = acpErrorMessage(l10n, e));
      return false;
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  // ---------------------------------------------------------------- jobs

  void _watch(AcpJob job) {
    setState(() => _jobs[job.agentId] = job);
    _poll ??= Timer.periodic(
      Duration(milliseconds: _remote ? 1000 : 500),
      (_) => _tick(),
    );
  }

  Future<void> _tick() async {
    final l10n = AppLocalizations.of(context);
    var finished = false;
    for (final job in [..._jobs.values]) {
      AcpJob? next;
      try {
        next = _remote
            ? await _api.metaAcpJob(widget.serviceKey!, job.id)
            : await _api.acpJob(job.id);
      } catch (_) {
        continue;
      }
      if (!mounted) return;
      if (next == null || next.finished) {
        finished = true;
        setState(() {
          _jobs.remove(job.agentId);
          if (next?.state == 'failed') {
            final reason = acpCodeMessage(
              l10n,
              next!.errorCode,
              next.message ?? '',
            );
            _error = next.kind == 'install'
                ? l10n.acpInstallFailed(reason)
                : reason;
          }
        });
        if (next?.kind == 'host' && next?.state == 'done') {
          ref.invalidate(servicesProvider);
        }
      } else {
        setState(() => _jobs[job.agentId] = next!);
      }
    }
    if (_jobs.isEmpty) {
      _poll?.cancel();
      _poll = null;
    }
    if (finished) await _load();
  }

  Future<void> _install(AcpAgent agent, {String? version}) async {
    final l10n = AppLocalizations.of(context);
    setState(() => _error = null);
    try {
      final id = _remote
          ? await _api.metaAcpInstall(widget.serviceKey!, agent.id)
          : await _api.acpInstall(agent.id, version: version);
      if (!mounted) return;
      _watch(
        AcpJob(id: id, kind: 'install', agentId: agent.id, state: 'queued'),
      );
    } catch (e) {
      if (mounted) setState(() => _error = acpErrorMessage(l10n, e));
    }
  }

  Future<void> _installRegistry(AcpAgent agent) async {
    final l10n = AppLocalizations.of(context);
    final ok = await _confirm(
      l10n.acpRegistryInstallWarning,
      key: const Key('acp-registry-install-confirm'),
      okLabel: l10n.acpInstall,
    );
    if (ok) await _install(agent, version: agent.registryVersion);
  }

  Future<void> _uninstall(AcpAgent agent) async {
    final l10n = AppLocalizations.of(context);
    final ok = await _confirm(
      l10n.acpUninstallConfirm(agent.name),
      key: const Key('acp-uninstall-confirm'),
      okLabel: l10n.acpUninstall,
    );
    if (!ok) return;
    try {
      await _api.acpUninstall(agent.id);
      await _load();
    } catch (e) {
      if (mounted) setState(() => _error = acpErrorMessage(l10n, e));
    }
  }

  Future<void> _host(AcpAgent agent) async {
    final l10n = AppLocalizations.of(context);
    setState(() => _error = null);
    try {
      final id = await _api.metaAcpHost(widget.serviceKey!, agent.id);
      if (!mounted) return;
      _watch(AcpJob(id: id, kind: 'host', agentId: agent.id, state: 'queued'));
    } catch (e) {
      if (mounted) setState(() => _error = acpErrorMessage(l10n, e));
    }
  }

  // ------------------------------------------------------------- flags

  String _flagLabel(AppLocalizations l10n, String key) => switch (key) {
    'acpSubscriptionLogin' => l10n.acpSubscriptionLogin,
    _ => key,
  };

  String _flagConfirm(AppLocalizations l10n, String key) => switch (key) {
    'acpSubscriptionLoginConfirmBody' => l10n.acpSubscriptionLoginConfirmBody,
    _ => key,
  };

  Future<void> _toggleFlag(AcpAgentFlag flag, bool value) async {
    final l10n = AppLocalizations.of(context);
    final confirmKey = flag.confirmKey;
    if (value && confirmKey != null) {
      final ok = await _confirm(
        _flagConfirm(l10n, confirmKey),
        title: flag.labelKey == 'acpSubscriptionLogin'
            ? l10n.acpSubscriptionLoginConfirmTitle
            : null,
        key: const Key('acp-flag-confirm'),
      );
      if (!ok) return;
    }
    final flags = [
      for (final f in _settings?.flags ?? const <AcpAgentFlag>[])
        f.agentId == flag.agentId && f.setting == flag.setting
            ? AcpAgentFlag(
                agentId: f.agentId,
                setting: f.setting,
                labelKey: f.labelKey,
                confirmKey: f.confirmKey,
                value: value,
              )
            : f,
    ];
    final saved = await _save(_edited(flags: flags));
    if (saved && mounted) showToast(context, l10n.acpRestartToApply);
  }

  // ----------------------------------------------------------- gateway

  List<AcpAuthMethod> _gatewayMethods(String agentId) {
    for (final host in _hosts.where((h) => h.agentId == agentId)) {
      final auth = _api.appAuthState(host.appServiceKey);
      final methods = auth?.methods.where((m) => m.kind == 'gateway').toList();
      if (methods != null && methods.isNotEmpty) return methods;
    }
    return const [];
  }

  Future<void> _editGateway(AcpAgent agent) async {
    final l10n = AppLocalizations.of(context);
    final current = _settings?.gateways
        .where((g) => g.agentId == agent.id)
        .firstOrNull;
    final result = await showDialog<AcpGateway>(
      context: context,
      builder: (_) => _GatewayDialog(
        agent: agent,
        current: current,
        methods: _gatewayMethods(agent.id),
      ),
    );
    if (result == null || !mounted) return;
    final gateways = [
      for (final g in _settings?.gateways ?? const <AcpGateway>[])
        if (g.agentId != agent.id) g,
      result,
    ];
    if (!await _save(_edited(gateways: gateways))) return;
    // Running instances apply the new gateway on their next start.
    for (final host in _hosts.where((h) => h.agentId == agent.id)) {
      try {
        await _api.acpAuthRecheck(host.name);
      } catch (e) {
        if (mounted) setState(() => _error = acpErrorMessage(l10n, e));
      }
    }
  }

  // ------------------------------------------------------ custom agents

  Future<void> _editCustom([AcpCustomAgent? existing]) async {
    final l10n = AppLocalizations.of(context);
    final result = await showDialog<AcpCustomAgent>(
      context: context,
      builder: (_) => _CustomAgentDialog(existing: existing),
    );
    if (result == null) return;
    try {
      await _api.acpCustomAgentPut(result);
      await _load();
    } catch (e) {
      if (mounted) setState(() => _error = acpErrorMessage(l10n, e));
    }
  }

  Future<void> _deleteCustom(AcpCustomAgent agent) async {
    final l10n = AppLocalizations.of(context);
    try {
      await _api.acpCustomAgentDelete(agent.id);
      await _load();
    } catch (e) {
      if (mounted) setState(() => _error = acpErrorMessage(l10n, e));
    }
  }

  // ---------------------------------------------------------------- UI

  String _stateLabel(AppLocalizations l10n, AcpAgent a) => switch (a.state) {
    'installed' => l10n.acpInstalled,
    'installing' => l10n.acpInstalling,
    'failed' => l10n.acpInstallFailed(a.detail ?? ''),
    'unsupported_platform' => l10n.acpUnsupportedPlatform,
    'engine_missing' => l10n.acpEngineMissing,
    'engine_incompatible' => a.detail ?? l10n.acpEngineMissing,
    _ => l10n.acpNotInstalled,
  };

  bool _installable(AcpAgent a) =>
      a.source == 'catalog' &&
      a.state != 'unsupported_platform' &&
      a.state != 'engine_missing' &&
      a.state != 'installing';

  Widget _agentTile(AcpAgent agent) {
    final l10n = AppLocalizations.of(context);
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final small = theme.textTheme.bodySmall;
    final muted = small?.copyWith(color: scheme.onSurfaceVariant);
    final job = _jobs[agent.id];
    final locked = _remote && !_remoteManagement;
    final upgrade =
        agent.installed &&
        agent.pinnedVersion != null &&
        agent.installedVersion != agent.pinnedVersion;
    final inUse = agent.hostedNames.isNotEmpty;
    final flags = (_settings?.flags ?? const <AcpAgentFlag>[])
        .where((f) => f.agentId == agent.id)
        .toList();
    final archive = agent.source == 'catalog' && !agent.needsNode;
    final actions = <Widget>[
      if (!_remote) ...[
        if (!agent.installed && _installable(agent))
          FilledButton.tonal(
            key: Key('acp-install-${agent.id}'),
            onPressed: _busy || job != null ? null : () => _install(agent),
            child: Text(l10n.acpInstall),
          ),
        if (upgrade && _installable(agent))
          FilledButton.tonal(
            key: Key('acp-upgrade-${agent.id}'),
            onPressed: _busy || job != null ? null : () => _install(agent),
            child: Text(l10n.acpUpgrade),
          ),
        if (agent.installed)
          Tooltip(
            message: inUse ? l10n.acpUninstallInUse : '',
            child: OutlinedButton(
              key: Key('acp-uninstall-${agent.id}'),
              onPressed: _busy || inUse || job != null
                  ? null
                  : () => _uninstall(agent),
              child: Text(l10n.acpUninstall),
            ),
          ),
        OutlinedButton.icon(
          key: Key('acp-gateway-${agent.id}'),
          onPressed: _busy ? null : () => _editGateway(agent),
          icon: const Icon(Icons.hub_outlined, size: 18),
          label: Text(l10n.acpGateway),
        ),
        if (agent.registryVersion != null)
          PopupMenuButton<VoidCallback>(
            key: Key('acp-more-${agent.id}'),
            onSelected: (action) => action(),
            itemBuilder: (_) => [
              PopupMenuItem(
                key: Key('acp-registry-install-${agent.id}'),
                value: () => _installRegistry(agent),
                child: Text(l10n.acpRegistryInstall),
              ),
            ],
          ),
      ] else ...[
        if (!agent.installed && _installable(agent))
          FilledButton.tonal(
            key: Key('acp-install-${agent.id}'),
            onPressed: locked || job != null || !agent.remoteInstallAllowed
                ? null
                : () => _install(agent),
            child: Text(l10n.acpInstall),
          ),
        if (agent.installed && !inUse)
          FilledButton(
            key: const Key('acp-host-btn'),
            onPressed: locked || job != null ? null : () => _host(agent),
            child: Text(l10n.acpHostStart),
          ),
      ],
    ];
    return Padding(
      key: Key('acp-agent-${agent.id}'),
      padding: const EdgeInsets.fromLTRB(14, 10, 14, 10),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            agent.name,
            style: theme.textTheme.titleSmall?.copyWith(
              fontWeight: FontWeight.w600,
            ),
          ),
          if (agent.description.isNotEmpty)
            Text(agent.description, style: muted),
          const SizedBox(height: 4),
          Text(
            [
              _stateLabel(l10n, agent),
              if (agent.installedVersion != null)
                l10n.acpInstalledVersion(agent.installedVersion!),
              if (agent.pinnedVersion != null)
                l10n.acpPinnedVersion(agent.pinnedVersion!),
              if (agent.approxSizeMb > 0) l10n.acpSizeMb(agent.approxSizeMb),
            ].join(' · '),
            key: Key('acp-state-${agent.id}'),
            style: small,
          ),
          if (agent.registryVersion != null)
            Text(
              l10n.acpRegistryNewer(agent.registryVersion!),
              style: small?.copyWith(color: cautionColor(scheme)),
            ),
          if (inUse)
            Text(l10n.acpHostedAs(agent.hostedNames.join(', ')), style: muted),
          if (job != null) ...[
            const SizedBox(height: 6),
            LinearProgressIndicator(
              key: Key('acp-progress-${agent.id}'),
              value: job.fraction,
            ),
          ],
          if (actions.isNotEmpty) ...[
            const SizedBox(height: 8),
            Wrap(spacing: 8, runSpacing: 8, children: actions),
          ],
          if (!_remote && (flags.isNotEmpty || archive))
            ExpansionTile(
              key: Key('acp-advanced-${agent.id}'),
              tilePadding: EdgeInsets.zero,
              title: Text(l10n.acpAdvanced, style: small),
              children: [
                for (final f in flags)
                  SwitchListTile(
                    key: Key('acp-flag-${f.agentId}-${f.setting}'),
                    contentPadding: EdgeInsets.zero,
                    title: Text(_flagLabel(l10n, f.labelKey)),
                    value: f.value,
                    onChanged: _busy ? null : (v) => _toggleFlag(f, v),
                  ),
                if (archive)
                  TextField(
                    key: Key('acp-binary-${agent.id}'),
                    controller: _binaryController(agent.id),
                    decoration: InputDecoration(
                      labelText: l10n.acpBinaryOverride,
                    ),
                    onSubmitted: (_) => _save(_edited()),
                  ),
              ],
            ),
        ],
      ),
    );
  }

  Future<void> _pickDataMode(String family, String mode) async {
    if (mode == 'shared') {
      final ok = await _confirm(
        AppLocalizations.of(context).acpSharedDataWarning,
        key: const Key('acp-shared-data-confirm'),
      );
      if (!ok) return;
    }
    setState(() => _dataModes[family] = mode);
  }

  Widget _settingsCard() {
    final l10n = AppLocalizations.of(context);
    final small = Theme.of(context).textTheme.bodySmall;
    return GroupCard(
      title: l10n.settingsTitle,
      children: [
        SwitchListTile(
          key: const Key('acp-remote-toggle'),
          title: Text(l10n.acpRemoteManagement),
          subtitle: Text(l10n.acpRemoteManagementHint, style: small),
          value: _remoteToggle,
          onChanged: _busy ? null : (v) => setState(() => _remoteToggle = v),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(14, 4, 14, 8),
          child: Column(
            children: [
              TextField(
                key: const Key('acp-npm-registry'),
                controller: _npmRegistry,
                decoration: InputDecoration(labelText: l10n.acpNpmRegistry),
              ),
              TextField(
                key: const Key('acp-codex-binary'),
                controller: _codexBinary,
                decoration: InputDecoration(labelText: l10n.acpCodexBinary),
              ),
              for (final family in _dataModes.keys)
                DropdownButtonFormField<String>(
                  key: Key('acp-data-$family'),
                  initialValue: _dataModes[family],
                  decoration: InputDecoration(
                    labelText: l10n.acpDataMode(family),
                  ),
                  items: [
                    DropdownMenuItem(
                      value: 'auto',
                      child: Text(l10n.acpDataAuto),
                    ),
                    DropdownMenuItem(
                      value: 'shared',
                      child: Text(l10n.acpDataShared),
                    ),
                    DropdownMenuItem(
                      value: 'isolated',
                      child: Text(l10n.acpDataIsolated),
                    ),
                  ],
                  onChanged: _busy
                      ? null
                      : (v) {
                          if (v != null) _pickDataMode(family, v);
                        },
                ),
              ExpansionTile(
                key: const Key('acp-settings-advanced'),
                tilePadding: EdgeInsets.zero,
                title: Text(l10n.acpAdvanced),
                children: [
                  TextField(
                    key: const Key('acp-claude-engine'),
                    controller: _claudeEngine,
                    decoration: InputDecoration(
                      labelText: l10n.acpClaudeEnginePath,
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 8),
              Align(
                alignment: Alignment.centerRight,
                child: FilledButton(
                  key: const Key('acp-settings-save'),
                  onPressed: _busy ? null : () => _save(_edited()),
                  child: Text(l10n.save),
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }

  Widget _customCard() {
    final l10n = AppLocalizations.of(context);
    final muted = Theme.of(context).textTheme.bodySmall?.copyWith(
      color: Theme.of(context).colorScheme.onSurfaceVariant,
    );
    return GroupCard(
      title: l10n.acpCustomAgent,
      trailing: TextButton.icon(
        key: const Key('acp-custom-add'),
        onPressed: () => _editCustom(),
        icon: const Icon(Icons.add, size: 18),
        label: Text(l10n.acpCustomAdd),
      ),
      children: [
        for (final c in _custom)
          ListTile(
            key: Key('acp-custom-${c.id}'),
            title: Text(c.name.isEmpty ? c.id : c.name),
            subtitle: Text('${c.id} · ${c.command}', style: muted),
            trailing: Wrap(
              children: [
                IconButton(
                  key: Key('acp-custom-edit-${c.id}'),
                  icon: const Icon(Icons.edit_outlined),
                  onPressed: () => _editCustom(c),
                ),
                IconButton(
                  key: Key('acp-custom-delete-${c.id}'),
                  icon: const Icon(Icons.delete_outline),
                  tooltip: l10n.acpCustomDelete,
                  onPressed: () => _deleteCustom(c),
                ),
              ],
            ),
          ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final agents = _agents;
    return UtilityPage(
      route: '/settings',
      title: l10n.acpAgentsTitle,
      parent: UtilityParent(title: l10n.settingsTitle, route: '/settings'),
      body: SafeArea(
        child: Align(
          alignment: Alignment.topCenter,
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 680),
            child: ListView(
              padding: const EdgeInsets.fromLTRB(16, 16, 16, 24),
              children: [
                if (_remote && !_remoteManagement)
                  Card(
                    key: const Key('acp-remote-disabled'),
                    color: scheme.errorContainer,
                    child: Padding(
                      padding: const EdgeInsets.all(12),
                      child: Text(
                        l10n.acpRemoteDisabled,
                        style: TextStyle(color: scheme.onErrorContainer),
                      ),
                    ),
                  ),
                if (_error != null)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 8),
                    child: Text(
                      _error!,
                      key: const Key('acp-error'),
                      style: TextStyle(color: scheme.error),
                    ),
                  ),
                if (agents == null)
                  const LinearProgressIndicator()
                else
                  GroupCard(
                    title: l10n.acpAgentsTitle,
                    children: [for (final a in agents) _agentTile(a)],
                  ),
                if (!_remote && _settings != null) ...[
                  const SizedBox(height: 12),
                  _settingsCard(),
                  const SizedBox(height: 12),
                  _customCard(),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// Edits one agent's model gateway (D20). The token field is write-only.
class _GatewayDialog extends StatefulWidget {
  const _GatewayDialog({
    required this.agent,
    required this.current,
    required this.methods,
  });

  final AcpAgent agent;
  final AcpGateway? current;

  /// Gateway methods the running agent offers (empty when not hosted).
  final List<AcpAuthMethod> methods;

  @override
  State<_GatewayDialog> createState() => _GatewayDialogState();
}

class _GatewayDialogState extends State<_GatewayDialog> {
  late final _url = TextEditingController(text: widget.current?.baseUrl ?? '');
  final _token = TextEditingController();
  late final _provider = TextEditingController(
    text: widget.current?.providerName ?? '',
  );
  late final _headers = TextEditingController(
    text: (widget.current?.extraHeaders ?? const [])
        .map((h) => '${h.name}=${h.value}')
        .join('\n'),
  );
  late String? _method = widget.current?.methodId;

  @override
  void dispose() {
    _url.dispose();
    _token.dispose();
    _provider.dispose();
    _headers.dispose();
    super.dispose();
  }

  String? get _protocol =>
      widget.methods
          .where((m) => m.id == _method)
          .firstOrNull
          ?.gatewayProtocol ??
      widget.methods.firstOrNull?.gatewayProtocol;

  String _methodLabel(AppLocalizations l10n, AcpAuthMethod m) {
    final protocol = m.gatewayProtocol;
    final name = m.name.isEmpty ? m.id : m.name;
    return protocol == null
        ? name
        : '$name · ${l10n.acpGatewayProtocol(protocol)}';
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final small = Theme.of(context).textTheme.bodySmall;
    final protocol = _protocol;
    final current = widget.current;
    final insecure = _url.text.trim().toLowerCase().startsWith('http://');
    return AlertDialog(
      key: const Key('acp-gateway-dialog'),
      title: Text('${l10n.acpGateway} · ${widget.agent.name}'),
      content: SizedBox(
        width: 420,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              DropdownButtonFormField<String?>(
                key: const Key('acp-gateway-method'),
                initialValue: widget.methods.any((m) => m.id == _method)
                    ? _method
                    : null,
                decoration: InputDecoration(labelText: l10n.acpGatewayMethod),
                items: [
                  DropdownMenuItem<String?>(
                    value: null,
                    child: Text(l10n.acpGatewayAuto),
                  ),
                  for (final m in widget.methods)
                    DropdownMenuItem<String?>(
                      value: m.id,
                      child: Text(_methodLabel(l10n, m)),
                    ),
                ],
                onChanged: (v) => setState(() => _method = v),
              ),
              TextField(
                key: const Key('acp-gateway-url'),
                controller: _url,
                decoration: InputDecoration(labelText: l10n.acpGatewayUrl),
                onChanged: (_) => setState(() {}),
              ),
              const SizedBox(height: 4),
              if (protocol != 'openai')
                Text(l10n.acpGatewayUrlHintAnthropic, style: small),
              if (protocol != 'anthropic')
                Text(l10n.acpGatewayUrlHintOpenai, style: small),
              if (insecure)
                Text(
                  l10n.acpGatewayInsecureHttp,
                  style: small?.copyWith(color: cautionColor(scheme)),
                ),
              TextField(
                key: const Key('acp-gateway-token'),
                controller: _token,
                obscureText: true,
                decoration: InputDecoration(
                  labelText: l10n.acpGatewayToken,
                  helperText: current?.hasToken ?? false
                      ? l10n.acpGatewayTokenKept
                      : null,
                ),
              ),
              TextField(
                key: const Key('acp-gateway-provider'),
                controller: _provider,
                decoration: InputDecoration(labelText: l10n.acpGatewayProvider),
              ),
              TextField(
                key: const Key('acp-gateway-headers'),
                controller: _headers,
                minLines: 2,
                maxLines: 5,
                decoration: InputDecoration(labelText: l10n.acpGatewayHeaders),
              ),
              const SizedBox(height: 8),
              Text(l10n.acpGatewayNote, style: small),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(l10n.cancel),
        ),
        if (current != null)
          TextButton(
            key: const Key('acp-gateway-clear'),
            onPressed: () => Navigator.of(context).pop(
              AcpGateway(agentId: widget.agent.id, baseUrl: '', clear: true),
            ),
            child: Text(l10n.acpGatewayClear),
          ),
        FilledButton(
          key: const Key('acp-gateway-save'),
          onPressed: _url.text.trim().isEmpty
              ? null
              : () => Navigator.of(context).pop(
                  AcpGateway(
                    agentId: widget.agent.id,
                    methodId: _method,
                    baseUrl: _url.text.trim(),
                    token: _token.text.isEmpty ? null : _token.text,
                    hasToken: current?.hasToken ?? false,
                    providerName: _provider.text.trim().isEmpty
                        ? null
                        : _provider.text.trim(),
                    extraHeaders: _pairs(_headers.text),
                  ),
                ),
          child: Text(l10n.save),
        ),
      ],
    );
  }
}

/// `KEY=VALUE` lines → pairs (blank and malformed lines are skipped).
List<AcpEnvVar> _pairs(String text) => [
  for (final line in text.split('\n'))
    if (line.contains('='))
      AcpEnvVar(
        name: line.substring(0, line.indexOf('=')).trim(),
        value: line.substring(line.indexOf('=') + 1).trim(),
      ),
];

/// Adds or edits a custom agent (D15).
class _CustomAgentDialog extends StatefulWidget {
  const _CustomAgentDialog({this.existing});

  final AcpCustomAgent? existing;

  @override
  State<_CustomAgentDialog> createState() => _CustomAgentDialogState();
}

class _CustomAgentDialogState extends State<_CustomAgentDialog> {
  late final _id = TextEditingController(text: widget.existing?.id ?? '');
  late final _name = TextEditingController(text: widget.existing?.name ?? '');
  late final _command = TextEditingController(
    text: widget.existing?.command ?? '',
  );
  late final _args = TextEditingController(
    text: (widget.existing?.args ?? const []).join('\n'),
  );
  late final _env = TextEditingController(
    text: (widget.existing?.env ?? const [])
        .map((e) => '${e.name}=${e.value}')
        .join('\n'),
  );

  @override
  void dispose() {
    for (final c in [_id, _name, _command, _args, _env]) {
      c.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final small = Theme.of(context).textTheme.bodySmall;
    final ready = _id.text.trim().isNotEmpty && _command.text.trim().isNotEmpty;
    return AlertDialog(
      key: const Key('acp-custom-dialog'),
      title: Text(l10n.acpCustomAgent),
      content: SizedBox(
        width: 420,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextField(
                key: const Key('acp-custom-id'),
                controller: _id,
                readOnly: widget.existing != null,
                decoration: InputDecoration(labelText: l10n.acpCustomId),
                onChanged: (_) => setState(() {}),
              ),
              TextField(
                key: const Key('acp-custom-name'),
                controller: _name,
                decoration: InputDecoration(labelText: l10n.acpCustomName),
              ),
              TextField(
                key: const Key('acp-custom-command'),
                controller: _command,
                decoration: InputDecoration(labelText: l10n.acpCustomCommand),
                onChanged: (_) => setState(() {}),
              ),
              TextField(
                key: const Key('acp-custom-args'),
                controller: _args,
                minLines: 2,
                maxLines: 6,
                decoration: InputDecoration(labelText: l10n.acpCustomArgs),
              ),
              TextField(
                key: const Key('acp-custom-env'),
                controller: _env,
                minLines: 2,
                maxLines: 6,
                decoration: InputDecoration(labelText: l10n.acpCustomEnv),
              ),
              const SizedBox(height: 8),
              Text(l10n.acpCustomEnvNote, style: small),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(l10n.cancel),
        ),
        FilledButton(
          key: const Key('acp-custom-save'),
          onPressed: !ready
              ? null
              : () => Navigator.of(context).pop(
                  AcpCustomAgent(
                    id: _id.text.trim(),
                    name: _name.text.trim(),
                    command: _command.text.trim(),
                    args: [
                      for (final a in _args.text.split('\n'))
                        if (a.trim().isNotEmpty) a.trim(),
                    ],
                    env: _pairs(_env.text),
                  ),
                ),
          child: Text(l10n.save),
        ),
      ],
    );
  }
}
