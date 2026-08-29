import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/attachment_refs.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/service_key.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart'
    show appLocalPort;
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/time_ago.dart';
import 'package:pocket_codex/src/widgets/app_toast.dart';
import 'package:pocket_codex/src/widgets/error_retry.dart';
import 'package:pocket_codex/src/widgets/loading.dart';
import 'package:pocket_codex/src/widgets/search_field.dart';
import 'package:pocket_codex/src/widgets/status_dots.dart';
import 'package:pocket_codex/src/widgets/takeover_dialog.dart';
import 'package:pocket_codex/src/widgets/utility_page.dart';

/// Where a sessions view reads from: this machine's `CODEX_HOME` directly
/// ([SessionSource.local]), or a (possibly remote) host's CODEX_HOME over its
/// meta tunnel, keyed by that host's app-server [serviceKey]
/// ([SessionSource.remote]). The remote source is what makes a desktop host's
/// sessions viewable + resumable from a phone (and, since a meta tunnel hosted
/// by this app is served over loopback, the same path covers local hosting).
class SessionSource {
  /// Read this machine's sessions directly.
  const SessionSource.local() : serviceKey = null;

  /// Read the host behind [key]'s sessions over its meta tunnel.
  const SessionSource.remote(String key) : serviceKey = key;

  /// The app-server service key whose host to read, or null for local.
  final String? serviceKey;

  /// Whether this reads a (possibly remote) host over the meta tunnel.
  bool get isRemote => serviceKey != null;

  /// List the source's sessions.
  Future<List<LocalSession>> list(BridgeApi b) =>
      serviceKey == null ? b.appLocalSessions() : b.metaSessions(serviceKey!);

  /// One session's liveness from the source.
  Future<SessionLiveness> liveness(BridgeApi b, String threadId) =>
      serviceKey == null
      ? b.appSessionLiveness(threadId)
      : b.metaSessionLiveness(serviceKey!, threadId);

  /// One session's read-only transcript from the source.
  Future<List<ThreadItem>> transcript(BridgeApi b, String threadId) =>
      serviceKey == null
      ? b.appLocalSessionTranscript(threadId)
      : b.metaSessionTranscript(serviceKey!, threadId);
}

/// Lists codex sessions under a host's `CODEX_HOME` — including ones created by
/// the desktop app / CLI / VS Code extension — annotated with whether each is
/// safe to resume, and lets the user force-take-over a finished session that
/// another process still holds open.
///
/// With the default [SessionSource.local] it reads this machine directly. With a
/// [SessionSource.remote] — reached from a device's session-sharing capability —
/// it reads a (possibly remote) host over that host's meta tunnel.
class LocalSessionsScreen extends ConsumerStatefulWidget {
  /// Default constructor.
  const LocalSessionsScreen({
    super.key,
    this.clock,
    this.source = const SessionSource.local(),
  });

  /// Clock used to bucket sessions by activity time (今天 / 更早) and to render
  /// relative times. Defaults to [DateTime.now]; injectable so tests can pin
  /// "now" to a fixed mid-day instant. Without this the grouping flakes when the
  /// suite runs just after midnight (UTC): minutes-ago "today" timestamps fall
  /// into the previous calendar day and the 今天 group renders empty.
  final DateTime Function()? clock;

  /// Where to read sessions from (local machine vs a host's meta tunnel).
  final SessionSource source;

  @override
  ConsumerState<LocalSessionsScreen> createState() => _LocalSessionsState();
}

class _LocalSessionsState extends ConsumerState<LocalSessionsScreen> {
  bool _loading = true;
  String? _error;
  List<LocalSession> _sessions = const [];
  String _query = '';
  _SessionViewFilter _filter = _SessionViewFilter.all;

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
      final sessions = await widget.source.list(ref.read(bridgeApiProvider));
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
        final liveness = await widget.source.liveness(
          ref.read(bridgeApiProvider),
          session.threadId,
        );
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
      source: widget.source,
    );
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final body = AnimatedSwitcher(
      duration: const Duration(milliseconds: 250),
      child: _buildBody(l10n),
    );
    // Reading another device's sessions is reached from that device, so name it:
    // otherwise a remote list is indistinguishable from this machine's own.
    final key = widget.source.serviceKey;
    final device = key == null ? '' : serviceKeyDevice(key);
    final remoteDevice = device.isEmpty ? null : device;
    return UtilityPage(
      route: '/sessions',
      title: remoteDevice ?? l10n.localSessionsTitle,
      // No parent segment even for a remote device's list. The middle
      // "本地会话" crumb pointed at THIS machine's sessions, which is a sibling
      // of the remote list rather than a level above it — the chat origin is
      // the real way out, and the page menu switches between them.
      actions: [
        IconButton(
          key: const Key('local-sessions-refresh'),
          icon: const Icon(Icons.refresh),
          tooltip: l10n.refreshStatus,
          onPressed: _loading ? null : _load,
        ),
      ],
      body: body,
    );
  }

  Widget _buildBody(AppLocalizations l10n) {
    if (_loading) {
      return const ListLoadingSkeleton(key: ValueKey('local-loading'));
    }
    if (_error != null) {
      return ErrorRetry(
        key: const ValueKey('local-error'),
        errorKey: const Key('local-error-message'),
        message: _error!,
        onRetry: _load,
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
    final desktop = isDesktop && MediaQuery.sizeOf(context).width >= 840;
    final now = (widget.clock ?? DateTime.now)();
    final q = _query.trim().toLowerCase();
    bool matches(LocalSession s) {
      bool has(String? v) => v != null && v.toLowerCase().contains(q);
      return has(s.preview) || has(s.cwd) || has(s.source);
    }

    bool matchesFilter(LocalSession session) => switch (_filter) {
      _SessionViewFilter.all => true,
      _SessionViewFilter.active => session.safety == 'ownedRunning',
      _SessionViewFilter.resumable =>
        session.allowsResume && !session.requiresTakeover,
      _SessionViewFilter.takeover => session.requiresTakeover,
    };

    final filtered = _sessions
        .where(
          (session) =>
              (q.isEmpty || matches(session)) && matchesFilter(session),
        )
        .toList();

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

    final rows = <Widget>[];
    void section(String label, List<LocalSession> items) {
      if (items.isEmpty) return;
      rows.add(_sectionLabel(label));
      rows.addAll(
        items.map(
          (s) => _SessionRow(
            session: s,
            now: now,
            onResume: _onResume,
            serviceKey: widget.source.serviceKey,
          ),
        ),
      );
    }

    section(l10n.groupActive, active);
    section(l10n.groupToday, today);
    section(l10n.groupEarlier, earlier);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (desktop)
          _desktopToolbar(l10n)
        // Quick filter on compact layouts is shown only when it pays for its
        // vertical space. Desktop always keeps search + state filters visible.
        else if (_sessions.length > 6)
          _searchBox(l10n),
        Expanded(
          child: filtered.isEmpty
              ? Center(
                  key: const ValueKey('local-no-match'),
                  child: Text(
                    l10n.noMatchingThreads,
                    style: TextStyle(color: scheme.outline),
                  ),
                )
              : desktop
              ? Padding(
                  padding: const EdgeInsets.fromLTRB(12, 4, 12, 12),
                  child: Card(
                    clipBehavior: Clip.antiAlias,
                    child: RefreshIndicator(
                      key: const ValueKey('local-list'),
                      onRefresh: _load,
                      child: ListView(
                        padding: const EdgeInsets.only(bottom: 8),
                        children: rows,
                      ),
                    ),
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

  Widget _desktopToolbar(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    Widget filter(_SessionViewFilter value, String label) => ChoiceChip(
      key: Key('session-filter-${value.name}'),
      label: Text(label),
      selected: _filter == value,
      onSelected: (_) => setState(() => _filter = value),
      showCheckmark: false,
      side: BorderSide(color: scheme.outlineVariant),
    );
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 12, 12, 4),
      child: Row(
        children: [
          Expanded(child: _searchField(l10n)),
          const SizedBox(width: 10),
          filter(_SessionViewFilter.all, l10n.logsLevelAll),
          const SizedBox(width: 6),
          filter(_SessionViewFilter.active, l10n.groupActive),
          const SizedBox(width: 6),
          filter(_SessionViewFilter.resumable, l10n.sessionResumable),
          const SizedBox(width: 6),
          filter(_SessionViewFilter.takeover, l10n.forceTakeover),
        ],
      ),
    );
  }

  Widget _searchBox(AppLocalizations l10n) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 8, 12, 4),
      child: _searchField(l10n),
    );
  }

  Widget _searchField(AppLocalizations l10n) => SearchField(
    key: const Key('local-search'),
    hintText: l10n.searchLocalSessions,
    onChanged: (v) => setState(() => _query = v),
  );

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

enum _SessionViewFilter { all, active, resumable, takeover }

/// One session row: preview + cwd/source/time, a resume-safety chip, and a
/// resume / force-takeover action when allowed.
class _SessionRow extends StatelessWidget {
  const _SessionRow({
    required this.session,
    required this.now,
    required this.onResume,
    this.serviceKey,
  });

  final LocalSession session;
  final DateTime now;
  final Future<void> Function(LocalSession) onResume;

  /// The remote host's app-server key, carried into the viewer route so it reads
  /// the transcript over the meta tunnel; null for the local source.
  final String? serviceKey;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    // A file-only first message's preview IS the raw attached-files wire
    // block; show the placeholder instead of the header markup.
    final preview = previewWithoutFileRefs(
      session.preview.trim(),
      l10n.fileOnlyMessage,
    );
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
    // Matches the conversation list's row: an ink row on the control radius
    // rather than a ListTile, whose fixed leading/trailing metrics sit taller
    // than the rest of the app's lists.
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 1),
      child: Material(
        color: Colors.transparent,
        borderRadius: BorderRadius.circular(kControlRadius),
        child: InkWell(
          key: Key('local-${session.threadId}'),
          mouseCursor: clickable,
          borderRadius: BorderRadius.circular(kControlRadius),
          // Tapping any row opens the read-only transcript viewer (works even
          // for a session another client owns — it reads the on-disk rollout).
          onTap: () {
            final q = <String>[
              'tid=${Uri.encodeComponent(session.threadId)}',
              if (session.cwd != null && session.cwd!.trim().isNotEmpty)
                'cwd=${Uri.encodeComponent(session.cwd!.trim())}',
              if (preview.isNotEmpty) 'preview=${Uri.encodeComponent(preview)}',
              if (serviceKey != null) 'svc=${Uri.encodeComponent(serviceKey!)}',
            ];
            context.push('/sessions/view?${q.join('&')}');
          },
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 9),
            child: Row(
              children: [
                Icon(
                  session.heldOpen
                      ? Icons.lock_outline
                      : Icons.chat_bubble_outline,
                  size: 18,
                  color: session.heldOpen ? scheme.error : scheme.primary,
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        preview.isEmpty ? session.threadId : preview,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(
                          fontSize: 13,
                          fontWeight: FontWeight.w500,
                        ),
                      ),
                      if (subtitleParts.isNotEmpty) ...[
                        const SizedBox(height: 2),
                        Text(
                          subtitleParts.join(' · '),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: Theme.of(context).textTheme.bodySmall
                              ?.copyWith(color: scheme.onSurfaceVariant),
                        ),
                      ],
                    ],
                  ),
                ),
                const SizedBox(width: 8),
                _safetyChip(context, l10n),
                if (session.allowsResume) ...[
                  const SizedBox(width: 6),
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
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _safetyChip(BuildContext context, AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    // Both "in use elsewhere" states are degraded-but-not-failed, so they share
    // the caution amber; only a session actively running elsewhere is an error.
    final (Color color, String label) = switch (session.safety) {
      'resumable' => (successColor(scheme), l10n.sessionResumable),
      'resumableUnfinished' => (cautionColor(scheme), l10n.sessionUnfinished),
      'ownedRunning' => (scheme.error, l10n.sessionRunningElsewhere),
      'ownedIdle' => (cautionColor(scheme), l10n.sessionInUseElsewhere),
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
  SessionSource source = const SessionSource.local(),
}) async {
  final l10n = AppLocalizations.of(context);
  final messenger = ToastMessenger.of(context);
  // Resolve the app-server to resume into. For a remote host that IS the host
  // behind the source key: it evicts + resumes into its own colocated
  // app-server, so the target is simply that key. For the local source, ensure
  // a live app-server (connect one if needed) rather than requiring the user to
  // have opened one first.
  final String? target;
  if (source.isRemote) {
    target = source.serviceKey;
    // The Sessions tab / viewer reach the host only over the meta tunnel, so the
    // app-server session isn't connected yet. Connect it now (mirroring the
    // local path's ensureResumeTarget) — otherwise the conversation we open
    // below resolves to "not connected" and shows an error after the success.
    final bridge = ref.read(bridgeApiProvider);
    if (target != null && !bridge.appIsConnected(target)) {
      try {
        await bridge.appConnect(target, appLocalPort);
      } catch (e) {
        if (context.mounted) {
          messenger.error(friendlyError(e));
        }
        return;
      }
    }
    if (!context.mounted) return;
  } else {
    target = await ensureResumeTarget(ref);
    if (!context.mounted) return;
  }

  if (requiresTakeover) {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (_) =>
          TakeoverDialog(holders: holders, hasTarget: target != null),
    );
    if (confirmed != true) return;
  }
  if (target == null) {
    messenger.error(l10n.takeoverNoTarget);
    return;
  }

  ForceResumeReport report;
  try {
    final bridge = ref.read(bridgeApiProvider);
    // Remote: the host does the eviction + resume over loopback into its own
    // app-server. Local: we evict + resume into the resolved target ourselves.
    report = source.isRemote
        ? await bridge.metaForceResume(target, threadId)
        : await bridge.appForceResume(target, threadId);
  } catch (e) {
    messenger.error(friendlyError(e));
    return;
  }
  if (!report.resumed) {
    messenger.error(l10n.takeoverResumeFailed(report.resumeError ?? ''));
    return;
  }
  final parts = <String>[
    l10n.takeoverResumed,
    if (report.killed.isNotEmpty) l10n.takeoverKilled(report.killed.length),
    if (report.stillHeld) l10n.takeoverStillHeld,
  ];
  messenger.notice(parts.join(' · '));

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
