part of 'local_host_dialog.dart';

/// The ACP half of [LocalHostDialog] (TRD §4.7.2): pick an installed agent
/// (installing or upgrading it first when needed), start hosting, log in, and
/// the running host's details. The hub owns the agent process, so stopping
/// hosting ends it.
extension _AcpHost on _LocalHostDialogState {
  AcpAgent? get _acpSelected {
    final agents = _acpAgents;
    if (agents == null || agents.isEmpty) return null;
    return agents.where((a) => a.id == _acpAgentId).firstOrNull;
  }

  Future<void> _loadAcpAgents() async {
    try {
      final agents = await ref.read(bridgeApiProvider).acpAgents();
      if (!mounted) return;
      _update(() {
        _acpAgents = agents;
        final keep = agents.any((a) => a.id == _acpAgentId);
        if (!keep && agents.isNotEmpty) {
          final installed = agents.where((a) => a.installed).firstOrNull;
          _selectAcpAgent((installed ?? agents.first).id);
        }
      });
    } catch (e) {
      if (mounted) {
        _update(() {
          _acpAgents = const [];
          _error = acpErrorMessage(AppLocalizations.of(context), e);
        });
      }
    }
  }

  void _selectAcpAgent(String id) {
    _acpAgentId = id;
    if (!_acpNameEdited) _acpName.text = id;
  }

  String _acpStateLabel(AppLocalizations l10n, AcpAgent agent) =>
      switch (agent.state) {
        'installed' => l10n.acpInstalled,
        'installing' => l10n.acpInstalling,
        'failed' => l10n.acpInstallFailed(agent.detail ?? ''),
        'unsupported_platform' => l10n.acpUnsupportedPlatform,
        'engine_missing' => l10n.acpEngineMissing,
        'engine_incompatible' => agent.detail ?? l10n.acpEngineMissing,
        _ => l10n.acpNotInstalled,
      };

  bool _acpCanInstall(AcpAgent agent) =>
      agent.source == 'catalog' &&
      agent.state != 'unsupported_platform' &&
      agent.state != 'engine_missing' &&
      agent.state != 'installing' &&
      (!agent.installed || agent.installedVersion != agent.pinnedVersion);

  List<Widget> _acpForm() {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final small = Theme.of(context).textTheme.bodySmall;
    final muted = small?.copyWith(color: scheme.onSurfaceVariant);
    final agents = _acpAgents;
    final agent = _acpSelected;
    final job = _acpJob;
    final installing = job != null && !job.finished;
    return [
      _providerPicker(l10n),
      const SizedBox(height: 12),
      if (agents == null)
        const LinearProgressIndicator()
      else ...[
        DropdownButtonFormField<String>(
          key: const Key('acp-agent-picker'),
          initialValue: agent?.id,
          isExpanded: true,
          decoration: InputDecoration(labelText: l10n.acpAgentLabel),
          items: [
            for (final a in agents)
              DropdownMenuItem(
                value: a.id,
                child: Text(
                  '${a.name} · ${_acpStateLabel(l10n, a)}',
                  overflow: TextOverflow.ellipsis,
                ),
              ),
          ],
          onChanged: _busy || installing
              ? null
              : (id) {
                  if (id != null) _update(() => _selectAcpAgent(id));
                },
        ),
        if (agent != null) ...[
          const SizedBox(height: 8),
          if (agent.installedVersion != null)
            Text(
              l10n.acpInstalledVersion(agent.installedVersion!),
              style: small,
            ),
          if (agent.pinnedVersion != null)
            Text(l10n.acpPinnedVersion(agent.pinnedVersion!), style: muted),
          if (agent.approxSizeMb > 0)
            Text(l10n.acpSizeMb(agent.approxSizeMb), style: muted),
          if (agent.state != 'installed' && agent.state != 'not_installed')
            Text(
              _acpStateLabel(l10n, agent),
              key: const Key('acp-agent-state'),
              style: small?.copyWith(color: scheme.error),
            ),
          if (agent.registryVersion != null)
            Text(
              l10n.acpRegistryNewer(agent.registryVersion!),
              style: small?.copyWith(color: cautionColor(scheme)),
            ),
          const SizedBox(height: 8),
          if (installing)
            LinearProgressIndicator(
              key: const Key('acp-install-progress'),
              value: job.fraction,
            )
          else if (_acpCanInstall(agent))
            Align(
              alignment: Alignment.centerLeft,
              child: OutlinedButton.icon(
                key: const Key('acp-install-btn'),
                onPressed: _busy ? null : () => _installAcp(agent),
                icon: const Icon(Icons.download, size: 18),
                label: Text(
                  agent.installed ? l10n.acpUpgrade : l10n.acpInstall,
                ),
              ),
            ),
        ],
        const SizedBox(height: 12),
        TextField(
          key: const Key('acp-name'),
          controller: _acpName,
          decoration: InputDecoration(labelText: l10n.localHostName),
          onChanged: (_) => _acpNameEdited = true,
        ),
        Align(
          alignment: Alignment.centerLeft,
          child: TextButton(
            key: const Key('acp-manage-link'),
            onPressed: () {
              final router = GoRouter.of(context);
              Navigator.of(context).pop();
              router.push('/settings/acp');
            },
            child: Text(l10n.acpManageAgents),
          ),
        ),
      ],
    ];
  }

  Future<void> _installAcp(AcpAgent agent) async {
    final api = ref.read(bridgeApiProvider);
    final l10n = AppLocalizations.of(context);
    _update(() => _error = null);
    try {
      final id = await api.acpInstall(agent.id);
      if (!mounted) return;
      _update(
        () => _acpJob = AcpJob(
          id: id,
          kind: 'install',
          agentId: agent.id,
          state: 'queued',
        ),
      );
      _acpPoll?.cancel();
      _acpPoll = Timer.periodic(const Duration(milliseconds: 500), (
        timer,
      ) async {
        final job = await api.acpJob(id);
        if (!mounted) {
          timer.cancel();
          return;
        }
        if (job == null || job.finished) {
          timer.cancel();
          _update(() {
            _acpJob = null;
            if (job?.state == 'failed') {
              _error = l10n.acpInstallFailed(
                acpCodeMessage(l10n, job!.errorCode, job.message ?? ''),
              );
            }
          });
          await _loadAcpAgents();
        } else {
          _update(() => _acpJob = job);
        }
      });
    } catch (e) {
      if (mounted) _update(() => _error = acpErrorMessage(l10n, e));
    }
  }

  Future<void> _startAcp() async {
    final agent = _acpSelected;
    if (agent == null) return;
    _update(() {
      _busy = true;
      _error = null;
    });
    final api = ref.read(bridgeApiProvider);
    try {
      final typed = _acpName.text.trim();
      final name = typed.isEmpty ? agent.id : typed;
      final result = await api.appServeStartAcp(name: name, agentId: agent.id);
      ref
          .read(uiPrefsProvider.notifier)
          .setAutoHostAcp(
            AutoHostAcpPrefs(name: result.name, agentId: agent.id),
          );
      ref.invalidate(localServeListProvider);
      ref.invalidate(servicesProvider);
      if (result.auth.required) {
        // Stay open on the new host so the user can log in right away.
        final hosts = await api.appServeStatus();
        final host = hosts.where((h) => h.name == result.name).firstOrNull;
        if (!mounted) return;
        if (host != null) {
          _update(() {
            _acpHosted = host;
            _acpAuth = result.auth;
          });
          unawaited(_loadAcpHost(host));
          return;
        }
      }
      if (mounted) Navigator.of(context).pop();
    } catch (e) {
      // Name collisions and start failures from the bridge are specific
      // enough to show verbatim.
      if (mounted) _update(() => _error = friendlyError(e));
    } finally {
      if (mounted) _update(() => _busy = false);
    }
  }

  /// Auth state and the configured gateway address of a running host.
  Future<void> _loadAcpHost(AppServeStatus host) async {
    final api = ref.read(bridgeApiProvider);
    final auth = api.appAuthState(host.appServiceKey);
    String? gateway;
    try {
      final settings = await api.acpSettings();
      gateway = settings.gateways
          .where((g) => g.agentId == host.agentId)
          .firstOrNull
          ?.baseUrl;
    } catch (_) {
      // Settings are optional detail here.
    }
    if (!mounted) return;
    _update(() {
      _acpAuth ??= auth;
      _acpGatewayUrl = gateway;
    });
  }

  Future<void> _stopAcp(AppServeStatus host) async {
    final l10n = AppLocalizations.of(context);
    final api = ref.read(bridgeApiProvider);
    var running = const <String>[];
    try {
      running = await api.appRunningThreads(host.appServiceKey);
    } catch (_) {
      // Not connected to this host here: nothing known to be running.
    }
    if (!mounted) return;
    if (running.isNotEmpty) {
      final ok = await showDialog<bool>(
        context: context,
        builder: (dialog) => AlertDialog(
          key: const Key('acp-stop-confirm'),
          content: Text(l10n.acpStopRunningConfirm),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(dialog).pop(false),
              child: Text(l10n.cancel),
            ),
            FilledButton(
              key: const Key('acp-stop-confirm-ok'),
              onPressed: () => Navigator.of(dialog).pop(true),
              child: Text(l10n.stopHosting),
            ),
          ],
        ),
      );
      if (ok != true || !mounted) return;
    }
    _update(() {
      _busy = true;
      _error = null;
    });
    try {
      await api.appServeStop(host.name);
      ref.read(uiPrefsProvider.notifier).removeAutoHostAcp(host.name);
      ref
          .read(pendingRemovalProvider.notifier)
          .update((set) => {...set, host.appServiceKey});
      ref.invalidate(localServeListProvider);
      ref.invalidate(servicesProvider);
      if (mounted) Navigator.of(context).pop();
    } catch (e) {
      if (mounted) _update(() => _error = friendlyError(e));
    } finally {
      if (mounted) _update(() => _busy = false);
    }
  }

  List<Widget> _acpExisting(AppServeStatus host) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final small = Theme.of(context).textTheme.bodySmall;
    final muted = small?.copyWith(color: scheme.onSurfaceVariant);
    return [
      Row(
        children: [
          ProviderBadge(provider: 'acp', label: host.agentName),
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              '${host.agentId ?? '—'} · ${host.providerVersion ?? '—'}',
              key: const Key('acp-agent-version'),
              style: small,
            ),
          ),
        ],
      ),
      if (!host.providerVerified)
        Text(
          l10n.acpUnpinned,
          key: const Key('acp-unpinned'),
          style: small?.copyWith(color: cautionColor(scheme)),
        ),
      const SizedBox(height: 12),
      Text(l10n.providerAcp, style: small),
      Text(l10n.localHostListening(host.appListenAddr), style: small),
      SelectableText(host.appServiceKey, style: muted),
      const SizedBox(height: 8),
      Text(l10n.hostMetaLabel, style: small),
      Text(l10n.localHostListening(host.metaListenAddr), style: small),
      SelectableText(host.metaServiceKey, style: muted),
      const Divider(height: 24),
      ..._acpAuthSection(host),
      const Divider(height: 24),
      ProjectFoldersEditor(serviceKey: host.appServiceKey),
    ];
  }

  Future<void> _acpLogin(AppServeStatus host, AcpAuthMethod method) async {
    final api = ref.read(bridgeApiProvider);
    _update(() => _error = null);
    try {
      if (method.kind == 'terminal') {
        await api.acpAuthTerminal(host.name, method.id);
      } else {
        final auth = await api.acpAuthAgent(host.name, method.id);
        if (mounted) _update(() => _acpAuth = auth);
      }
    } catch (e) {
      if (mounted) {
        _update(
          () => _error = acpErrorMessage(AppLocalizations.of(context), e),
        );
      }
    }
  }

  Future<void> _acpRecheck(AppServeStatus host) async {
    _update(() {
      _busy = true;
      _error = null;
    });
    try {
      final auth = await ref.read(bridgeApiProvider).acpAuthRecheck(host.name);
      if (mounted) _update(() => _acpAuth = auth);
    } catch (e) {
      if (mounted) {
        _update(
          () => _error = acpErrorMessage(AppLocalizations.of(context), e),
        );
      }
    } finally {
      if (mounted) _update(() => _busy = false);
    }
  }

  Widget _acpLoginButton(AppServeStatus host, AcpAuthMethod method) {
    final usable =
        method.kind == 'agent' ||
        (method.kind == 'terminal' && method.available);
    return Tooltip(
      message: method.description,
      child: OutlinedButton(
        key: Key('acp-login-${method.id}'),
        onPressed: _busy || !usable ? null : () => _acpLogin(host, method),
        child: Text(method.name.isEmpty ? method.id : method.name),
      ),
    );
  }

  List<Widget> _acpAuthSection(AppServeStatus host) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final small = Theme.of(context).textTheme.bodySmall;
    final auth = _acpAuth;
    final methods = auth?.methods ?? const <AcpAuthMethod>[];
    final gateways = methods.where((m) => m.kind == 'gateway').toList();
    final configured = gateways.any((m) => m.gatewayConfigured);
    final logins = methods.where((m) => m.kind != 'gateway').toList();
    final buttons = [for (final m in logins) _acpLoginButton(host, m)];
    return [
      Text(l10n.acpLogin, style: small?.copyWith(fontWeight: FontWeight.w600)),
      const SizedBox(height: 4),
      if (auth?.required ?? false)
        Text(
          l10n.acpAuthRequired,
          key: const Key('acp-auth-required'),
          style: small?.copyWith(color: scheme.error),
        ),
      if (auth?.message case final message? when message.isNotEmpty)
        Text(message, style: small),
      if (gateways.isNotEmpty && configured) ...[
        Text(
          l10n.acpGatewayConfigured(_acpGatewayUrl ?? ''),
          key: const Key('acp-gateway-configured'),
          style: small,
        ),
        if (buttons.isNotEmpty)
          ExpansionTile(
            key: const Key('acp-other-logins'),
            tilePadding: EdgeInsets.zero,
            title: Text(l10n.acpOtherLogins, style: small),
            children: [Wrap(spacing: 8, runSpacing: 8, children: buttons)],
          ),
      ] else ...[
        if (gateways.isNotEmpty)
          Align(
            alignment: Alignment.centerLeft,
            child: TextButton.icon(
              key: const Key('acp-gateway-configure'),
              onPressed: () {
                final router = GoRouter.of(context);
                Navigator.of(context).pop();
                router.push(
                  '/settings/acp?gateway=${Uri.encodeQueryComponent(host.agentId ?? '')}',
                );
              },
              icon: const Icon(Icons.hub_outlined, size: 18),
              label: Text(l10n.acpGatewayConfigure),
            ),
          ),
        if (buttons.isNotEmpty)
          Wrap(spacing: 8, runSpacing: 8, children: buttons),
      ],
      const SizedBox(height: 8),
      Align(
        alignment: Alignment.centerLeft,
        child: TextButton.icon(
          key: const Key('acp-recheck-btn'),
          onPressed: _busy ? null : () => _acpRecheck(host),
          icon: const Icon(Icons.refresh, size: 18),
          label: Text(l10n.acpRecheck),
        ),
      ),
    ];
  }
}
