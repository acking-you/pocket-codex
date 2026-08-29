import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/widgets/adaptive_sheet.dart';
import 'package:pocket_codex/src/widgets/app_toast.dart';
import 'package:pocket_codex/src/widgets/error_retry.dart';

/// Show the host file-transfer panel over [serviceKey]'s meta tunnel: browse
/// the host's configured project roots, download a host file to local disk, or
/// upload a local file into the current directory. Root-confined by the host
/// (a client can't free-roam the host filesystem). Bottom sheet — reachable on
/// a phone, fine on desktop. Desktop-only: the save/open dialogs it uses have
/// no mobile implementation, so callers gate the entry point on desktop.
Future<void> showFileBrowser(
  BuildContext context, {
  required String serviceKey,
}) => showAdaptivePanel<void>(
  context: context,
  // The body brings its own scrollable list.
  scrollable: false,
  // And its own header row, which is its top edge.
  insetTop: false,
  maxWidth: 720,
  builder: (_) => _FileBrowser(serviceKey: serviceKey),
);

class _FileBrowser extends ConsumerStatefulWidget {
  const _FileBrowser({required this.serviceKey});

  final String serviceKey;

  @override
  ConsumerState<_FileBrowser> createState() => _FileBrowserState();
}

class _FileBrowserState extends ConsumerState<_FileBrowser> {
  // Configured roots (fetched once). Empty until loaded / when none configured.
  List<String> _roots = const [];
  // Browse stack: the folders drilled into. Empty = at the roots list.
  final List<String> _stack = [];
  List<HostDirEntry> _dirs = const [];
  List<HostFileEntry> _files = const [];
  bool _loading = true;
  bool _busy = false; // a download/upload is in flight
  String? _error;

  /// The folder currently shown (null while at the roots list).
  String? get _current => _stack.isEmpty ? null : _stack.last;

  @override
  void initState() {
    super.initState();
    _loadRoots();
  }

  Future<void> _loadRoots() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final cfg = await ref
          .read(bridgeApiProvider)
          .metaProjectConfig(widget.serviceKey);
      if (!mounted) return;
      _roots = cfg.projectRoots;
      if (_roots.length == 1) {
        // A single root is a needless tap — drop straight into it.
        _stack.add(_roots.first);
        await _loadDir(_roots.first);
      } else {
        setState(() {
          _dirs = const [];
          _files = const [];
          _loading = false;
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _error = friendlyError(e);
          _loading = false;
        });
      }
    }
  }

  Future<void> _loadDir(String path) async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final api = ref.read(bridgeApiProvider);
      final dirs = await api.metaListDir(widget.serviceKey, path);
      final files = await api.metaListFiles(widget.serviceKey, path);
      if (!mounted) return;
      setState(() {
        _dirs = dirs;
        _files = files;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = friendlyError(e);
        _loading = false;
      });
    }
  }

  void _enter(String path) {
    setState(() => _stack.add(path));
    _loadDir(path);
  }

  Future<void> _up() async {
    if (_stack.isEmpty) return;
    setState(() => _stack.removeLast());
    final parent = _current;
    if (parent == null) {
      setState(() {
        _dirs = const [];
        _files = const [];
      });
    } else {
      await _loadDir(parent);
    }
  }

  String _leaf(String path) {
    final segs = path.split(RegExp(r'[\\/]'))..removeWhere((s) => s.isEmpty);
    return segs.isEmpty ? path : segs.last;
  }

  Future<void> _download(HostFileEntry file) async {
    final l10n = AppLocalizations.of(context);
    final messenger = ToastMessenger.of(context);
    setState(() => _busy = true);
    try {
      final bytes = await ref
          .read(bridgeApiProvider)
          .metaReadFile(widget.serviceKey, file.path);
      final location = await getSaveLocation(suggestedName: file.name);
      if (location == null) return;
      await File(location.path).writeAsBytes(bytes);
      messenger.ok(l10n.fileDownloaded(location.path));
    } catch (e) {
      messenger.error(l10n.fileDownloadFailed(friendlyError(e)));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _upload() async {
    final dir = _current;
    if (dir == null) return;
    final l10n = AppLocalizations.of(context);
    final messenger = ToastMessenger.of(context);
    final picked = await openFile();
    if (picked == null) return;
    setState(() => _busy = true);
    try {
      final bytes = await picked.readAsBytes();
      await ref
          .read(bridgeApiProvider)
          .metaWriteFile(widget.serviceKey, dir, picked.name, bytes);
      messenger.ok(l10n.fileUploaded(picked.name));
      await _loadDir(dir);
    } catch (e) {
      messenger.error(l10n.hostFileUploadFailed(friendlyError(e)));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final atRoots = _stack.isEmpty;
    final dirRows = atRoots
        ? [
            for (final r in _roots)
              HostDirEntry(name: _leaf(r), path: r, isGitRepo: false),
          ]
        : _dirs;

    // A plain bounded column, so this body works in BOTH forms the adaptive
    // panel opens: a centred dialog on desktop and a bottom sheet on a phone.
    // `DraggableScrollableSheet` only works inside a sheet route.
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 10, 8, 4),
          child: Row(
            children: [
              if (!atRoots)
                IconButton(
                  key: const Key('file-up-btn'),
                  icon: const Icon(Icons.arrow_back),
                  tooltip: l10n.folderUp,
                  onPressed: (_loading || _busy) ? null : _up,
                ),
              Expanded(
                child: Text(
                  _current == null ? l10n.hostFiles : _leaf(_current!),
                  style: Theme.of(context).textTheme.titleMedium,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              if (_busy)
                const Padding(
                  padding: EdgeInsets.only(right: 12),
                  child: SizedBox(
                    width: 18,
                    height: 18,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  ),
                ),
            ],
          ),
        ),
        if (_current != null)
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 0, 16, 6),
            child: Align(
              alignment: Alignment.centerLeft,
              child: Text(
                _current!,
                style: Theme.of(
                  context,
                ).textTheme.bodySmall?.copyWith(color: scheme.onSurfaceVariant),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
            ),
          ),
        const Divider(height: 1),
        Expanded(child: _body(l10n, scheme, dirRows, null)),
        SafeArea(
          top: false,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(16, 8, 16, 12),
            child: Row(
              children: [
                TextButton(
                  onPressed: () => Navigator.of(context).pop(),
                  child: Text(l10n.cancel),
                ),
                const Spacer(),
                FilledButton.icon(
                  key: const Key('file-upload-btn'),
                  // Upload targets the current folder; disabled at the roots
                  // list (no folder chosen) or while busy.
                  onPressed: (_current == null || _busy) ? null : _upload,
                  icon: const Icon(Icons.upload_file, size: 18),
                  label: Text(l10n.fileUpload),
                ),
              ],
            ),
          ),
        ),
      ],
    );
  }

  Widget _body(
    AppLocalizations l10n,
    ColorScheme scheme,
    List<HostDirEntry> dirRows,
    ScrollController? scrollController,
  ) {
    if (_loading) {
      // Not a bare spinner: retries happen HERE, and the progress stream is
      // live-only, so something has to be watching while they run.
      return const LoadingWithRetry();
    }
    if (_error != null) {
      return ErrorRetry(
        message: _error!,
        errorKey: const Key('file-browser-error'),
        onRetry: () => _current == null ? _loadRoots() : _loadDir(_current!),
      );
    }
    if (_stack.isEmpty && _roots.isEmpty) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Text(
            l10n.folderPickerNoRoots,
            key: const Key('file-browser-no-roots'),
            textAlign: TextAlign.center,
            style: TextStyle(color: scheme.onSurfaceVariant),
          ),
        ),
      );
    }
    if (dirRows.isEmpty && _files.isEmpty) {
      return Center(
        child: Text(
          l10n.fileBrowserEmpty,
          style: TextStyle(color: scheme.onSurfaceVariant),
        ),
      );
    }
    // Directories first (drill in), then files (download).
    return ListView(
      controller: scrollController,
      children: [
        for (final d in dirRows)
          ListTile(
            key: Key('file-dir-${d.path}'),
            leading: Icon(
              d.isGitRepo ? Icons.source_outlined : Icons.folder_outlined,
              color: d.isGitRepo ? scheme.primary : scheme.onSurfaceVariant,
            ),
            title: Text(d.name, maxLines: 1, overflow: TextOverflow.ellipsis),
            trailing: const Icon(Icons.chevron_right, size: 20),
            onTap: (_loading || _busy) ? null : () => _enter(d.path),
          ),
        for (final f in _files)
          ListTile(
            key: Key('file-row-${f.path}'),
            leading: Icon(
              Icons.insert_drive_file_outlined,
              color: scheme.onSurfaceVariant,
            ),
            title: Text(f.name, maxLines: 1, overflow: TextOverflow.ellipsis),
            subtitle: Text(
              _fmtSize(f.size),
              style: TextStyle(fontSize: 11, color: scheme.onSurfaceVariant),
            ),
            trailing: IconButton(
              key: Key('file-download-${f.path}'),
              icon: const Icon(Icons.download_outlined),
              tooltip: l10n.fileDownload,
              onPressed: _busy ? null : () => _download(f),
            ),
          ),
      ],
    );
  }
}

/// Human-readable byte size (B / KB / MB / GB), one decimal past KB.
String _fmtSize(int bytes) {
  if (bytes < 1024) return '$bytes B';
  const units = ['KB', 'MB', 'GB', 'TB'];
  var size = bytes / 1024;
  var unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit++;
  }
  return '${size.toStringAsFixed(1)} ${units[unit]}';
}
