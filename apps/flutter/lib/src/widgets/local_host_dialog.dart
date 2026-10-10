import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/hosting/acp_host_details.dart';
import 'package:pocket_codex/src/hosting/acp_host_form.dart';
import 'package:pocket_codex/src/hosting/host_provider_picker.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/widgets/project_folders_editor.dart';
import 'package:pocket_codex/src/widgets/provider_badge.dart';

part 'local_host_opencode.dart';

/// Manage one local host. With [existing] set it shows that host's listen
/// address + service key and a Stop button. Otherwise it's the "new host" form
/// with a provider choice: Codex (codex path, port, instance name, proxy),
/// the OpenCode service (instance name, optional opencode path), or an ACP
/// agent (preset / saved / custom program and arguments, see
/// `hosting/acp_host_form.dart`). codex is auto-detected (with a "change
/// path" override) or picked when not on PATH.
///
/// Shared by the manage page's hosting tab and the chat-first home screen's
/// "start hosting" hero action, so both entry points behave identically.
class LocalHostDialog extends ConsumerStatefulWidget {
  /// Creates the dialog; [existing] switches it to manage-a-running-host mode.
  const LocalHostDialog({super.key, this.existing});

  /// The running host this dialog manages, or null to host a new one.
  final AppServeStatus? existing;

  @override
  ConsumerState<LocalHostDialog> createState() => _LocalHostDialogState();
}

class _LocalHostDialogState extends ConsumerState<LocalHostDialog> {
  final _port = TextEditingController(text: '18080');
  final _path = TextEditingController();
  final _name = TextEditingController(text: 'default');
  // Codex needs a proxy to reach chatgpt.com on most networks, so hosting
  // defaults to a proxy (a local HTTP proxy on :11111) unless the user opts out.
  final _proxy = TextEditingController(text: 'http://127.0.0.1:11111');
  bool _useProxy = true;
  bool _overridePath = false; // user chose to customize the codex path
  String? _codexPath; // auto-detected codex (config → PATH), null = not found
  bool _codexChecked = false;
  bool _busy = false;
  String? _error;
  // New-host provider: `codex`, `opencode` (HTTP service) or `acp`.
  String _provider = 'codex';
  bool get _openCode => _provider == 'opencode';
  AcpFormValue? _acp;
  final _ocName = TextEditingController(text: 'opencode');
  final _ocPath = TextEditingController();
  bool _ocChecked = false;
  String? _ocLocated; // auto-detected opencode, null = not found
  bool get _isExisting => widget.existing != null;
  bool get _codexFound => _codexPath != null;

  /// [setState] for the OpenCode part of this library.
  void _update(VoidCallback fn) => setState(fn);

  @override
  void initState() {
    super.initState();
    if (_isExisting) return;
    // Auto-detect codex: when found we just show "available" (with a "change
    // path" override); when not, the user picks a path (persisted on start) or
    // installs codex and taps "re-detect".
    Future.microtask(_detectCodex);
  }

  /// (Re-)resolve codex from PATH + persisted config. Safe to call again from a
  /// "re-detect" button: a user who hadn't installed codex yet can install it,
  /// tap re-detect, and have it picked up — no need to type a full path.
  Future<void> _detectCodex() async {
    if (!mounted) return;
    setState(() => _codexChecked = false); // show the progress indicator
    final found = await ref.read(bridgeApiProvider).codexLocate();
    if (!mounted) return;
    setState(() {
      _codexPath = found;
      _codexChecked = true;
      if (found != null) {
        _path.text = found; // prefill the override field
        _overridePath = false; // a fresh detection supersedes a manual override
      }
    });
  }

  @override
  void dispose() {
    _port.dispose();
    _path.dispose();
    _name.dispose();
    _proxy.dispose();
    _ocName.dispose();
    _ocPath.dispose();
    super.dispose();
  }

  Future<void> _browseCodex() async {
    final file = await openFile();
    if (file != null && mounted) setState(() => _path.text = file.path);
  }

  Future<void> _start() async {
    final l10n = AppLocalizations.of(context);
    final port = int.tryParse(_port.text.trim());
    // 0 is allowed (the engine picks an ephemeral port); reject out-of-range,
    // incl. negatives, which would otherwise wrap silently to a u16.
    if (port == null || port < 0 || port > 65535) {
      setState(() => _error = l10n.localHostPort);
      return;
    }
    final manual = !_codexFound || _overridePath;
    final path = manual ? _path.text.trim() : '';
    if (manual && path.isEmpty) {
      setState(() => _error = l10n.codexPathRequired);
      return;
    }
    final override = path.isEmpty ? null : path;
    // A proxy is mandatory unless the user explicitly turned it off.
    final proxy = _useProxy ? _proxy.text.trim() : null;
    if (_useProxy && (proxy == null || proxy.isEmpty)) {
      setState(() => _error = l10n.proxyRequired);
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final name = _name.text.trim();
      await ref
          .read(bridgeApiProvider)
          .appServeStart(
            port: port,
            binaryOverride: override,
            name: name.isEmpty ? null : name,
            proxy: proxy,
            embedded: false,
          );
      // Remember the params so a desktop cold start can restore this hosting
      // without another trip through this dialog.
      ref
          .read(uiPrefsProvider.notifier)
          .setAutoHost(
            AutoHostPrefs(
              port: port,
              name: name.isEmpty ? 'default' : name,
              proxy: proxy,
              embedded: false,
              binaryOverride: override,
            ),
          );
      ref.invalidate(localServeListProvider);
      ref.invalidate(servicesProvider);
      if (mounted) Navigator.of(context).pop();
    } catch (e) {
      if (mounted) {
        // A duplicate-name refusal (another live instance owns this name) gets
        // the localized guidance instead of the raw broker reason.
        final raw = friendlyError(e);
        setState(
          () => _error = isHostNameConflict(raw)
              ? AppLocalizations.of(context).hostNameConflict
              : raw,
        );
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _startAcp() async {
    final value = _acp;
    if (value == null || !value.valid) {
      setState(() => _error = AppLocalizations.of(context).acpProgramNotFound);
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final result = await ref
          .read(bridgeApiProvider)
          .appServeStartAcp(
            name: value.name.isEmpty ? null : value.name,
            spec: value.spec,
          );
      final prefs = ref.read(uiPrefsProvider.notifier)
        ..setAutoHostAcp(AutoHostAcpPrefs(name: result.name, spec: value.spec));
      if (value.save) prefs.saveAcpAgent(value.spec);
      ref.invalidate(localServeListProvider);
      ref.invalidate(servicesProvider);
      if (mounted) Navigator.of(context).pop();
    } catch (e) {
      if (mounted) setState(() => _error = friendlyError(e));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _stopAcp() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final host = widget.existing!;
      // Stops the agent and cleans up the processes it started.
      await ref.read(bridgeApiProvider).appServeStop(host.name);
      ref.read(uiPrefsProvider.notifier).clearAutoHostAcp(host.name);
      ref
          .read(pendingRemovalProvider.notifier)
          .update((set) => {...set, host.appServiceKey});
      ref.invalidate(localServeListProvider);
      ref.invalidate(servicesProvider);
      if (mounted) Navigator.of(context).pop();
    } catch (e) {
      if (mounted) setState(() => _error = friendlyError(e));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _stop() async {
    if (widget.existing!.isOpenCode) return _stopOpenCode();
    if (widget.existing!.isAcp) return _stopAcp();
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final host = widget.existing!;
      // Full stop: kills codex + the API proxy, aborts both tunnels, and
      // force-drops both relay keys.
      await ref.read(bridgeApiProvider).appServeStop(host.name);
      // The user stopped hosting on purpose — don't resurrect it at boot.
      ref.read(uiPrefsProvider.notifier).clearAutoHost();
      // Optimistically hide both discovery entries so they leave at once.
      ref
          .read(pendingRemovalProvider.notifier)
          .update((set) => {...set, host.appServiceKey, host.apiServiceKey});
      ref.invalidate(localServeListProvider);
      ref.invalidate(servicesProvider);
      if (mounted) Navigator.of(context).pop();
    } catch (e) {
      if (mounted) setState(() => _error = friendlyError(e));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final small = Theme.of(context).textTheme.bodySmall;
    final existing = widget.existing;
    if (existing != null ? existing.isOpenCode : _openCode) {
      return _dialog(
        existing != null ? _openCodeExisting(existing) : _openCodeForm(),
      );
    }
    if (existing != null ? existing.isAcp : _provider == 'acp') {
      return _dialog(
        existing != null
            ? [AcpHostDetails(host: existing)]
            : [
                _providerPicker(l10n),
                const SizedBox(height: 12),
                AcpHostForm(
                  enabled: !_busy,
                  onChanged: (value) => _acp = value,
                ),
              ],
        wide: true,
      );
    }
    final children = <Widget>[
      if (existing == null) ...[
        _providerPicker(l10n),
        const SizedBox(height: 12),
      ],
      Text(l10n.localHostHint),
    ];
    if (existing != null) {
      // Two tunnels under one name: show each kind's listen address + relay key.
      children
        ..add(const SizedBox(height: 12))
        ..add(Text(l10n.tunnelAppLabel, style: small))
        ..add(
          Text(l10n.localHostListening(existing.appListenAddr), style: small),
        )
        ..add(
          SelectableText(
            existing.appServiceKey,
            style: small?.copyWith(color: scheme.onSurfaceVariant),
          ),
        )
        ..add(const SizedBox(height: 8))
        ..add(Text(l10n.tunnelApiLabel, style: small))
        ..add(
          Text(l10n.localHostListening(existing.apiListenAddr), style: small),
        )
        ..add(
          SelectableText(
            existing.apiServiceKey,
            style: small?.copyWith(color: scheme.onSurfaceVariant),
          ),
        )
        // Runtime details for the external Codex process.
        ..add(const Divider(height: 24))
        ..add(
          Text(
            l10n.hostRuntimeInfo,
            style: small?.copyWith(fontWeight: FontWeight.w600),
          ),
        )
        ..add(const SizedBox(height: 4))
        ..add(
          Text(
            '${l10n.hostRuntimeMode}: '
            '${l10n.codexSourceExternal}',
            style: small,
          ),
        )
        ..add(
          SelectableText(
            '${l10n.hostCodexPath}: ${existing.codexBinary ?? '—'}',
            style: small?.copyWith(color: scheme.onSurfaceVariant),
          ),
        )
        ..add(
          Text(
            '${l10n.hostProxyLabel}: ${existing.proxy ?? l10n.hostProxyInherit}',
            style: small,
          ),
        )
        // Project folders: the roots a phone's folder browser is confined to,
        // and the default new conversations open in. Configured on the host.
        ..add(const Divider(height: 24))
        ..add(ProjectFoldersEditor(serviceKey: existing.appServiceKey));
    } else {
      children.add(const SizedBox(height: 16));
      children
        ..add(
          SegmentedButton<bool>(
            segments: [
              ButtonSegment(
                value: false,
                label: Text(l10n.codexSourceExternal),
              ),
              ButtonSegment(
                value: true,
                enabled: false,
                label: Text(l10n.codexSourceBuiltin),
              ),
            ],
            selected: const {false},
            onSelectionChanged: _busy ? null : (_) {},
          ),
        )
        ..add(const SizedBox(height: 8))
        ..add(Text(l10n.codexBuiltinNote, style: small))
        ..add(const SizedBox(height: 12));
      if (!_codexChecked) {
        children.add(const LinearProgressIndicator());
      } else if (_codexFound && !_overridePath) {
        children.add(_codexAvailable());
      } else {
        if (!_codexFound) {
          children
            ..add(
              Row(
                children: [
                  Expanded(
                    child: Text(
                      l10n.codexNotFound,
                      style: TextStyle(color: scheme.error),
                    ),
                  ),
                  // Installed codex just now? Re-detect instead of typing a path.
                  TextButton.icon(
                    key: const Key('redetect-codex-btn'),
                    onPressed: _busy ? null : _detectCodex,
                    icon: const Icon(Icons.refresh, size: 16),
                    label: Text(l10n.codexRedetect),
                  ),
                ],
              ),
            )
            ..add(const SizedBox(height: 8));
        }
        children.add(
          Row(
            crossAxisAlignment: CrossAxisAlignment.end,
            children: [
              Expanded(
                child: TextField(
                  key: const Key('codex-path-field'),
                  controller: _path,
                  decoration: InputDecoration(labelText: l10n.codexBinaryPath),
                ),
              ),
              const SizedBox(width: 8),
              OutlinedButton(
                key: const Key('browse-codex-btn'),
                onPressed: _busy ? null : _browseCodex,
                child: Text(l10n.chooseCodexPath),
              ),
            ],
          ),
        );
      }
      // --- port + name ---
      children
        ..add(const SizedBox(height: 12))
        ..add(
          TextField(
            controller: _port,
            decoration: InputDecoration(labelText: l10n.localHostPort),
            keyboardType: TextInputType.number,
          ),
        )
        ..add(const SizedBox(height: 12))
        ..add(
          TextField(
            controller: _name,
            decoration: InputDecoration(labelText: l10n.localHostName),
          ),
        )
        // --- proxy (mandatory unless turned off) ---
        ..add(
          SwitchListTile(
            key: const Key('use-proxy-switch'),
            contentPadding: EdgeInsets.zero,
            title: Text(l10n.useProxy),
            value: _useProxy,
            onChanged: _busy ? null : (v) => setState(() => _useProxy = v),
          ),
        );
      if (_useProxy) {
        children.add(
          TextField(
            key: const Key('proxy-field'),
            controller: _proxy,
            decoration: InputDecoration(labelText: l10n.proxyLabel),
          ),
        );
      } else {
        children.add(
          Text(
            l10n.noProxyWarning,
            style: TextStyle(color: cautionColor(scheme)),
          ),
        );
      }
    }
    return _dialog(children);
  }

  Widget _codexAvailable() {
    final l10n = AppLocalizations.of(context);
    final theme = Theme.of(context);
    final status = Row(
      children: [
        Icon(
          Icons.check_circle,
          size: 18,
          color: successColor(theme.colorScheme),
        ),
        const SizedBox(width: 6),
        Expanded(
          child: Text(
            l10n.codexFoundAt(_codexPath!),
            style: theme.textTheme.bodySmall,
            overflow: TextOverflow.ellipsis,
          ),
        ),
      ],
    );
    final customize = TextButton(
      key: const Key('customize-codex-btn'),
      onPressed: _busy ? null : () => setState(() => _overridePath = true),
      child: Text(l10n.customizeCodexPath),
    );
    return LayoutBuilder(
      builder: (context, constraints) {
        if (constraints.maxWidth < 320) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [status, customize],
          );
        }
        return Row(
          children: [
            Expanded(child: status),
            customize,
          ],
        );
      },
    );
  }

  /// The dialog frame shared by every provider: [children], the error line,
  /// and Cancel plus Start / Stop. [wide] gives the argument editor room on
  /// desktop; phones clamp the width to the screen either way.
  Widget _dialog(List<Widget> children, {bool wide = false}) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final existing = widget.existing;
    if (_error != null) {
      children
        ..add(const SizedBox(height: 12))
        ..add(Text(_error!, style: TextStyle(color: scheme.error)));
    }
    return AlertDialog(
      title: Text(l10n.localHostDialogTitle),
      content: SizedBox(
        width: wide ? 460 : 380,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: children,
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: _busy ? null : () => Navigator.of(context).pop(),
          child: Text(l10n.cancel),
        ),
        if (existing != null)
          FilledButton(
            key: const Key('stop-hosting-btn'),
            onPressed: _busy ? null : _stop,
            child: Text(l10n.stopHosting),
          )
        else
          FilledButton(
            key: const Key('start-hosting-btn'),
            onPressed: _busy
                ? null
                : switch (_provider) {
                    'opencode' => _startOpenCode,
                    'acp' => _startAcp,
                    _ => _start,
                  },
            child: Text(l10n.startHosting),
          ),
      ],
    );
  }
}
