import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart'
    show appLocalPort;
import 'package:pocket_codex/src/time_ago.dart';
import 'package:pocket_codex/src/widgets/loading.dart';
import 'package:pocket_codex/src/widgets/status_dots.dart';

/// App-server service detail: connect over the relay, then list threads grouped
/// by project and open or start a remote-control conversation.
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
  String _query = '';

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
    return _buildProjectList(l10n, running);
  }

  /// Threads grouped by project (cwd), each project ordered by its most recent
  /// activity, mirroring the local-sessions language: a lightweight folder
  /// header + lean conversation rows, with drive dividers only when projects
  /// actually span more than one drive.
  Widget _buildProjectList(AppLocalizations l10n, Set<String> running) {
    final scheme = Theme.of(context).colorScheme;
    final now = DateTime.now();
    final q = _query.trim().toLowerCase();
    bool matches(ThreadMeta t) =>
        t.preview.toLowerCase().contains(q) || t.cwd.toLowerCase().contains(q);
    final filtered = q.isEmpty ? _threads : _threads.where(matches).toList();

    // Group by project; sort threads (and then projects) newest-first.
    final projects = <String, List<ThreadMeta>>{};
    for (final t in filtered) {
      projects.putIfAbsent(t.cwd.trim(), () => <ThreadMeta>[]).add(t);
    }
    for (final ts in projects.values) {
      ts.sort((a, b) => b.updatedAt.compareTo(a.updatedAt));
    }
    int recent(List<ThreadMeta> ts) =>
        ts.fold(0, (m, t) => t.updatedAt > m ? t.updatedAt : m);
    final ordered = projects.entries.toList()
      ..sort((a, b) => recent(b.value).compareTo(recent(a.value)));

    // Drive headers only earn their keep when projects span >1 drive.
    final drives = ordered
        .map((e) => _drive(e.key))
        .where((d) => d.isNotEmpty)
        .toSet();
    final showDrives = drives.length > 1;

    final rows = <Widget>[];
    String? lastDrive;
    for (final entry in ordered) {
      final d = _drive(entry.key);
      if (showDrives && d.isNotEmpty && d != lastDrive) {
        rows.add(_driveDivider(d));
        lastDrive = d;
      }
      rows.add(_projectHeader(entry.key, entry.value, running, l10n));
      rows.addAll(entry.value.map((t) => _threadRow(t, running, now, l10n)));
    }

    return Column(
      key: const ValueKey('app-tree'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // Quick filter — shown once there are enough threads to scan.
        if (_threads.length > 6) _searchBox(l10n),
        Expanded(
          child: filtered.isEmpty
              ? Center(
                  key: const ValueKey('app-no-match'),
                  child: Text(
                    l10n.noMatchingThreads,
                    style: TextStyle(color: scheme.outline),
                  ),
                )
              : RefreshIndicator(
                  onRefresh: _refreshThreads,
                  child: ListView(
                    // Bottom room so the extended FAB never covers the last row.
                    padding: const EdgeInsets.only(top: 4, bottom: 88),
                    children: rows,
                  ),
                ),
        ),
      ],
    );
  }

  /// A folder header for one project: its name, full path, a running badge, and
  /// the only "+" (start a conversation in this cwd) — rows below stay clean.
  Widget _projectHeader(
    String cwd,
    List<ThreadMeta> threads,
    Set<String> running,
    AppLocalizations l10n,
  ) {
    final scheme = Theme.of(context).colorScheme;
    final leaf = _leaf(cwd);
    final label = leaf.isEmpty ? l10n.defaultFolder : leaf;
    final path = cwd.trim();
    final showPath = path.isNotEmpty && path != label;
    final runCount = threads.where((t) => running.contains(t.id)).length;
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 12, 4, 2),
      child: Row(
        children: [
          Icon(Icons.folder_outlined, size: 19, color: scheme.primary),
          const SizedBox(width: 9),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  label,
                  style: Theme.of(context).textTheme.titleSmall,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
                if (showPath)
                  Text(
                    path,
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: scheme.onSurfaceVariant,
                    ),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
              ],
            ),
          ),
          if (runCount > 0) ...[
            StatusChip(
              color: scheme.primary,
              label: l10n.runningSessions(runCount),
              pulsing: true,
            ),
            const SizedBox(width: 2),
          ],
          IconButton(
            icon: const Icon(Icons.add, size: 20),
            tooltip: l10n.newConversation,
            visualDensity: VisualDensity.compact,
            onPressed: () => _open(cwd: cwd),
          ),
        ],
      ),
    );
  }

  /// One conversation row: indented under its project, preview + relative-time,
  /// and a pulse when a turn is live.
  Widget _threadRow(
    ThreadMeta t,
    Set<String> running,
    DateTime now,
    AppLocalizations l10n,
  ) {
    final scheme = Theme.of(context).colorScheme;
    final isRunning = running.contains(t.id);
    final preview = t.preview.trim();
    final time = relativeTime(t.updatedAt, now, l10n);
    return ListTile(
      key: Key('thread-${t.id}'),
      dense: true,
      visualDensity: VisualDensity.compact,
      contentPadding: const EdgeInsets.only(left: 40, right: 14),
      horizontalTitleGap: 8,
      minLeadingWidth: 0,
      leading: Icon(
        Icons.chat_bubble_outline,
        size: 17,
        color: scheme.onSurfaceVariant,
      ),
      title: Text(
        preview.isEmpty ? l10n.untitledThread : preview,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
      ),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (time.isNotEmpty)
            Text(
              time,
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: scheme.onSurfaceVariant),
            ),
          if (isRunning) ...[
            const SizedBox(width: 8),
            PulsingDot(color: scheme.primary),
          ],
        ],
      ),
      onTap: () => _open(threadId: t.id, cwd: t.cwd),
    );
  }

  Widget _driveDivider(String drive) => Padding(
    padding: const EdgeInsets.fromLTRB(14, 14, 14, 0),
    child: Text(
      drive,
      style: TextStyle(
        fontSize: 11,
        fontWeight: FontWeight.w600,
        letterSpacing: .5,
        color: Theme.of(context).colorScheme.onSurfaceVariant,
      ),
    ),
  );

  Widget _searchBox(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 8, 12, 4),
      child: TextField(
        key: const Key('app-thread-search'),
        onChanged: (v) => setState(() => _query = v),
        style: const TextStyle(fontSize: 13),
        decoration: InputDecoration(
          isDense: true,
          prefixIcon: const Icon(Icons.search, size: 18),
          prefixIconConstraints: const BoxConstraints(
            minWidth: 34,
            minHeight: 34,
          ),
          hintText: l10n.searchConversations,
          hintStyle: TextStyle(fontSize: 13, color: scheme.onSurfaceVariant),
          filled: true,
          fillColor: scheme.surfaceContainerHighest,
          contentPadding: const EdgeInsets.symmetric(vertical: 9),
          border: OutlineInputBorder(
            borderRadius: BorderRadius.circular(10),
            borderSide: BorderSide.none,
          ),
        ),
      ),
    );
  }

  /// Drive prefix of [cwd] (`C:` / `D:` / `/` for unix), or empty when unknown.
  String _drive(String cwd) {
    final c = cwd.trim();
    if (c.isEmpty) return '';
    if (c.startsWith('/')) return '/';
    final m = RegExp(r'^([A-Za-z]:)').firstMatch(c);
    // `m?.` null-shorts the whole chain, so this is already crash-safe; `?.`
    // (over `!`) just keeps it bang-free and equally clear.
    return m?.group(1)?.toUpperCase() ?? '';
  }

  /// Leaf folder name of [cwd] (the project's own directory), or empty.
  String _leaf(String cwd) {
    final c = cwd.trim();
    if (c.isEmpty) return '';
    final segs = c.split(RegExp(r'[\\/]'))..removeWhere((s) => s.isEmpty);
    return segs.isEmpty ? c : segs.last;
  }
}
