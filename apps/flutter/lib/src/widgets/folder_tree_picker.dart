import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/providers.dart';

/// Show the host project-folder browser and return the absolute host path the
/// user picks, or null if they cancel. Confined to the host's configured
/// project roots (a phone can't free-roam the host filesystem). Rendered as a
/// bottom sheet — thumb-reachable on a phone, fine on desktop.
Future<String?> showFolderPicker(
  BuildContext context, {
  required String serviceKey,
  String? initialPath,
}) => showModalBottomSheet<String>(
  context: context,
  isScrollControlled: true,
  useSafeArea: true,
  builder: (_) =>
      _FolderTreePicker(serviceKey: serviceKey, initialPath: initialPath),
);

/// The browser body: drill the host's project tree from the configured roots.
class _FolderTreePicker extends ConsumerStatefulWidget {
  const _FolderTreePicker({required this.serviceKey, this.initialPath});

  final String serviceKey;
  final String? initialPath;

  @override
  ConsumerState<_FolderTreePicker> createState() => _FolderTreePickerState();
}

class _FolderTreePickerState extends ConsumerState<_FolderTreePicker> {
  // Configured roots (fetched once). Empty until loaded / when none configured.
  List<String> _roots = const [];
  // Browse stack: the chain of folders drilled into. Empty = at the roots list.
  final List<String> _stack = [];
  List<HostDirEntry> _entries = const [];
  bool _loading = true;
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
      // Open where the caller pointed us (the conversation's current folder),
      // when it sits inside a configured root — build the browse stack down to
      // it so it opens there and "up" walks back out. Falls through to the
      // default entry points when there's no usable initial path.
      if (_openAtInitialPath()) {
        await _loadDir(_stack.last);
      } else if (_roots.length == 1) {
        // A single root is a needless tap — drop straight into it.
        _stack.add(_roots.first);
        await _loadDir(_roots.first);
      } else {
        // Multiple roots (or none) start at the roots list.
        setState(() {
          _entries = const [];
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

  /// Seed [_stack] from [widget.initialPath] when it is a configured root or a
  /// folder inside one, so the browser opens at the conversation's current
  /// folder. Returns whether it seeded anything.
  bool _openAtInitialPath() {
    final start = widget.initialPath?.trim();
    if (start == null || start.isEmpty) return false;
    // Which root contains it? Match the root itself or a child (either
    // separator style, since the host may be Windows or Unix).
    final root = _roots.where(
      (r) => start == r || start.startsWith('$r/') || start.startsWith('$r\\'),
    );
    if (root.isEmpty) return false;
    final base = root.first;
    _stack.add(base);
    // Push each intermediate folder down to the initial path, rebuilding the
    // absolute path with the root's own separator so it round-trips as a real
    // host path.
    final sep = base.contains('\\') ? '\\' : '/';
    var cur = base;
    for (final seg
        in start
            .substring(base.length)
            .split(RegExp(r'[\\/]'))
            .where((s) => s.isNotEmpty)) {
      cur = '$cur$sep$seg';
      _stack.add(cur);
    }
    return true;
  }

  Future<void> _loadDir(String path) async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final entries = await ref
          .read(bridgeApiProvider)
          .metaListDir(widget.serviceKey, path);
      if (!mounted) return;
      setState(() {
        _entries = entries;
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

  /// Go up one level: to the parent folder, or back to the roots list.
  Future<void> _up() async {
    if (_stack.isEmpty) return;
    setState(() => _stack.removeLast());
    final parent = _current;
    if (parent == null) {
      setState(() => _entries = const []);
    } else {
      await _loadDir(parent);
    }
  }

  /// Leaf name of an absolute path for display (the root's own folder name).
  String _leaf(String path) {
    final segs = path.split(RegExp(r'[\\/]'))..removeWhere((s) => s.isEmpty);
    return segs.isEmpty ? path : segs.last;
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final atRoots = _stack.isEmpty;
    // At the roots list the "entries" are the configured roots themselves.
    final rows = atRoots
        ? [
            for (final r in _roots)
              HostDirEntry(name: _leaf(r), path: r, isGitRepo: false),
          ]
        : _entries;

    return DraggableScrollableSheet(
      expand: false,
      initialChildSize: 0.7,
      minChildSize: 0.4,
      maxChildSize: 0.95,
      builder: (context, scrollController) => Column(
        children: [
          // Grab handle + title.
          const SizedBox(height: 8),
          Container(
            width: 36,
            height: 4,
            decoration: BoxDecoration(
              color: scheme.outlineVariant,
              borderRadius: BorderRadius.circular(2),
            ),
          ),
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 10, 8, 4),
            child: Row(
              children: [
                if (!atRoots)
                  IconButton(
                    key: const Key('folder-up-btn'),
                    icon: const Icon(Icons.arrow_back),
                    tooltip: l10n.folderUp,
                    onPressed: _loading ? null : _up,
                  ),
                Expanded(
                  child: Text(
                    _current == null ? l10n.pickFolderTitle : _leaf(_current!),
                    style: Theme.of(context).textTheme.titleMedium,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
              ],
            ),
          ),
          // Current absolute path (so the user always knows where they are).
          if (_current != null)
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 0, 16, 6),
              child: Align(
                alignment: Alignment.centerLeft,
                child: Text(
                  _current!,
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: scheme.onSurfaceVariant,
                  ),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ),
          const Divider(height: 1),
          Expanded(child: _body(l10n, scheme, rows, scrollController)),
          // "Use this folder" — only meaningful once inside a folder.
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
                    key: const Key('folder-use-btn'),
                    onPressed: _current == null
                        ? null
                        : () => Navigator.of(context).pop(_current),
                    icon: const Icon(Icons.check, size: 18),
                    label: Text(l10n.useThisFolder),
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _body(
    AppLocalizations l10n,
    ColorScheme scheme,
    List<HostDirEntry> rows,
    ScrollController scrollController,
  ) {
    if (_loading) {
      return const Center(child: CircularProgressIndicator());
    }
    if (_error != null) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(
                _error!,
                key: const Key('folder-picker-error'),
                textAlign: TextAlign.center,
                style: TextStyle(color: scheme.error),
              ),
              const SizedBox(height: 12),
              FilledButton(
                onPressed: () =>
                    _current == null ? _loadRoots() : _loadDir(_current!),
                child: Text(l10n.retry),
              ),
            ],
          ),
        ),
      );
    }
    if (_stack.isEmpty && _roots.isEmpty) {
      // No roots configured on the host yet — tell the user where to set them.
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Text(
            l10n.folderPickerNoRoots,
            key: const Key('folder-picker-no-roots'),
            textAlign: TextAlign.center,
            style: TextStyle(color: scheme.onSurfaceVariant),
          ),
        ),
      );
    }
    if (rows.isEmpty) {
      return Center(
        child: Text(
          l10n.folderPickerEmpty,
          style: TextStyle(color: scheme.onSurfaceVariant),
        ),
      );
    }
    return ListView.builder(
      controller: scrollController,
      itemCount: rows.length,
      itemBuilder: (context, i) {
        final e = rows[i];
        return ListTile(
          key: Key('folder-row-${e.path}'),
          leading: Icon(
            e.isGitRepo ? Icons.source_outlined : Icons.folder_outlined,
            color: e.isGitRepo ? scheme.primary : scheme.onSurfaceVariant,
          ),
          title: Text(e.name, maxLines: 1, overflow: TextOverflow.ellipsis),
          subtitle: e.isGitRepo
              ? Text(
                  l10n.gitRepoLabel,
                  style: TextStyle(fontSize: 11, color: scheme.primary),
                )
              : null,
          trailing: const Icon(Icons.chevron_right, size: 20),
          onTap: _loading ? null : () => _enter(e.path),
        );
      },
    );
  }
}
