import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/local_sessions_screen.dart'
    show resumeLocalSession;
import 'package:pocket_codex/src/widgets/loading.dart';

/// Read-only viewer for a local codex session's transcript, parsed straight from
/// the on-disk rollout (no app-server, no resume, no write) — so a session that
/// another codex client (the desktop app / CLI / VS Code) is actively driving
/// can still be read here.
///
/// While a live process owns the session it stays read-only. The viewer polls
/// liveness, so the moment that other client's turn finishes and the session
/// goes idle, it offers a confirmed force-resume to take it over. A free
/// (unowned) session offers a plain resume.
class LocalSessionViewScreen extends ConsumerStatefulWidget {
  /// Creates the viewer for [threadId].
  const LocalSessionViewScreen({
    super.key,
    required this.threadId,
    this.cwd,
    this.preview,
  });

  /// The session/thread to view.
  final String threadId;

  /// Working directory the session controls, carried to the live session on
  /// resume.
  final String? cwd;

  /// First-user-message preview, used as the screen title.
  final String? preview;

  @override
  ConsumerState<LocalSessionViewScreen> createState() =>
      _LocalSessionViewState();
}

class _LocalSessionViewState extends ConsumerState<LocalSessionViewScreen> {
  /// How often to re-read the rollout + re-probe liveness while viewing, so a
  /// session another client is driving updates live and flips to resumable the
  /// moment its turn finishes.
  static const _pollInterval = Duration(seconds: 3);

  List<ThreadItem> _items = const [];
  SessionLiveness? _live;
  bool _loading = true;
  String? _error;
  Timer? _poll;
  final _scroll = ScrollController();

  @override
  void initState() {
    super.initState();
    _load(initial: true);
    _poll = Timer.periodic(_pollInterval, (_) => _load());
  }

  @override
  void dispose() {
    _poll?.cancel();
    _scroll.dispose();
    super.dispose();
  }

  Future<void> _load({bool initial = false}) async {
    if (initial) {
      setState(() {
        _loading = true;
        _error = null;
      });
    }
    final bridge = ref.read(bridgeApiProvider);
    try {
      final items = await bridge.appLocalSessionTranscript(widget.threadId);
      final live = await bridge.appSessionLiveness(widget.threadId);
      if (!mounted) return;
      // Auto-follow the tail if the reader is already near the bottom, so a
      // session being driven elsewhere streams in like a live conversation.
      final atBottom = !_scroll.hasClients ||
          _scroll.position.pixels >= _scroll.position.maxScrollExtent - 40;
      setState(() {
        _items = items;
        _live = live;
        _loading = false;
        _error = null;
      });
      if (atBottom) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (_scroll.hasClients) {
            _scroll.jumpTo(_scroll.position.maxScrollExtent);
          }
        });
      }
    } catch (e) {
      if (!mounted) return;
      // A transient poll failure shouldn't blow away a transcript already shown.
      setState(() {
        _loading = false;
        if (initial || _items.isEmpty) _error = friendlyError(e);
      });
    }
  }

  Future<void> _resume() async {
    final live = _live;
    if (live == null || !live.allowsResume) return;
    // Pause polling while the takeover runs so a mid-flight re-probe can't race
    // the eviction; resumeLocalSession navigates away on success.
    _poll?.cancel();
    await resumeLocalSession(
      context,
      ref,
      threadId: widget.threadId,
      cwd: widget.cwd,
      requiresTakeover: live.requiresTakeover,
      holders: live.holders,
    );
    if (mounted) {
      _poll = Timer.periodic(_pollInterval, (_) => _load());
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final title = (widget.preview != null && widget.preview!.trim().isNotEmpty)
        ? widget.preview!.trim()
        : widget.threadId;
    return Scaffold(
      appBar: AppBar(
        title: Text(title, maxLines: 1, overflow: TextOverflow.ellipsis),
        actions: [
          IconButton(
            key: const Key('local-view-refresh'),
            icon: const Icon(Icons.refresh),
            tooltip: l10n.refreshStatus,
            onPressed: _loading ? null : () => _load(initial: true),
          ),
        ],
      ),
      body: Column(
        children: [
          _banner(l10n),
          Expanded(child: _body(l10n)),
        ],
      ),
      bottomNavigationBar: _actionBar(l10n),
    );
  }

  /// A status strip shown when a live process owns the session.
  Widget _banner(AppLocalizations l10n) {
    final live = _live;
    if (live == null || !live.heldOpen) return const SizedBox.shrink();
    final scheme = Theme.of(context).colorScheme;
    final running = live.safety == 'ownedRunning';
    final color = running ? scheme.error : Colors.orange.shade800;
    final text = running
        ? '${l10n.sessionRunningElsewhere} · ${l10n.sessionReadOnly}'
        : l10n.sessionInUseElsewhere;
    return Container(
      width: double.infinity,
      color: color.withValues(alpha: 0.10),
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Row(
        children: [
          Icon(running ? Icons.lock_clock : Icons.lock_outline,
              size: 16, color: color),
          const SizedBox(width: 8),
          Expanded(
            child: Text(text, style: TextStyle(color: color, fontSize: 13)),
          ),
        ],
      ),
    );
  }

  Widget _body(AppLocalizations l10n) {
    if (_loading) {
      return const ListLoadingSkeleton(key: ValueKey('view-loading'));
    }
    if (_error != null) {
      return Center(
        key: const ValueKey('view-error'),
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(_error!, textAlign: TextAlign.center),
              const SizedBox(height: 12),
              FilledButton(
                onPressed: () => _load(initial: true),
                child: Text(l10n.retry),
              ),
            ],
          ),
        ),
      );
    }
    if (_items.isEmpty) {
      return Center(
        key: const ValueKey('view-empty'),
        child: Text(l10n.sessionTranscriptEmpty),
      );
    }
    return SelectionArea(
      child: ListView.builder(
        key: const ValueKey('view-transcript'),
        controller: _scroll,
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 12),
        itemCount: _items.length,
        itemBuilder: (context, i) => _TranscriptRow(item: _items[i]),
      ),
    );
  }

  /// Bottom action: a read-only note while the session is actively running, or a
  /// resume / force-takeover button once it is free / idle.
  Widget _actionBar(AppLocalizations l10n) {
    final live = _live;
    if (live == null) return const SizedBox.shrink();
    final scheme = Theme.of(context).colorScheme;
    return SafeArea(
      top: false,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 12),
        child: live.allowsResume
            ? FilledButton.icon(
                key: const Key('view-resume'),
                onPressed: _resume,
                icon: Icon(
                  live.requiresTakeover
                      ? Icons.bolt
                      : Icons.play_circle_outline,
                ),
                label: Text(
                  live.requiresTakeover
                      ? l10n.forceTakeover
                      : l10n.resumeSession,
                ),
              )
            : Row(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  Icon(Icons.lock_clock,
                      size: 16, color: scheme.onSurfaceVariant),
                  const SizedBox(width: 8),
                  Flexible(
                    child: Text(
                      l10n.readOnlyViewing,
                      style: TextStyle(color: scheme.onSurfaceVariant),
                    ),
                  ),
                ],
              ),
      ),
    );
  }
}

/// One read-only transcript row: user bubble, agent markdown, a reasoning note,
/// or a command + output block.
class _TranscriptRow extends StatelessWidget {
  const _TranscriptRow({required this.item});

  final ThreadItem item;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    switch (item.itemType) {
      case 'userMessage':
        return Align(
          alignment: Alignment.centerRight,
          child: Container(
            margin: const EdgeInsets.symmetric(vertical: 6),
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
            constraints: BoxConstraints(
              maxWidth: MediaQuery.of(context).size.width * 0.82,
            ),
            decoration: BoxDecoration(
              color: scheme.primaryContainer,
              borderRadius: BorderRadius.circular(14),
            ),
            child: Text(item.text),
          ),
        );
      case 'agentMessage':
        return Padding(
          padding: const EdgeInsets.symmetric(vertical: 6),
          child: MarkdownBody(data: item.text, selectable: false),
        );
      case 'reasoning':
        return Padding(
          padding: const EdgeInsets.symmetric(vertical: 6),
          child: Text(
            item.text,
            style: TextStyle(
              color: scheme.onSurfaceVariant,
              fontStyle: FontStyle.italic,
              fontSize: 13,
            ),
          ),
        );
      default: // commandExecution / tool activity
        return _CommandBlock(title: item.title, output: item.text);
    }
  }
}

/// A monospace command line + its (optional) captured output.
class _CommandBlock extends StatelessWidget {
  const _CommandBlock({required this.title, required this.output});

  final String title;
  final String output;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final mono = TextStyle(
      fontFamily: 'monospace',
      fontFamilyFallback: monoCjkFallback,
      fontSize: 12.5,
      color: scheme.onSurface,
    );
    return Container(
      margin: const EdgeInsets.symmetric(vertical: 6),
      decoration: BoxDecoration(
        color: scheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: scheme.outlineVariant, width: 0.5),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: EdgeInsets.fromLTRB(12, 10, 12, output.isEmpty ? 10 : 6),
            child: Row(
              children: [
                Icon(Icons.terminal, size: 15, color: scheme.onSurfaceVariant),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    title.isEmpty ? '—' : title,
                    style: mono,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
              ],
            ),
          ),
          if (output.isNotEmpty)
            Container(
              width: double.infinity,
              constraints: const BoxConstraints(maxHeight: 220),
              padding: const EdgeInsets.fromLTRB(12, 0, 12, 10),
              child: SingleChildScrollView(
                child: Text(
                  output,
                  style: mono.copyWith(color: scheme.onSurfaceVariant),
                ),
              ),
            ),
        ],
      ),
    );
  }
}
