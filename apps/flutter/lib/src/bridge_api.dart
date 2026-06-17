/// Plain Dart mirrors of the bridge DTOs (decoupled from FRB types so the
/// UI and tests do not import generated bindings).
library;

/// A discovered service on the relay.
class ServiceEntry {
  /// Creates a service entry.
  const ServiceEntry({
    required this.device,
    required this.kind,
    required this.name,
    required this.key,
  });

  /// Device id segment.
  final String device;

  /// `app` or `api`.
  final String kind;

  /// Instance name segment.
  final String name;

  /// Full `pcx:<device>:<kind>:<name>` key.
  final String key;
}

/// Status of one active local subscription.
class SubInfo {
  /// Creates a subscription status.
  const SubInfo({
    required this.key,
    required this.localAddr,
    required this.alive,
  });

  /// Service key being subscribed to.
  final String key;

  /// Local `host:port` the subscriber listener is bound on.
  final String localAddr;

  /// Whether the subscription task is still running.
  final bool alive;
}

/// View of persisted config (relay + whether a key is set).
class ConfigInfo {
  /// Creates a config view.
  const ConfigInfo({required this.relay, required this.hasKey, this.locale});

  /// Configured relay `host:port`, if any.
  final String? relay;

  /// Whether a 32-byte key is stored.
  final bool hasKey;

  /// Configured UI locale (BCP-47, e.g. `en`/`zh`), or `null` to follow the
  /// system locale.
  final String? locale;
}

/// A live app-server event (one JSON-RPC notification), flattened for the UI.
class AppEvent {
  /// Creates an app event.
  const AppEvent({
    required this.kind,
    this.threadId,
    this.itemId,
    this.itemType,
    this.title,
    this.text,
    this.requestId,
    required this.raw,
  });

  /// JSON-RPC method, e.g. `turn/started`, `item/agentMessage/delta`,
  /// `turn/completed`.
  final String kind;

  /// Thread id the event belongs to, when present.
  final String? threadId;

  /// Item id the event refers to, when present.
  final String? itemId;

  /// Item type tag when this event carries an item (`agentMessage`,
  /// `commandExecution`, `webSearch`, `mcpToolCall`, `fileChange`,
  /// `reasoning`, …); `null` for turn-level events.
  final String? itemType;

  /// One-line summary for tool/activity items (command, query, tool name…).
  final String? title;

  /// Text payload (a streaming delta or an item's body/detail).
  final String? text;

  /// Token to answer a server approval request via [BridgeApi.appRespondApproval].
  final String? requestId;

  /// Full params JSON for fields not modelled above.
  final String raw;
}

/// Summary metadata for one app-server thread.
class ThreadMeta {
  /// Creates thread metadata.
  const ThreadMeta({
    required this.id,
    required this.preview,
    required this.cwd,
    required this.updatedAt,
  });

  /// Thread id.
  final String id;

  /// Preview (usually the first user message).
  final String preview;

  /// Working directory — the project this thread controls.
  final String cwd;

  /// Unix seconds of last update.
  final int updatedAt;
}

/// A thread's recovered history plus whether a turn is still running, and the
/// metadata the status bar / git chip seed from on open.
class ThreadHistory {
  /// Creates a thread history.
  const ThreadHistory({
    required this.items,
    required this.running,
    this.branch,
    this.cwd,
    this.tokensUsed,
    this.contextWindow,
    this.collaborationMode,
    this.reasoningEffort,
  });

  /// Conversation items, oldest first.
  final List<ThreadItem> items;

  /// Whether the most recent turn is still in progress.
  final bool running;

  /// Current git branch of the thread's cwd, if it's a repo.
  final String? branch;

  /// The thread's resolved working directory (for git diff / status).
  final String? cwd;

  /// Tokens currently occupying the model context window.
  final int? tokensUsed;

  /// The model's context-window size in tokens.
  final int? contextWindow;

  /// The thread's sticky collaboration mode (`plan` / `default`), so the UI
  /// plan toggle reflects the server's real state instead of guessing.
  final String? collaborationMode;

  /// The thread's current reasoning effort (`low`/`medium`/`high`), so the UI
  /// can display the "thinking level" the thread runs with. Sourced from the
  /// thread/resume response (thread/read doesn't expose it).
  final String? reasoningEffort;
}

/// One model offered by the app-server.
class ModelInfo {
  /// Creates model info.
  const ModelInfo({
    required this.id,
    required this.displayName,
    required this.description,
    this.supportedReasoningEfforts = const [],
    this.defaultReasoningEffort,
  });

  /// Model id (used as the `model` param).
  final String id;

  /// Human-readable name.
  final String displayName;

  /// Short description.
  final String description;

  /// Reasoning efforts this model accepts (`minimal`/`low`/`medium`/`high`/
  /// `xhigh`/…), so the effort picker offers only the levels this model supports.
  final List<String> supportedReasoningEfforts;

  /// The model's default reasoning effort, if any.
  final String? defaultReasoningEffort;
}

/// One materialised conversation item from `thread/read`.
class ThreadItem {
  /// Creates a thread item.
  const ThreadItem({
    required this.id,
    required this.itemType,
    required this.title,
    required this.text,
  });

  /// Item id.
  final String id;

  /// Item type tag (`userMessage` / `agentMessage` / `commandExecution` /
  /// `webSearch` / `mcpToolCall` / `fileChange` / `reasoning` / …).
  final String itemType;

  /// One-line summary for tool/activity items.
  final String title;

  /// Body / detail text.
  final String text;
}

/// A process holding a session's rollout file open — a would-be force-takeover
/// target.
class Holder {
  /// Creates a holder.
  const Holder({required this.pid, required this.name});

  /// Operating-system process id.
  final int pid;

  /// Process image name (e.g. `codex.exe`).
  final String name;
}

/// One codex session discovered under the shared `CODEX_HOME`, annotated with
/// whether it is safe to resume.
class LocalSession {
  /// Creates a local session entry.
  const LocalSession({
    required this.threadId,
    this.cwd,
    required this.preview,
    this.source,
    required this.updatedAt,
    required this.turnState,
    required this.heldOpen,
    required this.safety,
    required this.allowsResume,
    required this.requiresTakeover,
  });

  /// Thread / conversation id.
  final String threadId;

  /// Working directory the session controls, when recorded.
  final String? cwd;

  /// Best-effort first-user-message preview.
  final String preview;

  /// Originating client (`cli` / `vscode` / …), when recorded.
  final String? source;

  /// Last-modified time of the rollout, unix seconds.
  final int updatedAt;

  /// Most-recent-turn state (`empty`/`completed`/`aborted`/`incomplete`).
  final String turnState;

  /// Whether the rollout is currently held open by a live process.
  final bool heldOpen;

  /// Resume-safety tag (`resumable`/`resumableUnfinished`/`ownedRunning`/
  /// `ownedIdle`).
  final String safety;

  /// Whether the UI may offer a resume action (false only while a turn is
  /// actively running).
  final bool allowsResume;

  /// Whether resuming requires a force takeover (a live owner must be evicted
  /// first).
  final bool requiresTakeover;
}

/// One session's liveness detail, including the processes a force takeover
/// would terminate (Pocket-Codex's own app-server already excluded).
class SessionLiveness {
  /// Creates a liveness view.
  const SessionLiveness({
    required this.threadId,
    required this.turnState,
    required this.heldOpen,
    required this.safety,
    required this.allowsResume,
    required this.requiresTakeover,
    required this.holders,
  });

  /// Thread / conversation id.
  final String threadId;

  /// Most-recent-turn state tag.
  final String turnState;

  /// Whether the rollout is currently held open.
  final bool heldOpen;

  /// Resume-safety tag.
  final String safety;

  /// Whether the UI may offer a resume action.
  final bool allowsResume;

  /// Whether resuming requires a force takeover.
  final bool requiresTakeover;

  /// Processes a force takeover would attempt to terminate.
  final List<Holder> holders;
}

/// Outcome of a force-resume: which holders were killed / survived, and whether
/// the subsequent resume took.
class ForceResumeReport {
  /// Creates a force-resume report.
  const ForceResumeReport({
    required this.killed,
    required this.survived,
    required this.stillHeld,
    required this.resumed,
    this.resumeError,
  });

  /// Holders that were successfully terminated.
  final List<Holder> killed;

  /// Holders the kill could not reach.
  final List<Holder> survived;

  /// Whether the rollout is still held open after the attempt (the resume
  /// proceeded regardless).
  final bool stillHeld;

  /// Whether the subsequent `thread/resume` succeeded.
  final bool resumed;

  /// The resume error message, when [resumed] is false.
  final String? resumeError;
}

/// The whole engine surface the UI is allowed to touch. One real impl wraps
/// flutter_rust_bridge; a fake backs widget tests.
abstract interface class BridgeApi {
  /// Current persisted config.
  Future<ConfigInfo> getConfig();

  /// Set the relay `host:port` and persist.
  Future<void> setRelay(String relay);

  /// Set the 32-byte MSG_HEADER_KEY and persist.
  Future<void> setKey(String key);

  /// Import a `pcx1:` share string; returns the relay. Throws on bad input.
  Future<String> importConfig(String text);

  /// Export the current relay+key as a `pcx1:` share string.
  Future<String> exportConfig();

  /// Discover services on the configured relay.
  Future<List<ServiceEntry>> discoverServices();

  /// Subscribe to an API service, exposing it on `127.0.0.1:<localPort>`.
  Future<SubInfo> apiSubscribe(String serviceKey, int localPort);

  /// Stop an API-service subscription.
  Future<void> apiUnsubscribe(String serviceKey);

  /// List all active subscriptions.
  Future<List<SubInfo>> subscriptions();

  /// Persist the UI locale (BCP-47, e.g. `en`/`zh`). An empty string clears
  /// it, meaning follow the system locale.
  Future<void> setLocale(String locale);

  // --- App-server remote control ---

  /// Connect to an app-server service: subscribe on `127.0.0.1:<localPort>`,
  /// open the JSON-RPC websocket and run the `initialize` handshake. Pass
  /// `localPort: 0` to let the bridge assign a free OS port per service (so
  /// several services can coexist).
  Future<void> appConnect(String serviceKey, int localPort);

  /// Whether a live app-server session exists for [serviceKey].
  bool appIsConnected(String serviceKey);

  /// Disconnect the app-server session and its pb-mapper subscription.
  Future<void> appDisconnect(String serviceKey);

  /// Probe whether an app-server is actually REACHABLE (its backend answers a
  /// handshake) rather than merely registered on the relay. The services list
  /// uses this so a registered-but-dead app-server shows as unreachable instead
  /// of a false "online". A live session short-circuits to true.
  Future<bool> appProbe(String serviceKey);

  /// Live event stream for [serviceKey] (turn/item notifications).
  Stream<AppEvent> appEvents(String serviceKey);

  /// List threads known to the app-server.
  Future<List<ThreadMeta>> appThreadList(String serviceKey);

  /// List the models the app-server offers.
  Future<List<ModelInfo>> appModelList(String serviceKey);

  /// Read the account rate-limit / quota snapshot as raw JSON (5h + weekly
  /// windows). Parsed on the Dart side since the shape is nested and volatile.
  Future<String> appRateLimits(String serviceKey);

  /// Unified diff of the repo at [cwd] vs its remote default branch. Empty when
  /// the cwd isn't a git repo or there are no changes.
  Future<String> appGitDiff(String serviceKey, String cwd);

  /// Start a manual conversation compaction; the server emits a
  /// `thread/compacted` event when done.
  Future<void> appCompact(String serviceKey, String threadId);

  /// Start a new thread / project. [approvalPolicy] is one of
  /// `untrusted`/`on-failure`/`on-request`/`never`; [sandbox] is one of
  /// `read-only`/`workspace-write`/`danger-full-access`. Returns the id.
  Future<String> appThreadStart(
    String serviceKey, {
    String? model,
    String? cwd,
    String? approvalPolicy,
    String? sandbox,
  });

  /// Resume an existing thread (load it into the session) before reading it or
  /// sending turns; otherwise the server reports "thread not found".
  Future<void> appThreadResume(String serviceKey, String threadId);

  /// Read a thread's history (items oldest first) and whether a turn is still
  /// running, so re-opening an in-flight thread restores its live state.
  Future<ThreadHistory> appThreadRead(String serviceKey, String threadId);

  /// Send a user message, starting a model turn. [model] / [approvalPolicy] /
  /// [sandbox] are optional per-turn overrides (apply to this and subsequent
  /// turns) so model and permission can change mid-conversation.
  /// [collaborationMode] ("plan" / "default", or null to leave unchanged) is
  /// sticky on the thread, so pass "default" to leave plan mode.
  /// [reasoningEffort] ("low"/"medium"/"high", or null for the model default) is
  /// the "thinking level" for this turn. The reply streams via [appEvents].
  Future<void> appTurnStart(
    String serviceKey,
    String threadId,
    String text, {
    String? model,
    String? approvalPolicy,
    String? sandbox,
    String? collaborationMode,
    String? reasoningEffort,
  });

  /// Interrupt the running turn. [turnId] (from the latest `turn/started`) is
  /// required by the server to identify which turn to abort.
  Future<void> appTurnInterrupt(
    String serviceKey,
    String threadId, {
    String? turnId,
  });

  /// Answer a server approval request ([requestId] from an [AppEvent]).
  /// [decision] is a ReviewDecision wire value (`approved`/`denied`/`abort`).
  Future<void> appRespondApproval(
    String serviceKey,
    String requestId,
    String decision,
  );

  // --- Local session takeover (shared CODEX_HOME) ---

  /// List every codex session under the shared `CODEX_HOME`, newest first,
  /// each annotated with whether it is safe to resume. Reads local disk +
  /// the process table; no app-server connection required.
  Future<List<LocalSession>> appLocalSessions();

  /// Inspect one session's live resume-safety and the processes a force
  /// takeover would evict. Poll before showing a resume button so the UI
  /// reflects live ownership.
  Future<SessionLiveness> appSessionLiveness(String threadId);

  /// Force-resume a session into the app-server behind [serviceKey]:
  /// best-effort evict the rollout's holders (never our own app-server), then
  /// `thread/resume` regardless of the eviction outcome. Gate on explicit user
  /// confirmation; do not call while a turn is actively running.
  Future<ForceResumeReport> appForceResume(String serviceKey, String threadId);

  /// Read a local session's transcript for READ-ONLY viewing. Parses the
  /// on-disk rollout directly (no app-server connection, no resume, no write),
  /// so it works even while another codex client still owns the session.
  /// Items are in the same shape as [appThreadRead]. Poll alongside
  /// [appSessionLiveness] to follow a running session and notice when it goes
  /// idle (resume-eligible).
  Future<List<ThreadItem>> appLocalSessionTranscript(String threadId);
}
