import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/providers.dart';

/// Desktop editor for a host's project folders: the roots a remote (phone)
/// folder browser is confined to, and which one new conversations default to.
/// Every change is persisted on the host via `metaSetProjectConfig` (loopback,
/// since the desktop editing this IS the host), so it is immediately visible to
/// every device that reaches this host.
///
/// Folders are picked with the native OS directory picker — this is a
/// desktop-only surface (a phone can't pick host folders; it browses the roots
/// this configures).
class ProjectFoldersEditor extends ConsumerStatefulWidget {
  /// Creates the editor for the host behind [serviceKey] (its app-server key).
  const ProjectFoldersEditor({super.key, required this.serviceKey});

  /// The host's app-server service key (`pcx:<device>:app:<name>`).
  final String serviceKey;

  @override
  ConsumerState<ProjectFoldersEditor> createState() =>
      _ProjectFoldersEditorState();
}

class _ProjectFoldersEditorState extends ConsumerState<ProjectFoldersEditor> {
  List<String> _roots = const [];
  String? _default;
  bool _loading = true;
  bool _busy = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final cfg = await ref
          .read(bridgeApiProvider)
          .metaProjectConfig(widget.serviceKey);
      if (!mounted) return;
      setState(() {
        _roots = cfg.projectRoots;
        _default = cfg.defaultProject;
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

  /// Persist the current roots + default on the host.
  Future<void> _save(List<String> roots, String? def) async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final cfg = await ref
          .read(bridgeApiProvider)
          .metaSetProjectConfig(widget.serviceKey, roots, def);
      if (!mounted) return;
      setState(() {
        _roots = cfg.projectRoots;
        _default = cfg.defaultProject;
        _busy = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = friendlyError(e);
        _busy = false;
      });
    }
  }

  Future<void> _addFolder() async {
    final dir = await getDirectoryPath();
    if (dir == null || !mounted) return;
    if (_roots.contains(dir)) return; // already a root
    final roots = [..._roots, dir];
    // First folder added becomes the default automatically.
    await _save(roots, _default ?? dir);
  }

  Future<void> _remove(String root) async {
    final roots = _roots.where((r) => r != root).toList();
    // Dropping the default clears it (or falls to the first remaining root).
    final def = _default == root
        ? (roots.isEmpty ? null : roots.first)
        : _default;
    await _save(roots, def);
  }

  Future<void> _setDefault(String root) => _save(_roots, root);

  /// Leaf folder name of an absolute path.
  String _leaf(String path) {
    final segs = path.split(RegExp(r'[\\/]'))..removeWhere((s) => s.isEmpty);
    return segs.isEmpty ? path : segs.last;
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Expanded(
              child: Text(
                l10n.projectFolders,
                style: Theme.of(context).textTheme.titleSmall,
              ),
            ),
            if (_busy || _loading)
              const SizedBox(
                width: 14,
                height: 14,
                child: CircularProgressIndicator(strokeWidth: 2),
              ),
          ],
        ),
        const SizedBox(height: 2),
        Text(
          l10n.projectFoldersHint,
          style: Theme.of(
            context,
          ).textTheme.bodySmall?.copyWith(color: scheme.onSurfaceVariant),
        ),
        const SizedBox(height: 8),
        if (_error != null)
          Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: Text(
              _error!,
              key: const Key('project-folders-error'),
              style: TextStyle(color: scheme.error, fontSize: 12),
            ),
          ),
        if (!_loading && _roots.isEmpty)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 6),
            child: Text(
              l10n.noProjectFolders,
              style: TextStyle(color: scheme.onSurfaceVariant),
            ),
          ),
        for (final root in _roots) _rootRow(l10n, scheme, root),
        const SizedBox(height: 6),
        Align(
          alignment: Alignment.centerLeft,
          child: OutlinedButton.icon(
            key: const Key('add-project-folder-btn'),
            onPressed: _busy ? null : _addFolder,
            icon: const Icon(Icons.create_new_folder_outlined, size: 18),
            label: Text(l10n.addProjectFolder),
          ),
        ),
      ],
    );
  }

  Widget _rootRow(AppLocalizations l10n, ColorScheme scheme, String root) {
    final isDefault = root == _default;
    return Padding(
      key: Key('project-root-$root'),
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Row(
        children: [
          // Default marker doubles as "set default": tap the star to promote.
          IconButton(
            key: Key('default-project-$root'),
            visualDensity: VisualDensity.compact,
            tooltip: isDefault ? l10n.defaultProjectFolder : l10n.setAsDefault,
            icon: Icon(
              isDefault ? Icons.star : Icons.star_border,
              size: 18,
              color: isDefault ? scheme.primary : scheme.onSurfaceVariant,
            ),
            onPressed: _busy || isDefault ? null : () => _setDefault(root),
          ),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  _leaf(root),
                  style: const TextStyle(fontWeight: FontWeight.w500),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
                Text(
                  root,
                  style: TextStyle(
                    fontSize: 11,
                    color: scheme.onSurfaceVariant,
                  ),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
              ],
            ),
          ),
          IconButton(
            key: Key('remove-project-$root'),
            visualDensity: VisualDensity.compact,
            tooltip: l10n.removeProjectFolder,
            icon: Icon(Icons.close, size: 18, color: scheme.error),
            onPressed: _busy ? null : () => _remove(root),
          ),
        ],
      ),
    );
  }
}
