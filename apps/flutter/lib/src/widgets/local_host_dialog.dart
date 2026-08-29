import 'dart:io' show Platform;

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/widgets/project_folders_editor.dart';

/// Manage one local host. With [existing] set it shows that host's listen
/// address + service key and a Stop button. Otherwise it's the "new host" form
/// (codex path, port, instance name, proxy) with a Start button. codex is
/// auto-detected (with a "change path" override) or picked when not on PATH.
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
  // Codex source: false = external codex (auto-detect/path, the default);
  // true = the app's built-in in-process app-server (desktop self-contained).
  bool _embedded = false;
  bool _busy = false;
  String? _error;
  // The built-in codex commit for a RUNNING embedded host (its "version"),
  // loaded lazily in initState. Null until loaded / for an external host.
  String? _embeddedVersion;

  bool get _isExisting => widget.existing != null;
  bool get _codexFound => _codexPath != null;
  // The built-in (in-process) codex ships only in the Windows + macOS desktop
  // builds (Linux desktop uses the external path — see the bridge's target-cfg).
  bool get _embeddedAvailable => Platform.isWindows || Platform.isMacOS;

  @override
  void initState() {
    super.initState();
    if (_isExisting) {
      // Running host: load the built-in codex commit (its version) for the
      // details panel when this host runs the embedded codex.
      if (widget.existing!.embedded) {
        Future.microtask(() async {
          final v = await ref.read(bridgeApiProvider).embeddedCodexVersion();
          if (mounted) setState(() => _embeddedVersion = v);
        });
      }
      return;
    }
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
    // codex source. Built-in (in-process) needs no binary. External: auto-
    // detected and not overridden → let the bridge resolve it; otherwise the
    // path the user typed / picked.
    String? override;
    if (!_embedded) {
      final manual = !_codexFound || _overridePath;
      final o = manual ? _path.text.trim() : '';
      if (manual && o.isEmpty) {
        setState(() => _error = l10n.codexPathRequired);
        return;
      }
      override = o.isEmpty ? null : o;
    }
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
            embedded: _embedded,
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
              embedded: _embedded,
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

  Future<void> _stop() async {
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
    final children = <Widget>[Text(l10n.localHostHint)];
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
        // Runtime details: built-in (in-process) vs external codex, its version
        // (the fork commit for the built-in one), and the active upstream proxy.
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
            '${existing.embedded ? l10n.codexSourceBuiltin : l10n.codexSourceExternal}',
            style: small,
          ),
        )
        ..add(
          existing.embedded
              ? Text(
                  '${l10n.hostCodexVersion}: '
                  '${_embeddedVersion == null ? '…' : 'fork @$_embeddedVersion'}',
                  style: small?.copyWith(color: scheme.onSurfaceVariant),
                )
              : SelectableText(
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
      // --- codex source: built-in (in-process) vs external (desktop only) ---
      if (_embeddedAvailable) {
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
                  label: Text(l10n.codexSourceBuiltin),
                ),
              ],
              selected: {_embedded},
              onSelectionChanged: _busy
                  ? null
                  : (s) => setState(() => _embedded = s.first),
            ),
          )
          ..add(const SizedBox(height: 12));
      }
      // --- codex availability (external only) ---
      if (_embedded) {
        children.add(
          Row(
            children: [
              Icon(Icons.bolt, size: 18, color: scheme.primary),
              const SizedBox(width: 6),
              Expanded(child: Text(l10n.codexBuiltinNote, style: small)),
            ],
          ),
        );
      } else if (!_codexChecked) {
        children.add(const LinearProgressIndicator());
      } else if (_codexFound && !_overridePath) {
        children.add(
          Row(
            children: [
              Icon(Icons.check_circle, size: 18, color: successColor(scheme)),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  l10n.codexFoundAt(_codexPath!),
                  style: small,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              TextButton(
                key: const Key('customize-codex-btn'),
                onPressed: _busy
                    ? null
                    : () => setState(() => _overridePath = true),
                child: Text(l10n.customizeCodexPath),
              ),
            ],
          ),
        );
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
    if (_error != null) {
      children
        ..add(const SizedBox(height: 12))
        ..add(Text(_error!, style: TextStyle(color: scheme.error)));
    }
    return AlertDialog(
      title: Text(l10n.localHostDialogTitle),
      content: SizedBox(
        width: 380,
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
            onPressed: _busy ? null : _start,
            child: Text(l10n.startHosting),
          ),
      ],
    );
  }
}
