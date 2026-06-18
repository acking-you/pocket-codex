import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart'
    show appLocalPort;
import 'package:pocket_codex/src/widgets/loading.dart';
import 'package:pocket_codex/src/widgets/status_dots.dart';

/// App-server service detail: connect over the relay, then list threads and
/// open or start a remote-control conversation.
class AppServiceScreen extends ConsumerStatefulWidget {
  /// Creates the screen for [serviceKey] (`pcx:<device>:app:<name>`).
  const AppServiceScreen({super.key, required this.serviceKey});

  /// Full relay key of the app-server service.
  final String serviceKey;

  @override
  ConsumerState<AppServiceScreen> createState() => _AppServiceState();
}

class _AppServiceState extends ConsumerState<AppServiceScreen> {
  bool _connecting = true;
  String? _error;
  List<ThreadMeta> _threads = const [];

  @override
  void initState() {
    super.initState();
    _connect();
  }

  // NOTE: we deliberately do NOT disconnect on dispose. Sessions are kept
  // alive for the app's lifetime so leaving and returning is instant and any
  // turn running in the background keeps streaming (async / resumable). The
  // host `codex app-server` keeps the turn running regardless, and on a cold
  // app restart conversations are recovered from disk via resume + read.

  Future<void> _connect() async {
    setState(() {
      _connecting = true;
      _error = null;
    });
    // Never attempt a connect to a backend the reachability probe says is dead
    // (a live relay registrant whose codex app-server is gone) — that just sits
    // on "connecting" until it times out, then retries. Await the probe's
    // future rather than a maybe-empty snapshot: it resolves instantly when the
    // services list already probed it (the common path) and is bounded by the
    // probe timeout otherwise. A probe error counts as unreachable. The retry
    // button re-probes, so a recovered backend still connects.
    var reachable = false;
    try {
      reachable = await ref.read(
        appReachableProvider(widget.serviceKey).future,
      );
    } catch (_) {
      // Probe failed (e.g. relay not configured) → treat as unreachable.
    }
    if (!mounted) return;
    if (!reachable) {
      setState(() {
        // Explain *why*: it's in the list, so the relay registration is up —
        // the dead link is the remote app-server backend itself.
        _error = AppLocalizations.of(context).unreachableReason;
        _connecting = false;
      });
      return;
    }
    final api = ref.read(bridgeApiProvider);
    try {
      await api.appConnect(widget.serviceKey, appLocalPort);
      final threads = await api.appThreadList(widget.serviceKey);
      if (!mounted) return;
      setState(() {
        _threads = threads;
        _connecting = false;
      });
    } catch (_) {
      // The service shows online (registered on the relay) but the socket may
      // be stale/half-open — a reused dead session fails with "closed
      // connection". Force one clean reconnect before surfacing the error.
      try {
        await api.appDisconnect(widget.serviceKey);
        await api.appConnect(widget.serviceKey, appLocalPort);
        final threads = await api.appThreadList(widget.serviceKey);
        if (!mounted) return;
        setState(() {
          _threads = threads;
          _connecting = false;
        });
      } catch (e) {
        if (!mounted) return;
        setState(() {
          _error = friendlyError(e);
          _connecting = false;
        });
      }
    }
  }

  /// Manual "refresh status": tear the session down and re-establish it, then
  /// reload threads. Unlike [_connect] (which reuses a live session), this
  /// always reconnects — the recovery for a stalled/slow tunnel.
  Future<void> _reconnect() async {
    setState(() {
      _connecting = true;
      _error = null;
    });
    final api = ref.read(bridgeApiProvider);
    try {
      await api.appDisconnect(widget.serviceKey);
      await api.appConnect(widget.serviceKey, appLocalPort);
      final threads = await api.appThreadList(widget.serviceKey);
      if (!mounted) return;
      setState(() {
        _threads = threads;
        _connecting = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = friendlyError(e);
        _connecting = false;
      });
    }
  }

  Future<void> _refreshThreads() async {
    try {
      final threads = await ref
          .read(bridgeApiProvider)
          .appThreadList(widget.serviceKey);
      if (mounted) setState(() => _threads = threads);
    } catch (_) {
      // Keep the existing list on a transient refresh failure.
    }
  }

  Future<void> _open({String? threadId, String? cwd}) async {
    final key = Uri.encodeComponent(widget.serviceKey);
    // Trim the folder path so a whitespace-only cwd isn't passed through as a
    // real working directory.
    final trimmedCwd = cwd?.trim();
    final q = <String>[
      if (threadId != null) 'tid=${Uri.encodeComponent(threadId)}',
      if (trimmedCwd != null && trimmedCwd.isNotEmpty)
        'cwd=${Uri.encodeComponent(trimmedCwd)}',
    ];
    final uri = '/app/$key/session${q.isEmpty ? '' : '?${q.join('&')}'}';
    await context.push(uri);
    await _refreshThreads();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    // Threads with an in-flight turn (live, derived from the event stream) so
    // running sessions are visible in the tree before they're opened.
    final running =
        ref.watch(runningThreadsProvider(widget.serviceKey)).valueOrNull ??
        const <String>{};
    return Scaffold(
      appBar: AppBar(
        title: Text(l10n.appServiceTitle),
        actions: [
          if (running.isNotEmpty)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 14),
              child: Center(
                child: StatusChip(
                  color: Theme.of(context).colorScheme.primary,
                  label: l10n.runningSessions(running.length),
                  pulsing: true,
                ),
              ),
            ),
          IconButton(
            key: const Key('reconnect-btn'),
            icon: const Icon(Icons.refresh),
            tooltip: l10n.refreshStatus,
            // Force a clean reconnect + reload (recovers a slow/stale tunnel).
            onPressed: _connecting ? null : _reconnect,
          ),
        ],
      ),
      body: AnimatedSwitcher(
        duration: const Duration(milliseconds: 250),
        child: _buildBody(l10n, running),
      ),
      floatingActionButton: (_connecting || _error != null)
          ? null
          : FloatingActionButton.extended(
              key: const Key('new-conversation-btn'),
              onPressed: () => _open(),
              icon: const Icon(Icons.add_comment),
              label: Text(l10n.newConversation),
            ),
    );
  }

  Widget _buildBody(AppLocalizations l10n, Set<String> running) {
    if (_connecting) {
      return const ListLoadingSkeleton(key: ValueKey('app-loading'));
    }
    if (_error != null) {
      return Center(
        key: const ValueKey('app-error'),
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(l10n.connectFailed, textAlign: TextAlign.center),
              const SizedBox(height: 8),
              Text(
                _error!,
                key: const Key('app-connect-error'),
                textAlign: TextAlign.center,
                style: Theme.of(context).textTheme.bodySmall,
              ),
              const SizedBox(height: 12),
              FilledButton(
                // Re-probe so a now-recovered backend can connect instead of
                // fast-failing on the stale "unreachable" result.
                onPressed: () {
                  ref.invalidate(appReachableProvider(widget.serviceKey));
                  _connect();
                },
                child: Text(l10n.retry),
              ),
            ],
          ),
        ),
      );
    }
    if (_threads.isEmpty) {
      return RefreshIndicator(
        key: const ValueKey('app-empty'),
        onRefresh: _refreshThreads,
        child: ListView(
          children: [
            const SizedBox(height: 120),
            Center(child: Text(l10n.noThreads)),
          ],
        ),
      );
    }
    // Build a collapsible folder tree from thread cwds (hierarchical,
    // collapsible project view).
    final root = _buildTree(_threads);
    return RefreshIndicator(
      key: const ValueKey('app-tree'),
      onRefresh: _refreshThreads,
      child: ListView(
        padding: const EdgeInsets.symmetric(vertical: 8),
        children: _nodeWidgets(root, l10n, 0, running),
      ),
    );
  }

  /// How many threads in [node]'s subtree currently have a running turn.
  int _runningCount(_Node node, Set<String> running) {
    var n = node.threads.where((t) => running.contains(t.id)).length;
    for (final c in node.children.values) {
      n += _runningCount(c, running);
    }
    return n;
  }

  /// Build a path tree from cwds, then collapse single-child chains.
  _Node _buildTree(List<ThreadMeta> threads) {
    final root = _Node('', '');
    for (final t in threads) {
      final cwd = t.cwd.trim();
      if (cwd.isEmpty) {
        root.children.putIfAbsent('', () => _Node('', '')).threads.add(t);
        continue;
      }
      final unix = cwd.startsWith('/');
      final sep = cwd.contains('\\') ? '\\' : '/';
      final segs = cwd.split(RegExp(r'[\\/]'))..removeWhere((s) => s.isEmpty);
      var node = root;
      var prefix = '';
      for (final seg in segs) {
        prefix = prefix.isEmpty ? (unix ? '/$seg' : seg) : '$prefix$sep$seg';
        node = node.children.putIfAbsent(seg, () => _Node(seg, prefix));
      }
      node.threads.add(t);
    }
    _compress(root);
    return root;
  }

  /// Merge folder nodes with a single child and no threads of their own, so
  /// chains like `C:` › `Users` › `me` render as one `C:\Users\me` node.
  void _compress(_Node node) {
    final merged = <String, _Node>{};
    for (var child in node.children.values) {
      _compress(child);
      while (child.threads.isEmpty && child.children.length == 1) {
        final only = child.children.values.first;
        final sep = only.fullPath.contains('\\') ? '\\' : '/';
        only.name = '${child.name}$sep${only.name}';
        child = only;
      }
      merged[child.name] = child;
    }
    node.children
      ..clear()
      ..addAll(merged);
  }

  List<Widget> _nodeWidgets(
    _Node node,
    AppLocalizations l10n,
    int depth,
    Set<String> running,
  ) {
    final scheme = Theme.of(context).colorScheme;
    final kids = node.children.values.toList()
      ..sort((a, b) => a.name.toLowerCase().compareTo(b.name.toLowerCase()));
    final widgets = <Widget>[];
    for (final child in kids) {
      final label = child.name.isEmpty ? l10n.defaultFolder : child.name;
      // Surface running sessions at the folder level too, so a collapsed
      // project still shows that something inside it is active.
      final runCount = _runningCount(child, running);
      widgets.add(
        ExpansionTile(
          key: PageStorageKey('proj-${child.fullPath}-$label'),
          initiallyExpanded: depth == 0,
          tilePadding: EdgeInsets.only(left: 16.0 + depth * 14, right: 4),
          childrenPadding: EdgeInsets.zero,
          leading: Icon(Icons.folder_outlined, color: scheme.primary),
          title: Row(
            children: [
              Expanded(
                child: Text(
                  label,
                  style: Theme.of(context).textTheme.titleSmall,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              if (runCount > 0) ...[
                StatusChip(
                  color: scheme.primary,
                  label: l10n.runningSessions(runCount),
                  pulsing: true,
                ),
                const SizedBox(width: 4),
              ],
              IconButton(
                icon: const Icon(Icons.add, size: 20),
                tooltip: l10n.newConversation,
                onPressed: () => _open(cwd: child.fullPath),
              ),
            ],
          ),
          children: [
            ...child.threads.map(
              (t) => Padding(
                padding: EdgeInsets.only(left: 16.0 + (depth + 1) * 14),
                child: ListTile(
                  key: Key('thread-${t.id}'),
                  dense: true,
                  leading: const Icon(Icons.chat_bubble_outline, size: 18),
                  title: Text(
                    t.preview.trim().isEmpty
                        ? l10n.untitledThread
                        : t.preview.trim(),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                  trailing: running.contains(t.id)
                      ? PulsingDot(color: scheme.primary)
                      : null,
                  onTap: () => _open(threadId: t.id, cwd: t.cwd),
                ),
              ),
            ),
            ..._nodeWidgets(child, l10n, depth + 1, running),
          ],
        ),
      );
    }
    return widgets;
  }
}

/// One folder node in the project tree.
class _Node {
  _Node(this.name, this.fullPath);
  String name;
  final String fullPath;
  final Map<String, _Node> children = {};
  final List<ThreadMeta> threads = [];
}
