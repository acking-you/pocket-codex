import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:pocket_codex/src/widgets/window_title_bar.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/app_modes.dart';
import 'package:pocket_codex/src/attachment_refs.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/local_sessions_screen.dart'
    show SessionSource, resumeLocalSession;
import 'package:pocket_codex/src/widgets/loading.dart';
import 'package:pocket_codex/src/widgets/message_images.dart';
import 'package:super_sliver_list/super_sliver_list.dart';

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
    this.serviceKey,
  });

  /// The session/thread to view.
  final String threadId;

  /// Working directory the session controls, carried to the live session on
  /// resume.
  final String? cwd;

  /// First-user-message preview, used as the screen title.
  final String? preview;

  /// The app-server key of the host that owns this session, when viewing a
  /// (possibly remote) host over its meta tunnel; null for the local machine.
  final String? serviceKey;

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

  // Resolved (base64-decoded) attachments cached by item id: rows rebuild on
  // every 3s poll, and re-decoding each image every tick would churn CPU and
  // memory. Ids are stable rollout line indexes; re-resolve only if an item's
  // image count changes (a live writer appending to the same message id).
  final Map<String, ({int count, List<ResolvedImage> images})> _imageCache = {};

  @override
  void didUpdateWidget(LocalSessionViewScreen oldWidget) {
    super.didUpdateWidget(oldWidget);
    // Ids are line indexes (`t0`, `t1`, …) — they collide ACROSS threads, so a
    // reused State showing a different thread must drop the previous thread's
    // cache or its images would render under the new thread's ids.
    if (oldWidget.threadId != widget.threadId) {
      _imageCache.clear();
    }
  }

  List<ResolvedImage> _imagesFor(ThreadItem item) {
    if (item.images.isEmpty) return const [];
    final cached = _imageCache[item.id];
    if (cached != null && cached.count == item.images.length) {
      return cached.images;
    }
    final resolved = resolveImageUrls(item.images);
    _imageCache[item.id] = (count: item.images.length, images: resolved);
    return resolved;
  }

  /// Where this viewer reads from (local machine vs the host's meta tunnel).
  SessionSource get _source => widget.serviceKey == null
      ? const SessionSource.local()
      : SessionSource.remote(widget.serviceKey!);

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
      // Liveness first (a tiny probe). The full transcript — which now embeds
      // every attachment's base64 data URL — is re-fetched on a poll tick ONLY
      // while a live writer holds the rollout (it can change) or on the tick
      // right after it lets go (to pick up the final append). An idle
      // session's rollout is immutable, and re-shipping megabytes of images
      // over the relay every 3s for an unchanged transcript would burn a
      // phone's metered data for nothing.
      final live = await _source.liveness(bridge, widget.threadId);
      final wasHeld = _live?.heldOpen ?? false;
      final fetchTranscript =
          initial || _items.isEmpty || live.heldOpen || wasHeld;
      final items = fetchTranscript
          ? await _source.transcript(bridge, widget.threadId)
          : _items;
      if (!mounted) return;
      // Auto-follow the tail if the reader is already near the bottom, so a
      // session being driven elsewhere streams in like a live conversation.
      final atBottom =
          !_scroll.hasClients ||
          _scroll.position.pixels >= _scroll.position.maxScrollExtent - 40;
      setState(() {
        _items = items;
        _live = live;
        _loading = false;
        _error = null;
      });
      if (fetchTranscript && atBottom) {
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
      source: _source,
    );
    // Only re-arm the poll when this viewer is still the visible route. On a
    // successful resume it pushed the live conversation on top of us, and
    // polling a backgrounded screen every 3s is wasted work.
    if (mounted && (ModalRoute.of(context)?.isCurrent ?? true)) {
      _poll = Timer.periodic(_pollInterval, (_) => _load());
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    // A file-only first message's preview is the raw attached-files wire
    // block — show the placeholder rather than header markup in the app bar.
    final title = (widget.preview != null && widget.preview!.trim().isNotEmpty)
        ? previewWithoutFileRefs(widget.preview!.trim(), l10n.fileOnlyMessage)
        : widget.threadId;
    return Scaffold(
      appBar: WindowTitleBar(
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
          Icon(
            running ? Icons.lock_clock : Icons.lock_outline,
            size: 16,
            color: color,
          ),
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
    // SuperListView (super_sliver_list) replaces ListView.builder for a stable
    // scrollbar: it reconciles per-item height estimates against real heights
    // as rows scroll through, instead of a single running-average estimate that
    // makes the thumb jump given the wide row-height variance here — the same
    // fix the live conversation view uses. Content is centred and capped at
    // ~820px so a wide desktop window doesn't stretch bubbles/markdown
    // edge-to-edge. Stable per-row keys let the sliver track measured heights
    // across the 3s transcript refreshes.
    return SelectionArea(
      child: LayoutBuilder(
        builder: (context, constraints) {
          final side = (constraints.maxWidth - 820) / 2;
          final pad = side < 12 ? 12.0 : side;
          return SuperListView.builder(
            key: const ValueKey('view-transcript'),
            controller: _scroll,
            padding: EdgeInsets.fromLTRB(pad, 12, pad, 12),
            itemCount: _items.length,
            itemBuilder: (context, i) => _TranscriptRow(
              key: ValueKey(_items[i].id),
              item: _items[i],
              images: _imagesFor(_items[i]),
            ),
          );
        },
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
                  Icon(
                    Icons.lock_clock,
                    size: 16,
                    color: scheme.onSurfaceVariant,
                  ),
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
  const _TranscriptRow({super.key, required this.item, this.images = const []});

  final ThreadItem item;

  /// The item's attachments, resolved once by the screen (cached across the
  /// 3s transcript polls).
  final List<ResolvedImage> images;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    switch (item.itemType) {
      case 'userMessage':
        // Document attachments ride the text as a trailing path-reference
        // block; show them as chips and only the typed text (display-only).
        final refs = splitFileRefs(item.text);
        // Cap the bubble to a fraction of the *content* width (the centred
        // column), not the whole screen — otherwise it stretches too wide on a
        // desktop window.
        return LayoutBuilder(
          builder: (context, c) => Align(
            alignment: Alignment.centerRight,
            child: Container(
              margin: const EdgeInsets.symmetric(vertical: 6),
              padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
              constraints: BoxConstraints(maxWidth: c.maxWidth * 0.82),
              decoration: BoxDecoration(
                color: scheme.primaryContainer,
                borderRadius: BorderRadius.circular(14),
              ),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.end,
                children: [
                  if (images.isNotEmpty) MessageImagesView(images: images),
                  if (images.isNotEmpty &&
                      (refs.text.isNotEmpty || refs.paths.isNotEmpty))
                    const SizedBox(height: 8),
                  if (refs.paths.isNotEmpty) FileRefChips(paths: refs.paths),
                  if (refs.paths.isNotEmpty && refs.text.isNotEmpty)
                    const SizedBox(height: 8),
                  if (refs.text.isNotEmpty) Text(refs.text),
                ],
              ),
            ),
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
      case 'turnContext':
        return _TurnContextCaption(item: item);
      default: // commandExecution / tool activity
        return _CommandBlock(title: item.title, output: item.text);
    }
  }
}

/// A subtle per-turn caption synthesized from the rollout's `turn_context`
/// record: which model (and effort / plan mode) actually handled the turn
/// that follows — the read-only analogue of the live conversation's per-turn
/// model stamp. The tooltip carries the recorded permissions. `title` is the
/// model id; `text` is a compact JSON object of the remaining settings
/// (parsed defensively — a caption is never worth an error row).
class _TurnContextCaption extends StatelessWidget {
  const _TurnContextCaption({required this.item});

  final ThreadItem item;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final muted = Theme.of(context).colorScheme.onSurfaceVariant;
    Map<String, dynamic> details = const {};
    try {
      final decoded = jsonDecode(item.text);
      if (decoded is Map<String, dynamic>) details = decoded;
    } catch (_) {
      // Not JSON (e.g. a future host packing something else) — model-only.
    }
    String? str(String key) {
      final v = details[key];
      return (v is String && v.isNotEmpty) ? v : null;
    }

    final effort = ReasoningEffort.fromWire(str('effort'));
    final plan = str('collaborationMode') == 'plan';
    final caption = [
      item.title,
      if (effort != null) effort.label(l10n),
      if (plan) l10n.statePlanMode,
    ].join(' · ');
    // Permissions for the tooltip: the app's preset label when the recorded
    // approval+sandbox pair maps onto one, else the raw wire tokens.
    final approval = str('approvalPolicy');
    final sandbox = str('sandboxMode');
    final preset = PermissionMode.values
        .where((m) => m.approval == approval && m.sandbox == sandbox)
        .firstOrNull;
    final permText = preset?.label(l10n) ?? [?approval, ?sandbox].join(' · ');
    final tooltip = [
      l10n.turnHandledBy(item.title),
      if (permText.isNotEmpty) '${l10n.permissionLabel} · $permText',
    ].join('\n');
    return Align(
      alignment: Alignment.centerLeft,
      child: Tooltip(
        message: tooltip,
        child: Padding(
          // Attach visually to the turn BELOW (the record precedes its turn).
          padding: const EdgeInsets.only(left: 2, top: 10, bottom: 2),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(Icons.auto_awesome, size: 11, color: muted),
              const SizedBox(width: 4),
              Flexible(
                child: Text(
                  caption,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(fontSize: 11.5, color: muted),
                ),
              ),
            ],
          ),
        ),
      ),
    );
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
