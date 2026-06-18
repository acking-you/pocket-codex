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

/// Lists codex sessions under the shared `CODEX_HOME` — including ones created
/// by the desktop app / CLI / VS Code extension — annotated with whether each
/// is safe to resume, and lets the user force-take-over a finished session that
/// another process still holds open.
///
/// The listing and liveness reads run locally (no app-server connection
/// needed); the actual resume targets the first connected app-server service.
class LocalSessionsScreen extends ConsumerStatefulWidget {
  /// Default constructor.
  const LocalSessionsScreen({super.key});

  @override
  ConsumerState<LocalSessionsScreen> createState() => _LocalSessionsState();
}

class _LocalSessionsState extends ConsumerState<LocalSessionsScreen> {
  bool _loading = true;
  String? _error;
  List<LocalSession> _sessions = const [];
  String _query = '';

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
      final sessions = await ref.read(bridgeApiProvider).appLocalSessions();
      if (!mounted) return;
      setState(() {
        _sessions = sessions;
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

  Future<void> _onResume(LocalSession session) async {
    // A finished-but-held session needs the holder-listing confirm; pre-fetch
    // the current holders so the dialog can name them.
    var holders = const <Holder>[];
    if (session.requiresTakeover) {
      try {
        final liveness = await ref
            .read(bridgeApiProvider)
            .appSessionLiveness(session.threadId);
        holders = liveness.holders;
      } catch (_) {
        // Best-effort; the dialog still warns, just without names.
      }
    }
    if (!mounted) return;
    await resumeLocalSession(
      context,
      ref,
      threadId: session.threadId,
      cwd: session.cwd,
      requiresTakeover: session.requiresTakeover,
      holders: holders,
    );
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Scaffold(
      appBar: AppBar(
        title: Text(l10n.localSessionsTitle),
        actions: [
          IconButton(
            key: const Key('local-sessions-refresh'),
            icon: const Icon(Icons.refresh),
            tooltip: l10n.refreshStatus,
            onPressed: _loading ? null : _load,
          ),
        ],
      ),
      body: AnimatedSwitcher(
        duration: const Duration(milliseconds: 250),
        child: _buildBody(l10n),
      ),
    );
  }

  Widget _buildBody(AppLocalizations l10n) {
    if (_loading) {
      return const ListLoadingSkeleton(key: ValueKey('local-loading'));
    }
    if (_error != null) {
      return Center(
        key: const ValueKey('local-error'),
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(_error!, textAlign: TextAlign.center),
              const SizedBox(height: 12),
              FilledButton(onPressed: _load, child: Text(l10n.retry)),
            ],
          ),
        ),
      );
    }
    if (_sessions.isEmpty) {
      return RefreshIndicator(
        key: const ValueKey('local-empty'),
        onRefresh: _load,
        child: ListView(
          children: [
            const SizedBox(height: 120),
            Center(child: Text(l10n.noLocalSessions)),
          ],
        ),
      );
    }
    return _buildList(l10n);
  }

  Widget _buildList(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    final now = DateTime.now();
    final q = _query.trim().toLowerCase();
    bool matches(LocalSession s) {
      bool has(String? v) => v != null && v.toLowerCase().contains(q);
      return has(s.preview) || has(s.cwd) || has(s.source);
    }

    final filtered = q.isEmpty ? _sessions : _sessions.where(matches).toList();

    // Group by activity time, mirroring the conversation list: actively-running
    // first, then today, then earlier. The source list is already sorted
    // newest-first (scan_sessions orders by Reverse(updated_at)).
    final active = <LocalSession>[];
    final today = <LocalSession>[];
    final earlier = <LocalSession>[];
    for (final s in filtered) {
      if (s.safety == 'ownedRunning') {
        active.add(s);
      } else if (isSameDay(s.updatedAt, now)) {
        today.add(s);
      } else {
        earlier.add(s);
      }
    }

    final rows = <Widget>[
      Padding(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 12),
        child: Text(
          l10n.localSessionsHint,
          style: Theme.of(context).textTheme.bodySmall,
        ),
      ),
    ];
    void section(String label, List<LocalSession> items) {
      if (items.isEmpty) return;
      rows.add(_sectionLabel(label));
      rows.addAll(
        items.map(
          (s) => _SessionRow(session: s, now: now, onResume: _onResume),
        ),
      );
    }

    section(l10n.groupActive, active);
    section(l10n.groupToday, today);
    section(l10n.groupEarlier, earlier);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // Quick filter — shown once there are enough sessions to scan.
        if (_sessions.length > 6) _searchBox(l10n),
        Expanded(
          child: filtered.isEmpty
              ? Center(
                  key: const ValueKey('local-no-match'),
                  child: Text(
                    l10n.noMatchingThreads,
                    style: TextStyle(color: scheme.outline),
                  ),
                )
              : RefreshIndicator(
                  key: const ValueKey('local-list'),
                  onRefresh: _load,
                  child: ListView(
                    padding: const EdgeInsets.only(bottom: 8),
                    children: rows,
                  ),
                ),
        ),
      ],
    );
  }

  Widget _searchBox(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 8, 12, 4),
      child: TextField(
        key: const Key('local-search'),
        onChanged: (v) => setState(() => _query = v),
        style: const TextStyle(fontSize: 13),
        decoration: InputDecoration(
          isDense: true,
          prefixIcon: const Icon(Icons.search, size: 18),
          prefixIconConstraints: const BoxConstraints(
            minWidth: 34,
            minHeight: 34,
          ),
          hintText: l10n.searchLocalSessions,
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

  Widget _sectionLabel(String text) => Padding(
    padding: const EdgeInsets.fromLTRB(16, 12, 16, 2),
    child: Text(
      text,
      style: TextStyle(
        fontSize: 11.5,
        color: Theme.of(context).colorScheme.onSurfaceVariant,
      ),
    ),
  );
}

/// One session row: preview + cwd/source/time, a resume-safety chip, and a
/// resume / force-takeover action when allowed.
class _SessionRow extends StatelessWidget {
  const _SessionRow({
    required this.session,
    required this.now,
    required this.onResume,
  });

  final LocalSession session;
  final DateTime now;
  final Future<void> Function(LocalSession) onResume;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final preview = session.preview.trim();
    // Running sessions read "运行中"; everything else gets a relative-time tag.
    final time = session.safety == 'ownedRunning'
        ? l10n.running
        : relativeTime(session.updatedAt, now, l10n);
    final subtitleParts = <String>[
      if (session.cwd != null && session.cwd!.trim().isNotEmpty)
        session.cwd!.trim(),
      if (session.source != null && session.source!.isNotEmpty) session.source!,
      if (time.isNotEmpty) time,
    ];
    return ListTile(
      key: Key('local-${session.threadId}'),
      // Tapping any row opens the read-only transcript viewer (works even for
      // a session another client owns — it reads the on-disk rollout).
      onTap: () {
        final q = <String>[
          'tid=${Uri.encodeComponent(session.threadId)}',
          if (session.cwd != null && session.cwd!.trim().isNotEmpty)
            'cwd=${Uri.encodeComponent(session.cwd!.trim())}',
          if (preview.isNotEmpty) 'preview=${Uri.encodeComponent(preview)}',
        ];
        context.push('/sessions/view?${q.join('&')}');
      },
      leading: Icon(
        session.heldOpen ? Icons.lock_outline : Icons.chat_bubble_outline,
        size: 20,
        color: session.heldOpen ? scheme.error : scheme.primary,
      ),
      title: Text(
        preview.isEmpty ? session.threadId : preview,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
      ),
      subtitle: subtitleParts.isEmpty
          ? null
          : Text(
              subtitleParts.join('  ·  '),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          _safetyChip(context, l10n),
          const SizedBox(width: 8),
          if (session.allowsResume)
            TextButton(
              key: Key('resume-${session.threadId}'),
              onPressed: () => onResume(session),
              child: Text(
                session.requiresTakeover
                    ? l10n.forceTakeover
                    : l10n.resumeSession,
              ),
            ),
        ],
      ),
    );
  }

  Widget _safetyChip(BuildContext context, AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    final (Color color, String label) = switch (session.safety) {
      'resumable' => (Colors.green.shade600, l10n.sessionResumable),
      'resumableUnfinished' => (Colors.amber.shade800, l10n.sessionUnfinished),
      'ownedRunning' => (scheme.error, l10n.sessionRunningElsewhere),
      'ownedIdle' => (Colors.orange.shade700, l10n.sessionInUseElsewhere),
      _ => (scheme.outline, session.safety),
    };
    return StatusChip(
      color: color,
      label: label,
      pulsing: session.safety == 'ownedRunning',
    );
  }
}

/// First connected app-server service key (the resume target), or null when
/// none is connected yet. Shared by the sessions list and the read-only viewer.
String? connectedAppKey(WidgetRef ref) {
  final bridge = ref.read(bridgeApiProvider);
  final services = ref.read(servicesProvider).valueOrNull ?? const [];
  for (final s in services) {
    if (s.kind == 'app' && bridge.appIsConnected(s.key)) return s.key;
  }
  return null;
}

/// Resolve a *live* app-server to resume into, connecting one if necessary.
///
/// Prefers a service that is already connected; otherwise connects the first
/// reachable app-server. Crucially it first awaits `servicesProvider` — a bare
/// `connectedAppKey` reads `valueOrNull` and sees `[]` while relay discovery is
/// still in flight, which is why a plain resume used to report "no app-server"
/// while a force takeover (which awaits liveness first, giving discovery time)
/// happened to work. Returns null only when no app-server can be reached.
Future<String?> ensureResumeTarget(WidgetRef ref) async {
  // Let discovery resolve so the sync `connectedAppKey` below can see services.
  try {
    await ref.read(servicesProvider.future);
  } catch (_) {
    // Discovery failed — fall through; there may still be a live connection.
  }
  final already = connectedAppKey(ref);
  if (already != null) return already;

  // No live connection (e.g. the backend was restarted and the old socket
  // died): connect the first reachable app-server. `appConnect` reuses or
  // reconnects and is bounded by the connect timeout, so a dead one fails
  // rather than hanging.
  final bridge = ref.read(bridgeApiProvider);
  final services = ref.read(servicesProvider).valueOrNull ?? const [];
  for (final s in services.where((s) => s.kind == 'app')) {
    try {
      await bridge.appConnect(s.key, appLocalPort);
      return s.key;
    } catch (_) {
      // Unreachable / handshake failed — try the next app-server.
    }
  }
  return null;
}

/// Resume — or, for a finished session another process still holds, force-take-
/// over — a local session into a connected app-server, then open it live.
///
/// Shared by the sessions list and the read-only viewer. For a takeover it first
/// shows the holder-listing confirm dialog; on cancel it returns silently. On
/// success it opens the live conversation; failures surface as a snackbar. The
/// caller must ensure the session is not actively running (resume is disabled
/// for `ownedRunning`).
Future<void> resumeLocalSession(
  BuildContext context,
  WidgetRef ref, {
  required String threadId,
  String? cwd,
  required bool requiresTakeover,
  List<Holder> holders = const [],
}) async {
  final l10n = AppLocalizations.of(context);
  final messenger = ScaffoldMessenger.of(context);
  // Ensure a live app-server to resume into (connect one if needed) rather than
  // requiring the user to have opened an app-server first.
  final target = await ensureResumeTarget(ref);
  if (!context.mounted) return;

  if (requiresTakeover) {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (_) =>
          TakeoverDialog(holders: holders, hasTarget: target != null),
    );
    if (confirmed != true) return;
  }
  if (target == null) {
    messenger.showSnackBar(SnackBar(content: Text(l10n.takeoverNoTarget)));
    return;
  }

  ForceResumeReport report;
  try {
    report = await ref.read(bridgeApiProvider).appForceResume(target, threadId);
  } catch (e) {
    messenger.showSnackBar(SnackBar(content: Text(friendlyError(e))));
    return;
  }
  if (!report.resumed) {
    messenger.showSnackBar(
      SnackBar(
        content: Text(l10n.takeoverResumeFailed(report.resumeError ?? '')),
      ),
    );
    return;
  }
  final parts = <String>[
    l10n.takeoverResumed,
    if (report.killed.isNotEmpty) l10n.takeoverKilled(report.killed.length),
    if (report.stillHeld) l10n.takeoverStillHeld,
  ];
  messenger.showSnackBar(SnackBar(content: Text(parts.join(' · '))));

  // Open the resumed conversation (only reached on success).
  if (!context.mounted) return;
  final key = Uri.encodeComponent(target);
  final q = <String>[
    'tid=${Uri.encodeComponent(threadId)}',
    if (cwd != null && cwd.trim().isNotEmpty)
      'cwd=${Uri.encodeComponent(cwd.trim())}',
  ];
  context.push('/app/$key/session?${q.join('&')}');
}

/// Confirm dialog for a force takeover: lists the holder processes that will be
/// terminated and warns about data loss.
class TakeoverDialog extends StatelessWidget {
  /// Creates the confirm dialog.
  const TakeoverDialog({
    super.key,
    required this.holders,
    required this.hasTarget,
  });

  final List<Holder> holders;
  final bool hasTarget;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return AlertDialog(
      key: const Key('takeover-dialog'),
      title: Text(l10n.takeoverTitle),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(l10n.takeoverBody(holders.length)),
          if (holders.isNotEmpty) ...[
            const SizedBox(height: 12),
            Text(
              l10n.takeoverWillTerminate,
              style: Theme.of(context).textTheme.labelMedium,
            ),
            const SizedBox(height: 4),
            ...holders.map(
              (h) => Padding(
                padding: const EdgeInsets.symmetric(vertical: 2),
                child: Text(
                  l10n.holderRow(h.name, h.pid),
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ),
            ),
          ],
          if (!hasTarget) ...[
            const SizedBox(height: 12),
            Text(
              l10n.takeoverNoTarget,
              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                color: Theme.of(context).colorScheme.error,
              ),
            ),
          ],
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(false),
          child: Text(l10n.cancel),
        ),
        FilledButton(
          key: const Key('takeover-confirm'),
          onPressed: hasTarget ? () => Navigator.of(context).pop(true) : null,
          child: Text(l10n.takeoverConfirm),
        ),
      ],
    );
  }
}
