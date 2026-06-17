import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/providers.dart';
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

  /// First connected app-server service key (the resume target), or null when
  /// none is connected yet.
  String? _connectedAppKey() {
    final bridge = ref.read(bridgeApiProvider);
    final services = ref.read(servicesProvider).valueOrNull ?? const [];
    for (final s in services) {
      if (s.kind == 'app' && bridge.appIsConnected(s.key)) return s.key;
    }
    return null;
  }

  Future<void> _onResume(LocalSession session) async {
    final l10n = AppLocalizations.of(context);
    final target = _connectedAppKey();

    // A finished-but-held session needs an explicit, holder-listing confirm.
    if (session.requiresTakeover) {
      final liveness = await _safeLiveness(session.threadId);
      if (!mounted) return;
      final confirmed = await showDialog<bool>(
        context: context,
        builder: (_) => _TakeoverDialog(
          holders: liveness?.holders ?? const [],
          hasTarget: target != null,
        ),
      );
      if (confirmed != true) return;
    } else if (target == null) {
      _snack(l10n.takeoverNoTarget);
      return;
    }

    if (target == null) {
      _snack(l10n.takeoverNoTarget);
      return;
    }
    await _doResume(target, session);
  }

  Future<SessionLiveness?> _safeLiveness(String threadId) async {
    try {
      return await ref.read(bridgeApiProvider).appSessionLiveness(threadId);
    } catch (_) {
      return null;
    }
  }

  Future<void> _doResume(String serviceKey, LocalSession session) async {
    final l10n = AppLocalizations.of(context);
    final messenger = ScaffoldMessenger.of(context);
    final router = GoRouter.of(context);
    ForceResumeReport report;
    try {
      report = await ref
          .read(bridgeApiProvider)
          .appForceResume(serviceKey, session.threadId);
    } catch (e) {
      if (mounted) _snack(friendlyError(e));
      return;
    }
    if (!mounted) return;

    if (!report.resumed) {
      _snack(l10n.takeoverResumeFailed(report.resumeError ?? ''));
      return;
    }
    final parts = <String>[
      l10n.takeoverResumed,
      if (report.killed.isNotEmpty) l10n.takeoverKilled(report.killed.length),
      if (report.stillHeld) l10n.takeoverStillHeld,
    ];
    messenger.showSnackBar(SnackBar(content: Text(parts.join(' · '))));

    // Open the resumed conversation.
    final key = Uri.encodeComponent(serviceKey);
    final q = <String>[
      'tid=${Uri.encodeComponent(session.threadId)}',
      if (session.cwd != null && session.cwd!.trim().isNotEmpty)
        'cwd=${Uri.encodeComponent(session.cwd!.trim())}',
    ];
    router.push('/app/$key/session?${q.join('&')}');
  }

  void _snack(String text) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(text)));
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
    return RefreshIndicator(
      key: const ValueKey('local-list'),
      onRefresh: _load,
      child: ListView.separated(
        padding: const EdgeInsets.symmetric(vertical: 8),
        itemCount: _sessions.length + 1,
        separatorBuilder: (_, _) => const Divider(height: 1),
        itemBuilder: (context, i) {
          if (i == 0) {
            return Padding(
              padding: const EdgeInsets.fromLTRB(16, 8, 16, 12),
              child: Text(
                l10n.localSessionsHint,
                style: Theme.of(context).textTheme.bodySmall,
              ),
            );
          }
          return _SessionRow(
            session: _sessions[i - 1],
            onResume: _onResume,
          );
        },
      ),
    );
  }
}

/// One session row: preview + cwd/source/time, a resume-safety chip, and a
/// resume / force-takeover action when allowed.
class _SessionRow extends StatelessWidget {
  const _SessionRow({required this.session, required this.onResume});

  final LocalSession session;
  final Future<void> Function(LocalSession) onResume;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final preview = session.preview.trim();
    final subtitleParts = <String>[
      if (session.cwd != null && session.cwd!.trim().isNotEmpty)
        session.cwd!.trim(),
      if (session.source != null && session.source!.isNotEmpty) session.source!,
    ];
    return ListTile(
      key: Key('local-${session.threadId}'),
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
      'resumableUnfinished' => (
        Colors.amber.shade800,
        l10n.sessionUnfinished,
      ),
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

/// Confirm dialog for a force takeover: lists the holder processes that will be
/// terminated and warns about data loss.
class _TakeoverDialog extends StatelessWidget {
  const _TakeoverDialog({required this.holders, required this.hasTarget});

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
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: Theme.of(context).colorScheme.error),
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
