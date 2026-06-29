import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:super_sliver_list/super_sliver_list.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/app_modes.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/context_status.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/git_diff.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/widgets/links.dart';
import 'package:pocket_codex/src/widgets/loading.dart';
import 'package:pocket_codex/src/widgets/status_dots.dart';

/// Local port for the app-server ws tunnel (shared with the service screen).
/// `0` is a sentinel: the bridge assigns a free OS port *per service* so several
/// app services can be connected at once. A fixed shared port would let only the
/// first service bind; the rest would hit the probe-bind failure. See
/// [BridgeApi.appConnect].
const appLocalPort = 0;

/// A live conversation with a remote codex app-server thread.
///
/// New conversations let the user pick model / permission mode / remote project
/// path (the composer chips); these apply at `thread/start`. The streamed reply
/// renders live; failures and dropped connections surface an actionable banner,
/// and command-approval prompts are answered inline.
class AppSessionScreen extends ConsumerStatefulWidget {
  /// Creates a session for [serviceKey]; [threadId] null starts a new thread,
  /// optionally seeded with [cwd] (a project chosen on the service screen).
  const AppSessionScreen({
    super.key,
    required this.serviceKey,
    this.threadId,
    this.cwd,
  });

  /// Full `pcx:<device>:app:<name>` key of the connected service.
  final String serviceKey;

  /// Existing thread to resume, or null to start a new one on first message.
  final String? threadId;

  /// Remote working directory to seed a new conversation with.
  final String? cwd;

  @override
  ConsumerState<AppSessionScreen> createState() => _AppSessionState();

  /// Clears the process-wide per-thread plan/effort memory. Test-only: these
  /// static caches intentionally survive screen teardown/rebuild in the running
  /// app (so a reopened thread keeps its mode before the persisted config
  /// lands), but must be reset between widget tests to avoid cross-test leakage.
  @visibleForTesting
  static void debugResetThreadMemory() {
    _AppSessionState._planByThread.clear();
    _AppSessionState._effortByThread.clear();
  }
}

/// One timeline entry: a message (user/agent) or a tool/activity item. The UI
/// renders messages as bubbles/markdown and everything else as activity cards.
class _Item {
  _Item({
    required this.id,
    required this.type,
    this.title = '',
    this.text = '',
    this.streaming = false,
  });
  final String id;
  String type; // userMessage | agentMessage | commandExecution | webSearch | …
  String title;
  String text;
  bool streaming;

  bool get isUser => type == 'userMessage';
  bool get isAgent => type == 'agentMessage';
  bool get isMessage => isUser || isAgent;

  /// Standalone system notices (compaction / stopped) that render as a centered
  /// divider and must never be folded into a tool-call group.
  bool get isNotice => type == 'contextCompaction' || type == 'interrupted';
}

/// One entry in an option-picker bottom sheet (model / permission / effort).
class _PickerOption<T> {
  const _PickerOption({
    required this.value,
    required this.icon,
    required this.label,
    this.description,
  });
  final T value;
  final IconData icon;
  final String label;
  final String? description;
}

class _AppSessionState extends ConsumerState<AppSessionScreen> {
  final _input = TextEditingController();
  final _inputFocus = FocusNode();
  final _scroll = ScrollController();
  // Ordered timeline + an id→index map for upserting streamed/updated items.
  final List<_Item> _items = [];
  final Map<String, int> _itemIndex = {};
  int _localSeq = 0; // ids for optimistic local user messages
  final List<AppEvent> _approvals = []; // pending command-approval prompts
  StreamSubscription<AppEvent>? _sub;

  String? _threadId;
  String? _cwd;
  ModelInfo? _model;
  PermissionMode _mode = PermissionMode.auto;
  bool _plan = false; // plan mode: the agent plans before implementing
  // Whether the thread is currently in plan mode server-side. Collaboration
  // mode is sticky on the thread, so we must send "default" to leave it — this
  // tracks when that's needed (i.e. the last turn ran in plan mode).
  bool _planActive = false;
  // Per-thread plan-mode memory, so switching/resuming a conversation restores
  // its true mode (thread/read doesn't expose collaborationMode, and the model's
  // last item often isn't a `plan` — the old heuristic left plan mode stuck on).
  // STATIC so it survives this screen being torn down + rebuilt (e.g. going back
  // to the session list and reopening a thread creates a fresh State). Without
  // this, reopening a just-planned thread before its async per-thread config
  // PUT lands leaves _planActive with no source → the implement bar vanishes.
  // The persisted meta config remains the cross-restart source of truth.
  static final Map<String, bool> _planByThread = {};
  // True when the user tapped the plan chip since the last send, so the next
  // turn explicitly carries the chosen mode (lets a stuck thread be turned off).
  bool _planToggledByUser = false;
  bool _implementDismissed = false; // user dismissed the implement prompt

  // Reasoning effort ("thinking level"). _effort is a *pending* user pick not yet
  // sent (null = no pending pick); _effortActive is the effort the thread is
  // currently running with (server truth, seeded from thread/resume, updated
  // after a send). The chip + the value sent each turn use the EFFECTIVE effort
  // `_effort ?? _effortActive`, so plan/permission turns re-assert it rather than
  // wiping it (effort is sticky server-side, and the server ignores the
  // top-level effort field when a collaborationMode is also sent). Effort can be
  // raised/lowered but not "un-set" — there is no model-default reset on the wire.
  // _effortByThread restores _effortActive when switching threads in place.
  // STATIC for the same reason as _planByThread: survive screen teardown/rebuild
  // so a reopened thread keeps its effort even before the persisted config lands.
  ReasoningEffort? _effort;
  ReasoningEffort? _effortActive;
  static final Map<String, ReasoningEffort?> _effortByThread = {};

  /// The effort the next turn will run with: a pending pick, else the thread's
  /// current effort. Drives the composer chip and what's sent on every turn.
  ReasoningEffort? get _effectiveEffort => _effort ?? _effortActive;

  // Context-window occupancy + account quota for the status gauge. _ctx seeds
  // from thread/read and updates on thread/tokenUsage/updated; _rate is fetched
  // lazily when the quota popover opens and refreshed on rateLimits events.
  ContextStatus? _ctx;
  RateLimits? _rate;

  // Git: current branch (seeded from thread/read) + the working-tree-vs-main
  // diff, refreshed after edits (turn/diff/updated, turn/completed, compacted).
  String? _branch;
  DiffModel? _diff;

  // 3-pane layout: left = this project's sessions, right = the diff panel.
  // Both collapsible; the chat stays centered regardless. _threads backs the
  // left pane (this project's conversations).
  bool _leftOpen = true;
  bool _rightOpen = false;
  List<ThreadMeta> _threads = const [];
  // Live filter text for the conversations pane search box.
  String _convQuery = '';

  bool _streaming = false;
  // Current running turn's id, captured from turn/started — required to
  // interrupt it (turn/interrupt rejects a threadId without a turnId).
  String? _turnId;
  // Set when the user taps stop; the next turn end renders a "stopped" marker
  // in the transcript so the interruption is visible.
  bool _pendingInterrupt = false;
  // Live elapsed-time clock for the running turn: started on turn/started,
  // ticked once a second, then frozen and recorded as a per-turn footnote on
  // turn end. Local-only (not persisted), like the "stopped" marker.
  DateTime? _turnStartedAt;
  Timer? _elapsedTicker;
  int _elapsedSecs = 0;
  // True while a thread's history is being (re)loaded, so the chat shows a
  // smooth skeleton instead of flashing empty when switching sessions.
  bool _loading = false;
  bool _sending = false;
  bool _atBottom = true; // is the list scrolled to the latest message?
  String? _error;
  VoidCallback? _retry; // action for the error banner's retry button
  bool _connectionLost = false;
  // True while an automatic reconnect is in progress (drives the status bar's
  // "reconnecting" state). Auto-reconnect is attempted on stream close, on a
  // periodic health check, and after a send fails on a dropped connection.
  bool _reconnecting = false;
  DateTime? _lastReconnectAt; // debounce rapid retriggers (flapping socket)
  Timer? _healthTimer;
  String? _lastUserText;

  /// Whether to offer the "implement this plan" choice. A plan-mode turn does
  /// NOT end on its `plan` item — the model appends its prose plan (目标/约束/假设)
  /// and/or reasoning items after it — so keying on "the last item is a plan"
  /// would never fire. Instead: the thread is in plan mode (`_planActive`) AND
  /// the most-recent `plan` item has no later user message (the user hasn't yet
  /// implemented or steered past it). Normal multi-step turns also emit `plan`
  /// checklists, but they run in default mode (`_planActive` false), so they
  /// don't trigger this. Scanning the timeline (rather than requiring the plan
  /// to be literally last) makes it survive trailing prose, a stray reasoning
  /// item, or a leftover duplicate user message, and persist across leave/
  /// restart until the user acts.
  bool get _planReady {
    if (_streaming || _implementDismissed || !_planActive || _items.isEmpty) {
      return false;
    }
    final lastPlan = _items.lastIndexWhere((it) => it.type == 'plan');
    if (lastPlan < 0) return false;
    for (var i = lastPlan + 1; i < _items.length; i++) {
      if (_items[i].type == 'userMessage') return false;
    }
    return true;
  }

  /// Show the "typing" indicator while a turn runs and the model hasn't begun
  /// streaming its text reply yet (tool steps may still be appearing above).
  bool get _showTyping =>
      _streaming &&
      (_items.isEmpty || !_items.last.isAgent || _items.last.text.isEmpty);

  /// The timeline collapsed for display: runs of ≥2 consecutive same-type
  /// non-message activity items become a single [_Group] (shown as one
  /// expandable row); everything else stays a [_Item]. Computed at build time
  /// so the flat `_items` upsert path is untouched.
  List<Object> get _rows {
    final out = <Object>[];
    var i = 0;
    while (i < _items.length) {
      final it = _items[i];
      // Messages and standalone notices are never grouped.
      if (it.isMessage || it.isNotice) {
        out.add(it);
        i++;
        continue;
      }
      var j = i + 1;
      while (j < _items.length &&
          !_items[j].isMessage &&
          !_items[j].isNotice &&
          _items[j].type == it.type) {
        j++;
      }
      if (j - i >= 2) {
        out.add(_Group(it.type, _items.sublist(i, j)));
      } else {
        out.add(it);
      }
      i = j;
    }
    return out;
  }

  @override
  void initState() {
    super.initState();
    _threadId = widget.threadId;
    _cwd = widget.cwd;
    // A brand-new conversation inherits the user's last-chosen model / mode /
    // plan / effort instead of resetting to hard defaults.
    if (_threadId == null) _seedDefaults();
    _scroll.addListener(_onScroll);
    _subscribe();
    if (_threadId != null) _resumeAndLoad();
    _loadThreads();
    // Periodically verify the connection is alive and auto-reconnect if not, so
    // a session left in the background recovers before the user notices (the
    // engine reports a dead socket via appIsConnected; the keepalive ping
    // surfaces it promptly).
    _healthTimer = Timer.periodic(const Duration(seconds: 12), (_) {
      if (!mounted || _reconnecting) return;
      if (!ref.read(bridgeApiProvider).appIsConnected(widget.serviceKey)) {
        _autoReconnect();
      }
    });
  }

  /// Load this project's conversations for the left sessions pane.
  Future<void> _loadThreads() async {
    try {
      final all = await ref
          .read(bridgeApiProvider)
          .appThreadList(widget.serviceKey);
      final cwd = _cwd?.trim();
      var mine = (cwd == null || cwd.isEmpty)
          ? all
          : all.where((t) => t.cwd.trim() == cwd).toList();
      // Keep the freshly-started conversation visible even if `thread/list`
      // hasn't caught up with `thread/start` yet, so it doesn't flicker out of
      // the pane after the optimistic insert in [_send].
      final tid = _threadId;
      if (tid != null && !mine.any((t) => t.id == tid)) {
        ThreadMeta? local;
        for (final t in _threads) {
          if (t.id == tid) {
            local = t;
            break;
          }
        }
        mine = [
          local ??
              ThreadMeta(
                id: tid,
                preview: _lastUserText ?? '',
                cwd: cwd ?? '',
                updatedAt: 0,
              ),
          ...mine,
        ];
      }
      // Prefer a non-empty local/optimistic preview over an empty server one: a
      // freshly-started thread's server preview stays empty until its first turn
      // commits, and a blind replace would flip the tile to "(untitled)".
      final localPreview = {for (final t in _threads) t.id: t.preview};
      mine = [
        for (final t in mine)
          (t.preview.isEmpty && (localPreview[t.id]?.isNotEmpty ?? false))
              ? ThreadMeta(
                  id: t.id,
                  preview: localPreview[t.id]!,
                  cwd: t.cwd,
                  updatedAt: t.updatedAt,
                )
              : t,
      ];
      if (mounted) setState(() => _threads = mine);
    } catch (_) {
      // Listing is best-effort; the pane just stays as it is.
    }
  }

  /// Seed a brand-new conversation's settings from the user's last choices
  /// ([sessionDefaultsProvider]) so it inherits them instead of resetting to
  /// hard defaults. Assigns fields directly; the caller handles setState/timing.
  void _seedDefaults() {
    final d = ref.read(sessionDefaultsProvider(widget.serviceKey));
    _model = d.model;
    _mode = d.mode;
    _plan = d.plan;
    _planActive = false; // a new thread hasn't been told a mode yet
    // A fresh conversation hasn't toggled plan; clear any stale pending toggle
    // (set on the previous thread without sending) so the first turn only sends
    // a collaborationMode when the seeded plan actually differs from default.
    _planToggledByUser = false;
    // Drop a seeded effort the seeded model can't run (mirrors _pickModel's
    // guard) so the first turn never asserts an unsupported level.
    final m = _model;
    _effort =
        (m != null &&
            d.effort != null &&
            !m.supportedReasoningEfforts.contains(d.effort!.wire))
        ? ReasoningEffort.fromWire(m.defaultReasoningEffort)
        : d.effort; // pending pick → asserted on the first turn
    _effortActive = null;
  }

  /// Remember the user's current settings as the default future new
  /// conversations on this service inherit. Called after each explicit pick
  /// (model / mode / plan / effort) — not on server-driven restoration.
  void _rememberDefaults() {
    ref
        .read(sessionDefaultsProvider(widget.serviceKey).notifier)
        .state = SessionDefaults(
      model: _model,
      mode: _mode,
      plan: _plan,
      effort: _effectiveEffort,
    );
  }

  /// The current per-thread settings as a persistable config.
  ThreadConfig _threadConfigSnapshot() => ThreadConfig(
    model: _model?.id,
    reasoningEffort: _effectiveEffort?.wire,
    permissionMode: _mode.name,
    planMode: _plan,
  );

  /// Persist the current per-thread settings on the host so re-opening this
  /// session — on this or another device — restores the user's model / effort /
  /// permission / plan instead of resetting to defaults (requirement #2). No-op
  /// for a brand-new conversation (no thread id yet; persisted at thread-start
  /// instead). Best-effort + fire-and-forget: a failure (e.g. the host meta
  /// tunnel briefly unreachable) must never disrupt the conversation.
  void _persistThreadConfig() {
    final tid = _threadId;
    if (tid == null) return;
    final cfg = _threadConfigSnapshot();
    unawaited(
      ref
          .read(bridgeApiProvider)
          .metaThreadConfigSet(widget.serviceKey, tid, cfg)
          .catchError((_) => cfg),
    );
  }

  /// Load a thread's persisted config from the host. Best-effort: returns an
  /// all-unset config when the host meta tunnel is unreachable, so the caller
  /// falls back to the server / in-memory restore.
  Future<ThreadConfig> _loadPersistedConfig(String threadId) async {
    try {
      return await ref
          .read(bridgeApiProvider)
          .metaThreadConfigGet(widget.serviceKey, threadId);
    } catch (_) {
      return const ThreadConfig();
    }
  }

  /// Switch the screen to another conversation (or a new one when [tid] is
  /// null) in place, resetting per-thread state. Used by the left sessions pane.
  void _openThread(String? tid, String? cwd) {
    setState(() {
      _threadId = tid;
      _cwd = cwd;
      _items.clear();
      _itemIndex.clear();
      _approvals.clear();
      _ctx = null;
      _diff = null;
      _branch = null;
      _rightOpen = false;
      _streaming = false;
      // Drop the previous thread's turn id; the engine tracks the active turn
      // per thread, so interrupt still works for a thread that's already
      // running when we open it (a stale id here would target the wrong turn).
      _turnId = null;
      _pendingInterrupt = false;
      // Show the loading skeleton while the opened thread's history loads (no-op
      // for a brand-new conversation, which has nothing to fetch).
      _loading = tid != null;
      _error = null;
      _retry = null;
      _planActive = false;
      // Reset the per-thread settings to neutral defaults too: an existing
      // thread restores model / permission / plan / effort from its persisted
      // config (+ server) in _resumeAndLoad, and a new one re-seeds below.
      // Without this, switching to a thread with no (or unreachable) persisted
      // config would inherit — and then re-persist — the previous thread's
      // model + permission mode (which the server never restores).
      _model = null;
      _mode = PermissionMode.auto;
      _plan = false;
      _planToggledByUser = false;
      // Drop the previous thread's effort (pending pick + active) so an unsent
      // pick on the old thread can't leak into this one; _resumeAndLoad re-seeds
      // _effortActive from the server / per-thread memory.
      _effort = null;
      _effortActive = null;
      _implementDismissed = false;
      _input.clear();
      // A brand-new conversation inherits the user's last-chosen settings; an
      // existing one is restored from the server by _resumeAndLoad below.
      if (tid == null) _seedDefaults();
    });
    if (tid != null) _resumeAndLoad();
  }

  @override
  void dispose() {
    _healthTimer?.cancel();
    _elapsedTicker?.cancel();
    _sub?.cancel();
    _input.dispose();
    _inputFocus.dispose();
    _scroll.removeListener(_onScroll);
    _scroll.dispose();
    super.dispose();
  }

  // Track whether we're pinned to the bottom so streaming auto-follows only
  // when the user hasn't scrolled up to read earlier messages.
  void _onScroll() {
    if (!_scroll.hasClients) return;
    final atBottom =
        _scroll.position.pixels >= _scroll.position.maxScrollExtent - 80;
    if (atBottom != _atBottom) setState(() => _atBottom = atBottom);
  }

  void _subscribe() {
    _sub?.cancel();
    _sub = ref
        .read(bridgeApiProvider)
        .appEvents(widget.serviceKey)
        // onError as well as onDone: the bridge errors the stream (rather than
        // closing it) when the service isn't connected yet, so treat both as a
        // dropped connection and auto-reconnect instead of waiting for the
        // periodic health check.
        .listen(
          _onEvent,
          onError: (_) => _onStreamClosed(),
          onDone: _onStreamClosed,
        );
  }

  void _onStreamClosed() {
    if (!mounted) return;
    // The event stream closing means the socket dropped — recover automatically
    // rather than leaving the session silently dead.
    setState(() => _streaming = false);
    _autoReconnect();
  }

  /// Open an existing thread: resume it into the session (so reads and turns
  /// resolve — otherwise the server returns "thread not found"), then load
  /// its history.
  Future<void> _resumeAndLoad() async {
    // Guard: a stale event (e.g. thread/compacted from a prior thread) can
    // arrive after switching to a new, unsaved conversation — don't `_threadId!`
    // through a null here.
    if (_threadId == null) return;
    setState(() {
      _loading = true;
      _error = null;
      _retry = null;
    });
    final startTid = _threadId!;
    try {
      final api = ref.read(bridgeApiProvider);
      await api.appThreadResume(widget.serviceKey, startTid);
      // Read the thread history and its persisted config concurrently. The
      // config is best-effort (an unreachable host meta tunnel yields an
      // all-unset config and we fall back to the server / in-memory restore).
      final historyFuture = api.appThreadRead(widget.serviceKey, startTid);
      final persistedFuture = _loadPersistedConfig(startTid);
      final history = await historyFuture;
      final persisted = await persistedFuture;
      // The server restores neither the model nor the permission mode, so the
      // persisted store is their only source across re-opens; resolve a stored
      // model id against this service's model list.
      ModelInfo? persistedModel;
      if (persisted.model != null) {
        try {
          final models = await _ensureModels();
          persistedModel = models
              .where((m) => m.id == persisted.model)
              .firstOrNull;
        } catch (_) {
          // Model list unavailable — leave the model unchanged.
        }
      }
      // The user may have switched threads during the awaits above.
      if (!mounted || _threadId != startTid) return;
      setState(() {
        _loading = false;
        _items.clear();
        _itemIndex.clear();
        for (final i in history.items) {
          // Defensively collapse a back-to-back duplicate user message (same
          // text, nothing between) — the artifact of a dropped-but-committed
          // send recorded twice. A genuine re-ask has the model's reply in
          // between, so it is preserved. (The retry-safety guard in _send is the
          // primary fix; this protects any other double-commit source.)
          if (i.itemType == 'userMessage' &&
              _items.isNotEmpty &&
              _items.last.type == 'userMessage' &&
              _items.last.text.trim() == i.text.trim()) {
            continue;
          }
          _itemIndex[i.id] = _items.length;
          _items.add(
            _Item(id: i.id, type: i.itemType, title: i.title, text: i.text),
          );
        }
        // Restore the "thinking" state if a turn was still running when we
        // left: live events (delivered after resume) will finish rendering it.
        _streaming = history.running;
        // Restore the running turn's live clock + loading animation. Without
        // this the streaming flag was set but the ticker wasn't, so the bottom
        // in-progress indicator showed a frozen 0:00 (looked "gone"). We can't
        // recover the real start time on a cold re-open, so count from now — the
        // point is to show, live, that the turn is still working.
        _elapsedTicker?.cancel();
        _elapsedTicker = null;
        if (history.running) {
          _elapsedSecs = 0;
          _startElapsedTicker();
        }
        // Restore the thread's plan mode authoritatively: prefer the server's
        // collaborationMode if it ever exposes it, else our per-thread memory.
        // (The old "last item is plan" guess was wrong — the model's reply
        // usually isn't a plan item — which left plan mode stuck on.) Don't
        // clobber a pending toggle the user set before a drop/reload.
        final tid = _threadId;
        // Permission mode + model: restored from the persisted store (the
        // server tracks neither, so without this they'd reset to defaults on
        // every re-open).
        final persistedMode = persisted.permissionMode == null
            ? null
            : PermissionMode.values
                  .where((m) => m.name == persisted.permissionMode)
                  .firstOrNull;
        if (persistedMode != null) _mode = persistedMode;
        if (persistedModel != null) _model = persistedModel;
        final serverMode = history.collaborationMode;
        final restored = serverMode != null
            ? serverMode == 'plan'
            : (persisted.planMode ??
                  (tid != null && (_planByThread[tid] ?? false)));
        final hadPendingToggle = _plan != _planActive;
        _planActive = restored;
        if (!hadPendingToggle) _plan = _planActive;
        // Restore the thread's current effort: prefer the server value (from the
        // resume response), else the persisted store, else our per-thread
        // memory. A pending pick (_effort) is left untouched — the chip shows
        // `_effort ?? _effortActive`, so it survives a drop/reload unclobbered.
        final serverEffort = ReasoningEffort.fromWire(history.reasoningEffort);
        _effortActive =
            serverEffort ??
            ReasoningEffort.fromWire(persisted.reasoningEffort) ??
            (tid != null ? _effortByThread[tid] : null);
        // Drop a restored effort the restored model can't run (mirrors the guard
        // in _pickModel/_seedDefaults) so a stale persisted pairing never asserts
        // an unsupported level on the next turn.
        final restoredModel = _model;
        final restoredEffort = _effortActive;
        if (restoredModel != null &&
            restoredEffort != null &&
            !restoredModel.supportedReasoningEfforts.contains(
              restoredEffort.wire,
            )) {
          _effortActive = ReasoningEffort.fromWire(
            restoredModel.defaultReasoningEffort,
          );
        }
        // Seed the status gauge + branch chip + cwd from the thread metadata.
        // _cwd may be null if the thread was opened without it (e.g. a default
        // folder that codex resolved to a real path) — adopt the resolved cwd
        // so the git diff (which needs a concrete cwd) works.
        _branch = history.branch;
        _cwd ??= history.cwd;
        final tu = history.tokensUsed, cw = history.contextWindow;
        if (tu != null && cw != null && cw > 0) {
          _ctx = ContextStatus(tokensUsed: tu, contextWindow: cw);
        }
      });
      _loadGit();
      _scrollToEnd(force: true);
    } catch (e) {
      if (mounted && _threadId == startTid) {
        setState(() {
          _loading = false;
          _error = friendlyError(e);
          _retry = _resumeAndLoad;
        });
      }
    }
  }

  void _onEvent(AppEvent e) {
    // Ignore events belonging to another thread. Before this conversation has a
    // thread id (a brand-new conversation, pre-`thread/start`), the app session
    // is shared and another thread's turn may still be streaming, so drop any
    // thread-scoped event rather than letting it pollute the blank conversation
    // (append messages/approvals, flip `_streaming`/`_turnId`). Truly global
    // events (no threadId — e.g. account/rate-limit updates) always pass.
    if (e.threadId != null && e.threadId != _threadId) {
      return;
    }
    if (!mounted) return;
    // Server-initiated approval prompt (carries a request id to answer).
    if (e.requestId != null) {
      setState(() => _approvals.add(e));
      return;
    }
    // Status-bar feeds: token usage + quota updates carry their data in `raw`
    // (map_event is a generic passthrough, so no item fields are set).
    if (e.kind == 'thread/tokenUsage/updated') {
      final ctx = ContextStatus.fromRaw(e.raw);
      if (ctx != null) setState(() => _ctx = ctx);
      return;
    }
    if (e.kind == 'account/rateLimits/updated') {
      // codex v2 sends a sparse/rolling partial here — merge into the last full
      // snapshot rather than replace, or omitted windows would blank out.
      final r = RateLimits.fromRaw(e.raw);
      if (r != null) {
        setState(() => _rate = _rate == null ? r : _rate!.merge(r));
      }
      return;
    }
    // The agent edited files: refresh the working-tree-vs-main diff badge.
    if (e.kind == 'turn/diff/updated') {
      _loadGit();
      return;
    }
    // Compaction finished: reload the (now shorter) history.
    if (e.kind == 'thread/compacted') {
      _resumeAndLoad();
      return;
    }
    switch (e.kind) {
      case 'turn/started':
        // A fresh turn supersedes any prior plan: re-enable the implement
        // prompt so a new plan (if this turn produces one) can offer it again.
        // Capture the turn id so the stop button can interrupt this turn.
        setState(() {
          _streaming = true;
          _turnId = _parseTurnId(e.raw);
          _implementDismissed = false;
          _pendingInterrupt = false;
          _elapsedSecs = 0;
        });
        _startElapsedTicker();
        _scrollToEnd();
      case 'turn/completed':
        // v2 reports turn FAILURES here (turn.status == 'failed' + error.message),
        // not via a separate turn/failed method — surface the error the same way.
        final failure = _turnFailureText(e.raw);
        setState(() {
          _streaming = false;
          _turnId = null;
          for (final it in _items) {
            it.streaming = false;
          }
          _finishTurn();
          // A turn the user stopped also ends as failed/aborted; show the
          // "stopped" marker rather than an error banner.
          if (_pendingInterrupt) {
            _addStoppedMarker();
          } else if (failure != null) {
            _error = failure.isNotEmpty
                ? failure
                : AppLocalizations.of(context).turnFailed;
            _retry = () => _send(retry: true);
          }
        });
        _loadGit(); // edits from the turn may have changed the diff
      case 'turn/failed':
        setState(() {
          _streaming = false;
          _turnId = null;
          for (final it in _items) {
            it.streaming = false;
          }
          _finishTurn();
          // A turn the user stopped also ends as failed/aborted; show the
          // "stopped" marker rather than an error banner.
          if (_pendingInterrupt) {
            _addStoppedMarker();
          } else {
            _error = e.text ?? AppLocalizations.of(context).turnFailed;
            _retry = () => _send(retry: true);
          }
        });
      default:
        _handleItemEvent(e);
    }
  }

  /// If a `turn/completed` event actually represents a FAILED turn (v2 reports
  /// failures here with `turn.status == 'failed'`), return its error message —
  /// or an empty string if it failed without one. Returns null when the turn
  /// completed successfully (so the caller leaves the transcript untouched).
  String? _turnFailureText(String raw) {
    try {
      final m = jsonDecode(raw);
      if (m is! Map) return null;
      final turn = m['turn'];
      if (turn is! Map || turn['status'] != 'failed') return null;
      final err = turn['error'];
      return (err is Map && err['message'] is String)
          ? err['message'] as String
          : '';
    } catch (_) {
      return null;
    }
  }

  /// Pull the turn id out of a turn/started event's raw params, tolerating
  /// `{turnId}`, `{turn:{id}}`, or `{id}` shapes.
  String? _parseTurnId(String raw) {
    try {
      final m = jsonDecode(raw);
      if (m is! Map) return null;
      final direct = m['turnId'] ?? m['id'];
      if (direct is String && direct.isNotEmpty) return direct;
      final turn = m['turn'];
      if (turn is Map && turn['id'] is String) return turn['id'] as String;
    } catch (_) {}
    return null;
  }

  /// Upsert a tool/activity or message item from an `item/*` event.
  void _handleItemEvent(AppEvent e) {
    final id = e.itemId, type = e.itemType;
    if (id == null || type == null) return;
    // The user's own message is shown optimistically on send; ignore the
    // server echo so it isn't duplicated.
    if (type == 'userMessage') return;
    final isDelta = e.kind.contains('delta');
    final running = e.kind.contains('started');
    setState(() {
      final idx = _itemIndex[id];
      if (idx == null) {
        _items.add(
          _Item(
            id: id,
            type: type,
            title: e.title ?? '',
            text: e.text ?? '',
            streaming: type == 'agentMessage' ? true : running,
          ),
        );
        _itemIndex[id] = _items.length - 1;
      } else {
        final it = _items[idx];
        it.type = type;
        if ((e.title ?? '').isNotEmpty) it.title = e.title!;
        if (isDelta) {
          it.text += e.text ?? '';
        } else if ((e.text ?? '').isNotEmpty || !it.isAgent) {
          it.text = e.text ?? '';
        }
        if (!it.isAgent) it.streaming = running;
      }
    });
    _scrollToEnd();
  }

  Future<void> _send({bool retry = false, String? overrideText}) async {
    final text = retry
        ? (_lastUserText ?? '')
        : (overrideText ?? _input.text.trim());
    // Block sends while reconnecting — a reconnect reloads history and would
    // wipe an optimistic message added mid-flight.
    if (text.isEmpty || _sending || _reconnecting) return;
    // Retry safety: a send can commit server-side just before the socket drops
    // (we reconnect with reload:false to keep the optimistic bubble for a
    // one-tap retry). Re-sending a committed turn records the prompt twice —
    // which both shows a duplicate user bubble and leaves a trailing duplicate
    // that hides the plan-implement choice. So on retry, ask the server first;
    // if this prompt is already the latest user turn, just reload its (possibly
    // in-progress) history instead of sending again.
    if (retry && await _turnAlreadyCommitted(text)) {
      await _resumeAndLoad();
      return;
    }
    setState(() {
      _sending = true;
      _error = null;
      _retry = null;
      _lastUserText = text;
      if (!retry) {
        final id = 'local-user-${_localSeq++}';
        _itemIndex[id] = _items.length;
        _items.add(_Item(id: id, type: 'userMessage', text: text));
        // Don't clear the composer for a programmatic send (e.g. "implement
        // the plan") — the user may have text in progress there.
        if (overrideText == null) _input.clear();
      }
    });
    _scrollToEnd(force: true);
    var dropped = false;
    try {
      final api = ref.read(bridgeApiProvider);
      // Collaboration mode for this turn. Send it when the user explicitly
      // toggled the plan chip (so an explicit on/off is always honored, even if
      // our view of the server mode is stale), or when the desired toggle
      // differs from the known mode; otherwise leave it unchanged (null) so
      // ordinary turns don't force a model.
      final collab = (_planToggledByUser || _plan != _planActive)
          ? (_plan ? 'plan' : 'default')
          : null;
      // The effort this turn runs with: a pending pick, else the thread's
      // current effort (re-asserted so a plan/permission turn can't drop it).
      final effort = _effectiveEffort;
      // Both plan and default collaboration settings require a concrete model
      // id; resolve one (and reflect it in the chip) if left on "default".
      var modelId = _model?.id;
      if (collab != null && modelId == null) {
        final models = await api.appModelList(widget.serviceKey);
        if (models.isNotEmpty) {
          modelId = models.first.id;
          if (mounted) setState(() => _model = models.first);
        }
      }
      // The server silently ignores collaborationMode without a concrete model,
      // which would leave the thread in its previous mode while we optimistically
      // flip _planActive below — a silent UI/server divergence. Refuse instead so
      // the switch (enter/leave plan mode) never appears to succeed when it can't.
      if (collab != null && modelId == null) {
        if (mounted) {
          setState(() {
            _error = AppLocalizations.of(context).noModelForMode;
            _retry = () => _send(retry: true);
          });
        }
        return;
      }
      final isNewThread = _threadId == null;
      _threadId ??= await api.appThreadStart(
        widget.serviceKey,
        model: modelId,
        cwd: _cwd,
        approvalPolicy: _mode.approval,
        sandbox: _mode.sandbox,
      );
      if (isNewThread) {
        // Surface the new session in the left pane immediately. `thread/list`
        // can lag `thread/start`, so optimistically insert it now (newest
        // first) and let _loadThreads reconcile once the server catches up.
        final tid = _threadId!;
        if (mounted && !_threads.any((t) => t.id == tid)) {
          setState(() {
            _threads = [
              ThreadMeta(id: tid, preview: text, cwd: _cwd ?? '', updatedAt: 0),
              ..._threads,
            ];
          });
        }
        _loadThreads();
        // Persist this new thread's config now that it has a server-side id, so
        // it's stored even if the first turn/start below fails.
        _persistThreadConfig();
      }
      // Pass the current model + permission + collaboration mode every turn:
      // turn/start overrides apply to this and subsequent turns, so switching
      // works mid-conversation.
      await api.appTurnStart(
        widget.serviceKey,
        _threadId!,
        text,
        model: modelId,
        approvalPolicy: _mode.approval,
        sandbox: _mode.sandbox,
        collaborationMode: collab,
        // Re-assert the effective effort every turn. The bridge puts it on the
        // top-level `effort` field AND (when a collaborationMode is sent) into
        // collaborationMode.settings, so toggling plan/permission never wipes the
        // thread's sticky effort. null only when no effort has ever been set.
        reasoningEffort: effort?.wire,
      );
      if (mounted) {
        setState(() {
          _planActive = _plan;
          _planToggledByUser = false;
          // The effort this turn ran with is now the thread's active effort.
          _effortActive = effort;
          // Clear the pending pick ONLY if the user hasn't chosen a NEWER effort
          // while this turn was being sent. A mid-send `_pickEffort` sets
          // `_effort` (and persists it) for the next turn; blindly nulling it
          // here would silently revert that change (R4).
          if (_effort == effort) _effort = null;
        });
        // Remember this thread's mode + effort so resuming/switching restores it.
        if (_threadId != null) {
          _planByThread[_threadId!] = _plan;
          _effortByThread[_threadId!] = effort;
        }
        // Persist the config now that the thread has an id — covers a brand-new
        // conversation whose settings were chosen before its first turn.
        _persistThreadConfig();
      }
    } catch (e) {
      final msg = friendlyError(e);
      if (mounted) {
        setState(() {
          _error = msg;
          _retry = () => _send(retry: true);
        });
      }
      if (_looksDisconnected(msg)) dropped = true;
    } finally {
      if (mounted) setState(() => _sending = false);
    }
    // The connection dropped mid-send: recover it in the background so a retry
    // (or the next message) succeeds. We do NOT auto-resend — the turn may have
    // committed server-side before the socket dropped, and resending would
    // duplicate it; the user retries with one tap instead. `reload: false`
    // keeps the optimistic message visible (and the plan toggle) for that retry.
    if (dropped) {
      await _autoReconnect(reload: false);
      // _autoReconnect cleared the error; re-offer the retry now that the
      // connection is back (retry reuses _lastUserText + the existing bubble).
      if (mounted && !_connectionLost) {
        setState(() {
          _error = AppLocalizations.of(context).turnFailed;
          _retry = () => _send(retry: true);
        });
      }
    }
  }

  /// Whether the server already recorded [text] as the latest user turn — i.e.
  /// a send committed before the socket dropped. Makes retry idempotent so a
  /// dropped-but-committed turn isn't sent (and recorded) twice. Returns false
  /// if the host can't be reached, so an indeterminate case falls through to a
  /// normal re-send (the prior, less-safe behaviour) rather than losing the turn.
  Future<bool> _turnAlreadyCommitted(String text) async {
    final tid = _threadId;
    if (tid == null) return false;
    try {
      final api = ref.read(bridgeApiProvider);
      await api.appThreadResume(widget.serviceKey, tid);
      final history = await api.appThreadRead(widget.serviceKey, tid);
      final want = text.trim();
      for (final i in history.items.reversed) {
        if (i.itemType == 'userMessage') return i.text.trim() == want;
      }
    } catch (_) {
      // Indeterminate (host unreachable) — fall through to a normal re-send.
    }
    return false;
  }

  /// Carry out the plan the model just produced: leave plan mode and start a
  /// normal turn instructing the agent to implement it. The instruction is
  /// shown as a user message so the transcript stays honest about what was sent.
  Future<void> _implement() async {
    final prompt = AppLocalizations.of(context).implementPlanPrompt;
    setState(() {
      _plan = false;
      _implementDismissed = false;
    });
    await _send(overrideText: prompt);
  }

  Future<void> _interrupt() async {
    if (_threadId == null) return;
    // Arm the marker; the turn's end event renders it. Cleared if the request
    // itself fails (no turn was actually stopped).
    setState(() => _pendingInterrupt = true);
    try {
      await ref
          .read(bridgeApiProvider)
          .appTurnInterrupt(widget.serviceKey, _threadId!, turnId: _turnId);
    } catch (e) {
      if (mounted) {
        setState(() {
          _pendingInterrupt = false;
          _error = friendlyError(e);
        });
      }
    }
  }

  /// Append a local "stopped" marker so an interrupted turn is visible in the
  /// transcript. Local-only (not persisted); call inside a `setState`.
  void _addStoppedMarker() {
    final id = 'local-stopped-${_localSeq++}';
    _itemIndex[id] = _items.length;
    _items.add(_Item(id: id, type: 'interrupted', text: ''));
    _pendingInterrupt = false;
  }

  /// Begin ticking the running turn's elapsed clock once a second so the status
  /// bar counts up live. Cheap: a single `setState` per second, only while a
  /// turn runs.
  void _startElapsedTicker() {
    _turnStartedAt = DateTime.now();
    _elapsedTicker?.cancel();
    _elapsedTicker = Timer.periodic(const Duration(seconds: 1), (_) {
      final started = _turnStartedAt;
      if (!mounted || started == null) return;
      setState(
        () => _elapsedSecs = DateTime.now().difference(started).inSeconds,
      );
    });
  }

  /// Stop the elapsed ticker and append a per-turn duration footnote (用时 X,
  /// hover → 完成于 HH:MM:SS). Local-only; call inside a `setState`.
  void _finishTurn() {
    _elapsedTicker?.cancel();
    _elapsedTicker = null;
    final started = _turnStartedAt;
    _turnStartedAt = null;
    if (started == null) return;
    final now = DateTime.now();
    final id = 'local-turndur-${_localSeq++}';
    _itemIndex[id] = _items.length;
    _items.add(
      _Item(
        id: id,
        type: 'turnDuration',
        title: _fmtElapsed(now.difference(started).inSeconds),
        text: _fmtClock(now),
      ),
    );
  }

  /// Stopwatch-format an elapsed-second count: `m:ss`, or `h:mm:ss` past an
  /// hour (e.g. `0:08`, `1:23`, `1:02:05`).
  String _fmtElapsed(int secs) {
    final s = secs < 0 ? 0 : secs;
    final h = s ~/ 3600;
    final m = (s % 3600) ~/ 60;
    final ss = (s % 60).toString().padLeft(2, '0');
    if (h > 0) return '$h:${m.toString().padLeft(2, '0')}:$ss';
    return '$m:$ss';
  }

  /// Wall-clock `HH:MM:SS` for a completion timestamp.
  String _fmtClock(DateTime t) {
    String p(int n) => n.toString().padLeft(2, '0');
    return '${p(t.hour)}:${p(t.minute)}:${p(t.second)}';
  }

  /// Return to the project / session picker (AppServiceScreen). Pops if this
  /// screen was pushed from there; otherwise navigates to it directly.
  void _backToProjects() {
    if (context.canPop()) {
      context.pop();
    } else {
      context.go('/app/${widget.serviceKey}');
    }
  }

  /// Refresh the diff-vs-main for the branch/changes badge. Keyed on the
  /// project cwd (what `gitDiffToRemote` needs); a no-op without one.
  Future<void> _loadGit() async {
    final cwd = _cwd?.trim();
    if (cwd == null || cwd.isEmpty) return;
    try {
      final raw = await ref
          .read(bridgeApiProvider)
          .appGitDiff(widget.serviceKey, cwd);
      if (!mounted) return;
      setState(() => _diff = DiffModel.parse(raw));
    } catch (_) {
      // Not a git repo / no remote: leave the badge as branch-only.
    }
  }

  /// Open the diff viewer: the inline right pane on desktop (≥1100px), a tall
  /// bottom sheet on narrower screens.
  Future<void> _showDiff() async {
    await _loadGit();
    if (!mounted) return;
    if (MediaQuery.of(context).size.width >= 1100) {
      setState(() => _rightOpen = true);
      return;
    }
    await showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      isScrollControlled: true,
      builder: (c) => FractionallySizedBox(
        heightFactor: 0.9,
        child: _DiffView(diff: _diff, branch: _branch),
      ),
    );
  }

  /// Manually compact the conversation after a confirm.
  Future<void> _compact() async {
    final tid = _threadId;
    if (tid == null) return;
    final l10n = AppLocalizations.of(context);
    final ok = await showDialog<bool>(
      context: context,
      builder: (c) => AlertDialog(
        title: Text(l10n.compact),
        content: Text(l10n.compactConfirm),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(c, false),
            child: Text(l10n.cancel),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(c, true),
            child: Text(l10n.compact),
          ),
        ],
      ),
    );
    if (ok != true) return;
    try {
      await ref.read(bridgeApiProvider).appCompact(widget.serviceKey, tid);
      // The server emits thread/compacted, which reloads history.
    } catch (e) {
      if (mounted) {
        final msg = friendlyError(e);
        setState(() {
          _error = msg;
          if (_looksDisconnected(msg)) _connectionLost = true;
        });
      }
    }
  }

  /// Re-establish the connection automatically, with a few backoff retries.
  /// Shows a "reconnecting" state while trying; only surfaces the manual
  /// "disconnected" banner if every attempt fails. Idempotent — concurrent
  /// triggers (stream close + health tick) collapse into one attempt.
  /// [reload] re-reads the thread history after reconnecting (to catch events
  /// missed while the socket was down). Pass `false` from the send-failure path
  /// so the just-added optimistic message and a pending plan toggle survive for
  /// the retry.
  Future<void> _autoReconnect({bool reload = true}) async {
    if (_reconnecting) return;
    // Debounce: a flapping socket (connect succeeds then drops) could otherwise
    // spin reconnect attempts. The periodic health check is the backstop.
    final now = DateTime.now();
    if (_lastReconnectAt != null &&
        now.difference(_lastReconnectAt!) < const Duration(seconds: 3)) {
      return;
    }
    _lastReconnectAt = now;
    if (mounted) {
      setState(() {
        _reconnecting = true;
        _connectionLost = false;
        _error = null;
        _retry = null;
      });
    }
    final api = ref.read(bridgeApiProvider);
    for (var attempt = 0; attempt < 4; attempt++) {
      if (!mounted) return;
      try {
        // appConnect reuses a live session but reconnects a dead one; drop
        // first to force a clean re-handshake regardless.
        await api.appDisconnect(widget.serviceKey);
        await api.appConnect(widget.serviceKey, appLocalPort);
        _subscribe();
        if (reload && _threadId != null) await _resumeAndLoad();
        if (mounted) {
          setState(() {
            _reconnecting = false;
            _connectionLost = false;
            _error = null;
          });
        }
        return;
      } catch (_) {
        await Future<void>.delayed(Duration(seconds: 1 << attempt)); // 1/2/4/8s
      }
    }
    // Out of retries: fall back to the manual banner.
    if (mounted) {
      setState(() {
        _reconnecting = false;
        _connectionLost = true;
        _error = AppLocalizations.of(context).connectionLost;
        _retry = _autoReconnect;
      });
    }
  }

  /// Whether an error message looks like the app-server connection dropped, so
  /// the banner can offer "reconnect" (which re-establishes the session) rather
  /// than a plain retry that would hit the same dead connection.
  bool _looksDisconnected(String msg) {
    final m = msg.toLowerCase();
    return m.contains('connection closed') ||
        m.contains('closed connection') ||
        m.contains('not connected');
  }

  Future<void> _decide(AppEvent prompt, String decision) async {
    setState(() => _approvals.remove(prompt));
    await ref
        .read(bridgeApiProvider)
        .appRespondApproval(widget.serviceKey, prompt.requestId!, decision);
  }

  /// Answer a `request_user_input` elicitation. `answers` maps each question id
  /// to the chosen answer string(s); an empty map cancels (the turn proceeds
  /// without an answer). Sends the codex `ToolRequestUserInputResponse` shape.
  Future<void> _answerUserInput(
    AppEvent prompt,
    Map<String, List<String>> answers,
  ) async {
    setState(() => _approvals.remove(prompt));
    await ref
        .read(bridgeApiProvider)
        .appRespondUserInput(
          widget.serviceKey,
          prompt.requestId!,
          jsonEncode(answers),
        );
  }

  /// Scroll to the latest message. Auto-follow only when already pinned to the
  /// bottom (so reading earlier messages isn't interrupted); [force] overrides
  /// that (e.g. right after the user sends or taps the jump button).
  void _scrollToEnd({bool force = false}) {
    if (!force && !_atBottom) return;
    if (!force) {
      // Auto-follow while streaming: a smooth nudge to the latest content.
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (_scroll.hasClients) {
          _scroll.animateTo(
            _scroll.position.maxScrollExtent,
            duration: const Duration(milliseconds: 200),
            curve: Curves.easeOut,
          );
        }
      });
      return;
    }
    // Forced jump (opening a conversation, sending, the jump button): a tall
    // conversation lays its variable-height items out over several frames, so
    // maxScrollExtent keeps growing after the first jump. Re-jump to the bottom
    // each frame until it settles — otherwise a long conversation opens blank
    // / mid-content until the user scrolls manually.
    void settle(int tries) {
      if (!_scroll.hasClients) return;
      final before = _scroll.position.maxScrollExtent;
      _scroll.jumpTo(before);
      if (tries <= 0) return;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (_scroll.hasClients &&
            _scroll.position.maxScrollExtent > before + 1) {
          settle(tries - 1);
        }
      });
    }

    WidgetsBinding.instance.addPostFrameCallback((_) => settle(10));
  }

  String _projectName() {
    final c = _cwd?.trim();
    if (c == null || c.isEmpty) {
      return AppLocalizations.of(context).defaultFolder;
    }
    final parts = c.split(RegExp(r'[\\/]'))..removeWhere((s) => s.isEmpty);
    return parts.isEmpty ? c : parts.last;
  }

  /// Desktop hover text for the context gauge: a one-line token breakdown.
  String _contextTooltip(AppLocalizations l10n) {
    final c = _ctx;
    if (c == null) return l10n.contextLabel;
    return '${l10n.contextLabel}: ${_fmtTokens(c.tokensUsed)} / '
        '${_fmtTokens(c.contextWindow)} (${c.percent}%)';
  }

  /// Open the context/quota detail sheet (tap on desktop and mobile). Fetches
  /// the account quota lazily on first open.
  Future<void> _showContextDetail() async {
    if (_rate == null) {
      try {
        final raw = await ref
            .read(bridgeApiProvider)
            .appRateLimits(widget.serviceKey);
        final r = RateLimits.fromRaw(raw);
        if (r != null && mounted) setState(() => _rate = r);
      } catch (_) {
        // Quota is optional; the sheet still shows context usage.
      }
    }
    if (!mounted) return;
    await showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      builder: (c) => _contextSheet(AppLocalizations.of(c)),
    );
  }

  /// Detail-sheet body: context occupancy + 5h / weekly quota bars.
  Widget _contextSheet(AppLocalizations l10n) {
    final text = Theme.of(context).textTheme;
    final ctx = _ctx;
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(20, 4, 20, 24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(l10n.contextUsageTitle, style: text.titleMedium),
            const SizedBox(height: 16),
            if (ctx != null)
              _quotaRow(
                l10n.contextLabel,
                ctx.fraction,
                '${_fmtTokens(ctx.tokensUsed)} / ${_fmtTokens(ctx.contextWindow)}',
              ),
            if (_rate?.primary != null)
              _quotaRow(
                l10n.quota5h,
                _rate!.primary!.fraction,
                '${_rate!.primary!.usedPercent.round()}%',
                reset: _resetText(_rate!.primary!, l10n),
              ),
            if (_rate?.secondary != null)
              _quotaRow(
                l10n.quotaWeekly,
                _rate!.secondary!.fraction,
                '${_rate!.secondary!.usedPercent.round()}%',
                reset: _resetText(_rate!.secondary!, l10n),
              ),
            if (_rate == null)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(
                  l10n.quotaUnavailable,
                  style: text.bodySmall?.copyWith(
                    color: Theme.of(context).colorScheme.outline,
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }

  /// One labelled progress row: label + value on top, a bar, optional reset.
  Widget _quotaRow(
    String label,
    double fraction,
    String value, {
    String? reset,
  }) {
    final text = Theme.of(context).textTheme;
    return Padding(
      padding: const EdgeInsets.only(bottom: 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(child: Text(label, style: text.bodyMedium)),
              Text(value, style: text.bodySmall),
            ],
          ),
          const SizedBox(height: 6),
          ClipRRect(
            borderRadius: BorderRadius.circular(4),
            child: LinearProgressIndicator(
              value: fraction,
              minHeight: 7,
              backgroundColor: Theme.of(
                context,
              ).colorScheme.surfaceContainerHighest,
            ),
          ),
          if (reset != null)
            Padding(
              padding: const EdgeInsets.only(top: 4),
              child: Text(
                reset,
                style: text.bodySmall?.copyWith(
                  color: Theme.of(context).colorScheme.outline,
                ),
              ),
            ),
        ],
      ),
    );
  }

  /// Human "resets in 2h 15m" text for a quota window, or empty if unknown.
  String _resetText(RateLimitWindow w, AppLocalizations l10n) {
    Duration? remaining;
    if (w.resetsInSeconds != null) {
      remaining = Duration(seconds: w.resetsInSeconds!);
    } else if (w.resetsAtEpochMs != null) {
      final ms = w.resetsAtEpochMs! - DateTime.now().millisecondsSinceEpoch;
      if (ms > 0) remaining = Duration(milliseconds: ms);
    }
    if (remaining == null) return '';
    final h = remaining.inHours, m = remaining.inMinutes % 60;
    final span = h > 0 ? '${h}h ${m}m' : '${m}m';
    return l10n.resetsIn(span);
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final width = MediaQuery.of(context).size.width;
    final isMobile = width < 600;
    return Scaffold(
      // On phones the sessions list is a slide-in drawer (hamburger); on wider
      // screens it's an inline collapsible pane (see the body).
      drawer: isMobile
          ? Drawer(child: SafeArea(child: _sessionsPane(l10n, inDrawer: true)))
          : null,
      // Widen the edge-swipe-to-open zone (default ~20px). The narrow default
      // sits under Android's system back-gesture strip, so a left-edge swipe
      // almost always triggered "back" instead of the drawer; a 56px zone lets
      // the swipe start just inside that strip and open the sessions list.
      // (Swipe-to-close already works once the drawer is open.)
      drawerEdgeDragWidth: isMobile ? 56 : null,
      appBar: AppBar(
        // Mobile: the leading button OPENS the sessions list (drawer); "back to
        // projects" lives inside that drawer. Desktop: leading is back-to-projects
        // (the sessions pane is inline, toggled via the action button).
        leading: isMobile
            ? Builder(
                builder: (ctx) => IconButton(
                  tooltip: l10n.conversationsSection,
                  icon: const Icon(Icons.menu),
                  onPressed: () => Scaffold.of(ctx).openDrawer(),
                ),
              )
            : IconButton(
                tooltip: l10n.backToProjects,
                icon: const Icon(Icons.arrow_back),
                onPressed: _backToProjects,
              ),
        title: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(l10n.appServiceTitle, style: const TextStyle(fontSize: 16)),
            if (_cwd != null && _cwd!.trim().isNotEmpty)
              Text(
                '${l10n.currentProject}: ${_projectName()}',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.bodySmall,
              ),
          ],
        ),
        actions: [
          // Desktop only: toggle the inline sessions pane. On mobile the leading
          // button opens the drawer instead, so this is omitted there.
          if (!isMobile)
            IconButton(
              tooltip: l10n.conversationsSection,
              icon: Icon(_leftOpen ? Icons.menu_open : Icons.menu),
              onPressed: () => setState(() => _leftOpen = !_leftOpen),
            ),
          if (_ctx != null)
            _ContextGauge(
              status: _ctx!,
              onTap: _showContextDetail,
              tooltip: _contextTooltip(l10n),
            ),
          if (_threadId != null)
            PopupMenuButton<String>(
              tooltip: l10n.moreActions,
              onSelected: (v) {
                if (v == 'compact') _compact();
              },
              itemBuilder: (c) => [
                PopupMenuItem(value: 'compact', child: Text(l10n.compact)),
              ],
            ),
          const SizedBox(width: 4),
        ],
      ),
      body: LayoutBuilder(
        builder: (context, c) {
          final canLeft = c.maxWidth >= 600; // tablet+ : inline sessions pane
          final canRight = c.maxWidth >= 1100; // desktop : inline diff pane
          return Row(
            children: [
              if (canLeft && _leftOpen) ...[
                SizedBox(width: 280, child: _sessionsPane(l10n)),
                const VerticalDivider(width: 1),
              ],
              Expanded(child: _chatPane(l10n)),
              if (canRight && _rightOpen) ...[
                const VerticalDivider(width: 1),
                SizedBox(width: 420, child: _diffPane(l10n)),
              ],
            ],
          );
        },
      ),
    );
  }

  /// One colored status descriptor for the current session state. Reflects the
  /// REAL state — plan mode is driven by `_planActive` (server-side), so the
  /// indicator never disagrees with how the agent is actually behaving.
  ({Color color, String label, IconData icon}) _sessionState(
    AppLocalizations l10n,
  ) {
    final scheme = Theme.of(context).colorScheme;
    if (_reconnecting) {
      return (
        color: Colors.amber.shade800,
        label: l10n.stateReconnecting,
        icon: Icons.autorenew,
      );
    }
    if (_connectionLost) {
      return (
        color: scheme.error,
        label: l10n.stateDisconnected,
        icon: Icons.cloud_off,
      );
    }
    if (_streaming) {
      return (
        color: scheme.primary,
        label: _planActive ? l10n.statePlanning : l10n.stateWorking,
        icon: Icons.autorenew,
      );
    }
    if (_planActive) {
      return (
        color: Colors.amber.shade800,
        label: l10n.statePlanMode,
        icon: Icons.checklist_rtl,
      );
    }
    return (
      color: Colors.green.shade600,
      label: l10n.stateReady,
      icon: Icons.check_circle,
    );
  }

  /// A thin, always-visible status bar: a colored state chip (plan / working /
  /// ready / disconnected) + the git branch, so the session's true state is
  /// glanceable and consistent with the chat.
  Widget _statusBar(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    final st = _sessionState(l10n);
    final d = _diff;
    return Container(
      width: double.infinity,
      color: st.color.withValues(alpha: 0.10),
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 5),
      child: Row(
        children: [
          Icon(st.icon, size: 13, color: st.color),
          const SizedBox(width: 6),
          Text(
            st.label,
            style: TextStyle(
              fontSize: 12,
              color: st.color,
              fontWeight: FontWeight.w600,
            ),
          ),
          const Spacer(),
          // Live elapsed clock for the running turn — ticks each second next to
          // the working state, frozen + dropped into the transcript on turn end.
          if (_streaming) ...[
            Icon(Icons.schedule, size: 12, color: st.color),
            const SizedBox(width: 4),
            Text(
              _fmtElapsed(_elapsedSecs),
              style: TextStyle(
                fontSize: 12,
                color: st.color,
                fontFeatures: const [FontFeature.tabularFigures()],
              ),
            ),
            const SizedBox(width: 10),
          ],
          // Branch + working-tree change counts, tappable to open the diff.
          // This is the single, unified place the git state lives (no separate
          // app-bar chip), so the status bar is the one source of truth.
          if (_branch != null)
            Tooltip(
              message: (d != null && !d.isEmpty) ? l10n.viewDiff : _branch!,
              child: InkWell(
                onTap: _showDiff,
                borderRadius: BorderRadius.circular(20),
                child: Padding(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 6,
                    vertical: 2,
                  ),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(
                        Icons.account_tree_outlined,
                        size: 12,
                        color: scheme.onSurfaceVariant,
                      ),
                      const SizedBox(width: 4),
                      ConstrainedBox(
                        constraints: const BoxConstraints(maxWidth: 160),
                        child: Text(
                          _branch!,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            fontSize: 12,
                            color: scheme.onSurfaceVariant,
                          ),
                        ),
                      ),
                      if (d != null && !d.isEmpty) ...[
                        const SizedBox(width: 6),
                        Text(
                          '+${d.added}',
                          style: TextStyle(
                            fontSize: 11.5,
                            color: Colors.green.shade600,
                            fontFeatures: const [FontFeature.tabularFigures()],
                          ),
                        ),
                        const SizedBox(width: 3),
                        Text(
                          '−${d.removed}',
                          style: TextStyle(
                            fontSize: 11.5,
                            color: scheme.error,
                            fontFeatures: const [FontFeature.tabularFigures()],
                          ),
                        ),
                      ],
                    ],
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }

  /// The center column: conversation (kept centered with a max width) +
  /// approvals + implement bar + error + composer.
  Widget _chatPane(AppLocalizations l10n) {
    return Column(
      children: [
        _statusBar(l10n),
        Expanded(
          child: AnimatedSwitcher(
            duration: const Duration(milliseconds: 250),
            child: _loading
                ? const ChatLoadingSkeleton(key: ValueKey('chat-loading'))
                : KeyedSubtree(
                    key: const ValueKey('chat-content'),
                    child: _items.isEmpty && !_showTyping
                        // A brand-new conversation (no thread yet) gets a richer
                        // guidance view with tappable starter prompts; an empty
                        // resumed thread keeps the plain hint.
                        ? (_threadId == null
                              ? _newSessionGuidance(l10n)
                              : Center(
                                  child: Text(
                                    l10n.emptyConversation,
                                    style: Theme.of(context)
                                        .textTheme
                                        .bodyMedium
                                        ?.copyWith(
                                          color: Theme.of(
                                            context,
                                          ).colorScheme.outline,
                                        ),
                                  ),
                                ))
                        // One SelectionArea over the whole conversation so text can be
                        // drag-selected and copied (desktop drag, mobile long-press) —
                        // per-message actions appear on hover instead of always-on. The
                        // list is centered with a max width so it reads well even when
                        // both side panes are collapsed on a wide screen.
                        : Stack(
                            children: [
                              // Full-width scroll area so the scrollbar sits at
                              // the window's right edge instead of floating at
                              // the centred column's edge; the conversation
                              // column itself stays centred via horizontal
                              // padding computed from the available width.
                              SelectionArea(
                                child: LayoutBuilder(
                                  builder: (context, constraints) {
                                    final side =
                                        (constraints.maxWidth - 820) / 2;
                                    final pad = side < 16 ? 16.0 : side;
                                    // Materialize the collapsed timeline ONCE per
                                    // build: `_rows` is a getter that re-scans
                                    // `_items` on every access, so reading it for
                                    // itemCount and again per itemBuilder was
                                    // O(n²) per frame. Hoisting it here keeps each
                                    // build O(n).
                                    final rows = _rows;
                                    // SuperListView (super_sliver_list) replaces
                                    // ListView.builder to stabilize the scrollbar:
                                    // it derives scroll extent from per-item
                                    // estimates reconciled against real heights as
                                    // rows pass through the cache area, instead of
                                    // the single running-average estimate that
                                    // makes a plain ListView's thumb jump with the
                                    // wide row-height variance here. Same lazy
                                    // virtualization, same ScrollController — only
                                    // visible rows build, so streaming stays cheap.
                                    return SuperListView.builder(
                                      controller: _scroll,
                                      padding: EdgeInsets.fromLTRB(
                                        pad,
                                        12,
                                        pad,
                                        12,
                                      ),
                                      itemCount:
                                          rows.length + (_showTyping ? 1 : 0),
                                      itemBuilder: (c, i) {
                                        if (i >= rows.length) {
                                          return _TypingIndicator(
                                            elapsed: _fmtElapsed(_elapsedSecs),
                                          );
                                        }
                                        final row = rows[i];
                                        // Stable keys let the sliver's
                                        // extent-reconciliation track each row
                                        // across rebuilds (streaming upserts,
                                        // collapse-into-group transitions) instead
                                        // of recycling element/state by position —
                                        // which otherwise churns measured heights.
                                        // A group keys off its first item's stable
                                        // id plus length so expand/collapse and
                                        // run-growth produce a fresh measurement.
                                        return row is _Group
                                            ? _GroupedActivityCard(
                                                key: ValueKey(
                                                  'g:${row.items.first.id}:'
                                                  '${row.items.length}',
                                                ),
                                                group: row,
                                              )
                                            : _MessageView(
                                                key: ValueKey(
                                                  (row as _Item).id,
                                                ),
                                                item: row,
                                              );
                                      },
                                    );
                                  },
                                ),
                              ),
                              // Jump-to-latest button when scrolled up.
                              if (!_atBottom)
                                Positioned(
                                  right: 0,
                                  left: 0,
                                  bottom: 8,
                                  child: Center(
                                    child: FloatingActionButton.small(
                                      heroTag: null,
                                      elevation: 2,
                                      onPressed: () =>
                                          _scrollToEnd(force: true),
                                      child: const Icon(Icons.arrow_downward),
                                    ),
                                  ),
                                ),
                            ],
                          ),
                  ),
          ),
        ),
        // Inline server requests: a `request_user_input` elicitation renders as
        // an interactive question card (the model is asking the user, not
        // requesting permission); everything else is a command/file/permission
        // approval.
        for (final a in _approvals)
          if (a.kind == 'item/tool/requestUserInput')
            _UserInputCard(prompt: a, onAnswer: _answerUserInput)
          else
            _ApprovalCard(prompt: a, onDecide: _decide),
        // After a plan-mode turn, offer to implement the plan (persists across
        // restart since it's derived from the trailing plan item).
        if (_planReady) _implementBar(l10n),
        if (_error != null) _errorBanner(l10n),
        _composer(l10n),
      ],
    );
  }

  /// Guidance shown for a brand-new, empty conversation: a short intro plus a
  /// few tappable starter prompts tailored to remote-controlling a codex
  /// workspace (explore the project, run/fix tests, review git changes, plan a
  /// feature). Tapping a card prefills the composer — the user reviews and
  /// sends — rather than firing a remote action immediately.
  Widget _newSessionGuidance(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    final suggestions = <(IconData, String, String)>[
      (
        Icons.account_tree_outlined,
        l10n.suggestExploreTitle,
        l10n.suggestExplorePrompt,
      ),
      (Icons.science_outlined, l10n.suggestTestsTitle, l10n.suggestTestsPrompt),
      (
        Icons.difference_outlined,
        l10n.suggestDiffTitle,
        l10n.suggestDiffPrompt,
      ),
      (Icons.checklist_rtl, l10n.suggestPlanTitle, l10n.suggestPlanPrompt),
    ];
    return Center(
      child: SingleChildScrollView(
        padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 32),
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 560),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(Icons.auto_awesome, size: 30, color: scheme.primary),
              const SizedBox(height: 14),
              Text(
                l10n.newSessionTitle,
                textAlign: TextAlign.center,
                style: Theme.of(context).textTheme.titleLarge,
              ),
              const SizedBox(height: 6),
              Text(
                l10n.newSessionSubtitle,
                textAlign: TextAlign.center,
                style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                  color: scheme.onSurfaceVariant,
                ),
              ),
              const SizedBox(height: 22),
              for (final (icon, title, prompt) in suggestions) ...[
                _suggestionCard(icon, title, prompt),
                const SizedBox(height: 10),
              ],
            ],
          ),
        ),
      ),
    );
  }

  /// One tappable starter-prompt card; tapping prefills + focuses the composer.
  Widget _suggestionCard(IconData icon, String title, String prompt) {
    final scheme = Theme.of(context).colorScheme;
    return Material(
      color: scheme.surfaceContainerHighest,
      borderRadius: BorderRadius.circular(14),
      child: InkWell(
        borderRadius: BorderRadius.circular(14),
        onTap: () => _useSuggestion(prompt),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
          child: Row(
            children: [
              Icon(icon, size: 20, color: scheme.primary),
              const SizedBox(width: 14),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(title, style: Theme.of(context).textTheme.titleSmall),
                    const SizedBox(height: 2),
                    Text(
                      prompt,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: Theme.of(context).textTheme.bodySmall?.copyWith(
                        color: scheme.onSurfaceVariant,
                      ),
                    ),
                  ],
                ),
              ),
              const SizedBox(width: 8),
              Icon(Icons.north_east, size: 16, color: scheme.outline),
            ],
          ),
        ),
      ),
    );
  }

  /// Prefill the composer with [prompt] and focus it so the user can review or
  /// edit before sending.
  void _useSuggestion(String prompt) {
    _input.text = prompt;
    _input.selection = TextSelection.collapsed(offset: prompt.length);
    _inputFocus.requestFocus();
  }

  /// Left pane: this project's conversations + a "new session" button. Used
  /// inline on wide screens and inside a [Drawer] on phones. Wrapped in a
  /// [Builder] so the callbacks get a context *under* the Scaffold (a bare
  /// `context` here is the State's, which is above the Scaffold this build
  /// returns — `Scaffold.of` on it would throw).
  Widget _sessionsPane(AppLocalizations l10n, {bool inDrawer = false}) {
    final scheme = Theme.of(context).colorScheme;
    // Live set of running threads for this service, so other sessions show a
    // pulsing badge here too (not just the open one's status bar).
    final running =
        ref.watch(runningThreadsProvider(widget.serviceKey)).valueOrNull ??
        const <String>{};
    // Close the drawer (mobile) if this pane is inside an open one.
    void closeDrawerIfOpen(BuildContext ctx) {
      if (Scaffold.maybeOf(ctx)?.isDrawerOpen ?? false) Navigator.pop(ctx);
    }

    // Filter by the search query, then bucket by recency: running threads go to
    // "Active", today's to "Today", and the rest to "Earlier".
    final q = _convQuery.trim().toLowerCase();
    final filtered = q.isEmpty
        ? _threads
        : _threads
              .where((t) => t.preview.toLowerCase().contains(q))
              .toList(growable: false);
    final now = DateTime.now();
    final active = <ThreadMeta>[];
    final today = <ThreadMeta>[];
    final earlier = <ThreadMeta>[];
    for (final t in filtered) {
      if (running.contains(t.id)) {
        active.add(t);
      } else if (_isSameDay(t.updatedAt, now)) {
        today.add(t);
      } else {
        earlier.add(t);
      }
    }

    return Builder(
      builder: (ctx) {
        Widget tile(ThreadMeta t) => _conversationTile(
          thread: t,
          running: running.contains(t.id),
          selected: t.id == _threadId,
          now: now,
          l10n: l10n,
          onTap: () {
            closeDrawerIfOpen(ctx);
            if (t.id != _threadId) _openThread(t.id, t.cwd);
          },
        );
        final rows = <Widget>[];
        void group(String label, List<ThreadMeta> items) {
          if (items.isEmpty) return;
          rows.add(_sectionLabel(label));
          rows.addAll(items.map(tile));
        }

        group(l10n.groupActive, active);
        group(l10n.groupToday, today);
        group(l10n.groupEarlier, earlier);

        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            // In the mobile drawer, "back to projects" lives here (the AppBar's
            // leading button opens this drawer instead). Desktop shows it as the
            // AppBar leading, so the inline pane omits it.
            if (inDrawer) ...[
              ListTile(
                key: const Key('drawer-back-to-projects'),
                dense: true,
                leading: const Icon(Icons.arrow_back),
                title: Text(l10n.backToProjects),
                onTap: () {
                  closeDrawerIfOpen(ctx);
                  _backToProjects();
                },
              ),
              const Divider(height: 1),
            ],
            // Header: title + a circular "new conversation" button (echoes the
            // composer's send button).
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 14, 12, 2),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      l10n.conversationsSection,
                      style: Theme.of(context).textTheme.titleSmall,
                    ),
                  ),
                  Material(
                    color: scheme.primary,
                    shape: const CircleBorder(),
                    child: InkWell(
                      customBorder: const CircleBorder(),
                      onTap: () {
                        closeDrawerIfOpen(ctx);
                        _openThread(null, _cwd);
                      },
                      child: Tooltip(
                        message: l10n.newConversation,
                        child: Padding(
                          padding: const EdgeInsets.all(6),
                          child: Icon(
                            Icons.add,
                            size: 18,
                            color: scheme.onPrimary,
                          ),
                        ),
                      ),
                    ),
                  ),
                ],
              ),
            ),
            // Current project context.
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
              child: Row(
                children: [
                  Icon(
                    Icons.folder_outlined,
                    size: 13,
                    color: scheme.onSurfaceVariant,
                  ),
                  const SizedBox(width: 5),
                  Expanded(
                    child: Text(
                      _projectName(),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontSize: 12,
                        color: scheme.onSurfaceVariant,
                      ),
                    ),
                  ),
                ],
              ),
            ),
            // Quick filter — shown once there are enough conversations to scan.
            if (_threads.length > 6)
              Padding(
                padding: const EdgeInsets.fromLTRB(12, 0, 12, 8),
                child: TextField(
                  key: const Key('conv-search'),
                  onChanged: (v) => setState(() => _convQuery = v),
                  style: const TextStyle(fontSize: 13),
                  decoration: InputDecoration(
                    isDense: true,
                    prefixIcon: const Icon(Icons.search, size: 18),
                    prefixIconConstraints: const BoxConstraints(
                      minWidth: 34,
                      minHeight: 34,
                    ),
                    hintText: l10n.searchConversations,
                    hintStyle: TextStyle(
                      fontSize: 13,
                      color: scheme.onSurfaceVariant,
                    ),
                    filled: true,
                    fillColor: scheme.surfaceContainerHighest,
                    contentPadding: const EdgeInsets.symmetric(vertical: 9),
                    border: OutlineInputBorder(
                      borderRadius: BorderRadius.circular(10),
                      borderSide: BorderSide.none,
                    ),
                  ),
                ),
              ),
            Expanded(
              child: filtered.isEmpty
                  ? Center(
                      child: Text(
                        q.isEmpty ? l10n.noThreads : l10n.noMatchingThreads,
                        style: TextStyle(color: scheme.outline),
                      ),
                    )
                  : ListView(
                      padding: const EdgeInsets.fromLTRB(8, 2, 8, 8),
                      children: rows,
                    ),
            ),
          ],
        );
      },
    );
  }

  /// A muted section header for the conversations pane (Active / Today / …).
  Widget _sectionLabel(String text) => Padding(
    padding: const EdgeInsets.fromLTRB(8, 10, 8, 4),
    child: Text(
      text,
      style: TextStyle(
        fontSize: 11.5,
        color: Theme.of(context).colorScheme.onSurfaceVariant,
      ),
    ),
  );

  /// One conversation row: a soft rounded tile with a leading icon, the preview
  /// as title, and a relative-time (or "running") subtitle — the same card
  /// language used by the guidance/option cards across the app.
  Widget _conversationTile({
    required ThreadMeta thread,
    required bool running,
    required bool selected,
    required DateTime now,
    required AppLocalizations l10n,
    required VoidCallback onTap,
  }) {
    final scheme = Theme.of(context).colorScheme;
    final fg = selected ? scheme.onPrimaryContainer : scheme.onSurface;
    final muted = selected
        ? scheme.onPrimaryContainer.withValues(alpha: 0.75)
        : scheme.onSurfaceVariant;
    final subtitle = running
        ? l10n.running
        : _relativeTime(thread.updatedAt, now, l10n);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 1),
      child: Material(
        color: selected ? scheme.primaryContainer : Colors.transparent,
        borderRadius: BorderRadius.circular(10),
        child: InkWell(
          borderRadius: BorderRadius.circular(10),
          onTap: onTap,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 9),
            child: Row(
              children: [
                Icon(
                  Icons.chat_bubble_outline,
                  size: 17,
                  color: selected ? scheme.onPrimaryContainer : muted,
                ),
                const SizedBox(width: 11),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        thread.preview.isEmpty
                            ? l10n.untitledThread
                            : thread.preview,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          fontSize: 13,
                          fontWeight: FontWeight.w500,
                          color: fg,
                        ),
                      ),
                      if (subtitle.isNotEmpty) ...[
                        const SizedBox(height: 2),
                        Text(
                          subtitle,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            fontSize: 11.5,
                            color: running ? scheme.primary : muted,
                          ),
                        ),
                      ],
                    ],
                  ),
                ),
                if (running) ...[
                  const SizedBox(width: 8),
                  PulsingDot(color: scheme.primary, size: 7),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }

  /// Whether [unixSeconds] falls on the same calendar day as [now]. A 0/absent
  /// timestamp is treated as not-today (bucketed under "Earlier").
  bool _isSameDay(int unixSeconds, DateTime now) {
    if (unixSeconds <= 0) return false;
    final d = DateTime.fromMillisecondsSinceEpoch(unixSeconds * 1000);
    return d.year == now.year && d.month == now.month && d.day == now.day;
  }

  /// A short localized "time ago" for a thread's last-updated timestamp;
  /// empty when the timestamp is missing (0).
  String _relativeTime(int unixSeconds, DateTime now, AppLocalizations l10n) {
    if (unixSeconds <= 0) return '';
    final then = DateTime.fromMillisecondsSinceEpoch(unixSeconds * 1000);
    final diff = now.difference(then);
    if (diff.inMinutes < 1) return l10n.timeJustNow;
    if (diff.inMinutes < 60) return l10n.timeMinutesAgo(diff.inMinutes);
    if (_isSameDay(unixSeconds, now)) return l10n.timeHoursAgo(diff.inHours);
    final yesterday = now.subtract(const Duration(days: 1));
    if (then.year == yesterday.year &&
        then.month == yesterday.month &&
        then.day == yesterday.day) {
      return l10n.timeYesterday;
    }
    return l10n.timeDaysAgo(diff.inDays);
  }

  /// Right pane: the diff viewer (desktop). Collapsible via its close button.
  Widget _diffPane(AppLocalizations l10n) => _DiffView(
    diff: _diff,
    branch: _branch,
    onClose: () => setState(() => _rightOpen = false),
  );

  /// The "plan ready — implement?" choice bar shown under a finished plan-mode
  /// turn. Keep planning (dismiss) or implement (start a normal turn).
  Widget _implementBar(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      decoration: BoxDecoration(
        color: scheme.primaryContainer,
        borderRadius: BorderRadius.circular(14),
      ),
      padding: const EdgeInsets.fromLTRB(16, 10, 12, 10),
      child: Row(
        children: [
          Icon(Icons.checklist_rtl, size: 18, color: scheme.onPrimaryContainer),
          const SizedBox(width: 10),
          Expanded(
            child: Text(
              l10n.planReadyTitle,
              style: TextStyle(
                color: scheme.onPrimaryContainer,
                fontWeight: FontWeight.w500,
              ),
            ),
          ),
          TextButton(
            onPressed: () => setState(() => _implementDismissed = true),
            child: Text(l10n.keepPlanning),
          ),
          const SizedBox(width: 4),
          FilledButton(
            key: const Key('implement-btn'),
            onPressed: _implement,
            child: Text(l10n.implementPlan),
          ),
        ],
      ),
    );
  }

  Widget _errorBanner(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      decoration: BoxDecoration(
        color: scheme.errorContainer,
        borderRadius: BorderRadius.circular(14),
      ),
      padding: const EdgeInsets.fromLTRB(16, 6, 8, 6),
      child: Row(
        children: [
          Expanded(
            child: KeyedSubtree(
              key: const Key('session-error'),
              child: linkifyText(
                context,
                _error!,
                style: TextStyle(color: scheme.onErrorContainer),
              ),
            ),
          ),
          if (_connectionLost)
            TextButton(onPressed: _autoReconnect, child: Text(l10n.reconnect))
          else if (_retry != null)
            TextButton(onPressed: _retry, child: Text(l10n.retry)),
          IconButton(
            icon: const Icon(Icons.close, size: 18),
            onPressed: () => setState(() => _error = null),
          ),
        ],
      ),
    );
  }

  /// A single rounded composer surface: borderless multiline input on top, a
  /// row of compact setting pills + a circular send button below — modelled on
  /// common AI-chat composers rather than a bare TextField.
  Widget _composer(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    return SafeArea(
      top: false,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(12, 6, 12, 12),
        child: Container(
          decoration: BoxDecoration(
            color: scheme.surfaceContainerHigh,
            borderRadius: BorderRadius.circular(24),
            border: Border.all(color: scheme.outlineVariant),
          ),
          padding: const EdgeInsets.fromLTRB(14, 10, 10, 10),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              TextField(
                controller: _input,
                focusNode: _inputFocus,
                minLines: 1,
                maxLines: 6,
                textInputAction: TextInputAction.send,
                onSubmitted: (_) => _send(),
                style: Theme.of(context).textTheme.bodyLarge,
                decoration: InputDecoration(
                  hintText: l10n.messageHint,
                  border: InputBorder.none,
                  isCollapsed: true,
                ),
              ),
              const SizedBox(height: 10),
              Row(
                crossAxisAlignment: CrossAxisAlignment.end,
                children: [
                  // Settings pills wrap onto extra rows on narrow screens so
                  // none get clipped (a horizontal scroll left the last pill
                  // half-cut on mobile); the send button stays bottom-right.
                  Expanded(
                    child: Wrap(
                      spacing: 6,
                      runSpacing: 6,
                      children: [
                        _pill(
                          icon: Icons.auto_awesome,
                          label: _model?.displayName ?? l10n.modelDefault,
                          onTap: _pickModel,
                        ),
                        _pill(
                          icon: _modeIcon(),
                          label: _mode.label(l10n),
                          onTap: _pickMode,
                        ),
                        _pill(
                          icon: Icons.folder_outlined,
                          label: _projectName(),
                          onTap: _threadId == null ? _pickProject : null,
                        ),
                        // Plan-mode toggle: when on, the agent plans before
                        // implementing. Highlighted while active.
                        _pill(
                          icon: Icons.checklist_rtl,
                          label: l10n.planMode,
                          active: _plan,
                          onTap: () {
                            setState(() {
                              _plan = !_plan;
                              _planToggledByUser = true;
                            });
                            _rememberDefaults();
                            _persistThreadConfig();
                          },
                        ),
                        // Reasoning effort ("thinking level"): shows the effort
                        // the thread will run with (pending pick, else current),
                        // or just "Effort" when none is set (model default).
                        _pill(
                          icon: Icons.psychology_outlined,
                          label: _effectiveEffort == null
                              ? l10n.effort
                              : '${l10n.effort} · ${_effectiveEffort!.label(l10n)}',
                          active: _effectiveEffort != null,
                          onTap: _pickEffort,
                        ),
                      ],
                    ),
                  ),
                  const SizedBox(width: 8),
                  _sendButton(),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  IconData _modeIcon() => _modeIconFor(_mode);

  IconData _modeIconFor(PermissionMode m) => switch (m) {
    PermissionMode.full => Icons.lock_open,
    PermissionMode.readOnly => Icons.lock_outline,
    PermissionMode.auto => Icons.shield_outlined,
  };

  Widget _sendButton() {
    // While a turn is running the send button becomes a stop button (it
    // interrupts the turn), mirroring Gemini / ChatGPT.
    if (_streaming) {
      return IconButton.filled(
        key: const Key('stop-btn'),
        onPressed: _interrupt,
        tooltip: AppLocalizations.of(context).stop,
        icon: const Icon(Icons.stop_rounded, size: 20),
      );
    }
    return ValueListenableBuilder<TextEditingValue>(
      valueListenable: _input,
      builder: (context, value, _) {
        final canSend = !_sending && value.text.trim().isNotEmpty;
        return IconButton.filled(
          key: const Key('send-btn'),
          onPressed: canSend ? () => _send() : null,
          icon: const Icon(Icons.arrow_upward, size: 20),
        );
      },
    );
  }

  /// A compact, low-chrome setting pill (model / permission / project / plan).
  /// [active] highlights a toggled-on pill (e.g. plan mode).
  Widget _pill({
    required IconData icon,
    required String label,
    required VoidCallback? onTap,
    bool active = false,
  }) {
    final scheme = Theme.of(context).colorScheme;
    final enabled = onTap != null;
    final fg = active
        ? scheme.onPrimaryContainer
        : enabled
        ? scheme.onSurfaceVariant
        : scheme.onSurfaceVariant.withValues(alpha: 0.5);
    return Material(
      color: active ? scheme.primaryContainer : scheme.surfaceContainerHighest,
      borderRadius: BorderRadius.circular(20),
      child: InkWell(
        borderRadius: BorderRadius.circular(20),
        onTap: onTap,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 11, vertical: 6),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(icon, size: 15, color: fg),
              const SizedBox(width: 5),
              Text(label, style: TextStyle(fontSize: 12.5, color: fg)),
            ],
          ),
        ),
      ),
    );
  }

  // Cached model list (carries each model's supportedReasoningEfforts), fetched
  // lazily and shared by the model + effort pickers.
  List<ModelInfo> _models = const [];

  Future<List<ModelInfo>> _ensureModels() async {
    if (_models.isNotEmpty) return _models;
    try {
      _models = await ref
          .read(bridgeApiProvider)
          .appModelList(widget.serviceKey);
    } catch (_) {
      // Leave empty; pickers fall back to defaults.
    }
    return _models;
  }

  /// Shared bottom-sheet picker: a titled list of soft option rows (icon +
  /// label + description); the selected one is filled and checked. Returns the
  /// chosen value, or null if dismissed. Used by the model/mode/effort pickers.
  Future<T?> _optionSheet<T>({
    required String title,
    required List<_PickerOption<T>> options,
    required bool Function(T value) isSelected,
  }) {
    final scheme = Theme.of(context).colorScheme;
    return showModalBottomSheet<T>(
      context: context,
      builder: (c) => SafeArea(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Center(
              child: Container(
                width: 36,
                height: 4,
                margin: const EdgeInsets.only(top: 10, bottom: 8),
                decoration: BoxDecoration(
                  color: scheme.outlineVariant,
                  borderRadius: BorderRadius.circular(2),
                ),
              ),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 4, 20, 8),
              child: Text(title, style: Theme.of(context).textTheme.titleSmall),
            ),
            Flexible(
              child: ListView(
                shrinkWrap: true,
                padding: const EdgeInsets.fromLTRB(10, 0, 10, 10),
                children: [
                  for (final o in options)
                    _optionRow(
                      o,
                      isSelected(o.value),
                      () => Navigator.pop(c, o.value),
                    ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  /// One soft option row inside [_optionSheet].
  Widget _optionRow<T>(_PickerOption<T> o, bool selected, VoidCallback onTap) {
    final scheme = Theme.of(context).colorScheme;
    final fg = selected ? scheme.onPrimaryContainer : scheme.onSurface;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Material(
        color: selected ? scheme.primaryContainer : Colors.transparent,
        borderRadius: BorderRadius.circular(12),
        child: InkWell(
          borderRadius: BorderRadius.circular(12),
          onTap: onTap,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 11),
            child: Row(
              children: [
                Icon(
                  o.icon,
                  size: 20,
                  color: selected ? scheme.onPrimaryContainer : scheme.primary,
                ),
                const SizedBox(width: 14),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        o.label,
                        style: TextStyle(
                          fontSize: 14,
                          fontWeight: FontWeight.w500,
                          color: fg,
                        ),
                      ),
                      if (o.description != null &&
                          o.description!.isNotEmpty) ...[
                        const SizedBox(height: 2),
                        Text(
                          o.description!,
                          style: TextStyle(
                            fontSize: 12,
                            color: selected
                                ? scheme.onPrimaryContainer.withValues(
                                    alpha: 0.75,
                                  )
                                : scheme.onSurfaceVariant,
                          ),
                        ),
                      ],
                    ],
                  ),
                ),
                if (selected) ...[
                  const SizedBox(width: 8),
                  Icon(Icons.check, size: 18, color: scheme.onPrimaryContainer),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }

  Future<void> _pickModel() async {
    final l10n = AppLocalizations.of(context);
    final startTid = _threadId;
    final models = await _ensureModels();
    if (!mounted) return;
    final chosen = await _optionSheet<ModelInfo?>(
      title: l10n.modelLabel,
      isSelected: (v) => v?.id == _model?.id,
      options: [
        _PickerOption(
          value: null,
          icon: Icons.star_outline,
          label: l10n.modelDefault,
        ),
        for (final m in models)
          _PickerOption(
            value: m,
            icon: Icons.auto_awesome,
            label: m.displayName,
            description: m.description.isEmpty ? null : m.description,
          ),
      ],
    );
    // Guard a thread switch during the sheet, so the pick + persist target the
    // thread the user was actually looking at.
    if (!mounted || _threadId != startTid) return;
    if (chosen != null || models.isNotEmpty) {
      setState(() {
        _model = chosen;
        // If the new model doesn't support the current effort, fall back to its
        // default (or unset) so we never send a level the model rejects.
        final eff = _effectiveEffort;
        if (chosen != null &&
            eff != null &&
            !chosen.supportedReasoningEfforts.contains(eff.wire)) {
          _effort = ReasoningEffort.fromWire(chosen.defaultReasoningEffort);
          _effortActive = null;
        }
      });
      _rememberDefaults();
      _persistThreadConfig();
    }
  }

  Future<void> _pickMode() async {
    final l10n = AppLocalizations.of(context);
    final startTid = _threadId;
    final chosen = await _optionSheet<PermissionMode>(
      title: l10n.permissionLabel,
      isSelected: (v) => v == _mode,
      options: [
        for (final m in PermissionMode.values)
          _PickerOption(
            value: m,
            icon: _modeIconFor(m),
            label: m.label(l10n),
            description: m.describe(l10n),
          ),
      ],
    );
    if (!mounted || _threadId != startTid) return;
    if (chosen != null) {
      setState(() => _mode = chosen);
      _rememberDefaults();
      _persistThreadConfig();
    }
  }

  IconData _effortIcon(ReasoningEffort e) => switch (e.wire) {
    'none' => Icons.battery_0_bar,
    'minimal' => Icons.battery_2_bar,
    'low' => Icons.battery_3_bar,
    'medium' => Icons.battery_4_bar,
    'high' => Icons.battery_5_bar,
    'xhigh' => Icons.battery_full,
    _ => Icons.bolt, // unknown / custom level the model advertised
  };

  Future<void> _pickEffort() async {
    final l10n = AppLocalizations.of(context);
    final startTid = _threadId;
    // Offer only the levels the active model supports (the selected model, else
    // the default/first) — codex models differ (some support xhigh/minimal but
    // not low/high). Fall back to all known levels if the model lists none.
    // Effort is sticky server-side with no "model default" reset on the wire, so
    // there's no Auto entry; null result == dismissed.
    final models = await _ensureModels();
    if (!mounted) return;
    final model = _model ?? (models.isNotEmpty ? models.first : null);
    final supported = model?.supportedReasoningEfforts ?? const [];
    // Offer exactly what the model advertises (open string list — may include
    // `none`/`xhigh`/custom tokens beyond the classic levels). Fall back to the
    // common levels only when a model lists none.
    final efforts = supported.isEmpty
        ? ReasoningEffort.known
        : [for (final w in supported) ReasoningEffort(w)];
    final chosen = await _optionSheet<ReasoningEffort>(
      title: l10n.effort,
      isSelected: (v) => v == _effectiveEffort,
      options: [
        for (final e in efforts)
          _PickerOption(
            value: e,
            icon: _effortIcon(e),
            label: e.label(l10n),
            description: e.describe(l10n),
          ),
      ],
    );
    if (!mounted || _threadId != startTid) return;
    if (chosen != null) {
      setState(() => _effort = chosen);
      _rememberDefaults();
      _persistThreadConfig();
    }
  }

  Future<void> _pickProject() async {
    final l10n = AppLocalizations.of(context);
    final ctrl = TextEditingController(text: _cwd ?? '');
    final ok = await showDialog<bool>(
      context: context,
      builder: (c) => AlertDialog(
        title: Text(l10n.newProject),
        content: TextField(
          controller: ctrl,
          autofocus: true,
          decoration: InputDecoration(
            labelText: l10n.remotePathLabel,
            hintText: l10n.remotePathHint,
            border: const OutlineInputBorder(),
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(c, false),
            child: Text(l10n.cancel),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(c, true),
            child: Text(l10n.save),
          ),
        ],
      ),
    );
    if (ok == true) setState(() => _cwd = ctrl.text.trim());
  }
}

/// A blocking choice box for a server approval request (run a command, edit
/// files, grant permission, …). Decisions use the v2 wire values
/// (`accept`/`decline`/`acceptForSession`). The request stays pending on the
/// host until answered — even across app restarts (replayed on resume) — so
/// the user can always come back and decide.
/// One question parsed from a `request_user_input` elicitation.
class _UiQuestion {
  const _UiQuestion({
    required this.id,
    required this.header,
    required this.question,
    required this.isOther,
    required this.isSecret,
    required this.options,
  });
  final String id;
  final String header;
  final String question;
  final bool isOther;
  final bool isSecret;
  final List<({String label, String? description})> options;
}

/// Interactive card for an `item/tool/requestUserInput` elicitation: the model
/// is asking the user structured questions (NOT requesting permission to run a
/// command, so "完全放行" does not — and should not — suppress it). Each
/// question's options render as selectable chips; `isOther` adds a free-text
/// field; `isSecret` obscures it. Submitting sends one answer per question id;
/// cancel sends an empty answer set so the turn proceeds without input.
class _UserInputCard extends StatefulWidget {
  const _UserInputCard({required this.prompt, required this.onAnswer});
  final AppEvent prompt;
  final Future<void> Function(AppEvent, Map<String, List<String>>) onAnswer;

  @override
  State<_UserInputCard> createState() => _UserInputCardState();
}

class _UserInputCardState extends State<_UserInputCard> {
  // Sentinel "choice" meaning the free-text 其他 field for a question.
  static const _other = ' other';
  late final List<_UiQuestion> _questions = _parse(widget.prompt.raw);
  final Map<String, String> _choice = {}; // qid -> option label or _other
  final Map<String, TextEditingController> _otherCtrls = {};
  bool _submitting = false;

  @override
  void dispose() {
    for (final c in _otherCtrls.values) {
      c.dispose();
    }
    super.dispose();
  }

  static List<_UiQuestion> _parse(String raw) {
    try {
      final m = jsonDecode(raw) as Map<String, dynamic>;
      final qs = (m['questions'] as List?) ?? const [];
      final out = <_UiQuestion>[];
      for (final q in qs) {
        if (q is! Map<String, dynamic>) continue;
        final id = q['id'] as String?;
        if (id == null || id.isEmpty) continue;
        final opts = <({String label, String? description})>[];
        for (final o in (q['options'] as List?) ?? const []) {
          if (o is! Map<String, dynamic>) continue;
          final label = o['label'] as String?;
          if (label == null || label.isEmpty) continue;
          opts.add((label: label, description: o['description'] as String?));
        }
        out.add(
          _UiQuestion(
            id: id,
            header: (q['header'] as String?) ?? '',
            question: (q['question'] as String?) ?? '',
            isOther: (q['isOther'] as bool?) ?? false,
            isSecret: (q['isSecret'] as bool?) ?? false,
            options: opts,
          ),
        );
      }
      return out;
    } catch (_) {
      return const [];
    }
  }

  TextEditingController _ctrl(String qid) =>
      _otherCtrls.putIfAbsent(qid, TextEditingController.new);

  String? _answer(_UiQuestion q) {
    final c = _choice[q.id];
    if (c == null) return null;
    if (c == _other) {
      final t = _ctrl(q.id).text.trim();
      return t.isEmpty ? null : t;
    }
    return c;
  }

  bool get _complete =>
      _questions.isNotEmpty && _questions.every((q) => _answer(q) != null);

  Future<void> _submit() async {
    final answers = <String, List<String>>{};
    for (final q in _questions) {
      final a = _answer(q);
      if (a != null) answers[q.id] = [a];
    }
    setState(() => _submitting = true);
    await widget.onAnswer(widget.prompt, answers);
  }

  Future<void> _cancel() async {
    setState(() => _submitting = true);
    await widget.onAnswer(widget.prompt, const {});
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    return Container(
      key: const Key('user-input-card'),
      margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      decoration: BoxDecoration(
        color: scheme.surfaceContainerHigh,
        border: Border.all(color: scheme.outlineVariant, width: 0.5),
        borderRadius: BorderRadius.circular(14),
      ),
      child: Padding(
        padding: const EdgeInsets.all(14),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(Icons.help_outline, size: 18, color: scheme.primary),
                const SizedBox(width: 9),
                Expanded(
                  child: Text(
                    l10n.userInputTitle,
                    style: TextStyle(
                      color: scheme.onSurface,
                      fontWeight: FontWeight.w500,
                      fontSize: 14,
                    ),
                  ),
                ),
              ],
            ),
            if (_questions.isEmpty)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: SelectableText(
                  widget.prompt.raw,
                  style: const TextStyle(fontSize: 12),
                ),
              )
            else
              for (final q in _questions) _questionBlock(context, q, scheme),
            const SizedBox(height: 6),
            Wrap(
              alignment: WrapAlignment.end,
              spacing: 8,
              children: [
                TextButton(
                  onPressed: _submitting ? null : _cancel,
                  child: Text(l10n.cancel),
                ),
                FilledButton(
                  key: const Key('user-input-submit'),
                  onPressed: (_complete && !_submitting) ? _submit : null,
                  child: Text(l10n.userInputSubmit),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _questionBlock(BuildContext context, _UiQuestion q, ColorScheme scheme) {
    final l10n = AppLocalizations.of(context);
    return Padding(
      padding: const EdgeInsets.only(top: 10),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (q.header.isNotEmpty)
            Text(
              q.header,
              style: TextStyle(
                fontSize: 12,
                fontWeight: FontWeight.w600,
                color: scheme.onSurfaceVariant,
              ),
            ),
          if (q.question.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: 2),
              child: Text(q.question, style: const TextStyle(fontSize: 13.5)),
            ),
          const SizedBox(height: 6),
          Wrap(
            spacing: 8,
            runSpacing: 6,
            children: [
              for (final o in q.options)
                ChoiceChip(
                  label: Text(o.label),
                  tooltip: (o.description != null && o.description!.isNotEmpty)
                      ? o.description
                      : null,
                  selected: _choice[q.id] == o.label,
                  onSelected: _submitting
                      ? null
                      : (_) => setState(() => _choice[q.id] = o.label),
                ),
              if (q.isOther)
                ChoiceChip(
                  label: Text(l10n.userInputOther),
                  selected: _choice[q.id] == _other,
                  onSelected: _submitting
                      ? null
                      : (_) => setState(() => _choice[q.id] = _other),
                ),
            ],
          ),
          if (q.isOther && _choice[q.id] == _other)
            Padding(
              padding: const EdgeInsets.only(top: 6),
              child: TextField(
                controller: _ctrl(q.id),
                obscureText: q.isSecret,
                onChanged: (_) => setState(() {}),
                style: const TextStyle(fontSize: 13),
                decoration: const InputDecoration(
                  isDense: true,
                  border: OutlineInputBorder(),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

class _ApprovalCard extends StatelessWidget {
  const _ApprovalCard({required this.prompt, required this.onDecide});
  final AppEvent prompt;
  final Future<void> Function(AppEvent, String) onDecide;

  ({IconData icon, String title}) _meta(AppLocalizations l10n) {
    final k = prompt.kind;
    if (k.contains('fileChange')) {
      return (icon: Icons.edit_document, title: l10n.approvalFilePrompt);
    }
    if (k.contains('permissions')) {
      return (
        icon: Icons.shield_outlined,
        title: l10n.approvalPermissionPrompt,
      );
    }
    return (icon: Icons.terminal, title: l10n.approvalPrompt);
  }

  /// Best-effort detail from the request params (command / cwd / reason / files).
  String _detail() {
    try {
      final p = jsonDecode(prompt.raw) as Map<String, dynamic>;
      final parts = <String>[];
      if (p['command'] is String) parts.add(p['command'] as String);
      if (p['cwd'] is String) parts.add('cwd: ${p['cwd']}');
      if (p['reason'] is String) parts.add(p['reason'] as String);
      if (p['changes'] is List) {
        for (final c in (p['changes'] as List)) {
          if (c is Map && c['path'] is String) parts.add(c['path'] as String);
        }
      }
      if (parts.isNotEmpty) return parts.join('\n');
    } catch (_) {}
    return prompt.raw;
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final meta = _meta(l10n);
    return Container(
      key: const Key('approval-card'),
      margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      decoration: BoxDecoration(
        color: scheme.surfaceContainerHigh,
        border: Border.all(color: scheme.outlineVariant, width: 0.5),
        borderRadius: BorderRadius.circular(14),
      ),
      child: Padding(
        padding: const EdgeInsets.all(14),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(meta.icon, size: 18, color: scheme.primary),
                const SizedBox(width: 9),
                Expanded(
                  child: Text(
                    meta.title,
                    style: TextStyle(
                      color: scheme.onSurface,
                      fontWeight: FontWeight.w500,
                      fontSize: 14,
                    ),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 10),
            Container(
              width: double.infinity,
              constraints: const BoxConstraints(maxHeight: 160),
              padding: const EdgeInsets.all(10),
              decoration: BoxDecoration(
                color: scheme.surface.withValues(alpha: 0.6),
                borderRadius: BorderRadius.circular(8),
              ),
              child: SingleChildScrollView(
                child: linkifyText(
                  context,
                  _detail(),
                  selectable: true,
                  style: const TextStyle(
                    fontFamily: 'monospace',
                    fontFamilyFallback: monoCjkFallback,
                    fontSize: 12,
                  ),
                ),
              ),
            ),
            const SizedBox(height: 8),
            Wrap(
              alignment: WrapAlignment.end,
              spacing: 8,
              children: [
                TextButton(
                  onPressed: () => onDecide(prompt, 'decline'),
                  child: Text(l10n.deny),
                ),
                TextButton(
                  onPressed: () => onDecide(prompt, 'acceptForSession'),
                  child: Text(l10n.approveForSession),
                ),
                FilledButton(
                  key: const Key('approve-btn'),
                  onPressed: () => onDecide(prompt, 'accept'),
                  child: Text(l10n.approve),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

/// Compact token count: `840`, `12.3k`, `1.2M`.
String _fmtTokens(int n) {
  if (n < 1000) return '$n';
  if (n < 1000000) return '${(n / 1000).toStringAsFixed(1)}k';
  return '${(n / 1000000).toStringAsFixed(1)}M';
}

/// A small circular context-window gauge for the app bar: a ring filled to the
/// usage fraction with the percent in the middle. Hover shows [tooltip]
/// (desktop); tap opens the detail sheet via [onTap].
class _ContextGauge extends StatelessWidget {
  const _ContextGauge({
    required this.status,
    required this.onTap,
    required this.tooltip,
  });
  final ContextStatus status;
  final VoidCallback onTap;
  final String tooltip;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    // Warn (amber) past 75%, alert (error) past 90%.
    final f = status.fraction;
    final color = f >= 0.9
        ? scheme.error
        : f >= 0.75
        ? Colors.amber.shade700
        : scheme.primary;
    return Tooltip(
      message: tooltip,
      child: InkResponse(
        onTap: onTap,
        radius: 22,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 6),
          child: SizedBox(
            width: 30,
            height: 30,
            child: CustomPaint(
              painter: _GaugePainter(
                fraction: f,
                color: color,
                track: scheme.surfaceContainerHighest,
              ),
              child: Center(
                child: Text(
                  '${status.percent}',
                  style: TextStyle(
                    fontSize: 9.5,
                    fontWeight: FontWeight.w600,
                    color: color,
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _GaugePainter extends CustomPainter {
  _GaugePainter({
    required this.fraction,
    required this.color,
    required this.track,
  });
  final double fraction;
  final Color color;
  final Color track;

  @override
  void paint(Canvas canvas, Size size) {
    const stroke = 3.0;
    final rect = Offset.zero & size;
    final center = rect.center;
    final radius = (size.shortestSide - stroke) / 2;
    final base = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = stroke
      ..color = track;
    canvas.drawCircle(center, radius, base);
    final arc = Paint()
      ..style = PaintingStyle.stroke
      ..strokeCap = StrokeCap.round
      ..strokeWidth = stroke
      ..color = color;
    canvas.drawArc(
      Rect.fromCircle(center: center, radius: radius),
      -1.5708, // start at 12 o'clock
      6.28318 * fraction.clamp(0.0, 1.0),
      false,
      arc,
    );
  }

  @override
  bool shouldRepaint(_GaugePainter old) =>
      old.fraction != fraction || old.color != color || old.track != track;
}

/// Interactive diff viewer: a list of changed files (each expandable) with
/// colour-coded hunks and a copyable path. Used by the mobile bottom sheet and
/// the desktop right pane.
class _DiffView extends StatelessWidget {
  const _DiffView({required this.diff, this.branch, this.onClose});
  final DiffModel? diff;
  final String? branch;

  /// When set, a close button is shown (used by the desktop right pane; the
  /// mobile bottom sheet relies on its drag handle instead).
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final d = diff;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 8, 8, 8),
          child: Row(
            children: [
              Icon(
                Icons.account_tree_outlined,
                size: 16,
                color: scheme.onSurfaceVariant,
              ),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  branch == null
                      ? l10n.changesTitle
                      : '${l10n.changesTitle} · $branch',
                  style: Theme.of(context).textTheme.titleSmall,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              if (d != null && !d.isEmpty) ...[
                Text(
                  '+${d.added}',
                  style: TextStyle(color: Colors.green.shade600),
                ),
                const SizedBox(width: 6),
                Text('−${d.removed}', style: TextStyle(color: scheme.error)),
              ],
              if (onClose != null)
                IconButton(
                  icon: const Icon(Icons.close, size: 18),
                  tooltip: l10n.cancel,
                  onPressed: onClose,
                ),
            ],
          ),
        ),
        const Divider(height: 1),
        Expanded(
          child: (d == null || d.isEmpty)
              ? Center(
                  child: Text(
                    l10n.noChanges,
                    style: TextStyle(color: scheme.outline),
                  ),
                )
              : ListView.builder(
                  itemCount: d.files.length,
                  itemBuilder: (c, i) => _DiffFileTile(file: d.files[i]),
                ),
        ),
      ],
    );
  }
}

class _DiffFileTile extends StatelessWidget {
  const _DiffFileTile({required this.file});
  final DiffFile file;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final l10n = AppLocalizations.of(context);
    return ExpansionTile(
      initiallyExpanded: true,
      tilePadding: const EdgeInsets.symmetric(horizontal: 12),
      title: Row(
        children: [
          Expanded(
            child: Text(
              file.path,
              style: const TextStyle(
                fontFamily: 'monospace',
                fontFamilyFallback: monoCjkFallback,
                fontSize: 12.5,
              ),
              overflow: TextOverflow.ellipsis,
            ),
          ),
          const SizedBox(width: 8),
          Text(
            '+${file.added}',
            style: TextStyle(fontSize: 11, color: Colors.green.shade600),
          ),
          const SizedBox(width: 4),
          Text(
            '−${file.removed}',
            style: TextStyle(fontSize: 11, color: scheme.error),
          ),
          IconButton(
            tooltip: l10n.copy,
            visualDensity: VisualDensity.compact,
            icon: const Icon(Icons.copy_outlined, size: 15),
            onPressed: () => Clipboard.setData(ClipboardData(text: file.path)),
          ),
        ],
      ),
      children: [
        SingleChildScrollView(
          scrollDirection: Axis.horizontal,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              for (final line in file.lines) _diffLineRow(context, line),
            ],
          ),
        ),
      ],
    );
  }

  Widget _diffLineRow(BuildContext context, DiffLine line) {
    final scheme = Theme.of(context).colorScheme;
    final (bg, fg, prefix) = switch (line.kind) {
      DiffLineKind.added => (
        Colors.green.withValues(alpha: 0.14),
        Colors.green.shade800,
        '+',
      ),
      DiffLineKind.removed => (
        scheme.error.withValues(alpha: 0.12),
        scheme.error,
        '−',
      ),
      DiffLineKind.hunk => (
        scheme.primary.withValues(alpha: 0.08),
        scheme.primary,
        ' ',
      ),
      DiffLineKind.context => (
        Colors.transparent,
        scheme.onSurfaceVariant,
        ' ',
      ),
    };
    return Container(
      color: bg,
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 1),
      child: Text(
        line.kind == DiffLineKind.hunk ? line.text : '$prefix${line.text}',
        style: TextStyle(
          fontFamily: 'monospace',
          fontFamilyFallback: monoCjkFallback,
          fontSize: 12,
          color: fg,
        ),
      ),
    );
  }
}

/// Renders one timeline entry. Messages render Gemini-style (user = soft
/// right bubble, agent = full-width Markdown); tool/activity items render as a
/// collapsible [_ActivityCard]. Message copy fades in on hover (desktop);
/// touch uses the enclosing [SelectionArea]'s long-press.
class _MessageView extends StatefulWidget {
  const _MessageView({super.key, required this.item});
  final _Item item;

  @override
  State<_MessageView> createState() => _MessageViewState();
}

class _MessageViewState extends State<_MessageView> {
  // Hover drives only the copy-button fade. Held in a notifier (not setState)
  // so a hover repaint doesn't rebuild the message content — Linkify /
  // MarkdownBody allocate fresh TapGestureRecognizers per link on every build
  // and never dispose the old ones, so rebuilding them on hover leaks.
  final ValueNotifier<bool> _hover = ValueNotifier(false);

  @override
  void dispose() {
    _hover.dispose();
    super.dispose();
  }

  void _copy() {
    final l10n = AppLocalizations.of(context);
    Clipboard.setData(ClipboardData(text: widget.item.text));
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(l10n.copied),
        duration: const Duration(seconds: 1),
        behavior: SnackBarBehavior.floating,
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final item = widget.item;
    // Tool / activity items get specialised rendering: plans → checklist,
    // file changes → reviewable diff, compaction → a system notice; everything
    // else → a subtle single-line activity row.
    if (!item.isMessage) {
      final Widget child = switch (item.type) {
        'plan' => _PlanCard(item: item),
        'fileChange' => _FileChangeCard(item: item),
        'contextCompaction' => _SystemNotice(
          icon: Icons.compress,
          text: AppLocalizations.of(context).compacted,
        ),
        'interrupted' => _SystemNotice(
          icon: Icons.stop_circle_outlined,
          text: AppLocalizations.of(context).turnStopped,
        ),
        'turnDuration' => _TurnDurationFooter(
          duration: item.title,
          completedAt: item.text,
        ),
        _ => _ActivityCard(item: item),
      };
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: 1),
        child: child,
      );
    }

    final scheme = Theme.of(context).colorScheme;
    final isUser = item.isUser;
    final Widget content = isUser
        ? Container(
            constraints: const BoxConstraints(maxWidth: 600),
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 11),
            decoration: BoxDecoration(
              color: scheme.surfaceContainerHigh,
              borderRadius: BorderRadius.circular(20),
            ),
            child: linkifyText(
              context,
              item.text,
              style: Theme.of(
                context,
              ).textTheme.bodyLarge?.copyWith(height: 1.45),
            ),
          )
        : SizedBox(
            width: double.infinity,
            child: MarkdownBody(
              data: autolinkifyMarkdown(item.text),
              selectable: false,
              styleSheet: _markdownStyle(context),
              onTapLink: (text, href, title) =>
                  onTapMarkdownLink(context, text, href, title),
            ),
          );

    final showActions = !item.streaming;
    // Only this subtree rebuilds on hover; `content` above is built once.
    final actions = SizedBox(
      height: 30,
      child: ValueListenableBuilder<bool>(
        valueListenable: _hover,
        builder: (context, hover, _) {
          final visible = hover && showActions;
          return AnimatedOpacity(
            opacity: visible ? 1 : 0,
            duration: const Duration(milliseconds: 120),
            child: IgnorePointer(
              ignoring: !visible,
              child: Align(
                alignment: isUser
                    ? Alignment.centerRight
                    : Alignment.centerLeft,
                child: IconButton(
                  icon: const Icon(Icons.content_copy_outlined, size: 16),
                  visualDensity: VisualDensity.compact,
                  color: scheme.onSurfaceVariant,
                  tooltip: AppLocalizations.of(context).copy,
                  onPressed: _copy,
                ),
              ),
            ),
          );
        },
      ),
    );

    return MouseRegion(
      onEnter: (_) => _hover.value = true,
      onExit: (_) => _hover.value = false,
      child: Padding(
        padding: const EdgeInsets.only(top: 8),
        child: Column(
          crossAxisAlignment: isUser
              ? CrossAxisAlignment.end
              : CrossAxisAlignment.start,
          children: [content, actions],
        ),
      ),
    );
  }
}

/// A centered, subtle system notice (e.g. "conversation compacted") so
/// lifecycle state changes are visible inline in the transcript.
class _SystemNotice extends StatelessWidget {
  const _SystemNotice({required this.icon, required this.text});
  final IconData icon;
  final String text;

  @override
  Widget build(BuildContext context) {
    final muted = Theme.of(context).colorScheme.outline;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 8),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Expanded(child: Divider(color: muted.withValues(alpha: 0.3))),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 10),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(icon, size: 13, color: muted),
                const SizedBox(width: 5),
                Text(text, style: TextStyle(fontSize: 11.5, color: muted)),
              ],
            ),
          ),
          Expanded(child: Divider(color: muted.withValues(alpha: 0.3))),
        ],
      ),
    );
  }
}

/// A per-turn duration footnote dropped in after a turn ends: a subtle
/// `用时 m:ss` tag whose tooltip (hover on desktop, long-press on mobile)
/// reveals the wall-clock completion time `完成于 HH:MM:SS`.
class _TurnDurationFooter extends StatelessWidget {
  const _TurnDurationFooter({
    required this.duration,
    required this.completedAt,
  });

  final String duration;
  final String completedAt;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final muted = Theme.of(context).colorScheme.onSurfaceVariant;
    return Align(
      alignment: Alignment.centerLeft,
      child: Tooltip(
        message: l10n.completedAt(completedAt),
        child: Padding(
          padding: const EdgeInsets.only(left: 6, top: 2, bottom: 4),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(Icons.schedule, size: 12, color: muted),
              const SizedBox(width: 4),
              Text(
                l10n.turnElapsed(duration),
                style: TextStyle(
                  fontSize: 11.5,
                  color: muted,
                  fontFeatures: const [FontFeature.tabularFigures()],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// A reviewable file-change card: a low-chrome header (edited file(s) + ±counts)
/// that expands to the colored per-file diff (reusing [_DiffFileTile]), so the
/// agent's edits can be reviewed inline. Falls back to copyable path rows when
/// no diff text is present.
class _FileChangeCard extends StatefulWidget {
  const _FileChangeCard({required this.item});
  final _Item item;

  @override
  State<_FileChangeCard> createState() => _FileChangeCardState();
}

class _FileChangeCardState extends State<_FileChangeCard> {
  bool _expanded = false;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final muted = scheme.onSurfaceVariant;
    final diff = DiffModel.parse(widget.item.text);
    final hasDiff = !diff.isEmpty;
    final title = widget.item.title.trim();
    final expandable = hasDiff || widget.item.text.trim().isNotEmpty;
    return Container(
      margin: const EdgeInsets.symmetric(vertical: 2),
      decoration: BoxDecoration(
        border: Border.all(color: scheme.outlineVariant, width: 0.5),
        borderRadius: BorderRadius.circular(12),
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          InkWell(
            onTap: expandable
                ? () => setState(() => _expanded = !_expanded)
                : null,
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
              child: Row(
                children: [
                  Icon(Icons.edit_document, size: 17, color: scheme.primary),
                  const SizedBox(width: 10),
                  Text(
                    l10n.toolEdited,
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: scheme.onSurface,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontFamily: 'monospace',
                        fontFamilyFallback: monoCjkFallback,
                        fontSize: 12,
                        color: muted,
                      ),
                    ),
                  ),
                  if (hasDiff) ...[
                    Text(
                      '+${diff.added}',
                      style: TextStyle(
                        fontSize: 11.5,
                        color: Colors.green.shade600,
                      ),
                    ),
                    const SizedBox(width: 3),
                    Text(
                      '−${diff.removed}',
                      style: TextStyle(fontSize: 11.5, color: scheme.error),
                    ),
                    const SizedBox(width: 4),
                  ],
                  if (expandable)
                    Icon(
                      _expanded ? Icons.expand_less : Icons.expand_more,
                      size: 18,
                      color: muted.withValues(alpha: 0.7),
                    ),
                ],
              ),
            ),
          ),
          if (_expanded)
            DecoratedBox(
              decoration: BoxDecoration(
                border: Border(
                  top: BorderSide(color: scheme.outlineVariant, width: 0.5),
                ),
              ),
              // Color goes on a Material (not the box) so the diff's
              // ListTile-based tiles paint their ink/background correctly.
              child: Material(
                color: scheme.surfaceContainerHighest.withValues(alpha: 0.4),
                child: hasDiff
                    ? Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          for (final f in diff.files) _DiffFileTile(file: f),
                        ],
                      )
                    : Padding(
                        padding: const EdgeInsets.all(10),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            for (final p
                                in widget.item.text
                                    .split('\n')
                                    .map((s) => s.trim())
                                    .where((s) => s.isNotEmpty))
                              _CopyablePath(path: p),
                          ],
                        ),
                      ),
              ),
            ),
        ],
      ),
    );
  }
}

/// A monospace file path with a copy button.
class _CopyablePath extends StatelessWidget {
  const _CopyablePath({required this.path});
  final String path;

  @override
  Widget build(BuildContext context) {
    final muted = Theme.of(context).colorScheme.onSurfaceVariant;
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Expanded(
          child: SelectableText(
            path,
            style: const TextStyle(
              fontFamily: 'monospace',
              fontFamilyFallback: monoCjkFallback,
              fontSize: 12,
            ),
          ),
        ),
        InkResponse(
          radius: 16,
          onTap: () => Clipboard.setData(ClipboardData(text: path)),
          child: Padding(
            padding: const EdgeInsets.all(2),
            child: Icon(Icons.copy_outlined, size: 14, color: muted),
          ),
        ),
      ],
    );
  }
}

/// One parsed plan step.
typedef _PlanStep = ({String status, String text});

/// A status-iconed checklist for a `plan` item (codex `update_plan`). The
/// summarizer encodes the plan as an optional explanation plus `- [x|~| ] step`
/// lines; this renders each step with a completed / in-progress / pending icon.
class _PlanCard extends StatefulWidget {
  const _PlanCard({required this.item});
  final _Item item;

  @override
  State<_PlanCard> createState() => _PlanCardState();
}

class _PlanCardState extends State<_PlanCard> {
  bool _expanded = true;

  static final _stepRe = RegExp(r'^\s*-\s*\[(.)\]\s?(.*)$');

  (String explanation, List<_PlanStep> steps) _parse() {
    final explanation = <String>[];
    final steps = <_PlanStep>[];
    for (final line in widget.item.text.split('\n')) {
      final m = _stepRe.firstMatch(line);
      if (m != null) {
        final mark = m.group(1)!;
        final status = mark == 'x'
            ? 'completed'
            : mark == '~'
            ? 'in_progress'
            : 'pending';
        steps.add((status: status, text: m.group(2)!.trim()));
      } else if (line.trim().isNotEmpty) {
        explanation.add(line);
      }
    }
    return (explanation.join('\n'), steps);
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final (explanation, steps) = _parse();
    final done = steps.where((s) => s.status == 'completed').length;
    final muted = scheme.onSurfaceVariant;

    return Container(
      margin: const EdgeInsets.symmetric(vertical: 4),
      decoration: BoxDecoration(
        color: scheme.surfaceContainerHigh.withValues(alpha: 0.5),
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: scheme.outlineVariant),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // Header: icon + "Plan" + progress, tap to collapse.
          InkWell(
            borderRadius: BorderRadius.circular(12),
            onTap: () => setState(() => _expanded = !_expanded),
            child: Padding(
              padding: const EdgeInsets.fromLTRB(12, 10, 10, 10),
              child: Row(
                children: [
                  Icon(
                    Icons.checklist_rounded,
                    size: 17,
                    color: scheme.primary,
                  ),
                  const SizedBox(width: 8),
                  Text(
                    l10n.toolPlan,
                    style: Theme.of(context).textTheme.titleSmall,
                  ),
                  if (steps.isNotEmpty) ...[
                    const SizedBox(width: 8),
                    Text(
                      '$done/${steps.length}',
                      style: TextStyle(fontSize: 12, color: muted),
                    ),
                  ],
                  const Spacer(),
                  Icon(
                    _expanded ? Icons.expand_less : Icons.expand_more,
                    size: 18,
                    color: muted,
                  ),
                ],
              ),
            ),
          ),
          if (_expanded) ...[
            if (explanation.isNotEmpty)
              Padding(
                padding: const EdgeInsets.fromLTRB(14, 0, 14, 8),
                child: MarkdownBody(
                  data: autolinkifyMarkdown(explanation),
                  selectable: true,
                  styleSheet: _markdownStyle(context),
                  onTapLink: (text, href, title) =>
                      onTapMarkdownLink(context, text, href, title),
                ),
              ),
            if (steps.isNotEmpty)
              Padding(
                padding: const EdgeInsets.fromLTRB(12, 0, 12, 10),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [for (final s in steps) _stepRow(s)],
                ),
              ),
          ],
        ],
      ),
    );
  }

  Widget _stepRow(_PlanStep s) {
    final scheme = Theme.of(context).colorScheme;
    final (icon, color) = switch (s.status) {
      'completed' => (Icons.check_circle_rounded, Colors.green.shade600),
      'in_progress' => (Icons.timelapse_rounded, scheme.primary),
      _ => (Icons.radio_button_unchecked, scheme.onSurfaceVariant),
    };
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.only(top: 1, right: 8),
            child: Icon(icon, size: 16, color: color),
          ),
          Expanded(
            child: linkifyText(
              context,
              s.text,
              style: TextStyle(
                fontSize: 13.5,
                height: 1.35,
                color: s.status == 'completed'
                    ? scheme.onSurfaceVariant
                    : scheme.onSurface,
                fontWeight: s.status == 'in_progress'
                    ? FontWeight.w600
                    : FontWeight.normal,
                decoration: s.status == 'completed'
                    ? TextDecoration.lineThrough
                    : null,
                decorationColor: scheme.onSurfaceVariant,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// A run of ≥2 consecutive same-type activity items, collapsed into one row.
class _Group {
  _Group(this.type, this.items);
  final String type;
  final List<_Item> items;
}

/// Collapses a run of same-type tool calls (e.g. several shell commands) into a
/// single low-chrome row ("Ran command ×3") that expands to the individual
/// [_ActivityCard]s — so long tool sequences don't flood the transcript.
class _GroupedActivityCard extends StatefulWidget {
  const _GroupedActivityCard({super.key, required this.group});
  final _Group group;

  @override
  State<_GroupedActivityCard> createState() => _GroupedActivityCardState();
}

class _GroupedActivityCardState extends State<_GroupedActivityCard> {
  bool _expanded = false;

  ({IconData icon, String label}) _meta(AppLocalizations l10n) {
    switch (widget.group.type) {
      case 'webSearch':
        return (icon: Icons.travel_explore, label: l10n.toolSearched);
      case 'commandExecution':
        return (icon: Icons.terminal, label: l10n.toolRan);
      case 'fileChange':
        return (icon: Icons.edit_document, label: l10n.toolEdited);
      case 'mcpToolCall':
      case 'dynamicToolCall':
        return (icon: Icons.extension, label: l10n.toolCalled);
      case 'reasoning':
        return (icon: Icons.lightbulb_outline, label: l10n.toolThinking);
      case 'plan':
        return (icon: Icons.checklist, label: l10n.toolPlan);
      default:
        return (icon: Icons.bolt, label: l10n.toolActivity);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final muted = scheme.onSurfaceVariant;
    final meta = _meta(l10n);
    final n = widget.group.items.length;
    final anyStreaming = widget.group.items.any((i) => i.streaming);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Container(
          margin: const EdgeInsets.symmetric(vertical: 2),
          decoration: BoxDecoration(
            border: Border.all(color: scheme.outlineVariant, width: 0.5),
            borderRadius: BorderRadius.circular(12),
          ),
          child: InkWell(
            borderRadius: BorderRadius.circular(12),
            onTap: () => setState(() => _expanded = !_expanded),
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
              child: Row(
                children: [
                  Icon(meta.icon, size: 17, color: scheme.primary),
                  const SizedBox(width: 10),
                  Text(
                    '${meta.label} ×$n',
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: scheme.onSurface,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                  const Spacer(),
                  if (anyStreaming)
                    SizedBox(
                      width: 12,
                      height: 12,
                      child: CircularProgressIndicator(
                        strokeWidth: 1.8,
                        color: muted,
                      ),
                    )
                  else
                    Icon(
                      _expanded ? Icons.expand_less : Icons.expand_more,
                      size: 18,
                      color: muted.withValues(alpha: 0.7),
                    ),
                ],
              ),
            ),
          ),
        ),
        if (_expanded)
          Padding(
            padding: const EdgeInsets.only(left: 16, top: 2),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                for (final it in widget.group.items)
                  it.type == 'fileChange'
                      ? _FileChangeCard(item: it)
                      : _ActivityCard(item: it),
              ],
            ),
          ),
      ],
    );
  }
}

/// A collapsible card for a tool / activity item (web search, command, file
/// edit, MCP/skill call, reasoning, …) so the user can see — and expand — what
/// the agent is doing, like Codex / Gemini.
class _ActivityCard extends StatefulWidget {
  const _ActivityCard({required this.item});
  final _Item item;

  @override
  State<_ActivityCard> createState() => _ActivityCardState();
}

class _ActivityCardState extends State<_ActivityCard> {
  bool _expanded = false;

  ({IconData icon, String label}) _meta(AppLocalizations l10n) {
    switch (widget.item.type) {
      case 'webSearch':
        return (icon: Icons.travel_explore, label: l10n.toolSearched);
      case 'commandExecution':
        return (icon: Icons.terminal, label: l10n.toolRan);
      case 'fileChange':
        return (icon: Icons.edit_document, label: l10n.toolEdited);
      case 'mcpToolCall':
      case 'dynamicToolCall':
        return (icon: Icons.extension, label: l10n.toolCalled);
      case 'reasoning':
        return (icon: Icons.lightbulb_outline, label: l10n.toolThinking);
      case 'plan':
        return (icon: Icons.checklist, label: l10n.toolPlan);
      default:
        return (icon: Icons.bolt, label: l10n.toolActivity);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final item = widget.item;
    final meta = _meta(l10n);
    final muted = scheme.onSurfaceVariant;
    final title = item.title.trim();
    final detail = item.text.trim();
    // One-line value: the title (command/query/tool) or a peek of the detail.
    final value = title.isNotEmpty
        ? title
        : detail
              .split('\n')
              .firstWhere((s) => s.trim().isNotEmpty, orElse: () => '');
    // Expandable when there's detail or the value is long enough to truncate.
    final expandable = detail.isNotEmpty || value.length > 56;
    final body = [
      if (title.isNotEmpty) title,
      if (detail.isNotEmpty) detail,
    ].join('\n\n');

    // A soft bordered card matching the guidance/option cards: an accent icon,
    // the tool label, a one-line value, and a chevron that expands the detail.
    return Container(
      margin: const EdgeInsets.symmetric(vertical: 2),
      decoration: BoxDecoration(
        border: Border.all(color: scheme.outlineVariant, width: 0.5),
        borderRadius: BorderRadius.circular(12),
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          InkWell(
            onTap: expandable
                ? () => setState(() => _expanded = !_expanded)
                : null,
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
              child: Row(
                children: [
                  Icon(meta.icon, size: 17, color: scheme.primary),
                  const SizedBox(width: 10),
                  Text(
                    meta.label,
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: scheme.onSurface,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                  if (value.isNotEmpty) ...[
                    const SizedBox(width: 8),
                    Expanded(
                      child: Text(
                        value,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          fontFamily: 'monospace',
                          fontFamilyFallback: monoCjkFallback,
                          fontSize: 12,
                          color: muted,
                        ),
                      ),
                    ),
                  ] else
                    const Spacer(),
                  if (item.streaming)
                    SizedBox(
                      width: 12,
                      height: 12,
                      child: CircularProgressIndicator(
                        strokeWidth: 1.8,
                        color: muted,
                      ),
                    )
                  else if (expandable)
                    Icon(
                      _expanded ? Icons.expand_less : Icons.expand_more,
                      size: 18,
                      color: muted.withValues(alpha: 0.7),
                    ),
                ],
              ),
            ),
          ),
          if (_expanded && body.isNotEmpty)
            Container(
              width: double.infinity,
              padding: const EdgeInsets.all(11),
              constraints: const BoxConstraints(maxHeight: 320),
              decoration: BoxDecoration(
                border: Border(
                  top: BorderSide(color: scheme.outlineVariant, width: 0.5),
                ),
                color: scheme.surfaceContainerHighest.withValues(alpha: 0.4),
              ),
              child: SingleChildScrollView(
                child: linkifyText(
                  context,
                  body,
                  selectable: true,
                  style: const TextStyle(
                    fontFamily: 'monospace',
                    fontFamilyFallback: monoCjkFallback,
                    fontSize: 12,
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

/// A three-dot "typing" indicator shown while the model is starting a reply.
class _TypingIndicator extends StatefulWidget {
  const _TypingIndicator({required this.elapsed});

  /// Live elapsed-time label (the same value as the status-bar timer);
  /// empty leaves just the pulsing dots.
  final String elapsed;

  @override
  State<_TypingIndicator> createState() => _TypingIndicatorState();
}

class _TypingIndicatorState extends State<_TypingIndicator>
    with SingleTickerProviderStateMixin {
  late final AnimationController _c = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 1100),
  )..repeat();

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final color = Theme.of(context).colorScheme.onSurfaceVariant;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 14),
      child: Row(
        children: [
          ...List.generate(3, (i) {
            return AnimatedBuilder(
              animation: _c,
              builder: (context, _) {
                // Stagger each dot's pulse.
                final t = (_c.value + i * 0.2) % 1.0;
                final o = 0.3 + 0.7 * (1 - (t - 0.5).abs() * 2).clamp(0.0, 1.0);
                return Padding(
                  padding: const EdgeInsets.only(right: 5),
                  child: Opacity(
                    opacity: o,
                    child: Container(
                      width: 7,
                      height: 7,
                      decoration: BoxDecoration(
                        color: color,
                        shape: BoxShape.circle,
                      ),
                    ),
                  ),
                );
              },
            );
          }),
          // The same live elapsed clock as the status bar, trailing the dots.
          if (widget.elapsed.isNotEmpty) ...[
            const SizedBox(width: 2),
            Text(
              widget.elapsed,
              style: TextStyle(
                fontSize: 12,
                color: color,
                fontFeatures: const [FontFeature.tabularFigures()],
              ),
            ),
          ],
        ],
      ),
    );
  }
}

/// Theme-derived Markdown styling: comfortable line height, tinted code blocks.
MarkdownStyleSheet _markdownStyle(BuildContext context) {
  final theme = Theme.of(context);
  final scheme = theme.colorScheme;
  final body = theme.textTheme.bodyLarge?.copyWith(height: 1.5);
  return MarkdownStyleSheet.fromTheme(theme).copyWith(
    p: body,
    listBullet: body,
    a: linkStyleOf(context),
    pPadding: const EdgeInsets.only(bottom: 8),
    h1Padding: const EdgeInsets.only(top: 8, bottom: 4),
    h2Padding: const EdgeInsets.only(top: 8, bottom: 4),
    h3Padding: const EdgeInsets.only(top: 6, bottom: 4),
    blockSpacing: 10,
    code: theme.textTheme.bodyMedium?.copyWith(
      fontFamily: 'monospace',
      fontFamilyFallback: monoCjkFallback,
      backgroundColor: scheme.surfaceContainerHighest,
    ),
    codeblockDecoration: BoxDecoration(
      color: scheme.surfaceContainerHighest,
      borderRadius: BorderRadius.circular(10),
    ),
    codeblockPadding: const EdgeInsets.all(14),
    blockquoteDecoration: BoxDecoration(
      color: scheme.surfaceContainerHighest,
      borderRadius: BorderRadius.circular(8),
      border: Border(left: BorderSide(color: scheme.primary, width: 3)),
    ),
  );
}
