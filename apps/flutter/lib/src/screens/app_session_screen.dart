import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;

import 'package:desktop_drop/desktop_drop.dart';
import 'package:file_selector/file_selector.dart' show openFiles;
import 'package:flutter/foundation.dart'
    show listEquals, defaultTargetPlatform, TargetPlatform, kIsWeb;
import 'package:flutter/material.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/widgets/window_title_bar.dart';
import 'package:flutter/services.dart';
import 'package:pasteboard/pasteboard.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:image_picker/image_picker.dart';
import 'package:intl/intl.dart' show DateFormat;
import 'package:super_sliver_list/super_sliver_list.dart';
import 'package:window_manager/window_manager.dart' show DragToMoveArea;
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/app_modes.dart';
import 'package:pocket_codex/src/attachment_refs.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/code_highlight.dart';
import 'package:pocket_codex/src/context_status.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/git_diff.dart';
import 'package:pocket_codex/src/ide_context.dart';
import 'package:pocket_codex/src/image_attachments.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/realtime_delegation.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/widgets/adaptive_sheet.dart';
import 'package:pocket_codex/src/widgets/brand_logo.dart';
import 'package:pocket_codex/src/widgets/diff_review.dart';
import 'package:pocket_codex/src/widgets/file_browser_panel.dart';
import 'package:pocket_codex/src/widgets/folder_tree_picker.dart';
import 'package:pocket_codex/src/widgets/links.dart';
import 'package:pocket_codex/src/widgets/loading.dart';
import 'package:pocket_codex/src/widgets/markdown_view.dart';
import 'package:pocket_codex/src/widgets/message_images.dart';
import 'package:pocket_codex/src/widgets/project_menu.dart';
import 'package:pocket_codex/src/widgets/realtime_handoff_card.dart';
import 'package:pocket_codex/src/widgets/status_dots.dart';
import 'package:pocket_codex/src/widgets/takeover_dialog.dart';
import 'package:pocket_codex/src/widgets/theme_toggle.dart';

/// Local port for the app-server ws tunnel (shared with the service screen).
/// `0` is a sentinel: the bridge assigns a free OS port *per service* so several
/// app services can be connected at once. A fixed shared port would let only the
/// first service bind; the rest would hit the probe-bind failure. See
/// [BridgeApi.appConnect].
const appLocalPort = 0;

// Cold-open loads that retry on their own schedule when the first attempt loses
// the race against the connection. See `_retryOpenLoad`.
const String _kThreadsLoad = 'threads';
const String _kCwdSeed = 'cwd';

/// Attempts each cold-open load gets before giving up (1/2/4/8/16s apart).
/// Reaches ~31s of coverage, which is what a desktop auto-host restore needs:
/// the host is a child process this app spawns, and it took ~30s to become
/// ready in the field.
const int _kOpenLoadMaxRetries = 5;

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
    this.home = false,
    this.services = const [],
    this.onSwitchService,
  });

  /// Full `pcx:<device>:app:<name>` key of the connected service.
  final String serviceKey;

  /// Existing thread to resume, or null to start a new one on first message.
  final String? threadId;

  /// Remote working directory to seed a new conversation with.
  final String? cwd;

  /// True when this screen IS the app's home (embedded by [HomeScreen]): the
  /// sessions pane lists every conversation (not just the current project's),
  /// gains a service switcher + management shortcuts, and there is no "back"
  /// destination. False for the classic pushed route (`/app/:key/session`),
  /// which keeps its project-scoped pane and back button.
  final bool home;

  /// Connectable app services for the home-mode service switcher (label +
  /// key). Ignored unless [home].
  final List<ServiceEntry> services;

  /// Called when the user picks another service in the home-mode switcher.
  final void Function(String serviceKey)? onSwitchService;

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
    this.images = const [],
    this.imageUrls = const [],
    this.streaming = false,
    this.model,
    this.effortWire,
    this.modelConfirmed = false,
    this.modelRerouted = false,
    this.turnId = '',
    this.turnCompletedAt,
  });
  final String id;
  String type; // userMessage | agentMessage | commandExecution | webSearch | …
  String title;
  String text;
  // Image attachments of a user message, resolved once (data URLs decoded to
  // bytes; host-only paths kept as chips) so rebuilds never re-decode base64.
  List<ResolvedImage> images;
  // The same attachments' raw wire URLs, kept for CONTENT comparisons (the
  // duplicate-collapse guard): image-only messages all have empty text, so
  // only the URLs distinguish two different photos.
  List<String> imageUrls;
  bool streaming;
  // For a `turnDuration` footnote: the model/effort stamp of the turn it
  // closes (what the turn actually ran with), whether the server confirmed
  // it, and whether the server rerouted the model mid-turn. Local-only.
  String? model;
  String? effortWire;
  bool modelConfirmed;
  bool modelRerouted;

  /// The turn this item belongs to, per the server (`thread/read` nests items
  /// under their turn). Empty when the source carried no turn envelope — a
  /// locally synthesized marker, or a rollout file read from disk.
  String turnId;

  /// Unix seconds when this item's turn completed, when the server said.
  int? turnCompletedAt;

  bool get isUser => type == 'userMessage';
  bool get isAgent => type == 'agentMessage';
  bool get isMessage => isUser || isAgent;

  /// Standalone system notices (compaction / stopped) that render as a centered
  /// divider and must never be folded into a tool-call group.
  bool get isNotice => type == 'contextCompaction' || type == 'interrupted';
}

/// One composer attachment. An IMAGE is processed locally (EXIF-bake /
/// downscale / re-encode in a background isolate) and travels inline as a
/// data URL; a FILE is uploaded to the HOST over the meta tunnel and travels
/// as a path reference in the turn text (codex's native document workflow —
/// its input protocol has no document slot). Either kind shows a spinner chip
/// until [ready].
class _Attachment {
  _Attachment.image({required this.id, required this.name}) : isFile = false;
  _Attachment.file({required this.id, required this.name}) : isFile = true;
  final int id;
  final String name;
  final bool isFile;
  ProcessedImage? processed; // image: null while the isolate is still working
  String? hostPath; // file: null while the upload is still in flight

  /// Whether this attachment is sendable.
  bool get ready => isFile ? hostPath != null : processed != null;
}

/// A message the user composed while a turn was already running. It isn't sent
/// immediately — it waits in [_AppSessionState._queue] and fires as its own turn
/// once the running (and any earlier queued) turns finish (codex-cli parity).
/// The composer draft is snapshotted whole (text + its ready attachments) so the
/// entry can be sent verbatim later, or handed back to the composer on Esc.
class _Queued {
  _Queued({required this.id, required this.text, required this.attachments});
  final int id;
  final String text; // the raw composer text, as typed
  final List<_Attachment> attachments;

  /// One-line preview for the queued-messages strip.
  String get preview {
    final t = text.trim();
    if (t.isNotEmpty) return t;
    return attachments.isEmpty ? '' : attachments.first.name;
  }
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

class _AppSessionState extends ConsumerState<AppSessionScreen>
    with WidgetsBindingObserver {
  final _input = TextEditingController();
  final _inputFocus = FocusNode();
  final _scroll = ScrollController();
  // Index-based scrolling for the transcript (super_sliver_list): powers the
  // prev/next-turn jump buttons via `visibleRange` + `animateToItem`.
  final _listCtl = ListController();
  // Ordered timeline + an id→index map for upserting streamed/updated items.
  final List<_Item> _items = [];
  final Map<String, int> _itemIndex = {};
  int _localSeq = 0; // ids for optimistic local user messages
  final List<AppEvent> _approvals = []; // pending command-approval prompts
  StreamSubscription<AppEvent>? _sub;

  String? _threadId;
  String? _cwd;
  ModelInfo? _model;
  // True while the user holds a model pick that hasn't been sent yet, so a
  // server-confirmed model (thread/settings/updated) doesn't clobber it.
  bool _modelPickPending = false;
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

  // The SERVER-reported runtime config: what this thread's turns actually run
  // with, per the server itself. Seeded from the thread/start・thread/resume
  // response (via thread/read + the bridge cache) and kept fresh by live
  // `thread/settings/updated` notifications. This is ground truth — distinct
  // from the pills above, which are what the user *selected* — and drives the
  // status-bar model indicator so a switch is verifiable, not assumed.
  ThreadRuntimeConfig? _runtime;
  // When _runtime last changed (for the provenance line in the details sheet).
  DateTime? _runtimeAt;

  // Per-turn model stamp: what the CURRENT turn runs with. Captured at
  // turn/started from the freshest server truth — a settings update arrives
  // just before turn/started when a switch happened — falling back to what
  // this app actually sent (turn params override thread defaults, so on older
  // servers that never notify, the sent value IS the effective one).
  // `_turnStampConfirmed` records which source won; `_turnRerouted` flips when
  // the server reports it substituted the model mid-turn (model/rerouted).
  // Embedded into the turn's duration footnote at turn end.
  String? _turnStampModel;
  String? _turnStampEffort;
  bool _turnStampConfirmed = false;
  bool _turnRerouted = false;
  // What the last send put on the wire, the stamp's fallback for servers
  // that never notify settings (turn params override thread defaults, so the
  // sent values ARE the effective ones there).
  String? _sentModel;
  String? _sentEffort;

  /// Composite key for the process-wide per-thread caches. Scoped by service so
  /// two hosts can't collide on a shared thread id (matching how the persisted
  /// config is keyed per service); neither segment contains a space.
  String _threadKey(String threadId) => '${widget.serviceKey} $threadId';

  /// The effort the next turn will run with: a pending pick, else the thread's
  /// current effort. Drives the composer chip and what's sent on every turn.
  ReasoningEffort? get _effectiveEffort => _effort ?? _effortActive;

  // Context-window occupancy + account quota for the status gauge. _ctx seeds
  // from thread/read and updates on thread/tokenUsage/updated; _rate is fetched
  // lazily when the quota popover opens and refreshed on rateLimits events.
  ContextStatus? _ctx;
  RateLimits? _rate;

  /// Bumped whenever [_rate] changes. An open detail panel lives in its own
  /// route, so a parent setState can't reach it — this can.
  final ValueNotifier<int> _quotaRev = ValueNotifier<int>(0);

  // Git: current branch (seeded from thread/read) + the working-tree-vs-main
  // diff, refreshed after edits (turn/diff/updated, turn/completed, compacted).
  String? _branch;
  DiffModel? _diff;

  // A user-initiated diff fetch that the review is waiting on. Fetching a diff
  // on a large repo over the relay takes long enough to need both a spinner and
  // a way out, so the badge becomes a cancel control while this is set.
  //
  // Cancellation is cooperative: the bridge call can't be aborted, so the token
  // is what the completion checks against — a superseded or cancelled fetch
  // still returns, and is then discarded instead of opening the review.
  int _diffFetch = 0;
  bool _diffLoading = false;

  // Desktop layout: left = this project's sessions, right = the diff-review
  // split (a file tree + one file's diff). Both collapsible AND drag-resizable;
  // the chat stays centered regardless. _threads backs the left pane.
  bool _leftOpen = true;
  bool _reviewOpen = false; // the right-hand review split is showing
  double _leftWidth = 280;
  double _reviewWidth = 760; // width of the whole review split (diff + tree)
  double _treeWidth = 250; // the tree sub-pane inside the review
  String? _reviewFile; // path selected in the review, null = first changed

  /// Anchors the desktop turn-settings popover to the composer's model chip.
  final MenuController _modelMenu = MenuController();

  /// Anchors the environment-info popover to the app-bar button.
  final MenuController _envMenu = MenuController();
  List<ThreadMeta> _threads = const [];

  /// Cold-open loads still waiting to succeed → attempts spent so far, plus
  /// each one's pending timer. A key present means "not settled yet"; the
  /// loader removes its key on success. See [_retryOpenLoad].
  final Map<String, int> _openLoadRetries = {};
  final Map<String, Timer> _openLoadTimers = {};

  /// Debounce for the post-turn thread re-list. See [_scheduleThreadsRefresh].
  Timer? _threadsRefreshTimer;

  /// Distinct project roots across every conversation on this service (not
  /// just the current project's), for the project switcher. See [_loadThreads].
  List<String> _allProjects = const [];
  // Live filter text for the conversations pane search box.
  String _convQuery = '';

  // Top-bar title rename: true while the title is a text field (click to
  // enter, Enter/blur to commit, Esc to cancel).
  bool _editingTitle = false;
  final TextEditingController _titleCtrl = TextEditingController();
  final FocusNode _titleFocus = FocusNode();

  /// Monotonic id for rename requests, so a failure can tell whether it is
  /// still the newest attempt. Committing re-opens the field immediately, so a
  /// user can start a second rename while the first is still in flight; rolling
  /// back unconditionally would then restore a title the user has already
  /// replaced — and leave the UI disagreeing with the server for good, since
  /// the second request's success path has nothing left to re-apply.
  int _renameSeq = 0;

  /// Project groups the user has collapsed in the sidebar, keyed by cwd, plus
  /// the ones they expanded past the first [_projectPeek] rows. Both are
  /// view state only — deliberately not persisted, so a restart reads as the
  /// same tidy default.
  final Set<String> _collapsedProjects = {};
  final Set<String> _expandedProjects = {};

  /// How many conversations a project group shows before "show more".
  static const int _projectPeek = 5;

  /// True while the sidebar groups conversations by WHEN they happened (with a
  /// one-line gist each) instead of by which project they belong to. View state
  /// only — a restart returns to the project tree, which is the default because
  /// it answers "where am I working".
  bool _activityView = false;

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
  // A thread held by another app-server stays inside this chat surface. Its
  // rollout follows one host-meta stream until it becomes resumable.
  bool _externalWriterMode = false;
  SessionLiveness? _externalWriterLiveness;
  StreamSubscription<SessionFollowUpdate>? _externalWriterSub;
  Timer? _externalWriterReconnect;
  int _externalWriterEpoch = 0;
  bool _takingOver = false;
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
  // Images (data URLs) sent with the last user message, kept alongside
  // _lastUserText so a one-tap retry after a dropped send re-sends the same
  // attachments.
  List<String> _lastUserImages = const [];
  // Composer attachments not yet sent; an entry with `processed == null` is
  // still being downscaled/re-encoded in a background isolate.
  final List<_Attachment> _attachments = [];
  int _attachSeq = 0; // ids for attachment list entries
  // True while a file is being dragged over the chat (desktop) — shows the
  // "drop to attach" overlay.
  bool _dragging = false;

  // Messages composed while a turn was already in flight. They queue instead of
  // racing the running turn and each flushes as its own turn once the prior one
  // ends (codex-cli parity). Esc pops the most recent back into the composer.
  final List<_Queued> _queue = [];
  int _queueSeq = 0; // ids for queue entries
  // Whether the running turn has produced ANY output yet (reasoning, a tool
  // call, or reply text). Distinguishes "sent, nothing back" — where Esc undoes
  // the send and restores the text — from "output started", where Esc simply
  // interrupts. Reset on each turn/started; set on the first agent-side item.
  bool _outputStarted = false;
  // The raw text of the just-sent message, kept so an Esc "undo" (before any
  // output) can drop it back into the composer. Set at optimistic-send time for
  // ordinary sends only (a programmatic/retry send has no draft to restore).
  String? _undoableText;
  // Set when an interrupt is really an Esc "undo": the turn's end must NOT add a
  // "stopped" marker (the send is being taken back, not shown as stopped).
  bool _suppressStopMarker = false;

  /// Whether to offer the "implement this plan" choice — shown after a plan-mode
  /// turn has produced its proposal, until the user implements or steers past it.
  ///
  /// We deliberately do NOT key on a typed `plan` item: codex delivers the
  /// proposal inconsistently — sometimes a `plan` item, sometimes a plain agent
  /// message (it can even surface literal `<proposed_plan>` tags), and a re-plan
  /// after "keep planning" typically arrives as a plain message. Keying on the
  /// last `plan` item then points at a stale earlier plan with the new steering
  /// message after it, so the choice never re-appears. Instead: the thread is in
  /// plan mode (`_planActive`), no turn is running, the user hasn't dismissed,
  /// and the latest content item is the model's — i.e. the user hasn't steered
  /// since (the last non-reasoning item isn't their message). Normal multi-step
  /// turns also emit `plan` checklists but run in default mode (`_planActive`
  /// false), so they never trigger this. Derived from the timeline, so it
  /// survives leave/restart until the user acts.
  bool get _planReady {
    if (_streaming || _implementDismissed || !_planActive || _items.isEmpty) {
      return false;
    }
    for (final it in _items.reversed) {
      if (it.type == 'reasoning') continue; // trailing reasoning isn't a steer
      return it.type != 'userMessage';
    }
    return false;
  }

  /// Whether the host-meta snapshot says another process still has a live turn.
  bool get _externalWriterRunning =>
      _externalWriterMode && _externalWriterLiveness?.turnState == 'incomplete';

  /// Show the "typing" indicator while a local turn is waiting for its reply,
  /// or throughout an external writer's turn. The latter stays visible even as
  /// transcript snapshots arrive so read-only viewers can tell the feed is
  /// still live rather than looking at a static history dump.
  bool get _showTyping =>
      _externalWriterRunning ||
      (_streaming &&
          (_items.isEmpty || !_items.last.isAgent || _items.last.text.isEmpty));

  /// The timeline collapsed for display: runs of ≥2 consecutive same-type
  /// non-message activity items become a single [_Group] (shown as one
  /// expandable row); everything else stays a [_Item]. Computed at build time
  /// so the flat `_items` upsert path is untouched.
  List<Object> get _rows {
    final out = <Object>[];
    var i = 0;
    while (i < _items.length) {
      final it = _items[i];
      // One reply, one block. A turn's prose arrives as several `agentMessage`
      // items (the server gives each its own id — a preamble before a tool
      // batch, then the final answer), and rendering one block per item chopped
      // a single answer into pieces that each carried their own hover actions.
      //
      // The run is bounded by the server's own `turnId` where it is known, so
      // this is the real turn boundary rather than "consecutive agent prose".
      // Items whose turn is unknown (empty id — a rollout file read from disk,
      // or a live item that arrived before `turn/started`) fall back to
      // adjacency, which is what the sequence can tell us.
      if (it.isAgent) {
        var j = i + 1;
        while (j < _items.length &&
            _items[j].isAgent &&
            _items[j].turnId == it.turnId) {
          j++;
        }
        out.add(j - i >= 2 ? _AgentTurn(_items.sublist(i, j)) : it);
        i = j;
        continue;
      }
      // User messages and standalone notices are never grouped.
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
    // Remember where the user is chatting so the next cold start (and the
    // chat-first home) lands right back here. Deferred: provider writes are
    // not allowed while the tree is building. A thread-less mount (fresh
    // conversation) records nothing — the previous "last conversation" stays
    // the restore target until this one actually exists (first send).
    final initialThread = _threadId;
    Future.microtask(() {
      if (!mounted) return;
      final prefs = ref.read(uiPrefsProvider.notifier)
        ..setLastService(widget.serviceKey);
      if (initialThread != null) {
        prefs.setLastThread(widget.serviceKey, initialThread);
      }
    });
    // A brand-new conversation inherits the user's last-chosen model / mode /
    // plan / effort instead of resetting to hard defaults.
    if (_threadId == null) _seedDefaults();
    // ...and defaults its working folder to the host's configured default
    // project when the caller didn't seed one (async; won't override a picked
    // cwd — see the guard in _seedDefaultCwd).
    if (_threadId == null && (_cwd == null || _cwd!.trim().isEmpty)) {
      _seedDefaultCwd();
    }
    _scroll.addListener(_onScroll);
    // Focusing the composer raises the soft keyboard, which shrinks the
    // transcript viewport from the bottom — whatever the user was reading
    // slides up behind the keyboard. Re-pin to the latest message once the
    // inset has settled so the tail of the conversation stays in view.
    _inputFocus.addListener(_onComposerFocus);
    WidgetsBinding.instance.addObserver(this); // keyboard-inset metrics
    // Desktop: intercept Ctrl/Cmd+V so a clipboard image/file also attaches
    // (text paste still works — the handler never consumes the event).
    if (_isDesktop) HardwareKeyboard.instance.addHandler(_onHardwareKey);
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

  /// Load the conversations for the left sessions pane. As the home screen the
  /// pane lists EVERY conversation on the service (the user asked for "all
  /// sessions in the sidebar"); the classic pushed route keeps its
  /// project-scoped view (the project tree it was opened from already gives
  /// the full picture there).
  Future<void> _loadThreads() async {
    try {
      final all = await ref
          .read(bridgeApiProvider)
          .appThreadList(widget.serviceKey);
      // Every project this service has ever been used in, newest first. Taken
      // from the UNFILTERED list on purpose: the project switcher has to offer
      // the projects you're not in, which is exactly what `mine` drops below.
      final projects = <String>[];
      final seenProjects = <String>{};
      for (final t in all) {
        final p = t.cwd.trim();
        if (p.isNotEmpty && seenProjects.add(p)) projects.add(p);
      }
      _allProjects = projects;
      final cwd = _cwd?.trim();
      var mine = (widget.home || cwd == null || cwd.isEmpty)
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
      _openLoadDone(_kThreadsLoad);
      // Reaching here proves the service answers, which the subscribe-time
      // quota fetch can't assume on a cold open (it races the connection).
      // A brand-new conversation never reads a thread, so this is the only
      // retry it gets.
      if (_rate == null) _loadQuota();
    } catch (_) {
      // Listing is best-effort for the CONTENT — a failure leaves whatever the
      // pane already had. But a cold open racing the connection (or a host that
      // isn't up yet) must not strand the pane empty: nothing else re-lists
      // except a reconnect or a sent message, so retry a few times here.
      if (mounted) _retryOpenLoad(_kThreadsLoad);
    }
  }

  /// Re-attempt a cold-open load that failed, with a widening backoff.
  ///
  /// Every load kicked off in `initState` races the connection: the screen
  /// mounts and fires immediately, while the socket may still be handshaking —
  /// and on desktop the host itself can be a CHILD PROCESS THIS APP IS STILL
  /// SPAWNING (auto-host restore takes ~30s, far longer than the first
  /// attempt). Those loads all `catch (_)` because none of them is worth an
  /// error banner, which meant one lost race stranded the surface for the whole
  /// session: an empty conversation list, or a new chat silently rooted in the
  /// wrong folder.
  ///
  /// [key] identifies the load, so each retries on its own schedule and a
  /// success stops only its own chain. Retries stop once the load succeeds
  /// (the loader clears the key) or the budget runs out; a later failure with
  /// data already in hand is left alone, since something is on screen and the
  /// reconnect path will refresh it.
  void _retryOpenLoad(String key) {
    final attempt = _openLoadRetries[key] ?? 0;
    if (attempt >= _kOpenLoadMaxRetries) return;
    _openLoadRetries[key] = attempt + 1;
    _openLoadTimers[key]?.cancel();
    _openLoadTimers[key] = Timer(Duration(seconds: 1 << attempt), () {
      // 1/2/4/8/16s
      if (!mounted || !_openLoadRetries.containsKey(key)) return;
      switch (key) {
        case _kThreadsLoad:
          _loadThreads();
        case _kCwdSeed:
          _seedDefaultCwd();
      }
    });
  }

  /// Mark a cold-open load as settled, so its retry chain stops.
  void _openLoadDone(String key) {
    _openLoadRetries.remove(key);
    _openLoadTimers.remove(key)?.cancel();
  }

  /// Seed a brand-new conversation's settings from the user's last choices
  /// ([sessionDefaultsProvider]) so it inherits them instead of resetting to
  /// hard defaults. Assigns fields directly; the caller handles setState/timing.
  void _seedDefaults() {
    final d = ref.read(sessionDefaultsProvider(widget.serviceKey));
    _model = d.model;
    _modelPickPending = false;
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

  /// Parse a `thread/settings/updated` notification's raw params into a
  /// runtime config. The payload's `threadSettings` carries the full effective
  /// snapshot: `model`, `modelProvider`, `effort`, `approvalPolicy`,
  /// `sandboxPolicy` (camelCase-tagged object) and `collaborationMode`
  /// (`{mode, settings}`). Null on any shape surprise — never guess.
  static ThreadRuntimeConfig? _parseSettingsUpdate(String raw) {
    try {
      final params = jsonDecode(raw) as Map<String, dynamic>;
      final s = params['threadSettings'];
      if (s is! Map<String, dynamic>) return null;
      String? str(Object? v) => (v is String && v.isNotEmpty) ? v : null;
      // Sandbox: `{"type": "readOnly"}` → the kebab wire string the app speaks.
      String? sandbox;
      final sp = s['sandboxPolicy'];
      final tag = sp is Map<String, dynamic> ? str(sp['type']) : str(sp);
      if (tag != null) {
        sandbox = switch (tag) {
          'readOnly' => 'read-only',
          'workspaceWrite' => 'workspace-write',
          'dangerFullAccess' => 'danger-full-access',
          'externalSandbox' => 'external-sandbox',
          _ => tag,
        };
      }
      // Approval: a plain string, or the externally-tagged granular object.
      final ap = s['approvalPolicy'];
      final approval = ap is Map<String, dynamic>
          ? (ap.keys.isEmpty ? null : ap.keys.first)
          : str(ap);
      // Collaboration mode: `{mode: "plan"|"default", …}` or a bare string.
      final cm = s['collaborationMode'];
      final collab = cm is Map<String, dynamic> ? str(cm['mode']) : str(cm);
      return ThreadRuntimeConfig(
        model: str(s['model']),
        modelProvider: str(s['modelProvider']),
        reasoningEffort: str(s['effort']),
        approvalPolicy: approval,
        sandboxMode: sandbox,
        collaborationMode: collab,
        confirmedByUpdate: true,
      );
    } catch (_) {
      return null;
    }
  }

  /// Adopt a server-reported runtime config. A snapshot from a live
  /// `thread/settings/updated` ([ThreadRuntimeConfig.confirmedByUpdate]) is
  /// authoritative for the sticky selection state too — effort, plan mode and
  /// the model pill sync to it so the composer can never drift from the
  /// server. A start/resume snapshot only refreshes the indicator: it may
  /// predate settings this app already sent (older servers never notify), so
  /// it must not roll the selections back.
  void _applyRuntime(ThreadRuntimeConfig cfg) {
    if (!mounted) return;
    setState(() {
      _runtime = cfg;
      _runtimeAt = DateTime.now();
      if (!cfg.confirmedByUpdate) return;
      _effortActive = ReasoningEffort.fromWire(cfg.reasoningEffort);
      final collab = cfg.collaborationMode;
      if (collab != null) {
        _planActive = collab == 'plan';
        if (!_planToggledByUser) _plan = _planActive;
      }
      // Reflect the confirmed model onto the model pill — unless the user is
      // holding a different, not-yet-sent pick. An id missing from the model
      // list (e.g. an unlisted config default) still shows via the status-bar
      // indicator, which renders the raw id.
      final m = cfg.model;
      if (m != null && !_modelPickPending && _model?.id != m) {
        final resolved = _models.where((x) => x.id == m).firstOrNull;
        if (resolved != null) _model = resolved;
      }
    });
    if (!cfg.confirmedByUpdate) return;
    // Keep the per-thread memory on server truth so reopening restores it.
    final tid = _threadId;
    if (tid != null) {
      _effortByThread[_threadKey(tid)] = _effortActive;
      if (cfg.collaborationMode != null) {
        _planByThread[_threadKey(tid)] = _planActive;
      }
    }
  }

  /// Pull the bridge's cached runtime config (fed by start/resume responses
  /// and live settings notifications) — cheap, no RPC. Covers the gaps events
  /// can't: the thread/start response of a brand-new conversation, and
  /// notifications that streamed by while no screen was attached.
  void _refreshRuntimeFromCache() {
    final tid = _threadId;
    if (tid == null) return;
    ThreadRuntimeConfig? cfg;
    try {
      cfg = ref
          .read(bridgeApiProvider)
          .appThreadRuntimeConfig(widget.serviceKey, tid);
    } catch (_) {
      return; // best-effort
    }
    if (cfg != null) _applyRuntime(cfg);
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
    // Keep the "last conversation" record fresh for the chat-first home. A
    // new (id-less) conversation records nothing until its first send — an
    // abandoned draft shouldn't cost the user their restore target.
    if (tid != null) {
      ref.read(uiPrefsProvider.notifier).setLastThread(widget.serviceKey, tid);
    }
    _cancelExternalWriterSubscription();
    setState(() {
      _threadId = tid;
      _cwd = cwd;
      _externalWriterMode = false;
      _externalWriterLiveness = null;
      _takingOver = false;
      // An open rename belongs to the thread being left, so drop it rather
      // than let it commit the old title onto the new conversation.
      _editingTitle = false;
      _items.clear();
      _itemIndex.clear();
      _approvals.clear();
      _ctx = null;
      _diff = null;
      _branch = null;
      _reviewOpen = false;
      _reviewFile = null;
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
      _modelPickPending = false;
      _mode = PermissionMode.auto;
      _plan = false;
      _planToggledByUser = false;
      // Drop the previous thread's effort (pending pick + active) so an unsent
      // pick on the old thread can't leak into this one; _resumeAndLoad re-seeds
      // _effortActive from the server / per-thread memory.
      _effort = null;
      _effortActive = null;
      // The runtime config and turn stamps describe the previous thread.
      _runtime = null;
      _runtimeAt = null;
      _turnStampModel = null;
      _turnStampEffort = null;
      _turnStampConfirmed = false;
      _turnRerouted = false;
      _sentModel = null;
      _sentEffort = null;
      _implementDismissed = false;
      _input.clear();
      // Pending attachments are drafts of the previous thread's message —
      // clear them with the input (and the retry snapshot, which references a
      // turn on the previous thread).
      _attachments.clear();
      // The queue + undo state belong to the previous conversation.
      _queue.clear();
      _outputStarted = false;
      _undoableText = null;
      _suppressStopMarker = false;
      _lastUserText = null;
      _lastUserImages = const [];
      // A brand-new conversation inherits the user's last-chosen settings; an
      // existing one is restored from the server by _resumeAndLoad below.
      if (tid == null) _seedDefaults();
    });
    if (tid != null) _resumeAndLoad();
  }

  @override
  void deactivate() {
    // Stop claiming this link is down once nobody is watching it: the flag is an
    // OBSERVATION by an open conversation, and leaving it set would keep the
    // service red on the strength of a screen that no longer exists.
    //
    // Here rather than in `dispose`: Riverpod forbids touching `ref` once the
    // element is disposed, and doing so threw while finalizing the widget tree.
    final observed = ref.read(observedDisconnectedProvider.notifier);
    if (observed.state.contains(widget.serviceKey)) {
      observed.state = {...observed.state}..remove(widget.serviceKey);
    }
    super.deactivate();
  }

  @override
  void dispose() {
    _healthTimer?.cancel();
    _externalWriterReconnect?.cancel();
    final externalWriterSub = _externalWriterSub;
    if (externalWriterSub != null) unawaited(externalWriterSub.cancel());
    for (final t in _openLoadTimers.values) {
      t.cancel();
    }
    _threadsRefreshTimer?.cancel();
    _elapsedTicker?.cancel();
    _sub?.cancel();
    _input.dispose();
    WidgetsBinding.instance.removeObserver(this);
    _inputFocus.removeListener(_onComposerFocus);
    _inputFocus.dispose();
    if (_isDesktop) HardwareKeyboard.instance.removeHandler(_onHardwareKey);
    _scroll.removeListener(_onScroll);
    _scroll.dispose();
    _listCtl.dispose();
    _quotaRev.dispose();
    _titleCtrl.dispose();
    _titleFocus.dispose();
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

  /// Read a host-side image so it can render as a thumbnail instead of a
  /// filename chip:
  ///
  ///  * the host IS this machine (locally hosted service) — read the file off
  ///    disk, no tunnel involved;
  ///  * a remote host — ask for it as a transcript-referenced image, which the
  ///    host authorises against this thread's own user messages, so a pasted
  ///    screenshot in its temp directory is reachable without granting a
  ///    general file read;
  ///  * a host too old to serve that route — fall back to the root-confined
  ///    file read, which still covers an image that lives in a project folder.
  ///
  /// Null on any refusal, so the caller falls back to the chip.
  Future<Uint8List?> _loadHostImage(String path) async {
    final local = (ref.read(localServeListProvider).valueOrNull ?? const [])
        .any((h) => h.appServiceKey == widget.serviceKey);
    if (local) return readLocalImage(path);
    final api = ref.read(bridgeApiProvider);
    final threadId = _threadId;
    if (threadId != null) {
      final bytes = await _tryRead(
        () => api.metaReadThreadImage(widget.serviceKey, threadId, path),
      );
      if (bytes != null) return bytes;
    }
    return _tryRead(() => api.metaReadFile(widget.serviceKey, path));
  }

  /// Run a host read, mapping every refusal — outside the roots, unreferenced,
  /// route missing on an older host, file gone — to null, plus the two byte
  /// counts that aren't worth drawing.
  Future<Uint8List?> _tryRead(Future<Uint8List> Function() read) async {
    try {
      final bytes = await read();
      if (bytes.isEmpty || bytes.length > kMaxInlineImageBytes) return null;
      return bytes;
    } catch (_) {
      return null;
    }
  }

  /// Keep the tail of the conversation visible when the soft keyboard opens.
  /// Only when the user was already at the bottom — pulling someone back down
  /// while they're reading history would be worse than the keyboard.
  void _onComposerFocus() => _repinForKeyboard();

  /// The keyboard inset animates in over several frames, and each frame shrinks
  /// the transcript viewport a little more. `_scrollToEnd`'s settle loop gives
  /// up at the first frame that doesn't grow the extent — which an easing curve
  /// produces early on — so re-pin on every metrics change too, for as long as
  /// the composer holds focus.
  @override
  void didChangeMetrics() => _repinForKeyboard();

  void _repinForKeyboard() {
    if (isDesktop || !_inputFocus.hasFocus || !_atBottom) return;
    _scrollToEnd(force: true);
  }

  void _subscribe() {
    _sub?.cancel();
    // Warm the quota with the subscription rather than on the first tap: the
    // sidebar shows it inline, and a popover that spins for a round-trip every
    // time reads as the app being slow. From here on `account/rateLimits/
    // updated` keeps it fresh, so this fetch happens once per connection.
    _loadQuota();
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

  void _replaceTranscriptItems(List<ThreadItem> items) {
    _items.clear();
    _itemIndex.clear();
    for (final item in items) {
      // Defensively collapse a back-to-back duplicate user message. A genuine
      // re-ask has the model's reply in between, so it remains distinct.
      if (item.itemType == 'userMessage' &&
          _items.isNotEmpty &&
          _items.last.type == 'userMessage' &&
          _items.last.text.trim() == item.text.trim() &&
          listEquals(_items.last.imageUrls, item.images)) {
        continue;
      }
      if (item.itemType == 'userMessage' && isContextFragment(item.text)) {
        continue;
      }
      _itemIndex[item.id] = _items.length;
      _items.add(
        _Item(
          id: item.id,
          type: item.itemType,
          title: item.title,
          text: item.text,
          images: resolveImageUrls(item.images),
          imageUrls: item.images,
          streaming:
              item.itemType == 'contextCompaction' &&
              item.title == 'inProgress',
          turnId: item.turnId,
          turnCompletedAt: item.turnCompletedAt,
        ),
      );
    }
  }

  /// Open an existing thread: resume it into the session (so reads and turns
  /// resolve — otherwise the server returns "thread not found"), then load
  /// its history.
  Future<void> _resumeAndLoad() async {
    // Guard: a stale event (e.g. thread/compacted from a prior thread) can
    // arrive after switching to a new, unsaved conversation — don't `_threadId!`
    // through a null here.
    if (_threadId == null || _externalWriterMode) return;
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
      // Restore the model from the server's own report first (the resume
      // response says what the thread actually runs with); fall back to the
      // persisted pick for older servers that don't report one. Resolve the id
      // against this service's model list.
      final restoredModelId = history.model ?? persisted.model;
      ModelInfo? restoredModel;
      if (restoredModelId != null) {
        try {
          final models = await _ensureModels();
          restoredModel = models
              .where((m) => m.id == restoredModelId)
              .firstOrNull;
        } catch (_) {
          // Model list unavailable — leave the model unchanged.
        }
      }
      // The user may have switched threads during the awaits above.
      if (!mounted || _threadId != startTid) return;
      setState(() {
        _loading = false;
        _replaceTranscriptItems(history.items);
        // Restore the "thinking" state if a turn was still running when we
        // left: live events (delivered after resume) will finish rendering it.
        _streaming = history.running;
        // We can't tell whether a resumed turn has already produced output, and
        // it wasn't sent from this composer, so there's nothing to un-send —
        // treat it as output-started so Esc interrupts (with a marker) instead.
        _outputStarted = history.running;
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
        // The server-reported runtime config (model / effort / permissions the
        // thread actually runs with) — ground truth for the status-bar model
        // indicator. Absent on older servers that don't report it.
        if (history.model != null ||
            history.approvalPolicy != null ||
            history.sandboxMode != null ||
            history.reasoningEffort != null) {
          _runtime = ThreadRuntimeConfig(
            model: history.model,
            modelProvider: history.modelProvider,
            reasoningEffort: history.reasoningEffort,
            approvalPolicy: history.approvalPolicy,
            sandboxMode: history.sandboxMode,
            collaborationMode: history.collaborationMode,
            confirmedByUpdate: history.configConfirmed,
          );
          _runtimeAt = DateTime.now();
        }
        // Permission mode: the server's reported approval+sandbox pair wins
        // when it maps onto one of our presets; else the persisted pick (an
        // unmapped server combo still shows raw in the runtime sheet).
        final serverPreset = (history.approvalPolicy == null)
            ? null
            : PermissionMode.values
                  .where(
                    (m) =>
                        m.approval == history.approvalPolicy &&
                        m.sandbox == history.sandboxMode,
                  )
                  .firstOrNull;
        final persistedMode = persisted.permissionMode == null
            ? null
            : PermissionMode.values
                  .where((m) => m.name == persisted.permissionMode)
                  .firstOrNull;
        final restoredMode = serverPreset ?? persistedMode;
        if (restoredMode != null) _mode = restoredMode;
        if (restoredModel != null) _model = restoredModel;
        final serverCollab = history.collaborationMode;
        final restored = serverCollab != null
            ? serverCollab == 'plan'
            : (persisted.planMode ??
                  (tid != null && (_planByThread[_threadKey(tid)] ?? false)));
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
            (tid != null ? _effortByThread[_threadKey(tid)] : null);
        // Drop a restored effort the restored model can't run (mirrors the guard
        // in _pickModel/_seedDefaults) so a stale persisted pairing never asserts
        // an unsupported level on the next turn.
        final guardModel = _model;
        final guardEffort = _effortActive;
        if (guardModel != null &&
            guardEffort != null &&
            !guardModel.supportedReasoningEfforts.contains(guardEffort.wire)) {
          _effortActive = ReasoningEffort.fromWire(
            guardModel.defaultReasoningEffort,
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
      // The subscribe-time quota fetch races the connection coming up (and
      // loses, on a cold open). A successful read proves the service is
      // reachable, so this is the attempt that actually lands.
      if (_rate == null) _loadQuota();
      _scrollToEnd(force: true);
      // A turn may have completed while we were disconnected: if the reload
      // landed idle with messages still queued, drain the backlog now (turn-end
      // events that would normally flush it were missed during the drop).
      _maybeFlushQueue();
    } catch (e) {
      if (!mounted || _threadId != startTid) return;
      if (_isActiveWriterError(e)) {
        _enterExternalWriterMode(startTid);
        return;
      }
      setState(() {
        _loading = false;
        _error = friendlyError(e);
        _retry = _resumeAndLoad;
      });
    }
  }

  bool _isActiveWriterError(Object error) =>
      error.toString().toLowerCase().contains('active writer');

  void _cancelExternalWriterSubscription() {
    _externalWriterReconnect?.cancel();
    _externalWriterReconnect = null;
    final sub = _externalWriterSub;
    _externalWriterSub = null;
    if (sub != null) unawaited(sub.cancel());
    if (_externalWriterMode) {
      _elapsedTicker?.cancel();
      _elapsedTicker = null;
      _turnStartedAt = null;
    }
    _externalWriterEpoch++;
  }

  void _enterExternalWriterMode(String threadId) {
    _cancelExternalWriterSubscription();
    final epoch = _externalWriterEpoch;
    _elapsedTicker?.cancel();
    _elapsedTicker = null;
    _turnStartedAt = null;
    setState(() {
      _externalWriterMode = true;
      _externalWriterLiveness = null;
      _takingOver = false;
      _loading = true;
      _streaming = false;
      _elapsedSecs = 0;
      _error = null;
      _retry = null;
      _approvals.clear();
    });
    _loadGit();
    _subscribeExternalWriter(threadId, epoch);
  }

  void _subscribeExternalWriter(String threadId, int epoch) {
    if (!mounted ||
        !_externalWriterMode ||
        _threadId != threadId ||
        epoch != _externalWriterEpoch) {
      return;
    }
    _externalWriterReconnect?.cancel();
    _externalWriterReconnect = null;
    final oldSub = _externalWriterSub;
    if (oldSub != null) unawaited(oldSub.cancel());
    _externalWriterSub = ref
        .read(bridgeApiProvider)
        .metaSessionEvents(widget.serviceKey, threadId)
        .listen(
          (update) => _applyExternalWriterUpdate(threadId, epoch, update),
          onError: (Object error, StackTrace stack) =>
              _externalWriterStreamLost(threadId, epoch, error),
          onDone: () => _externalWriterStreamLost(threadId, epoch, null),
          cancelOnError: true,
        );
  }

  void _applyExternalWriterUpdate(
    String threadId,
    int epoch,
    SessionFollowUpdate update,
  ) {
    if (!mounted ||
        !_externalWriterMode ||
        _threadId != threadId ||
        epoch != _externalWriterEpoch) {
      return;
    }
    final followTail = _loading || _atBottom;
    final wasRunning = _externalWriterRunning;
    final willRun = update.liveness.turnState == 'incomplete';
    final refreshDiff =
        _items
            .where((item) => item.type == 'fileChange')
            .map((item) => '${item.id}\u0000${item.text}')
            .join('\u0001') !=
        update.items
            .where((item) => item.itemType == 'fileChange')
            .map((item) => '${item.id}\u0000${item.text}')
            .join('\u0001');
    setState(() {
      _externalWriterLiveness = update.liveness;
      _replaceTranscriptItems(update.items);
      if (willRun && !wasRunning) _elapsedSecs = 0;
      _loading = false;
      _error = null;
      _retry = null;
    });
    if (willRun && !wasRunning) {
      _startElapsedTicker();
    } else if (!willRun && wasRunning) {
      _elapsedTicker?.cancel();
      _elapsedTicker = null;
      _turnStartedAt = null;
    }
    if (refreshDiff) _loadGit();
    if (followTail) _scrollToEnd(force: true);
  }

  void _externalWriterStreamLost(String threadId, int epoch, Object? error) {
    if (!mounted ||
        !_externalWriterMode ||
        _threadId != threadId ||
        epoch != _externalWriterEpoch ||
        _externalWriterReconnect != null) {
      return;
    }
    _externalWriterSub = null;
    setState(() {
      _loading = false;
      _error = error == null
          ? AppLocalizations.of(context).connectionLost
          : friendlyError(error);
      _retry = () => _retryExternalWriter(threadId, epoch);
    });
    _externalWriterReconnect = Timer(const Duration(seconds: 1), () {
      _externalWriterReconnect = null;
      _subscribeExternalWriter(threadId, epoch);
    });
  }

  void _retryExternalWriter(String threadId, int epoch) {
    _externalWriterReconnect?.cancel();
    _externalWriterReconnect = null;
    setState(() {
      _error = null;
      _retry = null;
      if (_items.isEmpty) _loading = true;
    });
    _subscribeExternalWriter(threadId, epoch);
  }

  Future<void> _takeOverExternalWriter() async {
    final threadId = _threadId;
    final liveness = _externalWriterLiveness;
    if (threadId == null ||
        liveness == null ||
        !liveness.allowsResume ||
        _takingOver) {
      return;
    }
    if (liveness.requiresTakeover) {
      final confirmed = await showDialog<bool>(
        context: context,
        builder: (_) =>
            TakeoverDialog(holders: liveness.holders, hasTarget: true),
      );
      if (confirmed != true || !mounted || _threadId != threadId) return;
    }

    final l10n = AppLocalizations.of(context);
    final messenger = ScaffoldMessenger.of(context);
    setState(() {
      _takingOver = true;
      _error = null;
      _retry = null;
    });
    try {
      ForceResumeReport? report;
      if (liveness.requiresTakeover) {
        report = await ref
            .read(bridgeApiProvider)
            .metaForceResume(widget.serviceKey, threadId);
        if (!report.resumed) {
          if (!mounted || _threadId != threadId) return;
          setState(() {
            _takingOver = false;
            _error = l10n.takeoverResumeFailed(report!.resumeError ?? '');
          });
          return;
        }
      }
      if (!mounted || _threadId != threadId) return;
      if (report != null) {
        final parts = <String>[
          l10n.takeoverResumed,
          if (report.killed.isNotEmpty)
            l10n.takeoverKilled(report.killed.length),
          if (report.stillHeld) l10n.takeoverStillHeld,
        ];
        messenger.showSnackBar(SnackBar(content: Text(parts.join(' · '))));
      }
      _cancelExternalWriterSubscription();
      setState(() {
        _externalWriterMode = false;
        _externalWriterLiveness = null;
        _takingOver = false;
      });
      await _resumeAndLoad();
    } catch (error) {
      if (!mounted || _threadId != threadId) return;
      setState(() {
        _takingOver = false;
        _error = friendlyError(error);
      });
    }
  }

  void _onEvent(AppEvent e) {
    if (!mounted) return;
    // A rename — possibly from another device, since the server persists the
    // title — applies to whichever thread it names, so it has to be handled
    // BEFORE the other-thread guard below: a sidebar row's rename would
    // otherwise be dropped and the list would keep the old title until the
    // next reload (nothing polls `_loadThreads`).
    if (e.kind == 'thread/name/updated') {
      _applyRemoteName(e);
      return;
    }
    // Ignore events belonging to another thread. Before this conversation has a
    // thread id (a brand-new conversation, pre-`thread/start`), the app session
    // is shared and another thread's turn may still be streaming, so drop any
    // thread-scoped event rather than letting it pollute the blank conversation
    // (append messages/approvals, flip `_streaming`/`_turnId`). Truly global
    // events (no threadId — e.g. account/rate-limit updates) always pass.
    if (e.threadId != null && e.threadId != _threadId) {
      // ...with one exception. A turn ending on ANOTHER thread changes that
      // thread's newest agent reply, which is exactly what its cached activity
      // summary holds. The cache is deliberately not auto-disposed, so without
      // this its row would show the previous reply for the rest of the session.
      // Dropping a cache entry can't pollute this conversation's state — the
      // reason the guard exists — so it is safe on this side of it.
      if (e.kind == 'turn/completed' || e.kind == 'turn/failed') {
        _invalidateSummary(e.threadId);
      }
      return;
    }
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
        _quotaRev.value++;
      }
      return;
    }
    // The server applied new effective thread settings — the authoritative
    // confirmation that a model / effort / permission / plan switch took
    // effect (possibly one this app just sent). Newer servers emit this with
    // the full snapshot whenever a turn's overrides change the settings.
    if (e.kind == 'thread/settings/updated') {
      final cfg = _parseSettingsUpdate(e.raw);
      if (cfg != null) _applyRuntime(cfg);
      return;
    }
    // The server substituted the model mid-turn (e.g. a policy or capacity
    // reroute): reflect it on the current turn's stamp so the transcript
    // records what actually handled the request.
    if (e.kind == 'model/rerouted') {
      String? to;
      try {
        final m = jsonDecode(e.raw) as Map<String, dynamic>;
        to = (m['toModel'] as String?)?.trim();
      } catch (_) {
        to = null;
      }
      if (to != null && to.isNotEmpty) {
        setState(() {
          _turnStampModel = to;
          _turnStampConfirmed = true;
          _turnRerouted = true;
        });
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
          // A fresh turn hasn't produced output yet, so Esc undoes the send
          // until the first item lands.
          _outputStarted = false;
          _elapsedSecs = 0;
          // Stamp what this turn runs with. A server that notifies settings
          // does so BEFORE turn/started, so a confirmed runtime is already
          // current here; otherwise fall back to what this app actually sent
          // (turn params override thread defaults, so on servers that never
          // notify, the sent value IS the effective one).
          final rt = _runtime;
          final confirmedRt = rt != null && rt.confirmedByUpdate;
          _turnStampModel = confirmedRt
              ? (rt.model ?? _sentModel)
              : (_sentModel ?? rt?.model);
          _turnStampEffort = confirmedRt
              ? rt.reasoningEffort
              : (_sentEffort ?? rt?.reasoningEffort);
          _turnStampConfirmed =
              _turnStampModel != null && _turnStampModel == rt?.model;
          _turnRerouted = false;
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
            _error = _humanizeTurnError(failure);
            _retry = () => _send(retry: true);
          }
        });
        _maybeFlushQueue(); // send the next queued message, if any
        _loadGit(); // edits from the turn may have changed the diff
        // The turn just produced a new agent reply, so the cached one-line
        // summary now describes the turn BEFORE it. Drop it and let the
        // activity view re-read on its next build; without this the cache is
        // permanently stale for the thread the user is actually working in.
        _invalidateSummary(e.threadId);
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
            _error = _humanizeTurnError(e.text);
            _retry = () => _send(retry: true);
          }
        });
        _maybeFlushQueue(); // send the next queued message, if any
        // An interrupted or failed turn can still have streamed a reply before
        // it stopped, so the cached summary is just as stale as on success.
        _invalidateSummary(e.threadId);
      default:
        _handleItemEvent(e);
    }
  }

  /// Turn a raw turn-failure string into what the user sees. An empty/absent
  /// message falls back to the generic notice; a Windows-sandbox helper failure
  /// (an embedded/自带 host that can't spawn its command sandbox) is rewritten
  /// into actionable guidance — switch to no-sandbox Full mode, or use an
  /// external host — instead of the raw "windows sandbox: spawn setup refresh".
  String _humanizeTurnError(String? message) {
    final l10n = AppLocalizations.of(context);
    final text = message?.trim() ?? '';
    if (text.isEmpty) return l10n.turnFailed;
    return isSandboxHelperFailure(text) ? l10n.sandboxHelperUnavailable : text;
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
      // Any agent-side item (reasoning, a tool call, or reply text) means the
      // turn has begun producing output: past this point Esc interrupts rather
      // than undoing the send.
      _outputStarted = true;
      final idx = _itemIndex[id];
      if (idx == null) {
        _items.add(
          _Item(
            id: id,
            type: type,
            title: e.title ?? '',
            text: e.text ?? '',
            streaming: type == 'agentMessage' ? true : running,
            // The live turn this item belongs to, so a reply that streams in as
            // several items groups the same way it will after a reload.
            turnId: _turnId ?? '',
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
    final typed = retry
        ? (_lastUserText ?? '')
        : (overrideText ?? _input.text.trim());
    // Attachments ride an ordinary composer send only: a programmatic send
    // (e.g. "implement the plan") must not consume them, and a retry re-sends
    // the snapshot taken at the original send.
    final ordinary = !retry && overrideText == null;
    final images = retry
        ? _lastUserImages
        : !ordinary
        ? const <String>[]
        : [
            for (final a in _attachments)
              if (!a.isFile) ?a.processed?.dataUrl,
          ];
    // File attachments were already uploaded to the host at pick time; they
    // travel as a path-reference block appended to the text (codex's native
    // document workflow). On retry `typed` already contains the block, so no
    // re-upload and no double-append.
    final filePaths = ordinary
        ? [
            for (final a in _attachments)
              if (a.isFile) ?a.hostPath,
          ]
        : const <String>[];
    final text = appendFileRefs(typed, filePaths);
    // Block sends while reconnecting — a reconnect reloads history and would
    // wipe an optimistic message added mid-flight.
    if ((text.isEmpty && images.isEmpty) || _sending || _reconnecting) return;
    // If this host is THIS machine's codex and it can't make model calls yet
    // (no login AND no custom provider), a turn would silently fail. Steer the
    // user to the setup wizard instead of starting an invalid conversation.
    if (_codexNeedsSetup(watch: false)) {
      _openCodexSetup();
      return;
    }
    // Never send while an attachment is still processing/uploading — the
    // message would silently ship without it. (The send button is disabled
    // too; this also guards the Enter-to-send path.)
    if (ordinary && _attachments.any((a) => !a.ready)) {
      return;
    }
    // Take the send lock up front, before the retry probe's await below, so the
    // composer can't start a second send during that round-trip (re-entrancy).
    setState(() => _sending = true);
    // Retry safety: a send can commit server-side just before the socket drops
    // (we reconnect with reload:false to keep the optimistic bubble for a
    // one-tap retry). Re-sending a committed turn records the prompt twice —
    // which both shows a duplicate user bubble and leaves a trailing duplicate
    // that hides the plan-implement choice. So on retry, ask the server first;
    // if this prompt is already the latest user turn, just reload its (possibly
    // in-progress) history instead of sending again.
    if (retry && await _turnAlreadyCommitted(text, images)) {
      if (mounted) setState(() => _sending = false);
      await _resumeAndLoad();
      return;
    }
    setState(() {
      _error = null;
      _retry = null;
      _lastUserText = text;
      _lastUserImages = images;
      if (!retry) {
        final id = 'local-user-${_localSeq++}';
        _itemIndex[id] = _items.length;
        _items.add(
          _Item(
            id: id,
            type: 'userMessage',
            text: text,
            images: resolveImageUrls(images),
            imageUrls: images,
          ),
        );
        // Remember the just-sent draft so an Esc "undo" (before any output)
        // can restore it to the composer. Only for an ordinary send — a
        // programmatic prompt (e.g. "implement the plan") has no user draft to
        // hand back.
        _undoableText = overrideText == null ? typed : null;
        // Don't clear the composer for a programmatic send (e.g. "implement
        // the plan") — the user may have text in progress there.
        if (overrideText == null) {
          _input.clear();
          _attachments.clear();
        }
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
        // The fresh conversation is now the one to restore on next launch.
        ref
            .read(uiPrefsProvider.notifier)
            .setLastThread(widget.serviceKey, _threadId);
        // Surface the new session in the left pane immediately. `thread/list`
        // can lag `thread/start`, so optimistically insert it now (newest
        // first) and let _loadThreads reconcile once the server catches up.
        final tid = _threadId!;
        if (mounted && !_threads.any((t) => t.id == tid)) {
          setState(() {
            _threads = [
              ThreadMeta(
                id: tid,
                // Preview the TYPED text (never the appended file-reference
                // block); an attachment-only first message gets a placeholder.
                preview: typed.isNotEmpty
                    ? typed
                    : filePaths.isNotEmpty
                    ? AppLocalizations.of(context).fileOnlyMessage
                    : AppLocalizations.of(context).imageOnlyMessage,
                cwd: _cwd ?? '',
                updatedAt: 0,
              ),
              ..._threads,
            ];
          });
        }
        _loadThreads();
        // Persist this new thread's config now that it has a server-side id, so
        // it's stored even if the first turn/start below fails.
        _persistThreadConfig();
      }
      // Record what this turn puts on the wire BEFORE sending: turn/started
      // (and the stamp it takes) can arrive while the await below is still in
      // flight. Turn params override thread defaults, so on servers that never
      // notify settings these ARE the effective values.
      _sentModel = modelId;
      _sentEffort = effort?.wire;
      // Pass the current model + permission + collaboration mode every turn:
      // turn/start overrides apply to this and subsequent turns, so switching
      // works mid-conversation.
      await api.appTurnStart(
        widget.serviceKey,
        _threadId!,
        text,
        images: images,
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
          // The user's model pick (if any) is now on the wire; server
          // confirmations may sync the pill again.
          _modelPickPending = false;
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
          _planByThread[_threadKey(_threadId!)] = _plan;
          _effortByThread[_threadKey(_threadId!)] = effort;
        }
        // Persist the config now that the thread has an id — covers a brand-new
        // conversation whose settings were chosen before its first turn.
        _persistThreadConfig();
        // Adopt the bridge's cached runtime config: for a brand-new thread the
        // start response reported the effective model/permissions, and any
        // settings notification that raced the send is folded in too.
        _refreshRuntimeFromCache();
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
  ///
  /// Idempotency compares the full message content — text AND the image URL
  /// list (history echoes the data URLs we sent verbatim): if the user
  /// deliberately re-asks an identical prompt and *that* send drops before
  /// committing, the latest committed user turn still matches, so the retry
  /// reloads instead of resending. Comparing URLs (not just a count) matters
  /// for image-only messages, which ALL have empty text — a count-only match
  /// would swallow a retry of a *different* photo. Accepted edge — avoiding a
  /// duplicate (costly) turn is preferred over re-sending a rare identical
  /// consecutive prompt.
  Future<bool> _turnAlreadyCommitted(String text, List<String> images) async {
    final tid = _threadId;
    if (tid == null) return false;
    try {
      final api = ref.read(bridgeApiProvider);
      await api.appThreadResume(widget.serviceKey, tid);
      final history = await api.appThreadRead(widget.serviceKey, tid);
      final want = text.trim();
      for (final i in history.items.reversed) {
        if (i.itemType == 'userMessage') {
          return i.text.trim() == want && listEquals(i.images, images);
        }
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

  // ── Send / queue / Esc state machine (codex-cli parity) ──────────────────
  //
  // Composer submit routes through _submit: while a turn is in flight the
  // message QUEUES (fires as its own turn once the running one ends) instead of
  // racing it. Esc is context-sensitive (see _onEscape):
  //   • queue non-empty         → pop the most recent queued message to the box
  //   • running, no output yet  → undo the send (interrupt + restore the text)
  //   • running, output started → interrupt the turn (stopped marker)

  /// Composer send: queue while a turn is in flight (or a backlog is still
  /// draining), otherwise send now.
  void _submit() {
    if (_streaming || _sending || _queue.isNotEmpty) {
      _enqueue();
      // Idle with a residual backlog (e.g. a flush skipped during a reconnect):
      // drain it now so nothing stays stuck.
      if (!_streaming && !_sending) _maybeFlushQueue();
    } else {
      _send();
    }
  }

  /// Snapshot the composer (text + its ready attachments) into the queue and
  /// clear it. A no-op for an empty draft or while an attachment is still
  /// processing (the message would ship without it).
  void _enqueue() {
    final atts = List<_Attachment>.from(_attachments);
    if (_input.text.trim().isEmpty && atts.isEmpty) return;
    if (atts.any((a) => !a.ready)) return;
    setState(() {
      _queue.add(
        _Queued(id: _queueSeq++, text: _input.text, attachments: atts),
      );
      _input.clear();
      _attachments.clear();
    });
  }

  /// Send the head of the queue as a new turn, if the session is idle. Called on
  /// every turn end (and after a reconnect). Restores the queued draft into the
  /// composer and reuses the ordinary send path, so attachments / file refs /
  /// retry-on-drop all behave exactly like a hand-typed send.
  void _maybeFlushQueue() {
    if (_queue.isEmpty || _streaming || _sending || _reconnecting) return;
    final q = _queue.removeAt(0); // FIFO
    _input.text = q.text;
    _attachments
      ..clear()
      ..addAll(q.attachments);
    // _send reads _input + _attachments synchronously (before its first await)
    // and clears them; no intermediate frame renders, so nothing flickers.
    unawaited(_send());
  }

  /// Esc handling. Returns true when it acted (so the key is consumed).
  bool _onEscape() {
    // 1. A queued message is the most recent thing the user did — hand the last
    //    one back to the composer before touching the running turn.
    if (_queue.isNotEmpty) {
      _dequeueLast();
      return true;
    }
    // 2. A turn is running.
    if (_streaming) {
      if (_outputStarted) {
        unawaited(_interrupt()); // already replying → stop it (stopped marker)
      } else {
        _undoSend(); // nothing back yet → take the send back into the composer
      }
      return true;
    }
    // 3. Nothing to interrupt or dequeue — let the key through.
    return false;
  }

  /// Pop the most recently queued message back into the composer (Esc).
  void _dequeueLast() {
    if (_queue.isNotEmpty) _restoreQueued(_queue.last);
  }

  /// Take a queued message out of the queue and back into the composer so the
  /// user can edit/resend it (Esc, or tapping its chip). Replaces the current
  /// draft — in the normal flow the box is empty after queueing.
  void _restoreQueued(_Queued q) {
    setState(() {
      _queue.removeWhere((e) => e.id == q.id);
      _attachments
        ..clear()
        ..addAll(q.attachments);
    });
    _input.text = q.text;
    _input.selection = TextSelection.collapsed(offset: _input.text.length);
    _inputFocus.requestFocus();
  }

  /// Discard a specific queued message (the ✕ on its chip). Unlike Esc, this
  /// drops it rather than restoring it — the user explicitly removed it.
  void _discardQueued(int id) {
    setState(() => _queue.removeWhere((q) => q.id == id));
  }

  /// Esc "undo" for a turn that hasn't produced output yet: interrupt it and
  /// drop the just-sent text back into the composer, taking the send back. The
  /// server may still record an aborted turn (accepted — it surfaces on reload),
  /// but live the optimistic bubble is removed so it reads as un-sent.
  void _undoSend() {
    final restore = _undoableText;
    setState(() {
      _removeLastLocalUserBubble();
      // This interrupt is an undo: suppress the "stopped" marker its turn-end
      // would otherwise add.
      _suppressStopMarker = true;
    });
    unawaited(_interrupt());
    if (restore != null && restore.isNotEmpty) {
      _input.text = restore;
      _input.selection = TextSelection.collapsed(offset: restore.length);
    }
    _inputFocus.requestFocus();
  }

  /// Remove the newest optimistic user bubble (id `local-user-*`) — the one the
  /// undone send added. Call inside a setState; rebuilds the id→index map.
  void _removeLastLocalUserBubble() {
    for (var i = _items.length - 1; i >= 0; i--) {
      if (_items[i].type == 'userMessage' &&
          _items[i].id.startsWith('local-user-')) {
        _items.removeAt(i);
        _rebuildItemIndex();
        return;
      }
    }
  }

  /// Rebuild `_itemIndex` from `_items` after a mid-list removal.
  void _rebuildItemIndex() {
    _itemIndex.clear();
    for (var i = 0; i < _items.length; i++) {
      _itemIndex[_items[i].id] = i;
    }
  }

  /// Append a local "stopped" marker so an interrupted turn is visible in the
  /// transcript. Local-only (not persisted); call inside a `setState`. An Esc
  /// "undo" interrupt suppresses the marker — the send is being taken back into
  /// the composer, not shown as a stopped turn.
  void _addStoppedMarker() {
    _pendingInterrupt = false;
    if (_suppressStopMarker) {
      _suppressStopMarker = false;
      return;
    }
    final id = 'local-stopped-${_localSeq++}';
    _itemIndex[id] = _items.length;
    _items.add(_Item(id: id, type: 'interrupted', text: ''));
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
  /// hover → 完成于 HH:MM:SS), stamped with the model/effort the turn actually
  /// ran with so a switch is verifiable per response. Local-only; call inside
  /// a `setState`.
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
        // Resolve to the display name now (the cached list can change later);
        // an unlisted id stays raw — still truthful.
        model: _modelDisplayLabel(_turnStampModel),
        effortWire: _turnStampEffort,
        modelConfirmed: _turnStampConfirmed,
        modelRerouted: _turnRerouted,
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

  /// Abandon the in-flight diff fetch. The bridge call itself keeps running to
  /// completion — it can't be aborted — but bumping the token means its result
  /// is dropped and the review never opens.
  void _cancelDiff() {
    if (!_diffLoading) return;
    _diffFetch++;
    setState(() => _diffLoading = false);
  }

  /// Re-read the diff for an already-open review, showing (and allowing the
  /// cancellation of) the fetch. Unlike [_showDiff] this never opens anything.
  Future<void> _refreshDiff() async {
    if (_diffLoading) return;
    final token = ++_diffFetch;
    setState(() => _diffLoading = true);
    await _loadGit();
    if (!mounted || token != _diffFetch) return;
    setState(() => _diffLoading = false);
  }

  /// Open the diff viewer: the inline review split on desktop (≥1100px), a tall
  /// bottom sheet on narrower screens.
  ///
  /// Opens on whatever diff is already in hand and refreshes behind the review,
  /// rather than making every press wait on a `gitDiffToRemote` round-trip. The
  /// badge only shows change counts once a diff has been read, so by the time
  /// this is reachable there is nearly always something to show — and it stays
  /// current anyway, since turn/diff/updated and turn/completed both refresh it.
  ///
  /// Only a cold open (no diff read yet) blocks, and that one shows the spinner
  /// and can be cancelled by pressing again.
  Future<void> _showDiff() async {
    if (_diffLoading) {
      _cancelDiff();
      return;
    }
    final compact = MediaQuery.of(context).size.width < 1100;
    // The sheet takes a snapshot of the diff when it's built, so a refresh
    // behind it couldn't reach the open sheet — only the review split, which
    // rebuilds from state, gets the background update.
    if (_diff != null && !compact) {
      _openReview();
      _refreshDiff();
      return;
    }
    final token = ++_diffFetch;
    setState(() => _diffLoading = true);
    await _loadGit();
    // Superseded or cancelled while the fetch was in flight: the newer caller
    // (or the cancel) owns the UI now, so this one must not touch it.
    if (!mounted || token != _diffFetch) return;
    setState(() => _diffLoading = false);
    if (compact) {
      await _showDiffSheet();
    } else {
      _openReview();
    }
  }

  /// The narrow-screen diff viewer.
  Future<void> _showDiffSheet() => showModalBottomSheet<void>(
    context: context,
    showDragHandle: true,
    isScrollControlled: true,
    builder: (c) => FractionallySizedBox(
      heightFactor: 0.9,
      child: _EnvPanel(diff: _diff, branch: _branch, cwd: _cwd),
    ),
  );

  /// Read a reviewed file's current lines so the review can expand an elided
  /// gap. The gap lines are unchanged, so the working-tree file holds them
  /// verbatim; [relPath] is relative to the conversation's cwd. Best-effort:
  /// returns null when there's no cwd or the host won't serve the read (no meta
  /// tunnel, or the file sits outside a configured root), and the gap then
  /// simply stays collapsed.
  Future<List<String>?> _loadReviewFile(String relPath) async {
    final cwd = _cwd?.trim();
    if (cwd == null || cwd.isEmpty) return null;
    // Join with the host's own separator; leave an already-absolute path alone.
    final abs =
        (relPath.startsWith('/') || RegExp(r'^[A-Za-z]:').hasMatch(relPath))
        ? relPath
        : cwd.contains('\\')
        ? '$cwd\\${relPath.replaceAll('/', '\\')}'
        : '$cwd/$relPath';
    try {
      final bytes = await ref
          .read(bridgeApiProvider)
          .metaReadFile(widget.serviceKey, abs);
      return utf8.decode(bytes, allowMalformed: true).split(RegExp(r'\r?\n'));
    } catch (_) {
      return null;
    }
  }

  /// Open the desktop review split, defaulting the selected file to [path] (or
  /// the first changed file).
  ///
  /// Deliberately does NOT fetch: every caller either just awaited a fetch or is
  /// opening on data it already has. Refreshing here as well meant one press
  /// paid for two round-trips of the same `gitDiffToRemote`, the second of which
  /// nobody was waiting on but which still had to finish.
  void _openReview({String? path}) {
    setState(() {
      _reviewOpen = true;
      final files = _diff?.files ?? const [];
      _reviewFile =
          path ??
          (files.any((f) => f.path == _reviewFile)
              ? _reviewFile
              : files.firstOrNull?.path);
    });
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
        final lost = _looksDisconnected(msg);
        setState(() {
          _error = msg;
          if (lost) _connectionLost = true;
        });
        if (lost) _publishLinkState(down: true);
      }
    }
  }

  /// Publish whether this conversation's link is down, so the services list
  /// agrees with what the transcript is showing.
  ///
  /// Called from every place the local flags move. Kept as one helper because
  /// the two used to drift: the flags lived only in this widget, so a service
  /// could read "online" in the list while this screen said "reconnecting".
  ///
  /// A reconnect in progress counts as down — the link genuinely isn't carrying
  /// traffic yet, and the alternative is a green row next to a spinner.
  void _publishLinkState({required bool down}) {
    final notifier = ref.read(observedDisconnectedProvider.notifier);
    final current = notifier.state;
    final has = current.contains(widget.serviceKey);
    if (down == has) return;
    notifier.state = down ? {...current, widget.serviceKey} : {...current}
      ..remove(widget.serviceKey);
    // The cached probe answered before the link changed, so it would otherwise
    // keep reporting the stale verdict for as long as the cache lives.
    ref.invalidate(appReachableProvider(widget.serviceKey));
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
      _publishLinkState(down: true);
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
        // Re-list too, not just the open transcript. `_loadThreads` runs once at
        // initState and is best-effort: if the host wasn't reachable then (it
        // restarted, or the app opened first), the failure was swallowed and
        // the pane stayed empty FOREVER — nothing else re-lists except sending
        // a message. A reconnect is exactly the moment the data became
        // available, so this is where it has to be retried.
        await _loadThreads();
        // Same reasoning for the other cold-open loads: a reconnect is the
        // moment their data became reachable. Both are no-ops once settled —
        // the cwd seed won't override a folder the user picked, and the quota
        // just refreshes.
        if (_openLoadRetries.containsKey(_kCwdSeed)) await _seedDefaultCwd();
        if (_rate == null) unawaited(_loadQuota());
        _loadGit(); // the working tree may have moved on while we were away
        if (mounted) {
          setState(() {
            _reconnecting = false;
            _connectionLost = false;
            _error = null;
          });
          _publishLinkState(down: false);
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
      _publishLinkState(down: true);
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
    try {
      await ref
          .read(bridgeApiProvider)
          .appRespondApproval(widget.serviceKey, prompt.requestId!, decision);
    } catch (e) {
      // The host may have dropped between the prompt and the answer; swallow so a
      // dead connection can't surface an uncaught async error.
      debugPrint('appRespondApproval failed: $e');
    }
  }

  /// Answer a `request_user_input` elicitation. `answers` maps each question id
  /// to the chosen answer string(s); an empty map cancels (the turn proceeds
  /// without an answer). Sends the codex `ToolRequestUserInputResponse` shape.
  Future<void> _answerUserInput(
    AppEvent prompt,
    Map<String, List<String>> answers,
  ) async {
    setState(() => _approvals.remove(prompt));
    try {
      await ref
          .read(bridgeApiProvider)
          .appRespondUserInput(
            widget.serviceKey,
            prompt.requestId!,
            jsonEncode(answers),
          );
    } catch (e) {
      // A dropped host between the elicitation and the answer must not surface an
      // uncaught async error.
      debugPrint('appRespondUserInput failed: $e');
    }
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

  /// Jump the transcript to the previous ([next] false) or next ([next] true)
  /// conversation turn — i.e. the nearest user message above/below the current
  /// viewport top — placing it at the top so its whole exchange is in view.
  /// User messages are never grouped, so each is its own row; we map row
  /// indices and drive super_sliver_list's index-aware scroll.
  void _gotoAdjacentTurn({required bool next}) {
    if (!_listCtl.isAttached || !_scroll.hasClients) return;
    final rows = _rows;
    final turnRows = <int>[
      for (var i = 0; i < rows.length; i++)
        if (rows[i] is _Item && (rows[i] as _Item).isUser) i,
    ];
    if (turnRows.isEmpty) return;
    // Topmost row currently in view (fallback to 0 before the first layout).
    final anchor = _listCtl.visibleRange?.$1 ?? 0;
    int? target;
    if (next) {
      for (final t in turnRows) {
        if (t > anchor) {
          target = t;
          break;
        }
      }
    } else {
      for (final t in turnRows) {
        if (t < anchor) {
          target = t;
        } else {
          break;
        }
      }
    }
    if (target == null) return;
    _listCtl.animateToItem(
      index: target,
      scrollController: _scroll,
      alignment: 0, // land the turn's user message at the top of the viewport
      duration: (est) => Duration(milliseconds: est.abs() > 2400 ? 420 : 260),
      curve: (_) => Curves.easeOutCubic,
    );
  }

  /// The bottom-right navigation cluster: prev/next-turn jumps (shown once
  /// there are ≥2 turns) and a jump-to-latest (shown when scrolled up). A
  /// single compact rounded bar rather than scattered FABs, so it stays out of
  /// the way of the messages.
  Widget _navCluster() {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final turns = _items.where((i) => i.isUser).length;
    final showTurnNav = turns >= 2;
    if (!showTurnNav && _atBottom) return const SizedBox.shrink();
    Widget btn(Key key, IconData icon, String tip, VoidCallback onTap) =>
        IconButton(
          key: key,
          tooltip: tip,
          visualDensity: VisualDensity.compact,
          iconSize: 22,
          padding: const EdgeInsets.all(6),
          constraints: const BoxConstraints(),
          color: scheme.onSurfaceVariant,
          onPressed: onTap,
          icon: Icon(icon),
        );
    // Opaque, and a shadow rather than a Material elevation: this floats over
    // the scrolling transcript, so a translucent ground would let text read
    // through it — and Material can't composite a wash against the page anyway.
    return DecoratedBox(
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(24),
        // Stronger than a panel's: this one floats over moving content.
        boxShadow: panelShadow(scheme, blur: 12),
      ),
      child: Material(
        elevation: 0,
        color: scheme.surfaceBright,
        borderRadius: BorderRadius.circular(24),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 4, horizontal: 2),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (showTurnNav) ...[
                btn(
                  const Key('nav-prev-turn'),
                  Icons.keyboard_arrow_up,
                  l10n.prevTurn,
                  () => _gotoAdjacentTurn(next: false),
                ),
                btn(
                  const Key('nav-next-turn'),
                  Icons.keyboard_arrow_down,
                  l10n.nextTurn,
                  () => _gotoAdjacentTurn(next: true),
                ),
              ],
              if (!_atBottom)
                btn(
                  const Key('nav-to-bottom'),
                  Icons.vertical_align_bottom,
                  l10n.jumpToLatest,
                  () => _scrollToEnd(force: true),
                ),
            ],
          ),
        ),
      ),
    );
  }

  String _projectName() {
    final c = _cwd?.trim();
    if (c == null || c.isEmpty) {
      return AppLocalizations.of(context).defaultFolder;
    }
    final parts = c.split(RegExp(r'[\\/]'))..removeWhere((s) => s.isEmpty);
    return parts.isEmpty ? c : parts.last;
  }

  /// Title-bar text: the current conversation, ChatGPT-desktop style. Falls
  /// back through the server-side thread preview, then the locally echoed
  /// last user message (a fresh thread's server preview stays empty until its
  /// first turn round-trips), then "new conversation".
  String _barTitle(AppLocalizations l10n) {
    final tid = _threadId;
    if (tid != null) {
      final match = _threads.where((t) => t.id == tid);
      final meta = match.isEmpty ? null : match.first;
      // A user-set name wins outright — that's the whole point of renaming.
      final named = meta?.title;
      if (named != null) return named;
      // Previews are the raw first user message, which may be wire text (an
      // attachment block); run them through the same cleaner the sidebar uses
      // so the two never disagree.
      final preview = meta == null
          ? ''
          : previewWithoutFileRefs(meta.preview, l10n.fileOnlyMessage);
      for (final candidate in [preview, _lastUserText ?? '']) {
        final line = candidate.trim().split('\n').first.trim();
        if (line.isNotEmpty) return line;
      }
    }
    return l10n.newConversation;
  }

  /// Commit a rename typed into the top bar. Empty clears the name (back to
  /// the preview). Applied locally first so the bar doesn't flicker back to
  /// the old title while the request is in flight.
  Future<void> _renameThread(String raw) async {
    final tid = _threadId;
    setState(() => _editingTitle = false);
    if (tid == null) return;
    final name = raw.trim();
    final idx = _threads.indexWhere((t) => t.id == tid);
    final before = idx < 0 ? null : _threads[idx];
    if (before != null && (before.title ?? '') == name) return;
    // Opening the field and committing it untouched must stay a no-op: the box
    // is seeded with the displayed title, and pinning an auto-preview as an
    // explicit name would silently stop it tracking the conversation.
    if (before?.title == null &&
        name == _barTitle(AppLocalizations.of(context)).trim()) {
      return;
    }
    final seq = ++_renameSeq;
    if (before != null) {
      setState(() {
        final next = [..._threads];
        next[idx] = before.withName(name.isEmpty ? null : name);
        _threads = next;
      });
    }
    try {
      await ref
          .read(bridgeApiProvider)
          .appSetThreadName(widget.serviceKey, tid, name);
    } catch (e) {
      // Put the old title back: the server is the source of truth, so a failed
      // rename must not leave the UI claiming it worked. Only when this is
      // still the newest attempt, though — a later rename has already replaced
      // what we'd be restoring, and its own request owns the outcome now.
      if (!mounted || seq != _renameSeq) return;
      if (before != null) {
        final at = _threads.indexWhere((t) => t.id == tid);
        if (at >= 0) {
          setState(() {
            final next = [..._threads];
            next[at] = before;
            _threads = next;
          });
        }
      }
      final l10n = AppLocalizations.of(context);
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('${l10n.renameFailed}: ${friendlyError(e)}')),
      );
    }
  }

  /// Adopt a `thread/name/updated` notification: the server persists titles, so
  /// this is how a rename made on ANOTHER device (or in another window) reaches
  /// this list. The echo of our OWN rename carries the name we already applied
  /// optimistically, so the equality check below makes it a no-op.
  void _applyRemoteName(AppEvent e) {
    final tid = e.threadId ?? _threadIdFromRaw(e.raw);
    if (tid == null) return;
    final idx = _threads.indexWhere((t) => t.id == tid);
    if (idx < 0) return;
    final name = _nameFromRaw(e.raw);
    if ((_threads[idx].title ?? '') == (name ?? '')) return;
    setState(() {
      final next = [..._threads];
      next[idx] = next[idx].withName(name);
      _threads = next;
    });
  }

  /// `threadId` out of a notification's raw JSON, when the typed field is unset.
  String? _threadIdFromRaw(String raw) => _rawString(raw, 'threadId');

  /// The `name` a `thread/name/updated` carries; null when cleared or absent.
  String? _nameFromRaw(String raw) {
    final n = _rawString(raw, 'name')?.trim();
    return (n == null || n.isEmpty) ? null : n;
  }

  /// One string field out of an event's raw JSON params, or null on any shape
  /// surprise (a malformed notification must not take the screen down).
  String? _rawString(String raw, String key) {
    try {
      final decoded = jsonDecode(raw);
      if (decoded is! Map) return null;
      final v = decoded[key];
      return v is String ? v : null;
    } catch (_) {
      return null;
    }
  }

  /// Desktop hover text for the context gauge: a one-line token breakdown.
  String _contextTooltip(AppLocalizations l10n) {
    final c = _ctx;
    if (c == null) return l10n.contextLabel;
    return '${l10n.contextLabel}: ${_fmtTokens(c.tokensUsed)} / '
        '${_fmtTokens(c.contextWindow)} (${c.percent}%)';
  }

  /// Read the account quota into [_rate]. Called once per connection (see
  /// [_subscribe]) so the numbers are already there whenever the user looks;
  /// `account/rateLimits/updated` keeps them current afterwards.
  Future<void> _loadQuota() async {
    try {
      final raw = await ref
          .read(bridgeApiProvider)
          .appRateLimits(widget.serviceKey)
          // Bounded: this runs on the open path, and an app-server that never
          // answers must not be able to wedge a surface waiting on it.
          .timeout(const Duration(seconds: 8));
      final r = RateLimits.fromRaw(raw);
      if (r != null && mounted) {
        setState(() => _rate = r);
        _quotaRev.value++;
      }
    } catch (_) {
      // Quota is optional (a custom provider may not report one, and an
      // unreachable host times out); every surface that shows it degrades to
      // "unavailable" rather than blocking. No retry of its own is needed: a
      // successful listing re-fetches it (see `_loadThreads`), and that listing
      // now retries — so the quota rides along.
    }
  }

  /// Open the context/quota detail sheet (tap on desktop and mobile). Opens
  /// immediately and fills in: the quota is normally already warm, and when it
  /// isn't, a panel that appears now and updates beats a tap that hangs.
  Future<void> _showContextDetail() async {
    if (_rate == null) unawaited(_loadQuota());
    await showAdaptivePanel<void>(
      context: context,
      // Rebuilt on every quota change, so a fetch that lands while the panel
      // is open fills the bars in place.
      builder: (c) => ValueListenableBuilder<int>(
        valueListenable: _quotaRev,
        builder: (c2, _, _) => _contextSheet(AppLocalizations.of(c2)),
      ),
    );
  }

  /// Detail-sheet body: context occupancy + 5h / weekly quota bars.
  Widget _contextSheet(AppLocalizations l10n) {
    final text = Theme.of(context).textTheme;
    final ctx = _ctx;
    return SafeArea(
      // Token counts and reset times are numbers people copy into notes.
      child: SelectionArea(
        child: Padding(
          // The panel owns the top inset (a drag handle on a sheet, breathing
          // room in a dialog), so this only sets the sides and the base.
          padding: const EdgeInsets.fromLTRB(20, 0, 20, 24),
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

  /// Whether THIS machine's codex (the local host behind [widget.serviceKey])
  /// still needs setup — no login AND no custom provider — so a model turn
  /// can't succeed. Only ever true for a LOCAL host; a remote host's codex
  /// config is the remote owner's concern, not ours to detect. Pass
  /// `watch: true` from `build` (so it rebuilds when the status resolves),
  /// `watch: false` from callbacks.
  bool _codexNeedsSetup({required bool watch}) {
    final locals = watch
        ? ref.watch(localServeListProvider)
        : ref.read(localServeListProvider);
    final isLocal = (locals.valueOrNull ?? const <AppServeStatus>[]).any(
      (h) => h.appServiceKey == widget.serviceKey,
    );
    if (!isLocal) return false;
    final status = watch
        ? ref.watch(codexSetupStatusProvider)
        : ref.read(codexSetupStatusProvider);
    return status.valueOrNull?.needsSetup ?? false;
  }

  /// Open the setup wizard, refreshing the status on return so the banner /
  /// send-guard clear once codex is configured.
  void _openCodexSetup() {
    context.push('/setup/codex').then((_) {
      if (mounted) ref.invalidate(codexSetupStatusProvider);
    });
  }

  /// A warning strip under the app bar when the local codex isn't configured —
  /// a persistent, tappable pointer to the setup wizard.
  PreferredSizeWidget _codexSetupBar(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    return PreferredSize(
      preferredSize: const Size.fromHeight(46),
      child: Material(
        color: scheme.errorContainer,
        child: InkWell(
          mouseCursor: clickable,
          onTap: _openCodexSetup,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(16, 8, 8, 8),
            child: Row(
              children: [
                Icon(
                  Icons.warning_amber_rounded,
                  size: 18,
                  color: scheme.onErrorContainer,
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    l10n.codexChatNeedsSetup,
                    key: const Key('codex-chat-needs-setup'),
                    style: TextStyle(
                      color: scheme.onErrorContainer,
                      fontSize: 13,
                    ),
                  ),
                ),
                TextButton(
                  key: const Key('codex-chat-setup-btn'),
                  onPressed: _openCodexSetup,
                  child: Text(l10n.codexSetup),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final width = MediaQuery.of(context).size.width;
    final needsCodexSetup = _codexNeedsSetup(watch: true);

    // Phones: classic Scaffold — the sessions list is a slide-in drawer, the
    // bar is a plain AppBar naming the conversation.
    if (width < 600) {
      return Scaffold(
        drawer: Drawer(
          // The scheme's container colours are translucent washes. A drawer
          // floats above a scrim, so resolve the wash onto its opaque ground
          // instead of letting the chat show through the sessions pane.
          backgroundColor: Color.alphaBlend(
            scheme.surfaceContainer,
            scheme.surface,
          ),
          surfaceTintColor: Colors.transparent,
          child: SafeArea(child: _sessionsPane(l10n, inDrawer: true)),
        ),
        // Widen the edge-swipe-to-open zone (default ~20px). The narrow
        // default sits under Android's system back-gesture strip, so a
        // left-edge swipe almost always triggered "back" instead of the
        // drawer; a 56px zone lets the swipe start just inside that strip.
        drawerEdgeDragWidth: 56,
        appBar: WindowTitleBar(
          bottom: needsCodexSetup ? _codexSetupBar(l10n) : null,
          leading: Builder(
            builder: (ctx) => IconButton(
              tooltip: l10n.conversationsSection,
              icon: const Icon(Icons.menu),
              onPressed: () => Scaffold.of(ctx).openDrawer(),
            ),
          ),
          title: Text(
            _barTitle(l10n),
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w600),
          ),
          actions: [
            if (_ctx != null)
              _ContextGauge(
                status: _ctx!,
                onTap: _showContextDetail,
                tooltip: _contextTooltip(l10n),
              ),
            // Same place as on desktop. It has to be here rather than in the
            // sessions pane, which on a phone lives behind the drawer.
            const ThemeToggle(),
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
        body: _dropWrap(_chatPane(l10n), l10n),
      );
    }

    // Desktop / tablet: no full-width app bar. The sidebar is a full-height
    // tinted column reaching the window's top edge (the macOS traffic lights
    // float over its top strip); the content column carries its own slim
    // header with the conversation title and the session actions.
    final canRight = width >= 1100; // inline review split
    // Keep the transcript from being squeezed out when the window is narrow:
    // the panes give up width before the chat does.
    final maxLeft = ((width - 420) / 2).clamp(200.0, 520.0);
    final maxReview = (width - 360).clamp(400.0, 1200.0);
    return Scaffold(
      body: Row(
        children: [
          if (_leftOpen) ...[
            SizedBox(
              width: _leftWidth.clamp(200, maxLeft),
              child: _sidebar(l10n),
            ),
            _splitter(
              key: const Key('left-splitter'),
              onDrag: (dx) => setState(
                () => _leftWidth = (_leftWidth + dx).clamp(200, 520),
              ),
            ),
          ],
          Expanded(
            child: Column(
              children: [
                _contentTopBar(l10n, width),
                if (needsCodexSetup) _codexSetupBar(l10n),
                Expanded(
                  child: Row(
                    children: [
                      Expanded(child: _dropWrap(_chatPane(l10n), l10n)),
                      if (canRight && _reviewOpen) ...[
                        _splitter(
                          key: const Key('review-splitter'),
                          // Dragging left widens a right-hand pane, hence the
                          // negation.
                          onDrag: (dx) => setState(
                            () => _reviewWidth = (_reviewWidth - dx).clamp(
                              400,
                              1200,
                            ),
                          ),
                        ),
                        SizedBox(
                          width: _reviewWidth.clamp(400, maxReview),
                          child: _reviewSplit(l10n),
                        ),
                      ],
                    ],
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  /// The full-height left sidebar: a tinted column that reaches the window's
  /// top edge, ChatGPT-desktop style. Its top strip hosts — once, quietly —
  /// everything the old title bar duplicated: the macOS traffic lights float
  /// over its leading inset, then the brand, then the collapse control. The
  /// strip also drags the window.
  Widget _sidebar(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    final isMac = defaultTargetPlatform == TargetPlatform.macOS;
    final strip = SizedBox(
      height: 48,
      child: Stack(
        children: [
          if (isFramelessDesktop)
            const Positioned.fill(
              child: DragToMoveArea(child: SizedBox.expand()),
            ),
          Row(
            children: [
              SizedBox(width: isFramelessDesktop && isMac ? 76 : 16),
              const BrandLogo(size: 18),
              const SizedBox(width: 7),
              Expanded(
                child: Text(
                  l10n.appTitle,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(
                    fontSize: 13.5,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              IconButton(
                tooltip: l10n.conversationsSection,
                icon: const Icon(Icons.menu_open, size: 20),
                onPressed: () => setState(() => _leftOpen = false),
              ),
              const SizedBox(width: 4),
            ],
          ),
        ],
      ),
    );
    return Material(
      // A wash over the page rather than the page itself, so the rail reads as
      // a distinct column; the splitter's hairline carries the actual edge.
      color: Color.alphaBlend(scheme.surfaceContainer, scheme.surface),
      child: Column(
        children: [
          strip,
          Expanded(child: _sessionsPane(l10n)),
        ],
      ),
    );
  }

  /// The content pane's slim header (the desktop window has no full-width app
  /// bar): the conversation title and the few session actions, over a
  /// drag-to-move area. When the sidebar is collapsed it also carries the
  /// expand control and — on macOS — the traffic-light inset; on frameless
  /// Windows it draws the caption buttons at the trailing edge.
  Widget _contentTopBar(AppLocalizations l10n, double width) {
    final isMac = defaultTargetPlatform == TargetPlatform.macOS;
    return SizedBox(
      height: 48,
      child: Stack(
        children: [
          if (isFramelessDesktop)
            const Positioned.fill(
              child: DragToMoveArea(child: SizedBox.expand()),
            ),
          Row(
            children: [
              SizedBox(
                width: !_leftOpen && isFramelessDesktop && isMac ? 76 : 8,
              ),
              if (!_leftOpen)
                IconButton(
                  tooltip: l10n.conversationsSection,
                  icon: const Icon(Icons.menu, size: 20),
                  onPressed: () => setState(() => _leftOpen = true),
                ),
              if (!widget.home)
                IconButton(
                  tooltip: l10n.backToProjects,
                  icon: const Icon(Icons.arrow_back, size: 20),
                  onPressed: _backToProjects,
                ),
              const SizedBox(width: 8),
              // The title is a label, not a banner: cap it well short of the
              // bar so a long conversation preview truncates and the rest of
              // the strip stays empty (and draggable).
              Expanded(
                child: Align(
                  alignment: Alignment.centerLeft,
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(maxWidth: 280),
                    child: _barTitleWidget(l10n),
                  ),
                ),
              ),
              if (_ctx != null)
                _ContextGauge(
                  status: _ctx!,
                  onTap: _showContextDetail,
                  tooltip: _contextTooltip(l10n),
                ),
              // Environment info lives in a popover off this button — a
              // desktop affordance, hidden when the window can't host it.
              if (width >= 900) _envButton(l10n),
              // Appearance sits with the window's own controls rather than in
              // the sidebar footer: it acts on the whole window, and it was the
              // one frequently-used control that moved (or vanished) with the
              // sidebar.
              const ThemeToggle(),
              if (_threadId != null)
                PopupMenuButton<String>(
                  tooltip: l10n.moreActions,
                  onSelected: (v) {
                    if (v == 'compact') _compact();
                    if (v == 'rename') _beginTitleEdit();
                  },
                  itemBuilder: (c) => [
                    PopupMenuItem(
                      value: 'rename',
                      child: Text(l10n.renameConversation),
                    ),
                    PopupMenuItem(value: 'compact', child: Text(l10n.compact)),
                  ],
                ),
              if (isFramelessDesktop && !isMac)
                const WindowCaptionButtons()
              else
                const SizedBox(width: 8),
            ],
          ),
        ],
      ),
    );
  }

  /// Enter title-edit mode, seeded with the current title and fully selected
  /// (so typing replaces it, the way a rename should).
  void _beginTitleEdit() {
    if (_threadId == null) return;
    final l10n = AppLocalizations.of(context);
    _titleCtrl.text = _barTitle(l10n);
    _titleCtrl.selection = TextSelection(
      baseOffset: 0,
      extentOffset: _titleCtrl.text.length,
    );
    setState(() => _editingTitle = true);
    _titleFocus.requestFocus();
  }

  /// The top bar's conversation title: a compact label that becomes a text
  /// field on click (codex-app style). Enter commits, Esc cancels, and losing
  /// focus commits too — a click elsewhere reads as "done", not "undo".
  Widget _barTitleWidget(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    const style = TextStyle(fontSize: 12.5, fontWeight: FontWeight.w600);
    if (_editingTitle) {
      return Shortcuts(
        shortcuts: const {
          SingleActivator(LogicalKeyboardKey.escape): _CancelTitleEditIntent(),
        },
        child: Actions(
          actions: {
            _CancelTitleEditIntent: CallbackAction<_CancelTitleEditIntent>(
              onInvoke: (_) {
                setState(() => _editingTitle = false);
                return null;
              },
            ),
          },
          child: TextField(
            key: const Key('bar-title-field'),
            controller: _titleCtrl,
            focusNode: _titleFocus,
            style: style,
            maxLines: 1,
            textInputAction: TextInputAction.done,
            decoration: InputDecoration(
              isDense: true,
              hintText: l10n.renameConversationHint,
              contentPadding: const EdgeInsets.symmetric(
                horizontal: 8,
                vertical: 6,
              ),
              border: const OutlineInputBorder(),
            ),
            onSubmitted: _renameThread,
            onTapOutside: (_) {
              if (_editingTitle) _renameThread(_titleCtrl.text);
            },
          ),
        ),
      );
    }
    // A title too long for the bar fades out at its trailing edge instead of
    // ending in an ellipsis, so it reads as text continuing past the cap
    // rather than as a short label. Only when it actually overflows: fading a
    // title that fits would just look like the text was dimming for no reason.
    final text = _barTitle(l10n);
    final title = LayoutBuilder(
      key: const Key('bar-title'),
      builder: (ctx, box) {
        final label = Text(
          text,
          maxLines: 1,
          softWrap: false,
          overflow: TextOverflow.clip,
          style: style,
        );
        final painter = TextPainter(
          text: TextSpan(text: text, style: style),
          maxLines: 1,
          textDirection: Directionality.of(ctx),
        )..layout();
        if (painter.width <= box.maxWidth) return label;
        // A fixed-width fade, so the ramp looks the same however long the
        // title is (a percentage would stretch with the text).
        final fade = (24 / box.maxWidth).clamp(0.0, 1.0);
        return ShaderMask(
          shaderCallback: (bounds) => LinearGradient(
            begin: Alignment.centerLeft,
            end: Alignment.centerRight,
            colors: const [Colors.black, Colors.black, Colors.transparent],
            stops: [0, 1 - fade, 1],
          ).createShader(bounds),
          blendMode: BlendMode.dstIn,
          child: label,
        );
      },
    );
    // Only a real conversation can be renamed; a fresh one has no id yet.
    if (_threadId == null) return title;
    return Tooltip(
      // A clipped label can't be read in full, so hovering reveals the whole
      // title with the rename hint underneath it.
      richMessage: TextSpan(
        children: [
          TextSpan(text: text),
          TextSpan(
            text: '\n${l10n.renameHint}',
            style: TextStyle(
              color: scheme.onInverseSurface.withValues(alpha: 0.7),
              fontSize: 11,
            ),
          ),
        ],
      ),
      waitDuration: const Duration(milliseconds: 500),
      child: Material(
        color: Colors.transparent,
        borderRadius: BorderRadius.circular(7),
        child: InkWell(
          key: const Key('bar-title-tap'),
          borderRadius: BorderRadius.circular(7),
          // A single click edits, so the label needs to look clickable on
          // hover: a soft plate plus a text cursor, since the strip behind it
          // drags the window and would otherwise claim the whole area. A text
          // cursor rather than the hand every other target gets — clicking
          // here puts a caret in the title, it isn't a button.
          hoverColor: scheme.onSurface.withValues(alpha: 0.06),
          mouseCursor: SystemMouseCursors.text,
          onTap: _beginTitleEdit,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 4),
            child: title,
          ),
        ),
      ),
    );
  }

  /// A pane divider you can drag. Reads as the same hairline as before until
  /// the pointer is over it: a wider invisible hit area (a 1 px target is
  /// unhittable) plus a resize cursor, so the affordance appears on hover the
  /// way a desktop splitter should.
  Widget _splitter({
    required Key key,
    required ValueChanged<double> onDrag,
  }) => MouseRegion(
    key: key,
    cursor: SystemMouseCursors.resizeLeftRight,
    child: GestureDetector(
      behavior: HitTestBehavior.translucent,
      onHorizontalDragUpdate: (d) => onDrag(d.delta.dx),
      child: SizedBox(
        width: 7,
        child: Center(
          // The firmer hairline, not outlineVariant: this separates two
          // near-identical grounds, so the faint one disappears between them.
          child: VerticalDivider(
            width: 1,
            color: Theme.of(context).colorScheme.outline,
          ),
        ),
      ),
    ),
  );

  /// One colored status descriptor for the current session state. Reflects the
  /// REAL state — plan mode is driven by `_planActive` (server-side), so the
  /// indicator never disagrees with how the agent is actually behaving.
  ({Color color, String label, IconData icon, bool nominal}) _sessionState(
    AppLocalizations l10n,
  ) {
    final scheme = Theme.of(context).colorScheme;
    if (_externalWriterMode) {
      final resumable = _externalWriterLiveness?.allowsResume ?? false;
      return (
        color: cautionColor(scheme),
        label: resumable
            ? l10n.sessionInUseElsewhere
            : l10n.sessionRunningElsewhere,
        icon: resumable ? Icons.lock_open_outlined : Icons.lock_clock_outlined,
        nominal: false,
      );
    }
    if (_reconnecting) {
      return (
        color: cautionColor(scheme),
        label: l10n.stateReconnecting,
        icon: Icons.autorenew,
        nominal: false,
      );
    }
    if (_connectionLost) {
      return (
        color: scheme.error,
        label: l10n.stateDisconnected,
        icon: Icons.cloud_off,
        nominal: false,
      );
    }
    if (_streaming) {
      return (
        color: scheme.primary,
        label: _planActive ? l10n.statePlanning : l10n.stateWorking,
        icon: Icons.autorenew,
        nominal: false,
      );
    }
    if (_planActive) {
      return (
        color: cautionColor(scheme),
        label: l10n.statePlanMode,
        icon: Icons.checklist_rtl,
        nominal: false,
      );
    }
    // At rest, and therefore drawn in ink rather than a hue: this is the state
    // the bar sits in nearly all the time, so tinting it would make the least
    // urgent thing on screen the loudest. Colour escalates from here —
    // accent while working, amber while degraded, error when disconnected.
    return (
      color: scheme.onSurfaceVariant,
      label: l10n.stateReady,
      icon: Icons.check_circle_outline,
      nominal: true,
    );
  }

  /// Display name for a model id, falling back to the raw id when it isn't in
  /// the cached list (e.g. an unlisted config default). Null for null/empty.
  String? _modelDisplayLabel(String? id) {
    if (id == null || id.isEmpty) return null;
    return _models.where((m) => m.id == id).firstOrNull?.displayName ?? id;
  }

  /// The model actively serving this thread, per the best truth available:
  /// a server-confirmed runtime first; else what the app last sent (turn
  /// params override thread defaults, so on servers that never notify the
  /// sent value IS the effective one); else the server's start/resume
  /// snapshot. Deliberately NOT the user's unsent selection — that's the
  /// composer pill's job; this indicator only ever claims engaged state.
  /// `confirmed` is true only when the shown id matches the server's report.
  ({String id, bool confirmed})? _activeModelStatus() {
    final rt = _runtime;
    final String? id;
    if (rt != null && rt.confirmedByUpdate) {
      id = rt.model ?? _sentModel;
    } else {
      id = _sentModel ?? rt?.model;
    }
    if (id == null || id.isEmpty) return null;
    final rtModel = rt?.model;
    return (id: id, confirmed: rtModel != null && id == rtModel);
  }

  /// A thin, always-visible status bar: a colored state chip (plan / working /
  /// ready / disconnected) + the active model + the git branch, so the
  /// session's true state is glanceable and consistent with the chat.
  Widget _statusBar(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    final st = _sessionState(l10n);
    final d = _diff;
    final activeModel = _activeModelStatus();
    final running = _streaming || _externalWriterRunning;
    return Container(
      width: double.infinity,
      // At rest the bar is part of the page — a faint ink wash under a hairline,
      // like any other chrome. A state that needs attention tints the whole
      // strip, so colour arriving here means something actually changed.
      decoration: BoxDecoration(
        color: st.nominal
            ? scheme.surfaceContainerLowest
            : st.color.withValues(alpha: 0.10),
        border: Border(bottom: BorderSide(color: scheme.outlineVariant)),
      ),
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 5),
      child: Row(
        children: [
          if (_externalWriterRunning)
            PulsingDot(
              key: const Key('chat-status-running-pulse'),
              color: st.color,
              size: 8,
            )
          else
            Icon(st.icon, size: 13, color: st.color),
          const SizedBox(width: 6),
          Text(
            st.label,
            style: TextStyle(
              fontSize: 12,
              color: st.color,
              // Resting state reads as a label, not an alert.
              fontWeight: st.nominal ? FontWeight.w500 : FontWeight.w600,
            ),
          ),
          const Spacer(),
          // The model actively handling this thread's requests — server truth
          // when confirmed (✓), else the best-known effective value. Tap for
          // the full runtime configuration, so a model switch is verifiable.
          if (activeModel != null) ...[
            Tooltip(
              message: l10n.activeModelTooltip,
              child: InkWell(
                mouseCursor: clickable,
                key: const Key('active-model-chip'),
                onTap: _showRuntimeSheet,
                borderRadius: BorderRadius.circular(kControlRadius),
                child: Padding(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 6,
                    vertical: 2,
                  ),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(
                        Icons.auto_awesome,
                        size: 12,
                        color: scheme.onSurfaceVariant,
                      ),
                      const SizedBox(width: 4),
                      ConstrainedBox(
                        constraints: const BoxConstraints(maxWidth: 180),
                        child: Text(
                          _modelDisplayLabel(activeModel.id)!,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            fontSize: 12,
                            color: scheme.onSurfaceVariant,
                          ),
                        ),
                      ),
                      const SizedBox(width: 3),
                      Icon(
                        activeModel.confirmed
                            ? Icons.check_circle_outline
                            : Icons.hourglass_empty,
                        size: 11,
                        color: activeModel.confirmed
                            ? additionColor(scheme)
                            : scheme.onSurfaceVariant,
                      ),
                    ],
                  ),
                ),
              ),
            ),
            const SizedBox(width: 10),
          ],
          // Live elapsed clock for the running turn. A local turn freezes this
          // into its transcript footnote; an external turn counts from when
          // this read-only observer attached and disappears when it finishes.
          if (running) ...[
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
              message: _diffLoading
                  ? l10n.cancelDiffLoad
                  : (d != null && !d.isEmpty)
                  ? l10n.viewDiff
                  : _branch!,
              child: InkWell(
                mouseCursor: clickable,
                key: const Key('status-branch-chip'),
                onTap: _showDiff,
                borderRadius: BorderRadius.circular(kControlRadius),
                child: Padding(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 6,
                    vertical: 2,
                  ),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      // While the diff is loading the badge IS the cancel
                      // control: a spinner in place of the branch glyph, so the
                      // press that started it can also stop it.
                      if (_diffLoading)
                        SizedBox(
                          width: 12,
                          height: 12,
                          child: CircularProgressIndicator(
                            strokeWidth: 1.6,
                            color: scheme.primary,
                          ),
                        )
                      else
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
                            color: additionColor(scheme),
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
                                      listController: _listCtl,
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
                                            key: _externalWriterRunning
                                                ? const Key(
                                                    'chat-external-output-indicator',
                                                  )
                                                : null,
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
                                        if (row is _Group) {
                                          return _GroupedActivityCard(
                                            key: ValueKey(
                                              'g:${row.items.first.id}:'
                                              '${row.items.length}',
                                            ),
                                            group: row,
                                          );
                                        }
                                        // A merged reply renders through the
                                        // same view as a single one, so the two
                                        // can't drift apart: it is presented as
                                        // one item whose text is the whole turn.
                                        if (row is _AgentTurn) {
                                          return _MessageView(
                                            key: ValueKey(
                                              't:${row.items.first.id}:'
                                              '${row.items.length}',
                                            ),
                                            item: _Item(
                                              id: row.items.first.id,
                                              type: 'agentMessage',
                                              text: row.text,
                                              streaming: row.streaming,
                                              turnId: row.items.first.turnId,
                                              turnCompletedAt: row.completedAt,
                                            ),
                                            hostImageLoader: _loadHostImage,
                                          );
                                        }
                                        return _MessageView(
                                          key: ValueKey((row as _Item).id),
                                          item: row,
                                          hostImageLoader: _loadHostImage,
                                        );
                                      },
                                    );
                                  },
                                ),
                              ),
                              // Compact navigation cluster (bottom-right): jump
                              // between conversation turns, and to the latest
                              // message — so long transcripts are easy to move
                              // through on mobile and desktop alike.
                              Positioned(
                                right: 12,
                                bottom: 12,
                                child: _navCluster(),
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
        // Keyed by request id so State follows the right prompt if more than one
        // server request is pending and one is answered/removed out of order.
        for (final a in _externalWriterMode ? const <AppEvent>[] : _approvals)
          if (a.kind == 'item/tool/requestUserInput')
            _UserInputCard(
              key: ValueKey(a.requestId),
              prompt: a,
              onAnswer: _answerUserInput,
            )
          else
            _ApprovalCard(
              key: ValueKey(a.requestId),
              prompt: a,
              onDecide: _decide,
            ),
        // After a plan-mode turn, offer to implement the plan (persists across
        // restart since it's derived from the trailing plan item).
        if (!_externalWriterMode && _planReady) _implementBar(l10n),
        if (_error != null) _errorBanner(l10n),
        if (_externalWriterMode)
          _externalWriterAction(l10n)
        else
          _composer(l10n),
      ],
    );
  }

  Widget _externalWriterAction(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    final liveness = _externalWriterLiveness;
    final canResume = liveness?.allowsResume ?? false;
    final hasCwd = _cwd?.trim().isNotEmpty ?? false;
    final stateControl = SizedBox(
      width: double.infinity,
      height: 46,
      child: canResume
          ? FilledButton.icon(
              key: const Key('chat-takeover-action'),
              onPressed: _takingOver ? null : _takeOverExternalWriter,
              icon: _takingOver
                  ? const SizedBox(
                      width: 16,
                      height: 16,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Icon(Icons.bolt, size: 18),
              label: Text(
                liveness!.requiresTakeover
                    ? l10n.forceTakeover
                    : l10n.resumeSession,
              ),
            )
          : DecoratedBox(
              key: const Key('chat-read-only-action'),
              decoration: BoxDecoration(
                color: scheme.surfaceContainer,
                borderRadius: BorderRadius.circular(kPanelRadius),
                border: Border.all(color: scheme.outlineVariant),
              ),
              child: Row(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  Icon(
                    Icons.lock_outline,
                    size: 16,
                    color: scheme.onSurfaceVariant,
                  ),
                  const SizedBox(width: 8),
                  Flexible(
                    child: Text(
                      l10n.readOnlyViewing,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        color: scheme.onSurfaceVariant,
                        fontSize: 13,
                      ),
                    ),
                  ),
                ],
              ),
            ),
    );
    return SafeArea(
      top: false,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 6, 16, 14),
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 820),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                stateControl,
                if (hasCwd) ...[
                  const SizedBox(height: 8),
                  SizedBox(
                    width: double.infinity,
                    height: 42,
                    child: OutlinedButton.icon(
                      key: const Key('chat-external-diff-action'),
                      onPressed: _diffLoading ? _cancelDiff : _showDiff,
                      icon: _diffLoading
                          ? const SizedBox(
                              width: 16,
                              height: 16,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            )
                          : const Icon(Icons.difference_outlined, size: 18),
                      label: Text(
                        _diffLoading ? l10n.cancelDiffLoad : l10n.viewDiff,
                      ),
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
    return LayoutBuilder(
      builder: (context, c) {
        // Two columns of cards once there's room for them; a phone-width pane
        // keeps the single column (a 2-up grid there is unreadable).
        final twoUp = c.maxWidth >= 560;
        return Center(
          child: SingleChildScrollView(
            padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 32),
            child: ConstrainedBox(
              constraints: BoxConstraints(maxWidth: twoUp ? 620 : 460),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  // A terminal mark, not a sparkle: this drives a real shell on
                  // a real checkout.
                  Container(
                    width: 56,
                    height: 56,
                    alignment: Alignment.center,
                    decoration: BoxDecoration(
                      color: scheme.surfaceContainerHighest,
                      borderRadius: BorderRadius.circular(kPanelRadius),
                      border: Border.all(
                        color: scheme.outlineVariant,
                        width: 0.5,
                      ),
                    ),
                    child: Icon(
                      Icons.terminal_rounded,
                      size: 28,
                      color: scheme.primary,
                    ),
                  ),
                  const SizedBox(height: 18),
                  _newSessionHeadline(l10n),
                  const SizedBox(height: 8),
                  Text(
                    l10n.newSessionSubtitle,
                    textAlign: TextAlign.center,
                    style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                      color: scheme.onSurfaceVariant,
                    ),
                  ),
                  const SizedBox(height: 26),
                  if (twoUp)
                    // Pair the cards into rows so the two in a row share a
                    // height regardless of how long each prompt wraps.
                    for (var i = 0; i < suggestions.length; i += 2) ...[
                      IntrinsicHeight(
                        child: Row(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            Expanded(child: _suggestionCard(suggestions[i])),
                            const SizedBox(width: 12),
                            Expanded(
                              child: i + 1 < suggestions.length
                                  ? _suggestionCard(suggestions[i + 1])
                                  : const SizedBox.shrink(),
                            ),
                          ],
                        ),
                      ),
                      const SizedBox(height: 12),
                    ]
                  else
                    for (final s in suggestions) ...[
                      _suggestionCard(s),
                      const SizedBox(height: 10),
                    ],
                ],
              ),
            ),
          ),
        );
      },
    );
  }

  /// The empty state's headline. With a project in effect it names it inline
  /// and makes that word the project switcher, so changing what the next
  /// conversation is about is one click on the thing being changed — rather
  /// than three levels down a settings sheet.
  Widget _newSessionHeadline(AppLocalizations l10n) {
    final style = Theme.of(
      context,
    ).textTheme.headlineSmall?.copyWith(fontWeight: FontWeight.w600);
    final project = _cwd?.trim();
    if (project == null || project.isEmpty) {
      // No project: nothing to name, so the plain question + a switcher below.
      return Column(
        children: [
          Text(l10n.newSessionTitle, textAlign: TextAlign.center, style: style),
          const SizedBox(height: 10),
          _projectSwitcher(l10n, label: l10n.projectsSection, dimmed: true),
        ],
      );
    }
    final parts = l10n.newSessionTitleIn(_kProjectSlot).split(_kProjectSlot);
    return Wrap(
      alignment: WrapAlignment.center,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        if (parts.isNotEmpty && parts.first.isNotEmpty)
          Text(parts.first, style: style),
        _projectSwitcher(l10n, label: _projectName(), style: style),
        if (parts.length > 1 && parts[1].isNotEmpty)
          Text(parts[1], style: style),
      ],
    );
  }

  /// The project name rendered as a dropdown trigger, wired to [ProjectMenu].
  Widget _projectSwitcher(
    AppLocalizations l10n, {
    required String label,
    TextStyle? style,
    bool dimmed = false,
  }) {
    final scheme = Theme.of(context).colorScheme;
    return ProjectMenu(
      projects: _knownProjects(),
      current: _cwd?.trim().isEmpty ?? true ? null : _cwd!.trim(),
      onPick: (p) => setState(() => _cwd = p),
      onBrowse: () async {
        final picked = await showFolderPicker(
          context,
          serviceKey: widget.serviceKey,
          initialPath: _cwd,
        );
        if (picked != null && mounted) setState(() => _cwd = picked);
      },
      onClear: () => setState(() => _cwd = null),
      builder: (ctx, ctrl) => InkWell(
        mouseCursor: clickable,
        key: const Key('project-switcher-btn'),
        borderRadius: BorderRadius.circular(8),
        onTap: () => ctrl.isOpen ? ctrl.close() : ctrl.open(),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (dimmed) ...[
                Icon(
                  Icons.folder_outlined,
                  size: 16,
                  color: scheme.onSurfaceVariant,
                ),
                const SizedBox(width: 6),
              ],
              Text(
                label,
                style:
                    style?.copyWith(
                      decoration: TextDecoration.underline,
                      decorationColor: scheme.outlineVariant,
                    ) ??
                    TextStyle(fontSize: 13, color: scheme.onSurfaceVariant),
              ),
              const SizedBox(width: 2),
              Icon(
                Icons.expand_more,
                size: style == null ? 16 : 20,
                color: scheme.onSurfaceVariant,
              ),
            ],
          ),
        ),
      ),
    );
  }

  /// Projects this host is known to have: every distinct working directory
  /// across ALL its conversations, most recently used first, with the open
  /// project pinned to the top. That is the list the user actually works in —
  /// anything else is reachable via "new project".
  List<String> _knownProjects() {
    final seen = <String>{};
    final out = <String>[];
    void add(String? raw) {
      final p = raw?.trim();
      if (p == null || p.isEmpty) return;
      if (seen.add(p)) out.add(p);
    }

    add(_cwd);
    for (final p in _allProjects) {
      add(p);
    }
    return out;
  }

  /// One tappable starter-prompt card; tapping prefills + focuses the composer.
  Widget _suggestionCard((IconData, String, String) s) {
    final (icon, title, prompt) = s;
    final scheme = Theme.of(context).colorScheme;
    return Material(
      // The page ground under the wash: the container ladder is a translucent
      // ink, and Material composites its colour against nothing, so a wash
      // handed to it directly would paint as flat dark ink.
      color: scheme.surface,
      borderRadius: BorderRadius.circular(kPanelRadius),
      child: InkWell(
        mouseCursor: clickable,
        borderRadius: BorderRadius.circular(kPanelRadius),
        onTap: () => _useSuggestion(prompt),
        child: Container(
          decoration: BoxDecoration(
            color: scheme.surfaceContainerLow,
            border: Border.all(color: scheme.outlineVariant),
            borderRadius: BorderRadius.circular(kPanelRadius),
          ),
          padding: const EdgeInsets.fromLTRB(14, 13, 14, 13),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              Row(
                children: [
                  Icon(icon, size: 17, color: scheme.primary),
                  const SizedBox(width: 9),
                  Expanded(
                    child: Text(
                      title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: Theme.of(context).textTheme.titleSmall,
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 6),
              Text(
                prompt,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.bodySmall?.copyWith(
                  color: scheme.onSurfaceVariant,
                  height: 1.35,
                ),
              ),
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
    final running = <String>{
      ...?ref.watch(runningThreadsProvider(widget.serviceKey)).valueOrNull,
      if (_externalWriterRunning && _threadId != null) _threadId!,
    };
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
              .where(
                (t) =>
                    // A renamed conversation must be findable by the name the
                    // user gave it — that's the text the row actually shows,
                    // and searching the preview alone would miss it entirely.
                    (t.title?.toLowerCase().contains(q) ?? false) ||
                    t.preview.toLowerCase().contains(q) ||
                    // The home pane spans projects, so let the filter reach
                    // the project path too (mirrors the project tree's search).
                    (widget.home && t.cwd.toLowerCase().contains(q)),
              )
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
        Widget tile(ThreadMeta t, {required bool showProject}) =>
            _conversationTile(
              thread: t,
              running: running.contains(t.id),
              selected: t.id == _threadId,
              now: now,
              l10n: l10n,
              // Only rows NOT already sitting under a project heading carry
              // their own project name — under one it's just noise.
              project: showProject ? _leafOf(t.cwd) : null,
              onTap: () {
                closeDrawerIfOpen(ctx);
                if (t.id != _threadId) _openThread(t.id, t.cwd);
              },
            );
        // The activity view's taller row: name in bold over a one-line summary
        // of where the conversation got to.
        Widget activityTile(ThreadMeta t) => _activityTile(
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
        // Row BUILDERS, not built rows. The activity view's tiles each read a
        // thread summary (a full `thread/read` server-side) while building, so a
        // pre-built list would fire one per conversation the moment the view is
        // toggled — 500 transcripts across a relay tunnel to fill one viewport.
        // Deferring construction lets `ListView.builder` create only the rows it
        // actually shows, which is what makes the lazy fetch lazy.
        final rows = <Widget Function()>[];
        void group(
          String label,
          List<ThreadMeta> items, {
          bool showProject = false,
        }) {
          if (items.isEmpty) return;
          rows.add(() => _sectionLabel(label));
          rows.addAll(
            // Whichever view is on, one list means one row shape — an Active
            // group in compact rows above summarized ones would read as two
            // lists stapled together.
            items.map(
              (t) => _activityView
                  ? () => activityTile(t)
                  : () => tile(t, showProject: showProject),
            ),
          );
        }

        // Running threads always lead, whatever project they belong to — they
        // are the only rows the user may need to reach urgently.
        group(l10n.groupActive, active, showProject: widget.home);
        if (_activityView) {
          // "When was I working on this", grouped by the day it last moved.
          // Each row carries a one-line summary of the newest agent reply, so
          // the list answers "what happened" without opening anything.
          for (final d in _byDay(<ThreadMeta>[...today, ...earlier], now)) {
            group(d.label, d.threads);
          }
        } else if (widget.home) {
          // The home pane spans every project, so the rest reads as a tree:
          // project → its conversations, newest project first, with the open
          // project pinned to the top. Each project shows only its newest few
          // rows unless expanded, so many projects stay scannable at a glance.
          // A live search is the one case that shows everything: the user is
          // looking for a specific row, so hiding matches behind "show more"
          // would be actively unhelpful.
          final searching = q.isNotEmpty;
          for (final p in _byProject(<ThreadMeta>[...today, ...earlier])) {
            rows.add(
              () => _projectSectionLabel(p.cwd, count: p.threads.length),
            );
            // A search overrides a collapsed project: these rows already
            // matched the query, so hiding them would answer "no results" to a
            // search that DID find something.
            if (!searching && _collapsedProjects.contains(p.cwd)) continue;
            final expanded = searching || _expandedProjects.contains(p.cwd);
            var shown = expanded
                ? p.threads
                : p.threads.take(_projectPeek).toList(growable: false);
            // Never truncate away the conversation that's actually open: the
            // sidebar would show no selection at all, and the user loses where
            // they are. It's the newest rows that are worth previewing, so keep
            // them and append the open one rather than reordering.
            if (!expanded && !shown.any((t) => t.id == _threadId)) {
              final open = p.threads.where((t) => t.id == _threadId);
              if (open.isNotEmpty) shown = [...shown, open.first];
            }
            rows.addAll(
              shown.map(
                (t) =>
                    () => tile(t, showProject: false),
              ),
            );
            final hidden = p.threads.length - shown.length;
            if (!searching && (hidden > 0 || expanded)) {
              rows.add(
                () => _projectPeekToggle(
                  cwd: p.cwd,
                  hidden: hidden,
                  expanded: expanded,
                  l10n: l10n,
                ),
              );
            }
          }
        } else {
          // A project-scoped pane already has one project, so recency is the
          // only axis left worth splitting on.
          group(l10n.groupToday, today);
          group(l10n.groupEarlier, earlier);
        }

        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            // In the mobile drawer, "back to projects" lives here (the AppBar's
            // leading button opens this drawer instead). Desktop shows it as the
            // AppBar leading, so the inline pane omits it. The home has no back
            // destination at all.
            if (inDrawer && !widget.home) ...[
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
            // Mobile drawer only: the brand header (the desktop sidebar's top
            // strip already carries it).
            if (inDrawer && widget.home)
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 14, 16, 6),
                child: Row(
                  children: [
                    const BrandLogo(size: 20),
                    const SizedBox(width: 8),
                    Expanded(
                      child: Text(
                        l10n.appTitle,
                        style: Theme.of(context).textTheme.titleSmall?.copyWith(
                          fontWeight: FontWeight.w600,
                        ),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                  ],
                ),
              ),
            // Home only: which host this chat runs on, switchable when several
            // app services are available.
            if (widget.home) _serviceSwitcher(l10n),
            // Header: title + a circular "new conversation" button (echoes the
            // composer's send button).
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 14, 12, 2),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      _activityView
                          ? l10n.activityView
                          : l10n.conversationsSection,
                      style: Theme.of(context).textTheme.titleSmall,
                    ),
                  ),
                  // Group by project, or by when it happened. Two ways of
                  // asking "what was I doing", so it's a toggle rather than a
                  // replacement — the project tree answers "where", this
                  // answers "when".
                  IconButton(
                    key: const Key('activity-view-btn'),
                    icon: Icon(
                      _activityView
                          ? Icons.folder_outlined
                          : Icons.history_toggle_off,
                      size: 18,
                    ),
                    tooltip: _activityView
                        ? l10n.conversationsSection
                        : l10n.activityView,
                    visualDensity: VisualDensity.compact,
                    onPressed: () =>
                        setState(() => _activityView = !_activityView),
                  ),
                  const SizedBox(width: 2),
                  Material(
                    color: scheme.primary,
                    shape: const CircleBorder(),
                    child: InkWell(
                      mouseCursor: clickable,
                      key: const Key('new-conversation-btn'),
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
                      borderRadius: BorderRadius.circular(kControlRadius),
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
                  : ListView.builder(
                      padding: const EdgeInsets.fromLTRB(8, 2, 8, 8),
                      itemCount: rows.length,
                      itemBuilder: (_, i) => rows[i](),
                    ),
            ),
            // Account quota, always visible rather than buried behind the
            // gauge: it is warm from connect (see _loadQuota), so this row
            // costs nothing and answers "how much have I got left" at a glance.
            ?_quotaStrip(l10n),
            // Home only: everything that used to require navigating away from
            // the chat — management, the host's session browser (incl. force
            // takeover), logs, settings — one tap from the sidebar.
            if (widget.home) ...[
              const Divider(height: 1),
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
                // Each button keeps its own share of the row instead of its
                // intrinsic width: the sidebar drags down to 200 px, where a
                // fixed-width row of five (manage / sessions / logs / theme /
                // settings) would overflow rather than tighten.
                child: Row(
                  children: [
                    Expanded(
                      child: _paneShortcut(
                        key: 'sidebar-manage-btn',
                        icon: Icons.dns_outlined,
                        tooltip: l10n.manageServices,
                        ctx: ctx,
                        route: '/manage',
                      ),
                    ),
                    // The host session browser rides the meta tunnel, which is
                    // an account-mode feature (mirrors the manage page's
                    // Sessions tab gate).
                    if (ref.watch(configProvider).valueOrNull?.mode ==
                        'account')
                      Expanded(
                        child: _paneShortcut(
                          key: 'sidebar-history-btn',
                          icon: Icons.history,
                          tooltip: l10n.hostSessions,
                          ctx: ctx,
                          route: Uri(
                            path: '/sessions',
                            queryParameters: {'svc': widget.serviceKey},
                          ).toString(),
                        ),
                      ),
                    Expanded(
                      child: _paneShortcut(
                        key: 'sidebar-logs-btn',
                        icon: Icons.article_outlined,
                        tooltip: l10n.logsTitle,
                        ctx: ctx,
                        route: '/logs',
                      ),
                    ),
                    Expanded(
                      child: _paneShortcut(
                        key: 'sidebar-settings-btn',
                        icon: Icons.settings_outlined,
                        tooltip: l10n.settingsTitle,
                        ctx: ctx,
                        route: '/settings',
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ],
        );
      },
    );
  }

  /// Sidebar footer: how much account quota is left, on the window that is
  /// closest to running out. Null when the host reports no quota at all (a
  /// custom provider), so the footer simply doesn't grow the row.
  Widget? _quotaStrip(AppLocalizations l10n) {
    final r = _rate;
    if (r == null) return null;
    // Whichever window is furthest along is the one that will actually stop
    // the user, so that's the one worth showing in a single line.
    final windows = [
      if (r.primary != null) (l10n.quota5h, r.primary!),
      if (r.secondary != null) (l10n.quotaWeekly, r.secondary!),
    ]..sort((a, b) => b.$2.usedPercent.compareTo(a.$2.usedPercent));
    if (windows.isEmpty) return null;
    final (label, w) = windows.first;
    final scheme = Theme.of(context).colorScheme;
    final left = (100 - w.usedPercent).clamp(0, 100).round();
    // Same thresholds as the context gauge, so "amber" means the same thing
    // everywhere in the app.
    final color = w.fraction >= 0.9
        ? scheme.error
        : w.fraction >= 0.75
        ? cautionColor(scheme)
        : scheme.primary;
    final reset = _resetText(w, l10n);
    return InkWell(
      mouseCursor: clickable,
      key: const Key('sidebar-quota'),
      onTap: _showContextDetail,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 6),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(
                  Icons.data_usage,
                  size: 13,
                  color: scheme.onSurfaceVariant,
                ),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(
                    l10n.quotaRemaining,
                    style: TextStyle(
                      fontSize: 11.5,
                      color: scheme.onSurfaceVariant,
                    ),
                  ),
                ),
                Text(
                  '$left%',
                  style: TextStyle(
                    fontSize: 11.5,
                    fontWeight: FontWeight.w600,
                    color: color,
                    fontFeatures: const [FontFeature.tabularFigures()],
                  ),
                ),
              ],
            ),
            const SizedBox(height: 5),
            ClipRRect(
              borderRadius: BorderRadius.circular(3),
              child: LinearProgressIndicator(
                value: w.fraction,
                minHeight: 4,
                color: color,
                backgroundColor: scheme.surfaceContainerHighest,
              ),
            ),
            const SizedBox(height: 4),
            Text(
              reset.isEmpty ? label : '$label · $reset',
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(fontSize: 10.5, color: scheme.outline),
            ),
          ],
        ),
      ),
    );
  }

  /// Sidebar footer: flip the app between light and dark without a trip to
  /// settings — appearance is the one setting people change on a whim (and by
  /// time of day), so it earns a one-tap control next to the other shortcuts.
  ///
  /// Two states, not three. It used to cycle system → light → dark to keep
  /// "follow the OS" reachable, but a control whose next state you can't
  /// predict from its icon is a worse trade than losing one-tap access to a
  /// default that Settings still offers. The icon shows what is in effect now;
  /// tapping shows the other one.
  /// One sidebar-footer shortcut: closes the drawer (mobile) then pushes.
  Widget _paneShortcut({
    required String key,
    required IconData icon,
    required String tooltip,
    required BuildContext ctx,
    required String route,
  }) => IconButton(
    key: Key(key),
    icon: Icon(icon, size: 20),
    tooltip: tooltip,
    visualDensity: VisualDensity.compact,
    onPressed: () {
      if (Scaffold.maybeOf(ctx)?.isDrawerOpen ?? false) Navigator.pop(ctx);
      context.push(route);
    },
  );

  /// Home-pane header row: the host currently serving this chat. Renders a
  /// dropdown when more than one app service is connectable, else a static
  /// identity row — either way the user always sees WHERE the conversation
  /// runs.
  Widget _serviceSwitcher(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    String labelOf(ServiceEntry s) => '${s.device} · ${s.name}';
    final entries = widget.services;
    final multiple = entries.length > 1;
    final current = entries.where((s) => s.key == widget.serviceKey).toList();
    final currentLabel = current.isEmpty
        ? _serviceLabelFromKey(widget.serviceKey)
        : labelOf(current.first);
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 10, 12, 0),
      child: Row(
        children: [
          Icon(Icons.computer, size: 16, color: scheme.primary),
          const SizedBox(width: 8),
          Expanded(
            child: multiple
                ? DropdownButtonHideUnderline(
                    child: DropdownButton<String>(
                      key: const Key('sidebar-service-switcher'),
                      value: current.isEmpty ? null : widget.serviceKey,
                      hint: Text(
                        currentLabel,
                        style: const TextStyle(fontSize: 13),
                        overflow: TextOverflow.ellipsis,
                      ),
                      isExpanded: true,
                      isDense: true,
                      style: TextStyle(fontSize: 13, color: scheme.onSurface),
                      items: [
                        for (final s in entries)
                          DropdownMenuItem(
                            value: s.key,
                            child: Text(
                              labelOf(s),
                              overflow: TextOverflow.ellipsis,
                            ),
                          ),
                      ],
                      onChanged: (key) {
                        if (key != null && key != widget.serviceKey) {
                          widget.onSwitchService?.call(key);
                        }
                      },
                    ),
                  )
                : Text(
                    currentLabel,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      fontSize: 13,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
          ),
        ],
      ),
    );
  }

  /// `device · name` fallback parsed from a `pcx:<device>:app:<name>` key,
  /// for when the switcher's service list doesn't contain the active key.
  static String _serviceLabelFromKey(String key) {
    final parts = key.split(':');
    return parts.length >= 4
        ? '${parts[1]} · ${parts.sublist(3).join(':')}'
        : key;
  }

  /// Leaf folder name of [cwd], or empty when unknown.
  static String _leafOf(String cwd) {
    final c = cwd.trim();
    if (c.isEmpty) return '';
    final segs = c.split(RegExp(r'[\\/]'))..removeWhere((s) => s.isEmpty);
    return segs.isEmpty ? c : segs.last;
  }

  /// Bucket [threads] by the project they run in, preserving each bucket's
  /// incoming (recency) order. The currently-open project leads; the rest
  /// follow by how recently anything in them was touched.
  List<({String cwd, List<ThreadMeta> threads})> _byProject(
    List<ThreadMeta> threads,
  ) {
    final buckets = <String, List<ThreadMeta>>{};
    for (final t in threads) {
      buckets.putIfAbsent(t.cwd, () => <ThreadMeta>[]).add(t);
    }
    final current = _cwd?.trim();
    final keys = buckets.keys.toList()
      ..sort((a, b) {
        if (a == current) return b == current ? 0 : -1;
        if (b == current) return 1;
        // Each bucket keeps the source order, so its head IS its newest thread.
        return buckets[b]!.first.updatedAt.compareTo(
          buckets[a]!.first.updatedAt,
        );
      });
    return [for (final k in keys) (cwd: k, threads: buckets[k]!)];
  }

  /// Drops the cached activity-view summary for [threadId], so the next build
  /// re-reads it. Called when a turn ends: the summary is the newest agent
  /// reply, and that is exactly what just changed.
  void _invalidateSummary(String? threadId) {
    if (threadId == null || threadId.isEmpty) return;
    ref.invalidate(
      threadSummaryProvider(threadSummaryKey(widget.serviceKey, threadId)),
    );
    // A finished turn also moves the thread's `updatedAt`, which the activity
    // view groups BY — so a conversation resumed from an older day would keep
    // sitting under that old day, with a stale relative time and sort position,
    // until something else happened to re-list.
    _scheduleThreadsRefresh();
  }

  /// Re-list threads shortly, coalescing bursts into one request.
  ///
  /// Debounced because several threads can finish within moments of each other
  /// (queued sends, parallel conversations) and each end would otherwise issue
  /// its own `thread/list`. The delay also lets the server commit the turn
  /// before we ask for the timestamps it just changed.
  void _scheduleThreadsRefresh() {
    _threadsRefreshTimer?.cancel();
    _threadsRefreshTimer = Timer(const Duration(milliseconds: 400), () {
      if (mounted) _loadThreads();
    });
  }

  /// Buckets threads by the calendar day they last moved, newest day first —
  /// the activity view's spine.
  ///
  /// Labels lean on the calendar rather than elapsed time: "Today" /
  /// "Yesterday", then the weekday name for the rest of the past week (a date
  /// that recent reads better as "Thursday"), and a plain date beyond that.
  /// Threads with no timestamp land in one trailing "Earlier" bucket instead of
  /// claiming the epoch as their day.
  List<({String label, List<ThreadMeta> threads})> _byDay(
    List<ThreadMeta> threads,
    DateTime now,
  ) {
    final l10n = AppLocalizations.of(context);
    final locale = l10n.localeName;
    final today = DateTime(now.year, now.month, now.day);
    // Insertion order IS the output order: the input arrives newest-first, so
    // each new day appends after the days already seen.
    final buckets = <String, List<ThreadMeta>>{};
    final undated = <ThreadMeta>[];
    for (final t in threads) {
      if (t.updatedAt <= 0) {
        undated.add(t);
        continue;
      }
      final at = DateTime.fromMillisecondsSinceEpoch(t.updatedAt * 1000);
      final day = DateTime(at.year, at.month, at.day);
      final label = switch (today.difference(day).inDays) {
        <= 0 => l10n.groupToday,
        1 => l10n.groupYesterday,
        // Weekday names only stay unambiguous inside one week — past that
        // "Thursday" could be any Thursday, so switch to a date.
        < 7 => DateFormat.EEEE(locale).format(day),
        _ => DateFormat.yMMMd(locale).format(day),
      };
      buckets.putIfAbsent(label, () => <ThreadMeta>[]).add(t);
    }
    return [
      for (final e in buckets.entries) (label: e.key, threads: e.value),
      // Undated rows can't claim a day, and they sort nowhere in particular in
      // the source order — so they go last under one catch-all heading rather
      // than splitting the timeline wherever the first one happened to appear.
      if (undated.isNotEmpty) (label: l10n.groupEarlier, threads: undated),
    ];
  }

  /// A project heading in the home pane's conversation tree: the folder is the
  /// hit target, so clicking it collapses/expands the project's conversations
  /// (codex-app style). [count] is how many rows sit under it, shown while
  /// collapsed so a folded project still says how much it holds.
  Widget _projectSectionLabel(String cwd, {required int count}) {
    final scheme = Theme.of(context).colorScheme;
    final leaf = _leafOf(cwd);
    final collapsed = _collapsedProjects.contains(cwd);
    return Padding(
      padding: const EdgeInsets.only(top: 10, bottom: 2),
      child: Material(
        color: Colors.transparent,
        borderRadius: BorderRadius.circular(8),
        child: InkWell(
          mouseCursor: clickable,
          key: Key('project-header-$cwd'),
          borderRadius: BorderRadius.circular(8),
          onTap: () => setState(() {
            if (!_collapsedProjects.remove(cwd)) _collapsedProjects.add(cwd);
          }),
          child: Padding(
            padding: const EdgeInsets.fromLTRB(8, 5, 8, 5),
            child: Row(
              children: [
                // A chevron, not a folder: it states which way the group will
                // move when clicked, which is what the hit target actually
                // does. The folder glyph only restated "this is a project",
                // already obvious from the heading's weight and indent.
                Icon(
                  collapsed
                      ? Icons.keyboard_arrow_right
                      : Icons.keyboard_arrow_down,
                  size: 18,
                  color: scheme.onSurfaceVariant,
                ),
                const SizedBox(width: 3),
                Expanded(
                  child: Tooltip(
                    message: cwd.trim().isEmpty ? '' : cwd,
                    child: Text(
                      leaf.isEmpty
                          ? AppLocalizations.of(context).defaultFolder
                          : leaf,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        // A project is the tree's top level, so it reads a
                        // notch LARGER than the conversation rows beneath it
                        // (which are 13) — not just bolder.
                        fontSize: 14,
                        fontWeight: FontWeight.w700,
                        color: scheme.onSurface,
                      ),
                    ),
                  ),
                ),
                if (collapsed) ...[
                  const SizedBox(width: 6),
                  Text(
                    '$count',
                    style: TextStyle(
                      fontSize: 11,
                      color: scheme.onSurfaceVariant,
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

  /// The "show N more" / "show less" row that ends a partially-shown project
  /// group. A quiet text link, not a button: it's navigation within a list,
  /// and the rows above it are what the eye should land on.
  Widget _projectPeekToggle({
    required String cwd,
    required int hidden,
    required bool expanded,
    required AppLocalizations l10n,
  }) {
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.only(left: 22, bottom: 2),
      child: Align(
        alignment: Alignment.centerLeft,
        child: Material(
          color: Colors.transparent,
          borderRadius: BorderRadius.circular(6),
          child: InkWell(
            mouseCursor: clickable,
            key: Key('project-peek-$cwd'),
            borderRadius: BorderRadius.circular(6),
            onTap: () => setState(() {
              if (!_expandedProjects.remove(cwd)) _expandedProjects.add(cwd);
            }),
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 4),
              child: Text(
                expanded ? l10n.showLess : l10n.showMoreCount(hidden),
                style: TextStyle(
                  fontSize: 11.5,
                  color: scheme.onSurfaceVariant,
                ),
              ),
            ),
          ),
        ),
      ),
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
    String? project,
  }) {
    final scheme = Theme.of(context).colorScheme;
    final fg = selected ? scheme.onPrimaryContainer : scheme.onSurface;
    final muted = selected
        ? scheme.onPrimaryContainer.withValues(alpha: 0.75)
        : scheme.onSurfaceVariant;
    final when = running
        ? l10n.running
        : _relativeTime(thread.updatedAt, now, l10n);
    // Server previews are the first user message verbatim, which may be wire
    // text (an attachment block) or codex's own machinery (a context
    // fragment). Both resolve through the cleaner; nothing left means there is
    // no title to show.
    final cleaned = previewWithoutFileRefs(
      thread.preview,
      l10n.fileOnlyMessage,
    ).trim();
    // A user-set name wins over the preview, matching the top bar.
    final title =
        thread.title ?? (cleaned.isEmpty ? l10n.untitledThread : cleaned);
    // Cross-project pane rows show "project · time" so the user always knows
    // where a conversation lives.
    final subtitle = [
      if (project != null && project.isNotEmpty) project,
      if (when.isNotEmpty) when,
    ].join(' · ');
    return Padding(
      // Rows under a project heading are indented, so the folder reads as
      // their parent rather than as a sibling label.
      padding: EdgeInsets.only(
        top: 1,
        bottom: 1,
        left: project == null && widget.home ? 14 : 0,
      ),
      child: Material(
        key: Key('conv-tile-${thread.id}'),
        color: selected ? scheme.primaryContainer : Colors.transparent,
        borderRadius: BorderRadius.circular(kControlRadius),
        child: InkWell(
          mouseCursor: clickable,
          borderRadius: BorderRadius.circular(kControlRadius),
          onTap: onTap,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 9),
            child: Row(
              children: [
                // No leading glyph: every row is a conversation, so an icon per
                // row was a column of identical noise. The project heading's
                // chevron is the only icon the tree needs.
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        title,
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

  /// One activity-view row: the conversation's name in bold over a one-line
  /// summary of where it got to, plus its project and time.
  ///
  /// The summary needs a `thread/read` per row, which is far too much to fetch
  /// for a whole sidebar up front — so it loads when the row is first built
  /// (i.e. when it scrolls into range) and Riverpod keeps it cached for the rest
  /// of the session. Until it arrives the row shows the first user message,
  /// which is already in hand: the layout never jumps, and a row is readable
  /// immediately rather than blank behind a spinner.
  Widget _activityTile({
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
    final cleaned = previewWithoutFileRefs(
      thread.preview,
      l10n.fileOnlyMessage,
    ).trim();
    final title =
        thread.title ?? (cleaned.isEmpty ? l10n.untitledThread : cleaned);
    final summary =
        ref
            .watch(
              threadSummaryProvider(
                threadSummaryKey(widget.serviceKey, thread.id),
              ),
            )
            .valueOrNull ??
        // Fall back to the preview — unless the title already IS the preview,
        // in which case repeating it twice tells the user nothing.
        (thread.title == null ? '' : cleaned);
    final when = running
        ? l10n.running
        : _relativeTime(thread.updatedAt, now, l10n);
    // The home pane spans projects, so its rows say where they live; a
    // project-scoped pane has only one, and repeating it per row is noise.
    final meta = [
      if (widget.home) _leafOf(thread.cwd),
      if (when.isNotEmpty) when,
    ].where((s) => s.isNotEmpty).join(' · ');
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 1),
      child: Material(
        key: Key('activity-tile-${thread.id}'),
        color: selected ? scheme.primaryContainer : Colors.transparent,
        borderRadius: BorderRadius.circular(kControlRadius),
        child: InkWell(
          mouseCursor: clickable,
          borderRadius: BorderRadius.circular(kControlRadius),
          onTap: onTap,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 9),
            child: Row(
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        title,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          fontSize: 13,
                          // Heavier than the project tree's rows: here the
                          // title has a summary under it to stand apart from.
                          fontWeight: FontWeight.w600,
                          color: fg,
                        ),
                      ),
                      if (summary.isNotEmpty) ...[
                        const SizedBox(height: 3),
                        Text(
                          summary,
                          key: Key('activity-summary-${thread.id}'),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(fontSize: 12, color: muted),
                        ),
                      ],
                      if (meta.isNotEmpty) ...[
                        const SizedBox(height: 3),
                        Text(
                          meta,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            fontSize: 11,
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

  /// The app-bar environment button + its popover: a floating summary card
  /// (changes, host, branch) and the changed-file list. Picking a file — or
  /// "review all" — opens the desktop review split.
  Widget _envButton(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    final d = _diff;
    final files = d?.files ?? const <DiffFile>[];
    Widget line(IconData icon, String label, {Widget? trailing}) => Padding(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 7),
      child: Row(
        children: [
          Icon(icon, size: 15, color: scheme.onSurfaceVariant),
          const SizedBox(width: 10),
          Expanded(child: Text(label, style: const TextStyle(fontSize: 13))),
          ?trailing,
        ],
      ),
    );
    return MenuAnchor(
      controller: _envMenu,
      alignmentOffset: const Offset(0, 6),
      style: const MenuStyle(alignment: Alignment.bottomRight),
      menuChildren: [
        SizedBox(
          width: 320,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(14, 10, 14, 4),
                child: Text(
                  l10n.envTitle,
                  style: TextStyle(
                    fontSize: 10.5,
                    fontWeight: FontWeight.w700,
                    letterSpacing: 0.7,
                    color: scheme.onSurfaceVariant,
                  ),
                ),
              ),
              line(
                Icons.difference_outlined,
                l10n.changesTitle,
                trailing: (d != null && !d.isEmpty)
                    ? Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Text(
                            '+${d.added}',
                            style: TextStyle(
                              fontSize: 12.5,
                              fontWeight: FontWeight.w600,
                              color: additionColor(scheme),
                            ),
                          ),
                          const SizedBox(width: 6),
                          Text(
                            '−${d.removed}',
                            style: TextStyle(
                              fontSize: 12.5,
                              fontWeight: FontWeight.w600,
                              color: scheme.error,
                            ),
                          ),
                        ],
                      )
                    : Text(
                        l10n.noChanges,
                        style: TextStyle(fontSize: 12, color: scheme.outline),
                      ),
              ),
              line(Icons.computer, _hostLabel(l10n)),
              if (_branch != null) line(Icons.account_tree_outlined, _branch!),
              if (files.isNotEmpty) ...[
                const Divider(height: 9),
                Padding(
                  padding: const EdgeInsets.fromLTRB(14, 4, 14, 4),
                  child: Row(
                    children: [
                      Expanded(
                        child: Text(
                          l10n.envSource,
                          style: TextStyle(
                            fontSize: 10.5,
                            fontWeight: FontWeight.w700,
                            letterSpacing: 0.7,
                            color: scheme.onSurfaceVariant,
                          ),
                        ),
                      ),
                      TextButton(
                        key: const Key('env-review-all'),
                        onPressed: () {
                          _envMenu.close();
                          _openReview();
                        },
                        style: TextButton.styleFrom(
                          padding: const EdgeInsets.symmetric(horizontal: 8),
                          minimumSize: Size.zero,
                          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                        ),
                        child: Text(l10n.reviewTitle),
                      ),
                    ],
                  ),
                ),
                ConstrainedBox(
                  constraints: const BoxConstraints(maxHeight: 260),
                  child: SingleChildScrollView(
                    primary: false,
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        for (final f in files)
                          InkWell(
                            mouseCursor: clickable,
                            key: Key('env-file-${f.path}'),
                            onTap: () {
                              _envMenu.close();
                              _openReview(path: f.path);
                            },
                            child: Padding(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 14,
                                vertical: 6,
                              ),
                              child: Row(
                                children: [
                                  Icon(
                                    Icons.description_outlined,
                                    size: 14,
                                    color: scheme.onSurfaceVariant,
                                  ),
                                  const SizedBox(width: 8),
                                  Expanded(
                                    child: Text(
                                      f.path,
                                      maxLines: 1,
                                      overflow: TextOverflow.ellipsis,
                                      textAlign: TextAlign.left,
                                      style: const TextStyle(
                                        fontFamily: monoFontFamily,
                                        fontFamilyFallback: monoCjkFallback,
                                        fontSize: 12,
                                      ),
                                    ),
                                  ),
                                  const SizedBox(width: 6),
                                  Text(
                                    '+${f.added}',
                                    style: TextStyle(
                                      fontSize: 10.5,
                                      color: additionColor(scheme),
                                    ),
                                  ),
                                  const SizedBox(width: 3),
                                  Text(
                                    '−${f.removed}',
                                    style: TextStyle(
                                      fontSize: 10.5,
                                      color: scheme.error,
                                    ),
                                  ),
                                ],
                              ),
                            ),
                          ),
                      ],
                    ),
                  ),
                ),
              ],
              const SizedBox(height: 6),
            ],
          ),
        ),
      ],
      builder: (ctx, ctrl, _) => IconButton(
        key: const Key('env-panel-btn'),
        tooltip: l10n.envTitle,
        icon: Icon(_reviewOpen ? Icons.difference : Icons.difference_outlined),
        onPressed: () {
          if (ctrl.isOpen) {
            ctrl.close();
          } else {
            _loadGit();
            ctrl.open();
          }
        },
      ),
    );
  }

  /// The desktop review split: the selected file's diff beside the changed-file
  /// tree, with a draggable divider between them and a close button.
  Widget _reviewSplit(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    final files = _diff?.files ?? const <DiffFile>[];
    final selected =
        files.where((f) => f.path == _reviewFile).firstOrNull ??
        files.firstOrNull;
    return Column(
      children: [
        // Review header: title + refresh + close.
        Container(
          decoration: BoxDecoration(
            border: Border(
              bottom: BorderSide(color: scheme.outlineVariant, width: 1),
            ),
          ),
          padding: const EdgeInsets.fromLTRB(14, 6, 6, 6),
          child: Row(
            children: [
              Icon(Icons.rate_review_outlined, size: 16, color: scheme.primary),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  l10n.reviewTitle,
                  style: Theme.of(context).textTheme.titleSmall,
                ),
              ),
              // Refreshing in place: the same cancellable fetch as the badge, so
              // a slow re-read spins here and can be called off, rather than
              // leaving the tree looking current while it isn't.
              if (_diffLoading)
                IconButton(
                  key: const Key('review-refresh-cancel'),
                  icon: SizedBox(
                    width: 15,
                    height: 15,
                    child: CircularProgressIndicator(
                      strokeWidth: 1.6,
                      color: scheme.primary,
                    ),
                  ),
                  tooltip: l10n.cancelDiffLoad,
                  visualDensity: VisualDensity.compact,
                  onPressed: _cancelDiff,
                )
              else
                IconButton(
                  icon: const Icon(Icons.refresh, size: 17),
                  tooltip: l10n.envRefresh,
                  visualDensity: VisualDensity.compact,
                  onPressed: _refreshDiff,
                ),
              IconButton(
                key: const Key('review-close'),
                icon: const Icon(Icons.close, size: 18),
                tooltip: l10n.cancel,
                visualDensity: VisualDensity.compact,
                onPressed: () => setState(() => _reviewOpen = false),
              ),
            ],
          ),
        ),
        Expanded(
          child: files.isEmpty
              ? Center(
                  child: Text(
                    l10n.reviewNoFiles,
                    style: TextStyle(color: scheme.outline),
                  ),
                )
              : Row(
                  children: [
                    Expanded(
                      child: selected == null
                          ? Center(
                              child: Text(
                                l10n.reviewPickFile,
                                style: TextStyle(color: scheme.outline),
                              ),
                            )
                          : DiffReviewView(
                              key: ValueKey(selected.path),
                              file: selected,
                              branch: _branch,
                              onLoadFile: _loadReviewFile,
                            ),
                    ),
                    _splitter(
                      key: const Key('review-tree-splitter'),
                      onDrag: (dx) => setState(
                        () => _treeWidth = (_treeWidth - dx).clamp(180, 420),
                      ),
                    ),
                    SizedBox(
                      width: _treeWidth.clamp(180, 420),
                      child: ChangedFileTree(
                        files: files,
                        selected: selected?.path,
                        onSelect: (p) => setState(() => _reviewFile = p),
                      ),
                    ),
                  ],
                ),
        ),
      ],
    );
  }

  /// The "plan ready — implement?" choice bar shown under a finished plan-mode
  /// turn. Keep planning (dismiss) or implement (start a normal turn).
  Widget _implementBar(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      decoration: BoxDecoration(
        color: scheme.primaryContainer,
        borderRadius: BorderRadius.circular(kPanelRadius),
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
        borderRadius: BorderRadius.circular(kPanelRadius),
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
  /// Pick images to attach (photo library on Android/iOS via the system
  /// picker — no permission prompt; a file dialog on desktop). Each pick is
  /// processed (EXIF-bake / downscale / JPEG re-encode) on a background
  /// isolate before it becomes sendable, showing a spinner chip meanwhile.
  Future<void> _pickImages() async {
    final l10n = AppLocalizations.of(context);
    final messenger = ScaffoldMessenger.of(context);
    // Only IMAGE chips consume image slots — _attachments also holds document
    // chips, which have their own kMaxFilesPerMessage budget.
    final remaining =
        kMaxImagesPerMessage - _attachments.where((a) => !a.isFile).length;
    if (remaining <= 0) {
      messenger.showSnackBar(
        SnackBar(content: Text(l10n.imageTooMany(kMaxImagesPerMessage))),
      );
      return;
    }
    List<XFile> picked;
    try {
      // pickMultiImage's `limit` must be ≥ 2; fall back to the single picker
      // when only one slot is left.
      if (remaining == 1) {
        final one = await ImagePicker().pickImage(source: ImageSource.gallery);
        picked = [?one];
      } else {
        picked = await ImagePicker().pickMultiImage(limit: remaining);
      }
    } catch (_) {
      if (mounted) {
        messenger.showSnackBar(SnackBar(content: Text(l10n.imagePickFailed)));
      }
      return;
    }
    if (picked.isEmpty || !mounted) return;
    if (picked.length > remaining) {
      // Some platforms ignore the picker's limit; enforce ours.
      picked = picked.sublist(0, remaining);
      messenger.showSnackBar(
        SnackBar(content: Text(l10n.imageTooMany(kMaxImagesPerMessage))),
      );
    }
    setState(() {
      for (final file in picked) {
        final att = _Attachment.image(id: _attachSeq++, name: file.name);
        _attachments.add(att);
        unawaited(_processAttachment(att, file));
      }
    });
  }

  Future<void> _processAttachment(_Attachment att, XFile file) async {
    try {
      final bytes = await file.readAsBytes();
      await _processImageBytes(att, bytes);
    } catch (_) {
      _failImageAttachment(att);
    }
  }

  /// Downscale/re-encode raw image bytes for [att] (shared by picked/dropped
  /// files and pasted clipboard image bytes, which have no readable path).
  Future<void> _processImageBytes(_Attachment att, Uint8List bytes) async {
    try {
      final processed = await processImage(bytes);
      if (!mounted || !_attachments.contains(att)) return; // removed via ×
      setState(() => att.processed = processed);
    } catch (_) {
      _failImageAttachment(att);
    }
  }

  void _failImageAttachment(_Attachment att) {
    if (!mounted || !_attachments.contains(att)) return;
    setState(() => _attachments.remove(att));
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(AppLocalizations.of(context).imagePickFailed)),
    );
  }

  /// Extensions the image pipeline can decode; a file picked with one of
  /// these routes to the image path instead (mirrors the codex TUI, whose
  /// `@file` mention attaches images and path-references everything else).
  static const _imageExtensions = {'png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'};

  static bool _looksLikeImage(String name) {
    final dot = name.lastIndexOf('.');
    if (dot < 0 || dot == name.length - 1) return false;
    return _imageExtensions.contains(name.substring(dot + 1).toLowerCase());
  }

  /// Pick document/file attachments (any type). Each is uploaded to the HOST
  /// right away (spinner chip while in flight) and later travels as a path
  /// reference in the turn text; image files route to the image pipeline.
  Future<void> _pickFiles() async {
    final l10n = AppLocalizations.of(context);
    final messenger = ScaffoldMessenger.of(context);
    final remaining =
        kMaxFilesPerMessage - _attachments.where((a) => a.isFile).length;
    if (remaining <= 0) {
      messenger.showSnackBar(
        SnackBar(content: Text(l10n.fileTooMany(kMaxFilesPerMessage))),
      );
      return;
    }
    List<XFile> picked;
    try {
      picked = await openFiles();
    } catch (_) {
      if (mounted) {
        messenger.showSnackBar(SnackBar(content: Text(l10n.filePickFailed)));
      }
      return;
    }
    _addFiles(picked);
  }

  /// Route a batch of files (picked, DRAGGED-and-dropped, or PASTED as paths)
  /// into attachments: images go through the local image pipeline, everything
  /// else uploads to the host as a path reference. Enforces the per-message
  /// image/file caps, surfacing a snackbar for anything dropped over-cap so a
  /// selection never silently vanishes. Shared by [_pickFiles], the drop
  /// target, and clipboard paste.
  void _addFiles(List<XFile> picked) {
    if (picked.isEmpty || !mounted) return;
    final l10n = AppLocalizations.of(context);
    final messenger = ScaffoldMessenger.of(context);
    final remaining =
        kMaxFilesPerMessage - _attachments.where((a) => a.isFile).length;
    var files = 0;
    var filesDropped = 0;
    var imagesDropped = 0;
    setState(() {
      for (final f in picked) {
        // XFile.name is the path basename on dart:io and can be empty for
        // synthetic files; the name reaches the host (and the chip label), so
        // never let it be blank.
        final name = f.name.isNotEmpty ? f.name : 'file';
        if (_looksLikeImage(name)) {
          if (_attachments.where((a) => !a.isFile).length <
              kMaxImagesPerMessage) {
            final att = _Attachment.image(id: _attachSeq++, name: name);
            _attachments.add(att);
            unawaited(_processAttachment(att, f));
          } else {
            imagesDropped++;
          }
        } else if (files < remaining) {
          files++;
          final att = _Attachment.file(id: _attachSeq++, name: name);
          _attachments.add(att);
          unawaited(_uploadAttachment(att, f));
        } else {
          filesDropped++;
        }
      }
    });
    if (filesDropped > 0) {
      messenger.showSnackBar(
        SnackBar(content: Text(l10n.fileTooMany(kMaxFilesPerMessage))),
      );
    }
    if (imagesDropped > 0) {
      messenger.showSnackBar(
        SnackBar(content: Text(l10n.imageTooMany(kMaxImagesPerMessage))),
      );
    }
  }

  /// Wrap the chat pane in a drag-and-drop target on desktop: a file dragged
  /// anywhere over the conversation attaches it (image or document), with a
  /// clear "drop to attach" overlay while hovering. A no-op on mobile/web.
  Widget _dropWrap(Widget child, AppLocalizations l10n) {
    if (!_isDesktop) return child;
    return DropTarget(
      onDragEntered: (_) => setState(() => _dragging = true),
      onDragExited: (_) => setState(() => _dragging = false),
      onDragDone: (detail) {
        setState(() => _dragging = false);
        _onDrop(detail);
      },
      child: Stack(
        children: [
          child,
          if (_dragging)
            Positioned.fill(child: IgnorePointer(child: _dropOverlay(l10n))),
        ],
      ),
    );
  }

  Widget _dropOverlay(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      color: scheme.primary.withValues(alpha: 0.07),
      alignment: Alignment.center,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 16),
        decoration: BoxDecoration(
          color: scheme.surfaceContainerHighest,
          borderRadius: BorderRadius.circular(kPanelRadius),
          border: Border.all(color: scheme.primary, width: 2),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.file_download_outlined, color: scheme.primary),
            const SizedBox(width: 10),
            Text(
              l10n.dropToAttach,
              style: TextStyle(
                color: scheme.primary,
                fontWeight: FontWeight.w600,
              ),
            ),
          ],
        ),
      ),
    );
  }

  /// True on a desktop platform (where drag-and-drop + clipboard paste of
  /// images/files make sense). `defaultTargetPlatform` (not `dart:io`) so the
  /// web build still compiles.
  bool get _isDesktop =>
      !kIsWeb &&
      (defaultTargetPlatform == TargetPlatform.windows ||
          defaultTargetPlatform == TargetPlatform.macOS ||
          defaultTargetPlatform == TargetPlatform.linux);

  /// Files dropped onto the chat → attach them exactly like a pick.
  void _onDrop(DropDoneDetails detail) {
    if (_sending) return;
    _addFiles(detail.files);
  }

  /// Ctrl/Cmd+V while the composer is focused: attach a clipboard IMAGE (raw
  /// bytes — no readable path, so processed directly) or clipboard FILES (by
  /// path). Runs alongside the text field's own text-paste (this never consumes
  /// the key event), so pasting text still works; only image/file clipboards
  /// add an attachment.
  Future<void> _onClipboardPaste() async {
    if (_sending || !mounted) return;
    try {
      final img = await Pasteboard.image;
      if (img != null && img.isNotEmpty) {
        if (!mounted) return;
        if (_attachments.where((a) => !a.isFile).length >=
            kMaxImagesPerMessage) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(
              content: Text(
                AppLocalizations.of(context).imageTooMany(kMaxImagesPerMessage),
              ),
            ),
          );
          return;
        }
        final att = _Attachment.image(
          id: _attachSeq++,
          name: 'pasted-image.png',
        );
        setState(() => _attachments.add(att));
        unawaited(_processImageBytes(att, img));
        return;
      }
      final files = await Pasteboard.files();
      if (files.isNotEmpty && mounted) {
        _addFiles([for (final p in files) XFile(p)]);
      }
    } catch (_) {
      // Clipboard read is best-effort; a text paste already happened natively.
    }
  }

  /// Global key hook (desktop), active only while the composer is focused:
  ///   • Ctrl/Cmd+V → also attach a clipboard image/file (returns false so the
  ///     text field still handles ordinary text paste).
  ///   • Esc        → the interrupt / undo / dequeue state machine (returns true
  ///     when it acts, consuming the key).
  /// Gating on composer focus keeps Esc from firing while a dialog/picker is
  /// open (those steal focus), so their own Esc-to-dismiss still works.
  bool _onHardwareKey(KeyEvent e) {
    if (e is! KeyDownEvent || !_inputFocus.hasFocus) return false;
    final key = e.logicalKey;
    if (key == LogicalKeyboardKey.keyV && _isCtrlOrCmdDown()) {
      unawaited(_onClipboardPaste());
      return false; // never consume — text paste must still fire
    }
    if (key == LogicalKeyboardKey.escape) {
      return _onEscape();
    }
    return false;
  }

  /// Whether a Ctrl (Win/Linux) or Cmd (macOS) modifier is currently held.
  bool _isCtrlOrCmdDown() {
    final pressed = HardwareKeyboard.instance.logicalKeysPressed;
    return pressed.contains(LogicalKeyboardKey.controlLeft) ||
        pressed.contains(LogicalKeyboardKey.controlRight) ||
        pressed.contains(LogicalKeyboardKey.metaLeft) ||
        pressed.contains(LogicalKeyboardKey.metaRight);
  }

  Future<void> _uploadAttachment(_Attachment att, XFile file) async {
    final l10n = AppLocalizations.of(context);
    final messenger = ScaffoldMessenger.of(context);
    void rejectTooLarge() {
      setState(() => _attachments.remove(att));
      messenger.showSnackBar(
        SnackBar(
          content: Text(l10n.fileTooLarge(kMaxFileBytes ~/ (1024 * 1024))),
        ),
      );
    }

    try {
      // Enforce the cap BEFORE buffering: readAsBytes on a multi-GB pick
      // would materialize the whole file (OOM-killing a phone) just to be
      // rejected.
      final size = await file.length();
      if (!mounted || !_attachments.contains(att)) return; // removed via ×
      if (size > kMaxFileBytes) {
        rejectTooLarge();
        return;
      }
      final bytes = await file.readAsBytes();
      if (!mounted || !_attachments.contains(att)) return;
      if (bytes.length > kMaxFileBytes) {
        // Belt-and-braces: length() can be stale/absent for synthetic files.
        rejectTooLarge();
        return;
      }
      final path = await ref
          .read(bridgeApiProvider)
          .metaUploadFile(widget.serviceKey, att.name, bytes);
      if (!mounted || !_attachments.contains(att)) return;
      setState(() => att.hostPath = path);
    } catch (e) {
      if (!mounted || !_attachments.contains(att)) return;
      setState(() => _attachments.remove(att));
      messenger.showSnackBar(
        SnackBar(
          content: Text('${l10n.fileUploadFailed}: ${friendlyError(e)}'),
        ),
      );
    }
  }

  /// Horizontal strip of pending attachments above the composer input: a
  /// thumbnail (or a spinner while processing) with a remove button each.
  /// The pending-queue strip above the composer: messages the user sent while a
  /// turn was running, each shown as a chip (with a ✕ to discard). A caption
  /// explains they'll send next and that Esc pulls the last one back.
  Widget _queuedStrip(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Icon(Icons.schedule, size: 13, color: scheme.onSurfaceVariant),
            const SizedBox(width: 4),
            Flexible(
              child: Text(
                '${l10n.queuedCount(_queue.length)} · ${l10n.queuedHint}',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.labelSmall?.copyWith(
                  color: scheme.onSurfaceVariant,
                ),
              ),
            ),
          ],
        ),
        const SizedBox(height: 6),
        Wrap(
          spacing: 6,
          runSpacing: 6,
          children: [
            for (final q in _queue)
              Container(
                key: Key('queued-${q.id}'),
                constraints: const BoxConstraints(maxWidth: 260),
                decoration: BoxDecoration(
                  color: scheme.surfaceContainerHighest,
                  borderRadius: BorderRadius.circular(kPanelRadius),
                  border: Border.all(color: scheme.outlineVariant),
                ),
                padding: const EdgeInsets.fromLTRB(10, 5, 4, 5),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Flexible(
                      // Tapping a chip pulls it back into the composer to edit —
                      // the recover path on mobile, where there's no Esc.
                      child: InkWell(
                        mouseCursor: clickable,
                        onTap: () => _restoreQueued(q),
                        child: Text(
                          q.preview,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: Theme.of(context).textTheme.bodySmall,
                        ),
                      ),
                    ),
                    const SizedBox(width: 2),
                    Tooltip(
                      message: l10n.removeQueued,
                      child: InkWell(
                        mouseCursor: clickable,
                        key: Key('queued-remove-${q.id}'),
                        onTap: () => _discardQueued(q.id),
                        borderRadius: BorderRadius.circular(12),
                        child: Padding(
                          padding: const EdgeInsets.all(2),
                          child: Icon(
                            Icons.close,
                            size: 15,
                            color: scheme.onSurfaceVariant,
                          ),
                        ),
                      ),
                    ),
                  ],
                ),
              ),
          ],
        ),
      ],
    );
  }

  Widget _attachmentStrip(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    final scale = MediaQuery.of(context).devicePixelRatio;
    // Every staged IMAGE, in strip order — the set a preview can page through,
    // so opening one and swiping reaches the others (a per-tile viewer would
    // strand each image alone). Files have no pixels, so they aren't in it.
    final staged = [
      for (final a in _attachments)
        if (!a.isFile)
          if (a.processed case final p?) p.bytes,
    ];
    return SizedBox(
      // The remove button overhangs the tile's top corner, so the strip is
      // taller than the tiles it holds.
      height: kAttachmentTileSide + 8,
      child: ListView.separated(
        scrollDirection: Axis.horizontal,
        itemCount: _attachments.length,
        separatorBuilder: (_, _) => const SizedBox(width: 8),
        itemBuilder: (context, i) {
          final att = _attachments[i];
          // Where this tile's image sits among the previewable ones. Counted
          // over the same filter as `staged` so a mixed strip (a file between
          // two images) still opens the picture the user actually clicked.
          final previewIndex = att.isFile || att.processed == null
              ? -1
              : _attachments
                    .take(i)
                    .where((a) => !a.isFile && a.processed != null)
                    .length;
          final Widget body;
          if (!att.ready) {
            body = const Center(
              child: SizedBox(
                width: 20,
                height: 20,
                child: CircularProgressIndicator(strokeWidth: 2),
              ),
            );
          } else if (att.isFile) {
            // Uploaded document: icon + filename tile.
            body = Tooltip(
              message: att.name,
              child: Padding(
                padding: const EdgeInsets.all(6),
                child: Column(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    Icon(
                      Icons.description_outlined,
                      size: 22,
                      color: scheme.onSurfaceVariant,
                    ),
                    const SizedBox(height: 4),
                    Text(
                      att.name,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      textAlign: TextAlign.center,
                      style: const TextStyle(fontSize: 9),
                    ),
                  ],
                ),
              ),
            );
          } else if (att.processed case final processed?) {
            body = Image.memory(
              processed.bytes,
              fit: BoxFit.cover,
              // 2× the box so a landscape image's SHORT edge reaches the
              // square cover box without upscaling.
              cacheWidth: (kAttachmentTileSide * scale * 2).round(),
              gaplessPlayback: true,
            );
          } else {
            // Unreachable: an image attachment is `ready` iff processed is
            // set — kept bang-free per repo style.
            body = const SizedBox.shrink();
          }
          return AttachmentTile(
            key: Key('attachment-${att.id}'),
            removeKey: Key('attachment-remove-${att.id}'),
            removeTooltip: att.isFile ? l10n.removeFile : l10n.removeImage,
            onRemove: () => setState(() => _attachments.remove(att)),
            // A staged image opens the same viewer a sent one does, so you can
            // check what you attached BEFORE sending it. A file has no pixels
            // to show, and an image still processing has none yet.
            onTap: previewIndex < 0
                ? null
                : () => ImageViewerPage.show(context, staged, previewIndex),
            tapTooltip: previewIndex < 0 ? null : l10n.previewImage,
            child: body,
          );
        },
      ),
    );
  }

  Widget _composer(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    // A 24 px pill is a phone control. On desktop the composer is a field in a
    // window, so it squares up and sits tighter against the transcript.
    final doc = MediaQuery.sizeOf(context).width >= _docLayoutWidth;
    return SafeArea(
      top: false,
      child: Padding(
        padding: doc
            ? const EdgeInsets.fromLTRB(16, 4, 16, 14)
            : const EdgeInsets.fromLTRB(12, 6, 12, 12),
        // The card IS the input, so all of it takes a text cursor and focuses
        // the field on click — the padding and the slack beside a short line
        // shouldn't behave like dead chrome. The buttons inside sit deeper in
        // the tree, so they keep their own click cursor and their own taps.
        child: MouseRegion(
          cursor: SystemMouseCursors.text,
          child: GestureDetector(
            behavior: HitTestBehavior.translucent,
            onTap: () => _inputFocus.requestFocus(),
            // The raised card: opaque so it lifts off the page, a hairline to
            // hold the edge, and the design's soft offsetless shadow instead of
            // a Material elevation.
            child: Container(
              decoration: BoxDecoration(
                color: scheme.surfaceBright,
                borderRadius: BorderRadius.circular(kComposerRadius),
                border: Border.all(color: scheme.outline),
                boxShadow: panelShadow(scheme),
              ),
              padding: const EdgeInsets.fromLTRB(14, 10, 10, 10),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  if (_queue.isNotEmpty) ...[
                    _queuedStrip(l10n),
                    const SizedBox(height: 8),
                  ],
                  if (_attachments.isNotEmpty) ...[
                    _attachmentStrip(l10n),
                    const SizedBox(height: 8),
                  ],
                  // Desktop: where this turn will land — project, host, branch —
                  // sits above the field the turn is typed into. A phone has no
                  // room for it and shows the same facts in the status bar.
                  if (doc) ...[
                    _composerContext(l10n),
                    const SizedBox(height: 8),
                  ],
                  TextField(
                    key: const Key('composer-input'),
                    controller: _input,
                    focusNode: _inputFocus,
                    minLines: 1,
                    maxLines: 6,
                    textInputAction: TextInputAction.send,
                    onSubmitted: (_) => _submit(),
                    style: Theme.of(context).textTheme.bodyLarge,
                    decoration: InputDecoration(
                      hintText: l10n.messageHint,
                      border: InputBorder.none,
                      isCollapsed: true,
                    ),
                  ),
                  const SizedBox(height: 8),
                  // One row at every width. Attachments collapse into a single `+`
                  // menu and the five wrapping config pills collapse into two
                  // chips, so a 360 px phone lays out exactly like the desktop —
                  // no wrapping, no expand/collapse mode to get stuck in.
                  Row(
                    children: [
                      _attachMenu(l10n),
                      const SizedBox(width: 2),
                      // Flexible, not a plain child: a non-flex child takes its
                      // intrinsic width whatever the row can afford, so a long
                      // localised permission label would overflow a narrow phone
                      // instead of ellipsizing.
                      Flexible(child: _permissionChip(l10n)),
                      const SizedBox(width: 6),
                      // Right-aligned next to send, and Flexible so a long model
                      // name ellipsizes instead of pushing the row into overflow.
                      Expanded(
                        child: Align(
                          alignment: Alignment.centerRight,
                          child: _modelChip(l10n),
                        ),
                      ),
                      const SizedBox(width: 6),
                      _sendButton(),
                    ],
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }

  /// The composer's context line: which project, which host, which branch this
  /// turn will run against. Read-only facts except the project, which is still
  /// changeable up until the thread exists.
  Widget _composerContext(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    final muted = TextStyle(fontSize: 11.5, color: scheme.onSurfaceVariant);
    Widget chip(
      IconData icon,
      String label, {
      VoidCallback? onTap,
      Key? key,
      String? tip,
      bool busy = false,
    }) {
      final row = Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (busy)
            SizedBox(
              width: 13,
              height: 13,
              child: CircularProgressIndicator(
                strokeWidth: 1.6,
                color: scheme.primary,
              ),
            )
          else
            Icon(icon, size: 13, color: scheme.onSurfaceVariant),
          const SizedBox(width: 5),
          // Flexible, not a bare ConstrainedBox: when the review split narrows
          // the chat, the label must be able to shrink below its natural width
          // and ellipsize rather than overflow the row.
          Flexible(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 200),
              child: Text(
                label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: muted,
              ),
            ),
          ),
        ],
      );
      if (onTap == null) {
        // Static chips still explain themselves on hover — a phone user reads
        // the same fact in the status bar, so this is the desktop affordance.
        return Tooltip(
          message: tip ?? label,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 3),
            child: row,
          ),
        );
      }
      return Tooltip(
        message: tip ?? label,
        child: InkWell(
          mouseCursor: clickable,
          key: key,
          onTap: onTap,
          borderRadius: BorderRadius.circular(6),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 3),
            child: row,
          ),
        ),
      );
    }

    // A conversation's working directory is fixed once the thread exists, so
    // the project is only switchable before the first turn — and only THEN is
    // it worth a chip here. Once the thread exists the name is pure repetition:
    // the sidebar already heads the conversation's project, and a label you
    // can't act on adds nothing above the field you're typing in.
    final project = _threadId == null
        ? ProjectMenu(
            projects: _knownProjects(),
            current: (_cwd?.trim().isEmpty ?? true) ? null : _cwd!.trim(),
            onPick: (p) => setState(() => _cwd = p),
            onBrowse: () async {
              final picked = await showFolderPicker(
                context,
                serviceKey: widget.serviceKey,
                initialPath: _cwd,
              );
              if (picked != null && mounted) setState(() => _cwd = picked);
            },
            onClear: () => setState(() => _cwd = null),
            builder: (ctx, ctrl) => chip(
              Icons.folder_outlined,
              _projectName(),
              key: const Key('composer-project-chip'),
              tip: l10n.switchProjectTip,
              onTap: () => ctrl.isOpen ? ctrl.close() : ctrl.open(),
            ),
          )
        : null;

    return Row(
      children: [
        if (project != null) ...[
          Flexible(child: project),
          const SizedBox(width: 10),
        ],
        chip(Icons.computer, _hostLabel(l10n)),
        if (_branch != null) ...[
          const SizedBox(width: 10),
          Flexible(
            child: chip(
              Icons.account_tree_outlined,
              _branch!,
              key: const Key('composer-branch-chip'),
              tip: _diffLoading ? l10n.cancelDiffLoad : l10n.viewDiff,
              busy: _diffLoading,
              // The branch is the entry point to what has changed on it.
              onTap: _showDiff,
            ),
          ),
        ],
      ],
    );
  }

  /// Where the turn runs: "Local" when this service is hosted by this machine,
  /// else the remote device's own label.
  String _hostLabel(AppLocalizations l10n) {
    final locals = ref.watch(localServeListProvider).valueOrNull;
    final isLocal = (locals ?? const <AppServeStatus>[]).any(
      (h) => h.appServiceKey == widget.serviceKey,
    );
    if (isLocal) return l10n.envLocal;
    return widget.services
            .where((s) => s.key == widget.serviceKey)
            .firstOrNull
            ?.device ??
        _serviceLabelFromKey(widget.serviceKey);
  }

  /// The `+` attachment menu. One button instead of three icons: on a phone the
  /// three used to crowd the row, and two of them are rare enough to live a tap
  /// away.
  Widget _attachMenu(AppLocalizations l10n) => PopupMenuButton<void>(
    key: const Key('attach-menu-btn'),
    tooltip: l10n.addAttachment,
    enabled: !_sending,
    icon: Icon(
      Icons.add,
      size: 22,
      color: Theme.of(context).colorScheme.onSurfaceVariant,
    ),
    // `_` on purpose: an item's `onTap` fires *after* `Navigator.pop`, so the
    // menu route's context is already on its way out. Everything below uses
    // the State's context, which outlives the menu.
    itemBuilder: (_) => [
      PopupMenuItem<void>(
        key: const Key('attach-btn'),
        onTap: _pickImages,
        child: _menuRow(Icons.add_photo_alternate_outlined, l10n.attachImage),
      ),
      PopupMenuItem<void>(
        key: const Key('attach-file-btn'),
        onTap: _pickFiles,
        child: _menuRow(Icons.attach_file, l10n.attachFile),
      ),
      // Browsing / transferring host files uses the desktop save/open dialogs,
      // so it is gated the same way saving is.
      if (canSaveImages)
        PopupMenuItem<void>(
          key: const Key('host-files-btn'),
          onTap: () => showFileBrowser(context, serviceKey: widget.serviceKey),
          child: _menuRow(Icons.folder_open_outlined, l10n.hostFiles),
        ),
    ],
  );

  Widget _menuRow(IconData icon, String label) => Row(
    children: [
      Icon(icon, size: 19, color: Theme.of(context).colorScheme.primary),
      const SizedBox(width: 12),
      Text(label),
    ],
  );

  /// Permission stays a chip of its own rather than moving into the model
  /// menu: "full access" (no sandbox, never ask) must be readable without
  /// opening anything, which is also why it carries the amber icon.
  Widget _permissionChip(AppLocalizations l10n) => _pill(
    pillKey: const Key('permission-chip'),
    icon: _modeIcon(),
    label: _mode.label(l10n),
    warn: _mode == PermissionMode.full,
    onTap: _pickMode,
  );

  /// What this turn will run with — model, and effort/plan when they differ
  /// from the default — plus the settings it fronts.
  ///
  /// Desktop opens them as an anchored popover with everything on one surface;
  /// a phone keeps the sheet flow, because a popover pinned to a chip that sits
  /// right above the on-screen keyboard has nowhere to go.
  Widget _modelChip(AppLocalizations l10n) {
    Widget pill(VoidCallback onTap) => _pill(
      pillKey: const Key('model-chip'),
      icon: Icons.auto_awesome,
      // With no explicit pick, show the model the server actually runs (the
      // thread default) rather than an opaque "default".
      label: [
        _model?.displayName ??
            _modelDisplayLabel(_runtime?.model) ??
            l10n.modelDefault,
        if (_effectiveEffort != null) _effectiveEffort!.label(l10n),
        if (_plan) l10n.planMode,
      ].join(' · '),
      // Plan is the one setting here that changes what a turn *does*, and a
      // long model name can ellipsize its label away — so it tints the chip.
      active: _plan,
      trailing: Icons.expand_more,
      onTap: onTap,
    );
    if (!_isDesktop) return pill(() => _showConfigSheet(l10n));
    return MenuAnchor(
      controller: _modelMenu,
      alignmentOffset: const Offset(0, 6),
      menuChildren: [_turnSettingsPanel(l10n)],
      builder: (ctx, ctrl, _) => pill(() {
        if (ctrl.isOpen) {
          ctrl.close();
          return;
        }
        // The list is cached after the first read, so this only round-trips
        // once; the panel renders with whatever it has and fills in.
        _ensureModels().then((_) {
          if (mounted) setState(() {});
        });
        ctrl.open();
      }),
    );
  }

  /// The desktop turn-settings popover: model, reasoning effort, plan mode —
  /// all visible at once rather than three levels down a sheet.
  Widget _turnSettingsPanel(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    final selectedId = _model?.id ?? _runtime?.model ?? _sentModel;
    // Always show the model the thread actually runs — even one the host's list
    // doesn't advertise (e.g. a newer `gpt-5.6-*` the server picked). Without
    // this it wasn't selectable and looked absent though it was the live model.
    final models = <ModelInfo>[..._models];
    final activeId = _activeModelStatus()?.id ?? selectedId;
    if (activeId != null && !models.any((m) => m.id == activeId)) {
      models.insert(
        0,
        ModelInfo(
          id: activeId,
          displayName: _modelDisplayLabel(activeId) ?? activeId,
          description: '',
        ),
      );
    }
    return SizedBox(
      width: 320,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _menuHeading(l10n.model),
          if (models.isEmpty)
            Padding(
              padding: const EdgeInsets.fromLTRB(14, 2, 14, 8),
              child: Text(
                l10n.modelDefault,
                style: TextStyle(fontSize: 12.5, color: scheme.outline),
              ),
            )
          else
            // Not a lazy ListView: a menu panel measures its intrinsic width,
            // which a shrink-wrapped viewport refuses to report.
            ConstrainedBox(
              constraints: const BoxConstraints(maxHeight: 190),
              // `primary: false`: the menu panel already owns the primary
              // scroll controller, and a second attach breaks its scrollbar.
              child: SingleChildScrollView(
                primary: false,
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    for (final m in models)
                      InkWell(
                        mouseCursor: clickable,
                        key: Key('model-menu-item-${m.id}'),
                        onTap: () => _applyModel(m),
                        child: Padding(
                          padding: const EdgeInsets.symmetric(
                            horizontal: 14,
                            vertical: 8,
                          ),
                          child: Row(
                            children: [
                              Expanded(
                                child: Text(
                                  m.displayName,
                                  maxLines: 1,
                                  overflow: TextOverflow.ellipsis,
                                  style: const TextStyle(fontSize: 13),
                                ),
                              ),
                              if (m.id == selectedId)
                                Icon(
                                  Icons.check,
                                  size: 16,
                                  color: scheme.primary,
                                ),
                            ],
                          ),
                        ),
                      ),
                  ],
                ),
              ),
            ),
          const Divider(height: 9),
          _effortSlider(l10n),
          const Divider(height: 9),
          // Plan mode changes what a turn DOES, so it is a switch on the face
          // of the panel rather than another row to drill into.
          InkWell(
            mouseCursor: clickable,
            key: const Key('plan-toggle-row'),
            onTap: _togglePlan,
            child: Padding(
              padding: const EdgeInsets.fromLTRB(14, 2, 8, 2),
              child: Row(
                children: [
                  Icon(Icons.checklist_rtl, size: 16, color: scheme.primary),
                  const SizedBox(width: 10),
                  Expanded(
                    child: Text(
                      l10n.planMode,
                      style: const TextStyle(fontSize: 13),
                    ),
                  ),
                  Switch(value: _plan, onChanged: (_) => _togglePlan()),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }

  /// Reasoning effort as a stepped selector over the levels the active model
  /// advertises (ordered least→most thinking). Falls back to a static label for
  /// a model that offers exactly one level.
  Widget _effortSlider(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    final model = _model ?? (_models.isNotEmpty ? _models.first : null);
    final supported = model?.supportedReasoningEfforts ?? const <String>[];
    final efforts = supported.isEmpty
        ? ReasoningEffort.known
        : [for (final w in supported) ReasoningEffort(w)];
    final current = _effectiveEffort;
    return Padding(
      padding: const EdgeInsets.fromLTRB(14, 2, 14, 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(
                Icons.psychology_outlined,
                size: 16,
                color: scheme.onSurfaceVariant,
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  l10n.effort,
                  style: TextStyle(
                    fontSize: 11,
                    fontWeight: FontWeight.w600,
                    letterSpacing: 0.5,
                    color: scheme.onSurfaceVariant,
                  ),
                ),
              ),
              // Fixed-width so the longest label (极高) can't reflow — let alone
              // widen — the panel as the level changes.
              SizedBox(
                width: 52,
                child: Text(
                  current?.label(l10n) ?? l10n.runtimeEffortModelDefault,
                  textAlign: TextAlign.right,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 12.5,
                    fontWeight: FontWeight.w600,
                    color: scheme.primary,
                  ),
                ),
              ),
            ],
          ),
          if (efforts.length > 1)
            Padding(
              padding: const EdgeInsets.only(top: 10),
              child: _EffortSteps(
                key: const Key('effort-steps'),
                levels: efforts,
                current: current,
                onChanged: _applyEffort,
              ),
            ),
        ],
      ),
    );
  }

  /// Small all-caps heading inside a popover panel.
  Widget _menuHeading(String text) => Padding(
    padding: const EdgeInsets.fromLTRB(14, 8, 14, 6),
    child: Text(
      text.toUpperCase(),
      style: TextStyle(
        fontSize: 10.5,
        fontWeight: FontWeight.w700,
        letterSpacing: 0.7,
        color: Theme.of(context).colorScheme.onSurfaceVariant,
      ),
    ),
  );

  /// Select [m] for the next turn, mirroring [_pickModel]'s bookkeeping: hold
  /// the pick against server syncs until it's sent, and drop an effort the new
  /// model doesn't support so we never ship a level it would reject.
  void _applyModel(ModelInfo m) {
    setState(() {
      _model = m;
      _modelPickPending = true;
      final eff = _effectiveEffort;
      if (eff != null && !m.supportedReasoningEfforts.contains(eff.wire)) {
        _effort = ReasoningEffort.fromWire(m.defaultReasoningEffort);
        _effortActive = null;
      }
    });
    _rememberDefaults();
    _persistThreadConfig();
  }

  void _applyEffort(ReasoningEffort e) {
    if (e == _effectiveEffort) return;
    setState(() => _effort = e);
    _rememberDefaults();
    _persistThreadConfig();
  }

  void _togglePlan() {
    setState(() {
      _plan = !_plan;
      _planToggledByUser = true;
    });
    _rememberDefaults();
    _persistThreadConfig();
  }

  /// The settings the model chip fronts: model, effort, plan, project. A sheet
  /// rather than a popover because every picker it opens is already a sheet,
  /// and a popover anchored to a chip near the keyboard is awkward on a phone.
  Future<void> _showConfigSheet(AppLocalizations l10n) async {
    final model =
        _model?.displayName ??
        _modelDisplayLabel(_runtime?.model) ??
        l10n.modelDefault;
    final choice = await _optionSheet<String>(
      title: l10n.turnSettings,
      isSelected: (v) => v == 'plan' && _plan,
      options: [
        _PickerOption(
          value: 'model',
          icon: Icons.auto_awesome,
          label: l10n.model,
          description: model,
        ),
        _PickerOption(
          value: 'effort',
          icon: Icons.psychology_outlined,
          label: l10n.effort,
          description: _effectiveEffort?.label(l10n),
        ),
        _PickerOption(
          value: 'plan',
          icon: Icons.checklist_rtl,
          label: l10n.planMode,
        ),
        // The working directory is fixed once the thread exists, so offer it
        // only before the first turn.
        if (_threadId == null)
          _PickerOption(
            value: 'project',
            icon: Icons.folder_outlined,
            label: l10n.projectsSection,
            description: _projectName(),
          ),
      ],
    );
    if (!mounted) return;
    switch (choice) {
      case 'model':
        await _pickModel();
      case 'effort':
        await _pickEffort();
      case 'project':
        await _pickProject();
      case 'plan':
        _togglePlan();
    }
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
        // Sendable with text and/or attachments — but never while an
        // attachment is still processing/uploading (the message would ship
        // without it).
        final processing = _attachments.any((a) => !a.ready);
        final canSend =
            !_sending &&
            !processing &&
            (value.text.trim().isNotEmpty || _attachments.isNotEmpty);
        return IconButton.filled(
          key: const Key('send-btn'),
          onPressed: canSend ? () => _submit() : null,
          icon: const Icon(Icons.arrow_upward, size: 20),
        );
      },
    );
  }

  /// A compact, low-chrome setting pill (permission / model). [active]
  /// highlights a toggled-on pill; [warn] flags a risky setting (no-sandbox
  /// "full access"); [trailing] adds a chevron for a pill that opens a menu.
  ///
  /// On touch platforms the pill is grown to a 44 px tap target — the desktop
  /// height reads as a hairline chip under a thumb.
  Widget _pill({
    required IconData icon,
    required String label,
    required VoidCallback? onTap,
    Key? pillKey,
    bool active = false,
    bool warn = false,
    IconData? trailing,
  }) {
    final scheme = Theme.of(context).colorScheme;
    final enabled = onTap != null;
    final fg = active
        ? scheme.onPrimaryContainer
        : enabled
        ? scheme.onSurfaceVariant
        : scheme.onSurfaceVariant.withValues(alpha: 0.5);
    final touch = !isDesktop;
    // These sit inside the composer card, so the raised card is the ground the
    // resting wash composites against.
    return Material(
      color: active
          ? scheme.primaryContainer
          : Color.alphaBlend(scheme.surfaceContainer, scheme.surfaceBright),
      borderRadius: BorderRadius.circular(kControlRadius),
      child: InkWell(
        mouseCursor: clickable,
        key: pillKey,
        borderRadius: BorderRadius.circular(kControlRadius),
        onTap: onTap,
        child: ConstrainedBox(
          constraints: BoxConstraints(minHeight: touch ? 44 : 0),
          child: Padding(
            padding: EdgeInsets.symmetric(
              horizontal: touch ? 13 : 11,
              vertical: 6,
            ),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(icon, size: 15, color: warn ? cautionColor(scheme) : fg),
                const SizedBox(width: 5),
                Flexible(
                  child: Text(
                    label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    softWrap: false,
                    style: TextStyle(fontSize: 12.5, color: fg),
                  ),
                ),
                if (trailing != null) ...[
                  const SizedBox(width: 2),
                  Icon(trailing, size: 16, color: fg),
                ],
              ],
            ),
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
    return showAdaptivePanel<T>(
      context: context,
      // The body ends in a ListView of options; it scrolls itself.
      scrollable: false,
      // It also draws its own grab bar, which is its top edge.
      insetTop: false,
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
          mouseCursor: clickable,
          // Stable handle for tests: the label is localised and, for the turn
          // settings, repeated by the chip that opened the sheet. Values that
          // are not scalars fall back to the label — a model row's DTO has no
          // `toString`, so keying on it would give every model the same key.
          key: ValueKey(
            'opt-${switch (o.value) {
              final String s => s,
              final Enum e => e.name,
              _ => o.label,
            }}',
          ),
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
        // Hold this pick against server-confirmed syncs until it's sent.
        _modelPickPending = true;
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

  /// The full runtime configuration, opened from the status-bar model chip: a
  /// read-only sheet of what this thread's turns actually run with — model,
  /// reasoning effort, permissions, plan mode — with a provenance line saying
  /// whether the server confirmed it (thread/settings/updated), it came from
  /// the session's start/resume snapshot, or the server never reported and
  /// the values shown are what this app sends.
  void _showRuntimeSheet() {
    final l10n = AppLocalizations.of(context);
    final rt = _runtime;
    final active = _activeModelStatus();
    // Effort mirrors _activeModelStatus's precedence: confirmed server value
    // (null = the model's default), else the app's effective pick, else the
    // snapshot value.
    final String effortText;
    if (rt != null && rt.confirmedByUpdate) {
      effortText = rt.reasoningEffort == null
          ? l10n.runtimeEffortModelDefault
          : ReasoningEffort(rt.reasoningEffort!).label(l10n);
    } else {
      final eff = _effectiveEffort?.wire ?? rt?.reasoningEffort;
      effortText = eff == null
          ? l10n.runtimeEffortModelDefault
          : ReasoningEffort(eff).label(l10n);
    }
    // Permissions: the server-reported approval+sandbox pair (as a preset
    // label when it maps onto one), else the mode the app sends every turn.
    final String permText;
    final approval = rt?.approvalPolicy;
    final sandbox = rt?.sandboxMode;
    if (approval != null || sandbox != null) {
      final preset = PermissionMode.values
          .where((m) => m.approval == approval && m.sandbox == sandbox)
          .firstOrNull;
      permText =
          preset?.label(l10n) ?? '${approval ?? '—'} · ${sandbox ?? '—'}';
    } else {
      permText = _mode.label(l10n);
    }
    final collab = rt?.collaborationMode;
    final planText = (collab ?? (_planActive ? 'plan' : 'default')) == 'plan'
        ? l10n.statePlanMode
        : l10n.runtimeCollabDefault;
    final String provenance;
    if (rt == null) {
      provenance = l10n.runtimeUnavailable;
    } else {
      final at = _runtimeAt;
      final time = at == null ? '' : _fmtClock(at);
      provenance = rt.confirmedByUpdate
          ? l10n.runtimeConfirmedAt(time)
          : l10n.runtimeFromSnapshot(time);
    }
    final modelLabel = active == null ? '—' : _modelDisplayLabel(active.id)!;
    final modelSub = <String>[
      if (active != null && modelLabel != active.id) active.id,
      if (rt?.modelProvider != null) rt!.modelProvider!,
    ].join(' · ');
    showAdaptivePanel<void>(
      context: context,
      builder: (c) {
        final scheme = Theme.of(c).colorScheme;
        Widget row(IconData icon, String label, String value, {String? sub}) =>
            ListTile(
              dense: true,
              leading: Icon(icon, size: 20, color: scheme.primary),
              title: Text(label, style: const TextStyle(fontSize: 12)),
              subtitle: Text(
                sub == null || sub.isEmpty ? value : '$value\n$sub',
                style: TextStyle(fontSize: 14, color: scheme.onSurface),
              ),
            );
        return SafeArea(
          // Scrollable so the rows never overflow a short sheet (small phones,
          // landscape). Selectable so the model id / provider can be copied.
          child: SelectionArea(
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Padding(
                    padding: const EdgeInsets.fromLTRB(20, 0, 20, 4),
                    child: Text(
                      l10n.runtimeSheetTitle,
                      style: Theme.of(c).textTheme.titleMedium,
                    ),
                  ),
                  row(
                    Icons.auto_awesome,
                    l10n.modelLabel,
                    modelLabel,
                    sub: modelSub,
                  ),
                  row(Icons.psychology_outlined, l10n.effort, effortText),
                  row(_modeIcon(), l10n.permissionLabel, permText),
                  row(Icons.checklist_rtl, l10n.planMode, planText),
                  Padding(
                    padding: const EdgeInsets.fromLTRB(20, 8, 20, 16),
                    child: Row(
                      children: [
                        Icon(
                          rt != null && rt.confirmedByUpdate
                              ? Icons.verified_outlined
                              : Icons.info_outline,
                          size: 14,
                          color: rt != null && rt.confirmedByUpdate
                              ? additionColor(scheme)
                              : scheme.onSurfaceVariant,
                        ),
                        const SizedBox(width: 6),
                        Expanded(
                          child: Text(
                            provenance,
                            style: TextStyle(
                              fontSize: 12,
                              color: scheme.onSurfaceVariant,
                            ),
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ),
        );
      },
    );
  }

  /// Seed a brand-new conversation's working folder from the host's configured
  /// default project. Best-effort + guarded: only sets `_cwd` while it is still
  /// empty and the thread is still new, so it never overrides a folder the user
  /// picked or a resumed thread's cwd.
  Future<void> _seedDefaultCwd() async {
    try {
      final cfg = await ref
          .read(bridgeApiProvider)
          .metaProjectConfig(widget.serviceKey);
      // Answered, so there is nothing left to retry — whether or not a default
      // is configured. Settling here (not only on the happy path) stops a host
      // that legitimately has no default from being asked five times.
      _openLoadDone(_kCwdSeed);
      final def = cfg.defaultProject?.trim();
      if (!mounted || def == null || def.isEmpty) return;
      if (_threadId == null && (_cwd == null || _cwd!.trim().isEmpty)) {
        setState(() => _cwd = def);
      }
    } catch (_) {
      // No reachable meta → keep the codex default for now, but retry: this
      // runs at mount, so on a cold open it can lose the race against the
      // connection. Failing silently and permanently would leave a NEW
      // conversation rooted in the wrong folder — the agent would then read and
      // edit files somewhere the user never chose, which is worse than a slow
      // seed. The guard above keeps a folder the user picked meanwhile.
      if (mounted) _retryOpenLoad(_kCwdSeed);
    }
  }

  Future<void> _pickProject() async {
    final l10n = AppLocalizations.of(context);
    // Does the host offer a project-folder tree to browse? Best-effort — a
    // failure just means the manual path field (the fallback that always works,
    // e.g. self-host with no roots configured).
    var hasRoots = false;
    try {
      final cfg = await ref
          .read(bridgeApiProvider)
          .metaProjectConfig(widget.serviceKey);
      hasRoots = cfg.hasRoots;
    } catch (_) {
      // No reachable meta / no config → manual entry only.
    }
    if (!mounted) return;
    final ctrl = TextEditingController(text: _cwd ?? '');
    final ok = await showDialog<bool>(
      context: context,
      builder: (c) => AlertDialog(
        title: Text(l10n.newProject),
        content: StatefulBuilder(
          builder: (c, setInner) => Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              // Visual tree browse — the primary way on a phone. Confined to
              // the host's configured project roots.
              if (hasRoots) ...[
                FilledButton.tonalIcon(
                  key: const Key('browse-project-btn'),
                  onPressed: () async {
                    final picked = await showFolderPicker(
                      c,
                      serviceKey: widget.serviceKey,
                      initialPath: ctrl.text.trim().isEmpty
                          ? null
                          : ctrl.text.trim(),
                    );
                    if (picked != null) setInner(() => ctrl.text = picked);
                  },
                  icon: const Icon(Icons.folder_open, size: 18),
                  label: Text(l10n.browseProjectFolder),
                ),
                const SizedBox(height: 12),
                Text(
                  l10n.orEnterPathManually,
                  style: Theme.of(c).textTheme.bodySmall,
                ),
                const SizedBox(height: 6),
              ],
              TextField(
                controller: ctrl,
                autofocus: !hasRoots,
                decoration: InputDecoration(
                  labelText: l10n.remotePathLabel,
                  hintText: l10n.remotePathHint,
                  border: const OutlineInputBorder(),
                ),
              ),
            ],
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

/// Esc while renaming: leave the title edit without committing.
class _CancelTitleEditIntent extends Intent {
  const _CancelTitleEditIntent();
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
  const _UserInputCard({
    super.key,
    required this.prompt,
    required this.onAnswer,
  });
  final AppEvent prompt;
  final Future<void> Function(AppEvent, Map<String, List<String>>) onAnswer;

  @override
  State<_UserInputCard> createState() => _UserInputCardState();
}

class _UserInputCardState extends State<_UserInputCard> {
  // Sentinel "choice" meaning the free-text 其他 field for a question.
  static const _other = '\u0000other';
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
    // No options → pure free-text; an explicit "其他" pick → free-text too.
    if (q.options.isEmpty) {
      final t = _ctrl(q.id).text.trim();
      return t.isEmpty ? null : t;
    }
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
        borderRadius: BorderRadius.circular(kPanelRadius),
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

  Widget _questionBlock(
    BuildContext context,
    _UiQuestion q,
    ColorScheme scheme,
  ) {
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
          // A question with no options is a pure free-text prompt; otherwise show
          // the option chips (+ an "其他" chip when free text is also allowed).
          if (q.options.isNotEmpty)
            Wrap(
              spacing: 8,
              runSpacing: 6,
              children: [
                for (final o in q.options)
                  ChoiceChip(
                    label: Text(o.label),
                    tooltip:
                        (o.description != null && o.description!.isNotEmpty)
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
          if (q.options.isEmpty || (q.isOther && _choice[q.id] == _other))
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
  const _ApprovalCard({
    super.key,
    required this.prompt,
    required this.onDecide,
  });
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
        borderRadius: BorderRadius.circular(kPanelRadius),
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
                    fontFamily: monoFontFamily,
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

/// A stepped effort selector: a track with a stop per level and a thumb that
/// GLIDES between them when the level changes (tap or drag), rather than a
/// Material slider that snaps. Fixed layout — nothing here reflows the panel as
/// the level or its label changes.
///
/// Deliberately no `LayoutBuilder`: this lives inside a `MenuAnchor` panel,
/// which measures intrinsic width, and `LayoutBuilder` can't answer that. All
/// geometry is fractional (`Align` / `FractionallySizedBox`), which is
/// intrinsic-safe; a `GlobalKey` reads the track width to map a tap to a level.
class _EffortSteps extends StatefulWidget {
  const _EffortSteps({
    super.key,
    required this.levels,
    required this.current,
    required this.onChanged,
  });

  final List<ReasoningEffort> levels;
  final ReasoningEffort? current;
  final ValueChanged<ReasoningEffort> onChanged;

  @override
  State<_EffortSteps> createState() => _EffortStepsState();
}

class _EffortStepsState extends State<_EffortSteps> {
  static const double _thumbW = 14;
  static const double _height = 24;
  final GlobalKey _trackKey = GlobalKey();

  void _selectAt(double localDx) {
    final n = widget.levels.length;
    if (n <= 1) return;
    final box = _trackKey.currentContext?.findRenderObject() as RenderBox?;
    final w = box?.size.width ?? 0;
    final usable = w - _thumbW;
    if (usable <= 0) return;
    // The stops sit `_thumbW/2` in from each edge (so the thumb never
    // overflows); map the tap into that usable span.
    final f = ((localDx - _thumbW / 2) / usable).clamp(0.0, 1.0);
    final ni = (f * (n - 1)).round();
    if (widget.levels[ni] != widget.current) {
      widget.onChanged(widget.levels[ni]);
    }
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final l10n = AppLocalizations.of(context);
    final n = widget.levels.length;
    // An effort the model doesn't list (an unknown server default) parks the
    // thumb mid-scale rather than lying about the level.
    final found = widget.levels.indexWhere((e) => e == widget.current);
    final idx = found < 0 ? (n - 1) ~/ 2 : found;
    final target = n <= 1 ? 0.0 : idx / (n - 1);
    final atMax = idx == n - 1;

    // A bare GestureDetector reports no cursor, so this track would hover as
    // plain content despite being tappable AND draggable.
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTapDown: (d) => _selectAt(d.localPosition.dx),
        onHorizontalDragUpdate: (d) => _selectAt(d.localPosition.dx),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            SizedBox(
              key: _trackKey,
              height: _height,
              // One painter, exact geometry: the thumb sits ON each stop at every
              // level (Align mapped different-sized children to the bounds, which
              // drifted the thumb off the dots — worst at 极高). Animate `frac`.
              child: TweenAnimationBuilder<double>(
                tween: Tween<double>(end: target),
                duration: const Duration(milliseconds: 240),
                curve: Curves.easeOutCubic,
                builder: (context, frac, _) => CustomPaint(
                  size: const Size(double.infinity, _height),
                  painter: _EffortPainter(
                    frac: frac,
                    levels: n,
                    activeIdx: idx,
                    thumbW: _thumbW,
                    atMax: atMax,
                    track: scheme.surfaceContainerHighest,
                    thumbFill: scheme.surface,
                    thumbBorder: scheme.outlineVariant,
                    shadow: scheme.shadow,
                    dotOff: scheme.outline,
                    brightness: Theme.of(context).brightness,
                    fill: scheme.primary,
                    onFill: scheme.onPrimary,
                  ),
                ),
              ),
            ),
            const SizedBox(height: 4),
            // End labels — the reference's Faster ↔ Smarter poles.
            Row(
              children: [
                Text(
                  l10n.effortFaster,
                  style: TextStyle(
                    fontSize: 10.5,
                    color: scheme.onSurfaceVariant,
                  ),
                ),
                const Spacer(),
                Text(
                  l10n.effortSmarter,
                  style: TextStyle(
                    fontSize: 10.5,
                    color: scheme.onSurfaceVariant,
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

/// Paints the effort track: a rounded track, a flat violet fill up to the
/// thumb, a stop per level, and a rounded-square thumb centred exactly on
/// its stop. At the top level the fill gets a scattered "dither" shimmer —
/// the reference's Ultracode flourish.
class _EffortPainter extends CustomPainter {
  _EffortPainter({
    required this.frac,
    required this.levels,
    required this.activeIdx,
    required this.thumbW,
    required this.atMax,
    required this.track,
    required this.thumbFill,
    required this.thumbBorder,
    required this.shadow,
    required this.dotOff,
    required this.brightness,
    required this.fill,
    required this.onFill,
  });

  final double frac;
  final int levels;
  final int activeIdx;
  final double thumbW;
  final bool atMax;
  final Color track;
  final Color thumbFill;
  final Color thumbBorder;
  final Color shadow;
  final Color dotOff;
  final Brightness brightness;

  /// The flat fill up to the thumb — the theme's accent, so the slider belongs
  /// to the palette instead of being the one violet thing on screen. Flat, not
  /// a gradient, per the design's rules.
  final Color fill;

  /// Ink drawn ON the fill (the passed stops and the max-level shimmer). Taken
  /// from the theme rather than assumed white: the accent is light enough that
  /// white-on-accent barely shows.
  final Color onFill;

  // A fixed scatter of unit-square offsets for the max-level shimmer, generated
  // once so it doesn't flicker as `frac` animates.
  static final List<Offset> _dither = () {
    final r = math.Random(7);
    return [
      for (var i = 0; i < 90; i++) Offset(r.nextDouble(), r.nextDouble()),
    ];
  }();

  @override
  void paint(Canvas canvas, Size size) {
    final h = size.height;
    final cy = h / 2;
    const trackH = 6.0;
    const thumbH = 20.0;
    final usable = size.width - thumbW;
    double xAt(double f) => thumbW / 2 + usable * f;
    final cx = xAt(frac);

    // Track.
    final trackRect = RRect.fromLTRBR(
      0,
      cy - trackH / 2,
      size.width,
      cy + trackH / 2,
      const Radius.circular(trackH / 2),
    );
    canvas.drawRRect(trackRect, Paint()..color = track);

    // Flat fill up to the thumb.
    final fillRight = cx.clamp(trackH, size.width);
    final fillRect = RRect.fromLTRBR(
      0,
      cy - trackH / 2,
      fillRight,
      cy + trackH / 2,
      const Radius.circular(trackH / 2),
    );
    canvas.drawRRect(fillRect, Paint()..color = fill);

    // Max-level shimmer: scattered translucent squares over the fill.
    if (atMax) {
      canvas.save();
      canvas.clipRRect(fillRect);
      final dot = Paint()..color = onFill.withValues(alpha: 0.28);
      for (final o in _dither) {
        final x = o.dx * fillRight;
        final y = cy - trackH / 2 + o.dy * trackH;
        canvas.drawRect(Rect.fromLTWH(x, y, 1.4, 1.4), dot);
      }
      canvas.restore();
    }

    // A stop per level, on the track centre-line.
    for (var i = 0; i < levels; i++) {
      final x = xAt(levels <= 1 ? 0 : i / (levels - 1));
      final on = i <= activeIdx;
      canvas.drawCircle(
        Offset(x, cy),
        2,
        Paint()..color = on ? onFill.withValues(alpha: 0.9) : dotOff,
      );
    }

    // The thumb — a rounded square, centred exactly on the current stop.
    final thumbRect = RRect.fromRectAndRadius(
      Rect.fromCenter(center: Offset(cx, cy), width: thumbW, height: thumbH),
      const Radius.circular(5),
    );
    canvas.drawRRect(
      thumbRect.shift(const Offset(0, 1)),
      Paint()
        ..color = shadow.withValues(alpha: 0.22)
        ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 2),
    );
    canvas.drawRRect(thumbRect, Paint()..color = thumbFill);
    canvas.drawRRect(
      thumbRect,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1
        ..color = thumbBorder,
    );
  }

  @override
  bool shouldRepaint(_EffortPainter old) =>
      old.frac != frac ||
      old.activeIdx != activeIdx ||
      old.levels != levels ||
      old.atMax != atMax ||
      old.brightness != brightness ||
      // Follows the theme, so it changes on a light/dark switch.
      old.fill != fill ||
      old.onFill != onFill;
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
        ? cautionColor(scheme)
        : scheme.primary;
    return Tooltip(
      message: tooltip,
      child: InkResponse(
        mouseCursor: clickable,
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

/// The environment side panel: where this conversation is actually running —
/// the checkout it acts on, the branch, and everything it has changed there,
/// with each changed file expandable to its colour-coded hunks.
///
/// Every row is state the app genuinely knows; deliberately no commit/push or
/// pull-request controls, because nothing on the host side implements them and
/// a button that does nothing is worse than no button.
class _EnvPanel extends StatelessWidget {
  const _EnvPanel({required this.diff, this.branch, this.cwd});
  final DiffModel? diff;
  final String? branch;

  /// Project root this conversation runs in (the checkout being changed).
  final String? cwd;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final d = diff;
    final hasChanges = d != null && !d.isEmpty;
    // The whole panel is content to read and copy — branch, path, and above
    // all the diff. Wrapping it in a SelectionArea makes every bit of that
    // drag-selectable (desktop) / long-press-selectable (touch), the same as
    // the transcript.
    return SelectionArea(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 8, 8, 8),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    l10n.envTitle,
                    style: Theme.of(context).textTheme.titleSmall,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
              ],
            ),
          ),
          const Divider(height: 1),
          Expanded(child: _body(context, scheme, l10n, hasChanges, d)),
        ],
      ),
    );
  }

  /// The scrolling body. ONE scrollable, and a lazy one: the file rows are
  /// built by index so an expanded 1,400-line diff isn't laid out on every
  /// frame of a scroll through the other 54 files.
  Widget _body(
    BuildContext context,
    ColorScheme scheme,
    AppLocalizations l10n,
    bool hasChanges,
    DiffModel? d,
  ) {
    final leading = <Widget>[
      // Where the work lands: branch + working-tree totals, as one card.
      Padding(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 4),
        child: _card(context, scheme, l10n, hasChanges, d),
      ),
      if (cwd != null && cwd!.trim().isNotEmpty) ...[
        _heading(context, l10n.envProject),
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 0, 16, 4),
          child: Row(
            children: [
              Icon(
                Icons.folder_outlined,
                size: 14,
                color: scheme.onSurfaceVariant,
              ),
              const SizedBox(width: 7),
              Expanded(
                child: Tooltip(
                  message: cwd!,
                  child: Text(
                    cwd!,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontFamily: monoFontFamily,
                      fontFamilyFallback: monoCjkFallback,
                      fontSize: 11.5,
                      color: scheme.onSurfaceVariant,
                    ),
                  ),
                ),
              ),
            ],
          ),
        ),
      ],
      _heading(context, l10n.envSource),
      if (!hasChanges)
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 4, 16, 8),
          child: Text(
            l10n.noChanges,
            style: TextStyle(fontSize: 12.5, color: scheme.outline),
          ),
        ),
    ];
    final files = hasChanges ? d!.files : const <DiffFile>[];
    return ListView.builder(
      padding: const EdgeInsets.only(bottom: 16),
      itemCount: leading.length + files.length,
      itemBuilder: (c, i) {
        if (i < leading.length) return leading[i];
        final f = files[i - leading.length];
        return _DiffFileTile(
          // Lazy building recycles elements, so the expanded/collapsed state
          // has to live somewhere that survives scrolling off-screen.
          key: PageStorageKey<String>('env-diff-${f.path}'),
          file: f,
          initiallyExpanded: false,
        );
      },
    );
  }

  /// The branch card: which checkout state the agent is writing into.
  Widget _card(
    BuildContext context,
    ColorScheme scheme,
    AppLocalizations l10n,
    bool hasChanges,
    DiffModel? d,
  ) => Container(
    padding: const EdgeInsets.fromLTRB(12, 10, 12, 11),
    decoration: BoxDecoration(
      color: scheme.surfaceContainerLow,
      border: Border.all(color: scheme.outlineVariant, width: 0.5),
      borderRadius: BorderRadius.circular(12),
    ),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Icon(Icons.computer, size: 14, color: scheme.primary),
            const SizedBox(width: 7),
            Text(
              l10n.envLocal,
              style: TextStyle(
                fontSize: 12,
                fontWeight: FontWeight.w600,
                color: scheme.onSurface,
              ),
            ),
          ],
        ),
        const SizedBox(height: 8),
        Row(
          children: [
            Icon(
              Icons.account_tree_outlined,
              size: 14,
              color: scheme.onSurfaceVariant,
            ),
            const SizedBox(width: 7),
            Expanded(
              child: Text(
                branch ?? '—',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  fontFamily: monoFontFamily,
                  fontFamilyFallback: monoCjkFallback,
                  fontSize: 12.5,
                  color: scheme.onSurface,
                ),
              ),
            ),
          ],
        ),
        if (hasChanges) ...[
          const SizedBox(height: 8),
          Row(
            children: [
              Text(
                '+${d!.added}',
                style: TextStyle(
                  fontSize: 12.5,
                  fontWeight: FontWeight.w600,
                  color: additionColor(scheme),
                  fontFeatures: const [FontFeature.tabularFigures()],
                ),
              ),
              const SizedBox(width: 8),
              Text(
                '−${d.removed}',
                style: TextStyle(
                  fontSize: 12.5,
                  fontWeight: FontWeight.w600,
                  color: scheme.error,
                  fontFeatures: const [FontFeature.tabularFigures()],
                ),
              ),
              const SizedBox(width: 10),
              Flexible(
                child: Text(
                  l10n.envFilesChanged(d.files.length),
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
        ],
      ],
    ),
  );

  /// A small all-caps section heading, the panel's only structural chrome.
  Widget _heading(BuildContext context, String text) => Padding(
    padding: const EdgeInsets.fromLTRB(16, 14, 16, 6),
    child: Text(
      text.toUpperCase(),
      style: TextStyle(
        fontSize: 10.5,
        fontWeight: FontWeight.w700,
        letterSpacing: 0.7,
        color: Theme.of(context).colorScheme.onSurfaceVariant,
      ),
    ),
  );
}

/// Most diff lines rendered for one expanded file. Everything inside a
/// [SingleChildScrollView] is laid out whether or not it is on screen, so this
/// is the cap that keeps one enormous file from making the whole panel drag,
/// and bounds the one-time highlight cost when a file is expanded.
const int _maxDiffLines = 200;

/// Directory prefix of a diff path, keeping its trailing separator. Empty for
/// a file at the repo root.
String _dirOf(String path) {
  final i = path.lastIndexOf(RegExp(r'[\\/]'));
  return i < 0 ? '' : path.substring(0, i + 1);
}

/// File name of a diff path.
String _baseOf(String path) {
  final i = path.lastIndexOf(RegExp(r'[\\/]'));
  return i < 0 ? path : path.substring(i + 1);
}

class _DiffFileTile extends StatelessWidget {
  const _DiffFileTile({
    super.key,
    required this.file,
    this.initiallyExpanded = true,
  });
  final DiffFile file;

  /// Inline review cards open expanded (the diff IS the point); the side
  /// panel's file list stays collapsed so a wide change stays scannable.
  final bool initiallyExpanded;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final l10n = AppLocalizations.of(context);
    return ExpansionTile(
      initiallyExpanded: initiallyExpanded,
      tilePadding: const EdgeInsets.symmetric(horizontal: 12),
      title: Row(
        children: [
          // The file NAME is what identifies a row, so it never ellipsizes —
          // the leading directories give way instead. A plain one-line path
          // clips from the tail, which in a side panel eats exactly the part
          // the user is scanning for.
          Expanded(
            child: Row(
              children: [
                if (_dirOf(file.path).isNotEmpty)
                  Flexible(
                    child: Text(
                      _dirOf(file.path),
                      style: TextStyle(
                        fontFamily: monoFontFamily,
                        fontFamilyFallback: monoCjkFallback,
                        fontSize: 12.5,
                        color: scheme.onSurfaceVariant,
                      ),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                Text(
                  _baseOf(file.path),
                  style: const TextStyle(
                    fontFamily: monoFontFamily,
                    fontFamilyFallback: monoCjkFallback,
                    fontSize: 12.5,
                  ),
                  maxLines: 1,
                ),
              ],
            ),
          ),
          const SizedBox(width: 8),
          Text(
            '+${file.added}',
            style: TextStyle(fontSize: 11, color: additionColor(scheme)),
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
      children: [_DiffHunks(file: file)],
    );
  }
}

/// Extension of a diff path, fed to the highlighter as its language hint. Empty
/// for a dotfile or an extensionless name (Dockerfile, LICENSE) — the
/// highlighter then leaves it plain, which is correct.
String _languageForPath(String path) {
  final base = _baseOf(path);
  final dot = base.lastIndexOf('.');
  if (dot <= 0) return '';
  return base.substring(dot + 1);
}

/// New-file line number a `@@ -a,b +c,d @@` hunk header starts at, or null if
/// it doesn't parse.
int? _hunkNewStart(String header) {
  final m = RegExp(r'@@\s*-\d+(?:,\d+)?\s*\+(\d+)').firstMatch(header);
  return m == null ? null : int.tryParse(m.group(1)!);
}

/// One file's diff, rendered as a real code view: syntax-highlighted lines with
/// a line-number + add/remove gutter, on tinted rows.
///
/// The height is BOUNDED (its own scroll box) for a reason beyond looks: when
/// this lived as one tall child of the panel's outer `ListView.builder`, that
/// list estimated its total scroll extent from the heights of laid-out
/// children — and a single ~5,000px expanded `Cargo.lock` made the estimate
/// balloon and swing as items entered and left, so dragging the scrollbar thumb
/// (mapped through that estimate) flew across the whole file. Capping the box
/// keeps every outer item a similar size, so the thumb tracks the pointer. A
/// box that fits its content has no scroll extent of its own and lets the wheel
/// fall through to the panel; only a genuinely long diff scrolls internally.
class _DiffHunks extends StatefulWidget {
  const _DiffHunks({required this.file});
  final DiffFile file;

  @override
  State<_DiffHunks> createState() => _DiffHunksState();
}

class _DiffHunksState extends State<_DiffHunks> {
  final ScrollController _v = ScrollController();

  @override
  void dispose() {
    _v.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final l10n = AppLocalizations.of(context);
    final brightness = Theme.of(context).brightness;
    final lang = _languageForPath(widget.file.path);
    final shown = widget.file.lines.take(_maxDiffLines).toList();

    final rows = <Widget>[];
    int? newNo;
    for (final line in shown) {
      switch (line.kind) {
        case DiffLineKind.hunk:
          newNo = _hunkNewStart(line.text);
          rows.add(_hunkRow(scheme, line.text));
        case DiffLineKind.added:
          rows.add(_codeRow(scheme, brightness, lang, line, newNo));
          if (newNo != null) newNo++;
        case DiffLineKind.context:
          rows.add(_codeRow(scheme, brightness, lang, line, newNo));
          if (newNo != null) newNo++;
        case DiffLineKind.removed:
          rows.add(_codeRow(scheme, brightness, lang, line, null));
      }
    }

    // ~18px per single-line row. Fit-to-content for a short diff (no inner
    // scroll, wheel falls through); capped for a long one (scrolls in place).
    final boxH = (shown.length * 18.0 + 6).clamp(0.0, 460.0);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SizedBox(
          height: boxH,
          child: Scrollbar(
            controller: _v,
            thumbVisibility: true,
            child: SingleChildScrollView(
              // Distinct PageStorage identity, per file. Without one each inner
              // scrollable shares the enclosing ExpansionTile's bucket entry
              // with its expanded flag, and restoring the offset reads that
              // bool as a double: "type 'bool' is not a subtype of 'double?'".
              key: PageStorageKey<String>('diff-v-${widget.file.path}'),
              controller: _v,
              child: SingleChildScrollView(
                key: PageStorageKey<String>('diff-h-${widget.file.path}'),
                scrollDirection: Axis.horizontal,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: rows,
                ),
              ),
            ),
          ),
        ),
        if (widget.file.lines.length > _maxDiffLines)
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 8, 12, 10),
            child: Text(
              l10n.diffTruncated(widget.file.lines.length - _maxDiffLines),
              style: TextStyle(fontSize: 11.5, color: scheme.outline),
            ),
          ),
      ],
    );
  }

  static const _mono = TextStyle(
    fontFamily: monoFontFamily,
    fontFamilyFallback: monoCjkFallback,
    fontSize: 12,
    height: 1.35,
  );

  Widget _codeRow(
    ColorScheme scheme,
    Brightness brightness,
    String lang,
    DiffLine line,
    int? lineNo,
  ) {
    final (Color bg, String marker, Color markerColor) = switch (line.kind) {
      DiffLineKind.added => (
        additionColor(scheme).withValues(alpha: 0.13),
        '+',
        additionColor(scheme),
      ),
      DiffLineKind.removed => (
        scheme.error.withValues(alpha: 0.10),
        '−',
        scheme.error,
      ),
      _ => (Colors.transparent, ' ', scheme.onSurfaceVariant),
    };
    // The code keeps its syntax colours whatever the row tint — the way a real
    // diff viewer reads. Removed lines dim slightly so the eye lands on adds.
    final base = _mono.copyWith(
      color: line.kind == DiffLineKind.removed
          ? scheme.onSurfaceVariant
          : scheme.onSurface,
    );
    final span = highlightCode(
      code: line.text,
      language: lang,
      base: base,
      brightness: brightness,
    );
    return Container(
      color: bg,
      padding: const EdgeInsets.symmetric(vertical: 1),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 42,
            child: Text(
              lineNo?.toString() ?? '',
              textAlign: TextAlign.right,
              style: _mono.copyWith(fontSize: 11, color: scheme.outline),
            ),
          ),
          SizedBox(
            width: 16,
            child: Text(
              marker,
              textAlign: TextAlign.center,
              style: _mono.copyWith(color: markerColor),
            ),
          ),
          Padding(
            padding: const EdgeInsets.only(right: 12),
            child: Text.rich(span),
          ),
        ],
      ),
    );
  }

  // No explicit width: this sits inside a horizontal scroll view, which offers
  // unbounded width — `double.infinity` there is an error, so the row sizes to
  // its `@@…@@` text.
  Widget _hunkRow(ColorScheme scheme, String text) => Container(
    color: scheme.primary.withValues(alpha: 0.07),
    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 3),
    child: Text(
      text,
      style: _mono.copyWith(fontSize: 11.5, color: scheme.primary),
    ),
  );
}

/// In plan mode the model wraps its proposal in `<proposed_plan>…</proposed_plan>`
/// and codex doesn't always strip the tags, so they leak into the rendered
/// message. Detect them and return the text without the wrapper tags plus an
/// `isPlan` flag the UI uses to badge the message as a plan. Streaming-safe:
/// strips whichever tag has arrived so far (the open tag leads the content).
({bool isPlan, String text}) _readProposedPlan(String raw) {
  final open = RegExp(r'<\s*proposed_plan\s*>', caseSensitive: false);
  if (!open.hasMatch(raw)) return (isPlan: false, text: raw);
  final close = RegExp(r'<\s*/\s*proposed_plan\s*>', caseSensitive: false);
  return (
    isPlan: true,
    text: raw.replaceAll(open, '').replaceAll(close, '').trim(),
  );
}

/// Width at which the transcript switches from mobile chat bubbles to the
/// desktop document layout (role labels, accent rails, compact tool rows).
const double _docLayoutWidth = 720;

/// Stand-in fed to `newSessionTitleIn` so the localized sentence can be split
/// around the project name and the name rendered as a live dropdown. A private
///-use codepoint, so it can never collide with real text in any translation.
const String _kProjectSlot = '';

/// The "this reply is a plan" marker.
Widget _planBadge(BuildContext context, ColorScheme scheme) => Row(
  mainAxisSize: MainAxisSize.min,
  children: [
    Icon(Icons.checklist_rounded, size: 16, color: scheme.primary),
    const SizedBox(width: 6),
    Text(
      AppLocalizations.of(context).toolPlan,
      style: TextStyle(
        fontSize: 12,
        fontWeight: FontWeight.w600,
        color: scheme.primary,
      ),
    ),
  ],
);

/// Renders one timeline entry. Messages render Gemini-style (user = soft
/// right bubble, agent = full-width Markdown); tool/activity items render as a
/// collapsible [_ActivityCard]. Message copy fades in on hover (desktop);
/// touch uses the enclosing [SelectionArea]'s long-press.
class _MessageView extends StatefulWidget {
  const _MessageView({super.key, required this.item, this.hostImageLoader});
  final _Item item;

  /// Reads a host-side image so a mentioned file renders as a picture.
  final HostImageLoader? hostImageLoader;

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

  /// A turn's completion time, from `Turn.completedAt` (Unix seconds).
  ///
  /// Today's turns show only the clock — the date would be noise in a
  /// conversation you are still having. Anything older leads with the weekday
  /// (this week) or the date, so scrolling back through a long thread tells you
  /// when each answer happened.
  String _fmtTurnTime(int unixSeconds) {
    final at = DateTime.fromMillisecondsSinceEpoch(unixSeconds * 1000);
    final now = DateTime.now();
    final locale = Localizations.localeOf(context).toLanguageTag();
    final clock = DateFormat.Hm(locale).format(at);
    final sameDay =
        at.year == now.year && at.month == now.month && at.day == now.day;
    if (sameDay) return clock;
    final age = now.difference(at);
    if (age.inDays < 7) return '${DateFormat.EEEE(locale).format(at)} $clock';
    return '${DateFormat.Md(locale).format(at)} $clock';
  }

  void _copy() {
    final l10n = AppLocalizations.of(context);
    // Copy what's shown — without the <proposed_plan> wrapper tags.
    Clipboard.setData(
      ClipboardData(text: _readProposedPlan(widget.item.text).text),
    );
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
          text: item.streaming
              ? AppLocalizations.of(context).compactingContext
              : AppLocalizations.of(context).compacted,
          active: item.streaming,
        ),
        'interrupted' => _SystemNotice(
          icon: Icons.stop_circle_outlined,
          text: AppLocalizations.of(context).turnStopped,
        ),
        'turnDuration' => _TurnDurationFooter(
          duration: item.title,
          completedAt: item.text,
          model: item.model,
          effortWire: item.effortWire,
          confirmed: item.modelConfirmed,
          rerouted: item.modelRerouted,
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
    // A Live voice handoff is a different kind of turn: a stretch of spoken
    // back-and-forth, not one typed message. It gets its own card.
    final handoff = isUser ? parseRealtimeDelegation(item.text) : null;
    if (handoff != null) {
      return Padding(
        padding: const EdgeInsets.only(top: 8),
        child: RealtimeHandoffCard(handoff: handoff),
      );
    }
    // A plan-mode proposal streams in wrapped in `<proposed_plan>…</proposed_plan>`
    // tags that codex doesn't strip. Remove them at the display layer and badge
    // the message as a plan, so the UI perceives it as a plan instead of leaking
    // the raw markup. The stored item text is untouched (display-only).
    final proposal = _readProposedPlan(item.text);
    // Document attachments ride the text as a trailing path-reference block
    // (wire format); render them as chips and show only the typed text —
    // display-only, the stored item text (and the copy action) keep the block.
    // A message from a client with editor context (IDE extension, desktop app,
    // codex TUI) arrives with that context serialized ahead of the request;
    // upstream expects every transcript renderer to strip back to the request
    // and we surface the files it named as attachments. Display-only — the
    // stored text, and the copy action, keep the message verbatim.
    final ide = isUser
        ? splitIdeContext(item.text)
        : (text: item.text, files: const <IdeMentionedFile>[]);
    final refs = isUser
        ? splitFileRefs(ide.text)
        : (text: ide.text, paths: const <String>[]);
    final images = [
      ...item.images,
      for (final f in ide.files)
        if (looksLikeImagePath(f.path)) ResolvedImage.hostFile(f.path),
    ];
    final paths = [
      for (final f in ide.files)
        if (!looksLikeImagePath(f.path)) f.path,
      ...refs.paths,
    ];
    // Desktop reads as a document, not as a chat: a coding agent's transcript
    // is something you scan and scroll back through, so turns are blocks with
    // a role label and an accent rail rather than iOS bubbles. Narrow windows
    // keep the bubbles, which is the right idiom for a thumb.
    final doc = MediaQuery.sizeOf(context).width >= _docLayoutWidth;
    // The reference desktop app keeps the user's turn as a right-aligned
    // bubble and leaves the reply as plain prose — same shapes as the phone,
    // just tighter: a smaller radius and less padding, because a 20 px pill is
    // a touch-target look that reads as oversized under a pointer.
    // Attachments sit ABOVE the bubble, not inside it. An image wrapped in the
    // text bubble made the bubble a container for two unlike things — the
    // picture picked up the bubble's padding and background, and a
    // picture-only message rendered as a mostly-empty bubble. Separating them
    // also keeps the thumbnail identical to the one staged in the composer.
    final Widget? attachments = images.isEmpty && paths.isEmpty
        ? null
        : Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.end,
            children: [
              if (images.isNotEmpty)
                MessageImagesView(
                  images: images,
                  hostImageLoader: widget.hostImageLoader,
                ),
              if (images.isNotEmpty && paths.isNotEmpty)
                const SizedBox(height: 6),
              if (paths.isNotEmpty) FileRefChips(paths: paths),
            ],
          );
    final Widget content = isUser
        ? Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.end,
            children: [
              if (attachments != null) ...[
                ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 600),
                  child: attachments,
                ),
                // Only when text follows: a bare attachment shouldn't leave
                // trailing space under the last row.
                if (refs.text.isNotEmpty) const SizedBox(height: 6),
              ],
              // A message with no text at all is just its attachments — an
              // empty bubble under them would be a visible artifact.
              if (refs.text.isNotEmpty)
                Container(
                  constraints: const BoxConstraints(maxWidth: 600),
                  padding: const EdgeInsets.symmetric(
                    horizontal: 16,
                    vertical: 10,
                  ),
                  decoration: BoxDecoration(
                    color: scheme.surfaceContainerLow,
                    borderRadius: BorderRadius.circular(kPanelRadius),
                  ),
                  // The tighter 1.3 line: a bubble is a transcription of one
                  // utterance, not a paragraph to read down.
                  child: linkifyText(
                    context,
                    refs.text,
                    style: Theme.of(
                      context,
                    ).textTheme.bodyLarge?.copyWith(height: 1.3),
                  ),
                ),
            ],
          )
        : Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              if (proposal.isPlan) ...[
                _planBadge(context, scheme),
                const SizedBox(height: 6),
              ],
              SizedBox(
                width: double.infinity,
                child: MarkdownView(data: proposal.text),
              ),
            ],
          );

    // No copy for an image-only message (empty text) — it would clobber the
    // clipboard with an empty string while confirming "copied".
    final showActions = !item.streaming && item.text.trim().isNotEmpty;
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
                // Document layout runs left-aligned for both roles, so the
                // copy affordance follows the text rather than the bubble.
                alignment: isUser
                    ? Alignment.centerRight
                    : Alignment.centerLeft,
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    _MessageAction(
                      icon: Icons.content_copy_outlined,
                      tooltip: AppLocalizations.of(context).copy,
                      onPressed: _copy,
                    ),
                    // The turn's completion time, from the server's own
                    // `Turn.completedAt` — so it survives a reload, unlike the
                    // locally-observed duration marker.
                    if (item.turnCompletedAt != null) ...[
                      const SizedBox(width: 6),
                      Text(
                        _fmtTurnTime(item.turnCompletedAt!),
                        style: TextStyle(
                          fontSize: 11.5,
                          color: onSurfaceMuted(scheme),
                        ),
                      ),
                    ],
                  ],
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
        padding: EdgeInsets.only(top: doc ? 14 : 8),
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
  const _SystemNotice({
    required this.icon,
    required this.text,
    this.active = false,
  });
  final IconData icon;
  final String text;
  final bool active;

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
                if (active)
                  PulsingDot(
                    key: const Key('chat-compaction-progress'),
                    color: muted,
                    size: 8,
                  )
                else
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

/// A per-turn footnote dropped in after a turn ends: a subtle `用时 m:ss` tag
/// plus the model (and effort) that actually handled the turn, so a mid-chat
/// model switch is verifiable per response. The tooltip (hover on desktop,
/// long-press on mobile) reveals the wall-clock completion time and the
/// stamp's provenance — server-confirmed vs as-sent-by-the-app — and flags a
/// mid-turn server reroute.
class _TurnDurationFooter extends StatelessWidget {
  const _TurnDurationFooter({
    required this.duration,
    required this.completedAt,
    this.model,
    this.effortWire,
    this.confirmed = false,
    this.rerouted = false,
  });

  final String duration;
  final String completedAt;
  final String? model;
  final String? effortWire;
  final bool confirmed;
  final bool rerouted;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final muted = scheme.onSurfaceVariant;
    final effort = ReasoningEffort.fromWire(effortWire);
    final modelText = model == null
        ? null
        : effort == null
        ? model!
        : '$model · ${effort.label(l10n)}';
    final tooltip = [
      l10n.completedAt(completedAt),
      if (model != null) l10n.turnHandledBy(model!),
      if (rerouted)
        l10n.modelReroutedNote
      else if (model != null)
        confirmed ? l10n.runtimeConfirmed : l10n.runtimeUnconfirmed,
    ].join('\n');
    return Align(
      alignment: Alignment.centerLeft,
      child: Tooltip(
        message: tooltip,
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
              if (modelText != null) ...[
                const SizedBox(width: 8),
                Icon(Icons.auto_awesome, size: 11, color: muted),
                const SizedBox(width: 4),
                ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 260),
                  child: Text(
                    modelText,
                    key: const Key('turn-model-stamp'),
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(fontSize: 11.5, color: muted),
                  ),
                ),
                if (rerouted) ...[
                  const SizedBox(width: 3),
                  Icon(
                    Icons.sync_problem,
                    size: 11,
                    color: cautionColor(scheme),
                  ),
                ],
              ],
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
            mouseCursor: clickable,
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
                        fontFamily: monoFontFamily,
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
                        color: additionColor(scheme),
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
              // ListTile-based tiles paint their ink/background correctly —
              // which also means it has to be opaque, blended over the page.
              child: Material(
                color: Color.alphaBlend(
                  scheme.surfaceContainerLow,
                  scheme.surface,
                ),
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
              fontFamily: monoFontFamily,
              fontFamilyFallback: monoCjkFallback,
              fontSize: 12,
            ),
          ),
        ),
        InkResponse(
          mouseCursor: clickable,
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
        color: scheme.surfaceContainerLow,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: scheme.outlineVariant),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // Header: icon + "Plan" + progress, tap to collapse.
          InkWell(
            mouseCursor: clickable,
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
                child: MarkdownView(data: explanation),
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
      'completed' => (Icons.check_circle_rounded, additionColor(scheme)),
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

/// One glyph in a message's hover action row.
///
/// A washed square on hover rather than a bare icon: the reference app's row of
/// actions reads as a set of small controls, and an icon that only changes ink
/// gives no target to aim at under a pointer.
class _MessageAction extends StatefulWidget {
  const _MessageAction({
    required this.icon,
    required this.tooltip,
    required this.onPressed,
  });

  final IconData icon;
  final String tooltip;
  final VoidCallback onPressed;

  @override
  State<_MessageAction> createState() => _MessageActionState();
}

class _MessageActionState extends State<_MessageAction> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Tooltip(
      message: widget.tooltip,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: GestureDetector(
          onTap: widget.onPressed,
          child: Container(
            padding: const EdgeInsets.all(5),
            decoration: BoxDecoration(
              // Off-state is the wash at zero alpha, not transparent: Material
              // lerps through transparent BLACK, which flashes a dark box.
              color: _hovered
                  ? scheme.surfaceContainerHigh
                  : scheme.surfaceContainerHigh.withValues(alpha: 0),
              borderRadius: BorderRadius.circular(6),
            ),
            child: Icon(
              widget.icon,
              size: 15,
              color: _hovered ? scheme.onSurface : onSurfaceMuted(scheme),
            ),
          ),
        ),
      ),
    );
  }
}

/// A run of ≥2 consecutive `agentMessage` items — one turn's reply, split
/// across several server items — rendered as a single block.
class _AgentTurn {
  _AgentTurn(this.items);
  final List<_Item> items;

  /// Still producing text: the block keeps its actions hidden until the whole
  /// reply has settled, not just its first part.
  bool get streaming => items.any((i) => i.streaming);

  /// When the turn finished, per the server.
  int? get completedAt => items.first.turnCompletedAt;

  /// The reply as one document. Blank parts are dropped so a placeholder item
  /// can't open the block with an empty paragraph.
  String get text =>
      items.map((i) => i.text.trim()).where((t) => t.isNotEmpty).join('\n\n');
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
            mouseCursor: clickable,
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
    // Reasoning is prose the model wrote in Markdown (its summaries open with
    // a `**bold**` header); everything else here is literal output — a command
    // line, a tool payload, a diff — where a `*` means an asterisk and the
    // monospace column matters. Only the prose gets rendered as Markdown.
    final prose = item.type == 'reasoning';
    // One-line value: the title (command/query/tool) or a peek of the detail.
    final rawValue = title.isNotEmpty
        ? title
        : detail
              .split('\n')
              .firstWhere((s) => s.trim().isNotEmpty, orElse: () => '');
    final value = prose ? markdownPlainPreview(rawValue) : rawValue;
    // Expandable when there's detail or the value is long enough to truncate.
    final expandable = detail.isNotEmpty || value.length > 56;
    final body = [
      if (title.isNotEmpty) title,
      if (detail.isNotEmpty) detail,
    ].join('\n\n');

    // Two idioms. A phone gets a soft bordered card — a comfortable tap target
    // in a list of them. A desktop transcript gets a timeline: the steps of a
    // turn are rows hanging off one continuous rail, so a dozen tool calls read
    // as a sequence instead of a dozen boxes.
    final doc = MediaQuery.sizeOf(context).width >= _docLayoutWidth;
    return Container(
      margin: EdgeInsets.symmetric(vertical: doc ? 0 : 2),
      padding: doc ? const EdgeInsets.only(left: 12) : EdgeInsets.zero,
      decoration: BoxDecoration(
        border: doc
            ? Border(left: BorderSide(color: scheme.outlineVariant, width: 1.5))
            : Border.all(color: scheme.outlineVariant, width: 0.5),
        borderRadius: doc ? null : BorderRadius.circular(12),
      ),
      clipBehavior: doc ? Clip.none : Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          InkWell(
            mouseCursor: clickable,
            onTap: expandable
                ? () => setState(() => _expanded = !_expanded)
                : null,
            child: Padding(
              padding: EdgeInsets.symmetric(
                horizontal: doc ? 0 : 12,
                vertical: doc ? 4 : 10,
              ),
              child: Row(
                children: [
                  Icon(
                    meta.icon,
                    size: doc ? 15 : 17,
                    color: doc ? muted : scheme.primary,
                  ),
                  SizedBox(width: doc ? 8 : 10),
                  Text(
                    meta.label,
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: doc ? muted : scheme.onSurface,
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
                        style: prose
                            ? TextStyle(fontSize: 12.5, color: muted)
                            : TextStyle(
                                fontFamily: monoFontFamily,
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
              margin: EdgeInsets.only(bottom: doc ? 6 : 0),
              padding: const EdgeInsets.all(11),
              constraints: const BoxConstraints(maxHeight: 320),
              decoration: BoxDecoration(
                border: doc
                    ? null
                    : Border(
                        top: BorderSide(
                          color: scheme.outlineVariant,
                          width: 0.5,
                        ),
                      ),
                borderRadius: doc ? BorderRadius.circular(8) : null,
                color: scheme.surfaceContainerLowest,
              ),
              child: SingleChildScrollView(
                child: prose
                    ? MarkdownView(data: body, muted: true)
                    : linkifyText(
                        context,
                        body,
                        selectable: true,
                        style: const TextStyle(
                          fontFamily: monoFontFamily,
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
  const _TypingIndicator({super.key, required this.elapsed});

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
