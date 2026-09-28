part of 'local_host_dialog.dart';

/// The OpenCode half of [LocalHostDialog]: provider picker, new-host form,
/// start/stop and the running-host details. OpenCode hosting attaches to the
/// user's own OpenCode service, so there is no port, proxy or engine choice.
extension _OpenCodeHost on _LocalHostDialogState {
  Widget _providerPicker(AppLocalizations l10n) => SegmentedButton<String>(
    key: const Key('host-provider-picker'),
    segments: [
      ButtonSegment(
        value: 'codex',
        label: Text(l10n.providerCodex, key: const Key('provider-codex')),
      ),
      ButtonSegment(
        value: 'opencode',
        label: Text(l10n.providerOpenCode, key: const Key('provider-opencode')),
      ),
    ],
    selected: {_openCode ? 'opencode' : 'codex'},
    onSelectionChanged: _busy
        ? null
        : (selection) {
            final openCode = selection.first == 'opencode';
            _update(() {
              _openCode = openCode;
              _error = null;
            });
            if (openCode && !_ocChecked) _detectOpenCode();
          },
  );

  Future<void> _detectOpenCode() async {
    final found = await ref.read(bridgeApiProvider).opencodeLocate();
    if (!mounted) return;
    _update(() {
      _ocLocated = found;
      _ocChecked = true;
      if (found != null && _ocPath.text.trim().isEmpty) _ocPath.text = found;
    });
  }

  List<Widget> _openCodeForm() {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final small = Theme.of(context).textTheme.bodySmall;
    return [
      _providerPicker(l10n),
      const SizedBox(height: 12),
      Text(l10n.openCodeHostHint),
      const SizedBox(height: 12),
      TextField(
        key: const Key('opencode-name-field'),
        controller: _ocName,
        decoration: InputDecoration(labelText: l10n.localHostName),
      ),
      const SizedBox(height: 12),
      if (!_ocChecked)
        const LinearProgressIndicator()
      else ...[
        if (_ocLocated == null)
          Text(l10n.openCodeNotFound, style: TextStyle(color: scheme.error)),
        TextField(
          key: const Key('opencode-path-field'),
          controller: _ocPath,
          decoration: InputDecoration(labelText: l10n.openCodeBinaryPath),
        ),
        const SizedBox(height: 4),
        Text(l10n.openCodeBinaryHint, style: small),
      ],
    ];
  }

  Future<void> _startOpenCode() async {
    _update(() {
      _busy = true;
      _error = null;
    });
    try {
      final typedName = _ocName.text.trim();
      final name = typedName.isEmpty ? 'opencode' : typedName;
      final typedPath = _ocPath.text.trim();
      // The detected path needs no pinning; only a user-chosen one does.
      final override = typedPath.isEmpty || typedPath == _ocLocated
          ? null
          : typedPath;
      await ref
          .read(bridgeApiProvider)
          .appServeStartOpencode(name: name, binaryOverride: override);
      ref
          .read(uiPrefsProvider.notifier)
          .setAutoHostOpenCode(
            AutoHostOpenCodePrefs(name: name, binaryOverride: override),
          );
      ref.invalidate(localServeListProvider);
      ref.invalidate(servicesProvider);
      if (mounted) Navigator.of(context).pop();
    } catch (e) {
      // Name collisions and contract failures from the bridge are specific
      // enough to show verbatim.
      if (mounted) _update(() => _error = friendlyError(e));
    } finally {
      if (mounted) _update(() => _busy = false);
    }
  }

  Future<void> _stopOpenCode() async {
    _update(() {
      _busy = true;
      _error = null;
    });
    try {
      final host = widget.existing!;
      // Withdraws the gateway + meta keys; OpenCode itself keeps running.
      await ref.read(bridgeApiProvider).appServeStop(host.name);
      ref.read(uiPrefsProvider.notifier).clearAutoHostOpenCode();
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

  List<Widget> _openCodeExisting(AppServeStatus host) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final small = Theme.of(context).textTheme.bodySmall;
    final muted = small?.copyWith(color: scheme.onSurfaceVariant);
    final version = host.providerVersion ?? '—';
    return [
      Row(
        children: [
          const ProviderBadge(provider: 'opencode'),
          const SizedBox(width: 8),
          Expanded(child: Text(l10n.openCodeStopNote, style: small)),
        ],
      ),
      const SizedBox(height: 12),
      Text(l10n.openCodeGatewayLabel, style: small),
      Text(l10n.localHostListening(host.appListenAddr), style: small),
      SelectableText(host.appServiceKey, style: muted),
      const SizedBox(height: 8),
      Text(l10n.hostMetaLabel, style: small),
      Text(l10n.localHostListening(host.metaListenAddr), style: small),
      SelectableText(host.metaServiceKey, style: muted),
      const Divider(height: 24),
      Text(
        l10n.hostRuntimeInfo,
        style: small?.copyWith(fontWeight: FontWeight.w600),
      ),
      const SizedBox(height: 4),
      Text(
        '${l10n.openCodeVersionLabel}: $version'
        '${host.providerVerified ? ' · ${l10n.openCodeVerified}' : ''}',
        key: const Key('opencode-version'),
        style: small,
      ),
      if (!host.providerVerified)
        Text(
          l10n.openCodeUnverified,
          key: const Key('opencode-unverified'),
          style: small?.copyWith(color: cautionColor(scheme)),
        ),
      if (host.codexBinary != null)
        SelectableText(
          '${l10n.hostCodexPath}: ${host.codexBinary}',
          style: muted,
        ),
      const Divider(height: 24),
      ProjectFoldersEditor(serviceKey: host.appServiceKey),
    ];
  }
}
