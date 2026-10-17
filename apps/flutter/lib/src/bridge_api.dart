/// Plain Dart mirrors of the bridge DTOs (decoupled from FRB types so the
/// UI and tests do not import generated bindings).
library;

import 'dart:typed_data';

part 'bridge/capability_types.dart';
part 'bridge/connection_types.dart';
part 'bridge/host_session_types.dart';
part 'bridge/native_session_types.dart';
part 'bridge/hosting_types.dart';
part 'bridge/provider_types.dart';
part 'bridge/thread_types.dart';
part 'bridge/session_access_types.dart';
part 'bridge/management_types.dart';
part 'bridge/device_metadata.dart';

abstract interface class BridgeApi {
  /// Read only local disk; cached history is display-only until synchronized.
  Future<ThreadHistory?> appHistoryCached(String serviceKey, String threadId);
  Future<bool> appHistorySyncPrepare(String serviceKey);
  Future<void> appHistoryPrefetch(String serviceKey, String threadId);
  Future<void> appHistoryFocus(String serviceKey, String? threadId);
  Future<HistoryCacheStatus> historyCacheStatus();
  Future<void> historyCacheSetLimit(int limitMb);

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

  /// This app's publisher identity, available before hosting starts.
  DeviceMetadata localDeviceMetadata();

  /// Host-provided metadata; null means an older host without the route.
  Future<DeviceMetadata?> serviceDeviceMetadata(String serviceKey);

  /// Subscribe to an API service, exposing it on `127.0.0.1:<localPort>`.
  Future<SubInfo> apiSubscribe(String serviceKey, int localPort);

  /// Stop an API-service subscription.
  Future<void> apiUnsubscribe(String serviceKey);

  /// List all active subscriptions.
  Future<List<SubInfo>> subscriptions();

  /// Persist the UI locale (BCP-47, e.g. `en`/`zh`). An empty string clears
  /// it, meaning follow the system locale.
  Future<void> setLocale(String locale);

  // --- external Codex bootstrap: provider setup, ChatGPT login, system prompt ---

  /// Detect whether the external Codex has a usable provider + credentials. Drives
  /// the first-run setup wizard: `needsSetup` is true when neither a login nor a
  /// custom provider is configured.
  Future<CodexSetupStatus> codexSetupStatus();

  /// Configure a minimal custom OpenAI-compatible provider (base URL + API key)
  /// for the external Codex — writes `$CODEX_HOME/config.toml`, no login needed.
  /// [model] is optional (a sensible default is used when null/blank).
  Future<void> codexSetupProvider({
    required String baseUrl,
    required String apiKey,
    String? model,
  });

  /// The active external-Codex system-prompt variant (`default` / `non_degraded` /
  /// `custom`).
  Future<String> codexPromptVariant();

  /// Switch the external-Codex system prompt. `non_degraded` drops the commentary /
  /// intermediary-update mandates that can starve reasoning (openai/codex#30364);
  /// `default` restores codex's built-in prompt.
  Future<void> codexSetPromptVariant(String variant);

  // --- Hosted account (GitHub device-flow login) ---

  /// Begin a GitHub device-flow login. [backend] overrides the configured /
  /// default backend (remembered on success). Show the returned code + URL,
  /// then poll with [accountLoginPoll].
  Future<DeviceCode> accountLoginStart({String? backend});

  /// Poll a device flow once. On `authorized` the session is persisted and the
  /// app switches to account mode.
  Future<AccountPoll> accountLoginPoll(String pollHandle, String backend);

  /// Begin a web (browser-redirect) GitHub login — the convenient default.
  /// [redirectUri] is the platform-specific callback the browser is sent back to
  /// (the app's `pocketcodex://` scheme on mobile/macOS, a `http://localhost:…`
  /// loopback on Windows/Linux). [backend] overrides the configured / default
  /// backend (remembered on a successful exchange). Open the returned
  /// `authorizeUrl`, then call [accountWebLoginExchange].
  Future<WebLoginStart> accountWebLoginStart({
    required String redirectUri,
    String? backend,
  });

  /// Redeem the one-time [exchangeCode] from the browser redirect (with its PKCE
  /// [codeVerifier]) for a session. On success the session is persisted and the
  /// app switches to account mode; returns the signed-in identity.
  Future<AccountUser> accountWebLoginExchange({
    required String exchangeCode,
    required String codeVerifier,
    required String backend,
  });

  /// Sign out: revoke the refresh token (best effort) and clear the session.
  Future<void> accountLogout();

  /// Deregister one of the account's services from the relay (best-effort; a
  /// still-running host re-registers shortly after). [kind] is 'app' or 'api'.
  Future<void> accountDeregisterService({
    required String device,
    required String kind,
    required String name,
  });

  // --- Local hosting (desktop): run a local codex app-server + API proxy ---

  /// Start hosting under the signed-in account. Spawns (or reuses) codex on
  /// `127.0.0.1:<port>` and an in-app Responses API proxy, publishing both
  /// `app:<name>` and `api:<name>`. [binaryOverride] points at the codex binary
  /// when it isn't on `PATH` (remembered for next time); [proxy] is the upstream
  /// proxy both use to reach chatgpt.com (`null` = inherit env). Re-hosting a
  /// live name just re-registers any dropped tunnels. Desktop only.
  Future<AppServeResult> appServeStart({
    required int port,
    String? binaryOverride,
    String? name,
    String? proxy,
    required bool embedded,
  });

  /// Snapshot of every local host (for the status cards + periodic re-probe).
  Future<List<AppServeStatus>> appServeStatus();

  /// Take one tunnel ([kind] = 'app' or 'api') of a local host off the relay
  /// without stopping the host — a reversible unpublish. The codex / API proxy
  /// keep running; [appServeReregister] re-publishes it instantly.
  Future<void> appServeDeregister({required String name, required String kind});

  /// Re-publish a previously deregistered tunnel ([kind] = 'app'/'api') of a
  /// still-running local host.
  Future<void> appServeReregister({required String name, required String kind});

  /// Fully stop one local host by name (both tunnels + watchdog + API proxy, and
  /// stops codex).
  Future<void> appServeStop(String name);

  /// Stop every local host (called on app quit so a real quit leaves no orphan).
  Future<void> appServeStopAll();

  /// The resolved codex binary path (persisted config → `PATH`), or `null` so
  /// the UI can prompt the user to point at one.
  Future<String?> codexLocate();

  /// Capabilities of the connection behind [serviceKey] (no network). Codex
  /// is fixed; ACP reflects the host's current negotiation and
  /// is conservative until connected.
  AppCapabilities appCapabilities(String serviceKey);

  /// Ids of sessions executing now, from the engine itself (ACP).
  Future<List<String>> appRunningThreads(String serviceKey);

  /// Built-in ACP agent presets.
  List<AcpPreset> acpPresets();

  /// Detect the OpenCode executable ([binary] null: `PATH`) and its
  /// version; nothing is started or changed.
  Future<OpenCodeEnvironment> openCodeDetect({String? binary});

  /// Re-read the running OpenCode host [name]'s native catalog (providers,
  /// models, agents) into its status; a failure is kept as the status
  /// problem. Nothing is configured or restarted.
  Future<void> openCodeRefresh({required String name});

  /// Whether this device can host ACP agents (Unix desktops).
  bool acpHostingSupported();

  /// The executable [program] resolves to on this device, or `null`.
  Future<String?> acpLocate(String program, {String? profileId});

  /// Detect the Claude preset's adapter, Node.js and native CLI (nothing is
  /// installed or changed).
  Future<AcpClaudeStatus> acpClaudeStatus();

  /// Start preparing Claude's pinned ACP adapter in the app's cache, or join
  /// the preparation already running.
  Future<AcpClaudeSetup> acpClaudePrepareStart();

  /// Where the adapter preparation is.
  AcpClaudeSetup acpClaudePrepareState();

  /// Cancel preparation run [run] only (the one its caller started); a later
  /// run started by another caller keeps going.
  AcpClaudeSetup acpClaudePrepareCancelRun(int run);

  /// Launch and host an ACP agent as `acp:<name>` plus `meta:<name>`.
  /// Stopping hosting stops the agent.
  Future<AcpServeResult> appServeStartAcp({
    String? name,
    required AcpAgentSpec spec,
  });

  /// Restart the agent of a local ACP host.
  Future<void> appServeRestartAcp(String name);

  // --- Per-host model service ---

  /// The saved configuration of host [name] (inherited when never saved).
  Future<HostConfig> hostConfigGet(String name);

  /// What a Codex host ([spec] and [engine] null), the ACP agent [spec] or
  /// the engine [engine] (`opencode`) can select.
  HostConfigSupport hostConfigSupported({AcpAgentSpec? spec, String? engine});

  /// Save host [name]'s configuration for a Codex host ([spec] null) or the
  /// ACP agent [spec], and with [runtime] its desired runtime (an agent
  /// runtime is checked as that agent). [key] is required for
  /// [HostKeyAction.replace]. With [expectedRevision], a save made elsewhere
  /// meanwhile is refused. A running host keeps its configuration until
  /// [appServeApply].
  Future<HostConfig> hostConfigSave({
    required String name,
    required HostConfigInput input,
    required HostKeyAction keyAction,
    String? key,
    int? expectedRevision,
    AcpAgentSpec? spec,
    HostRuntime? runtime,
  });

  /// Restart running host [name] with its saved configuration and runtime.
  /// [expectedIncarnation] ([HostActive.incarnation]) and [expectedRevision]
  /// ([HostConfig.revision]) name what was reviewed: a replaced host or a
  /// newer save is refused. A failed start restores the previous one exactly.
  Future<HostApplyResult> appServeApply(
    String name, {
    String? expectedIncarnation,
    int? expectedRevision,
  });

  /// Save and start a new host [name] with [runtime] and this configuration
  /// as one operation; a failed start puts the previous saved configuration
  /// back. Refused when the name is already hosting.
  Future<HostStartResult> appServeStartConfigured({
    String? name,
    required HostRuntime runtime,
    required HostConfigInput input,
    required HostKeyAction keyAction,
    String? key,
  });

  /// Start a private loopback codex for one official sign-in (never on the
  /// relay), with [binary] (null: the saved codex, then `PATH`) and [proxy];
  /// returns its session id.
  Future<String> codexLocalSessionOpen({String? binary, String? proxy});

  /// Stop the private sign-in codex [sessionId].
  Future<void> codexLocalSessionClose(String sessionId);

  /// Begin the official ChatGPT login on the private sign-in codex.
  Future<CodexLoginStart> codexLocalLoginStart(String sessionId);

  /// The private sign-in codex's account and, with [loginId], how that login
  /// ended ([CodexAuthStatus.completed]).
  Future<CodexAuthStatus> codexLocalAuthStatus(
    String sessionId, {
    String? loginId,
  });

  /// Cancel login [loginId] on the private sign-in codex.
  Future<void> codexLocalLoginCancel(String sessionId, String loginId);

  /// Sign codex out through the private sign-in codex.
  Future<void> codexLocalLogout(String sessionId);

  /// The ACP agent's latest per-session state (no network), or `null`.
  SessionSettings? appSessionSettings(String serviceKey, String threadId);

  /// Session [threadId]'s current state, read again from the host where the
  /// engine has to ask it (native OpenCode reads its catalog over HTTP), or
  /// `null`. Other engines answer as [appSessionSettings].
  Future<SessionSettings?> appSessionSettingsRefresh(
    String serviceKey,
    String threadId,
  );

  /// The options a new session in [directory] would start with, for an
  /// engine whose options exist before its session
  /// ([AppCapabilities.draftOptions]); `null` when there are none. Native
  /// OpenCode reads that project's catalog; other engines answer as
  /// [appSessionSettings] with an empty thread id.
  Future<SessionSettings?> appDraftSessionSettings(
    String serviceKey,
    String? directory,
  );

  /// Set an ACP select option to one of its advertised values.
  Future<void> appSetSessionConfig(
    String serviceKey,
    String threadId,
    String configId,
    String value,
  );

  /// Switch an ACP session's legacy mode.
  Future<void> appSetSessionMode(
    String serviceKey,
    String threadId,
    String modeId,
  );

  // --- App-server remote control ---

  /// Connect to a session service, dispatched by its key's protocol: a Codex
  /// app-server (subscribe on `127.0.0.1:<localPort>`, open the JSON-RPC
  /// websocket, run `initialize`) or an ACP gateway (`/acp/v1` snapshot, then
  /// its event stream). Pass `localPort: 0` to let the bridge assign a free
  /// OS port per service (so several services can coexist).
  Future<void> appConnect(String serviceKey, int localPort);

  /// Whether a live app-server session exists for [serviceKey].
  bool appIsConnected(String serviceKey);

  /// Disconnect the app-server session and its pb-mapper subscription.
  Future<void> appDisconnect(String serviceKey);

  /// Probe whether a session service is actually REACHABLE (a Codex
  /// app-server answers `initialize`; an ACP gateway reports a ready agent)
  /// rather than merely registered on the relay, dispatched by the key's
  /// protocol. The services list uses this so a registered-but-dead host
  /// shows as unreachable instead of a false "online". A live session
  /// short-circuits to true.
  Future<bool> appProbe(String serviceKey);

  /// Why [appProbe] found a service unreachable, or null when it is reachable.
  ///
  /// A bare false can only be reported as "the app-server didn't respond", which
  /// is misleading when the tunnel answered and REFUSED the handshake — a relay
  /// rejecting a missing or stale authentication code needs a different fix from
  /// a dead backend, and the user can't tell them apart otherwise.
  Future<String?> appProbeReason(String serviceKey);

  /// Probe whether an API proxy is actually reachable — its host answers a
  /// minimal HTTP request — rather than merely registered on the relay, so a
  /// dead-but-registered proxy reads unreachable instead of a false "online".
  Future<bool> apiProbe(String serviceKey);

  /// Health-check an app-server THIS machine hosts itself, by its loopback
  /// app-listen address (e.g. `127.0.0.1:18080`). A direct `initialize`
  /// handshake with no relay hop, so — unlike a bare port-open check — a wedged
  /// or half-open codex (still accepting sockets but never answering RPC) reads
  /// false instead of a misleading "running". Fast because it stays on loopback.
  Future<bool> appProbeLocal(String localAddr);

  /// Health-check an API proxy THIS machine hosts itself, by its loopback
  /// api-listen address — a direct minimal HTTP request, no relay hop, so a
  /// local host's API tunnel reads "online" the instant its proxy is up rather
  /// than after a slower transient relay round-trip.
  Future<bool> apiProbeLocal(String localAddr);

  /// Live event stream for [serviceKey] (turn/item notifications).
  Stream<AppEvent> appEvents(String serviceKey);

  /// Upstream realtime controls and marked voice thread creation.
  Future<String> appRealtimeRequest(
    String serviceKey,
    String method,
    String paramsJson,
  );

  /// Captured `tracing` events for the in-app log viewer: retained recent
  /// history (oldest first) followed by every new event live.
  Stream<LogLine> logEvents();

  /// Retry progress for host meta requests. Notifications only — the request's
  /// own result still arrives through whichever call was made; this exists so a
  /// wait can say "retrying 2/10" instead of looking frozen.
  Stream<RetryProgress> metaRetryEvents();

  /// List threads known to the app-server.
  Future<List<ThreadMeta>> appThreadList(String serviceKey);

  /// Inspect metadata before deciding whether a thread can be controlled.
  Future<ThreadMeta?> appThreadMetadata(String serviceKey, String threadId);

  /// List the models the app-server offers.
  Future<List<ModelInfo>> appModelList(String serviceKey);

  /// The models offered in project [directory]. Native OpenCode reads that
  /// project's catalog; other engines answer as [appModelList].
  Future<List<ModelInfo>> appModelListForProject(
    String serviceKey,
    String? directory,
  );

  /// Read the account rate-limit / quota snapshot as raw JSON (5h + weekly
  /// windows). Parsed on the Dart side since the shape is nested and volatile.
  Future<String> appRateLimits(String serviceKey);

  /// Unified diff of the repo at [cwd] vs its remote default branch. Empty when
  /// the cwd isn't a git repo or there are no changes.
  Future<String> appGitDiff(String serviceKey, String cwd);

  /// Start a manual conversation compaction; the server emits a
  /// `thread/compacted` event when done.
  Future<void> appCompact(String serviceKey, String threadId);

  /// A one-line gist of where a thread got to (the first sentence of its most
  /// recent agent reply), or null when it has produced none.
  ///
  /// Its own call because `thread/list` carries no summary — the only source is
  /// a full thread read, so callers fetch these lazily per visible row rather
  /// than for every conversation.
  Future<String?> appThreadSummary(String serviceKey, String threadId);

  /// Rename a conversation. The app-server persists the title, so it follows
  /// the thread rather than the device; an empty [name] clears it and the UI
  /// falls back to the thread preview.
  Future<void> appSetThreadName(
    String serviceKey,
    String threadId,
    String name,
  );

  /// Start a new thread / project. [approvalPolicy] is one of
  /// `untrusted`/`on-failure`/`on-request`/`never`; [sandbox] is one of
  /// `read-only`/`workspace-write`/`danger-full-access`. Returns the id.
  Future<String> appThreadStart(
    String serviceKey, {
    String? model,
    String? cwd,
    String? approvalPolicy,
    String? approvalsReviewer,
    String? serviceTier,
    String? sandbox,
  });

  /// Resume an existing thread (load it into the session) before reading it or
  /// sending turns; otherwise the server reports "thread not found".
  Future<void> appThreadResume(String serviceKey, String threadId);

  /// Read a thread's history (items oldest first) and whether a turn is still
  /// running, so re-opening an in-flight thread restores its live state.
  ///
  /// A paginated thread returns only its newest turns' items, with
  /// [ThreadHistory.hasOlder] set and [ThreadHistory.turns] naming every turn.
  Future<ThreadHistory> appThreadRead(
    String serviceKey,
    String threadId, {
    bool includeTurnPages = true,
  });

  /// One page further back through a paginated thread's history. Returns an
  /// empty page when the thread reads whole or is already at its start.
  Future<OlderPage> appThreadOlderPage(String serviceKey, String threadId);

  /// Read a selected turn's cached prefix or continue its ascending cursor.
  Future<TurnItemsPage> appThreadTurnPage(
    String serviceKey,
    String threadId,
    String turnId, {
    bool loadMore = false,
  });

  /// The latest server-reported runtime config for a thread (from its
  /// start/resume response, kept fresh by `thread/settings/updated`
  /// notifications), or null when the server hasn't reported any. Reads the
  /// bridge's cache only — no RPC — so it's cheap to call after a send.
  ThreadRuntimeConfig? appThreadRuntimeConfig(
    String serviceKey,
    String threadId,
  );

  /// Send a user message (text and/or attached images), starting a model
  /// turn. [images] are `data:image/...;base64,...` URLs — the wire form that
  /// reaches BOTH local and relay-tunneled remote app-servers; [text] may be
  /// empty when at least one image is attached. [model] / [approvalPolicy] /
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
    List<String> images = const [],
    String? model,
    String? approvalPolicy,
    String? approvalsReviewer,
    String? serviceTier,
    String? sandbox,
    String? collaborationMode,
    String? reasoningEffort,
  });

  /// Send an asynchronous answer and return the accepted active turn ID.
  Future<String> appTurnSteer(
    String serviceKey,
    String threadId,
    String? turnId,
    String text, {
    List<String> images = const [],
  });

  /// Add input to the running turn [turnId] (Codex `turn/steer`, or an ACP
  /// agent's negotiated steering). An ACP agent with no turn running
  /// consumes nothing and says so (Codex fails instead); an error means it
  /// was refused or its fate is unknown, and it must not be retried or
  /// resent as a prompt.
  Future<TurnSupplement> appTurnSupplement(
    String serviceKey,
    String threadId,
    String? turnId,
    String text, {
    List<String> images = const [],
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

  /// Answer an `item/tool/requestUserInput` elicitation — the model asking
  /// structured questions (NOT a command/file approval). [requestId] is from the
  /// `item/tool/requestUserInput` [AppEvent]; [answersJson] is a JSON object
  /// mapping each question id to its chosen answer string(s)
  /// (e.g. `{"theme":["山水抒怀"]}`); an empty object `{}` cancels.
  Future<void> appRespondUserInput(
    String serviceKey,
    String requestId,
    String answersJson,
  );

  // --- Local session takeover (shared CODEX_HOME) ---

  /// List every codex session under the shared `CODEX_HOME`, newest first,
  /// each annotated with whether it is safe to resume. Reads local disk +
  /// the process table; no app-server connection required.
  Future<List<LocalSession>> appLocalSessions();

  /// Read provider-native files on this computer without starting an agent.
  Future<NativeSessionList> appNativeSessions(
    String provider, {
    String? cursor,
    String? sourceId,
  });
  Future<NativeSessionPage> appNativeSessionPage(
    String provider,
    String threadId, {
    String? before,
    String? sourceId,
  });
  Future<NativeSessionRevision> appNativeSessionRevision(
    String provider,
    String threadId, {
    String? sourceId,
  });

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

  // --- Remote (meta service) sessions + per-thread config ---
  //
  // The same inventory / transcript / force-resume as the `appLocal*` methods,
  // but for a (possibly remote) host reached over its `meta:` tunnel. Each
  // takes the app-server [serviceKey] being viewed; the meta key is derived
  // internally. Lets a phone view + resume a desktop host's sessions, and
  // persists per-thread config on the host (shared across devices).

  /// Remote analogue of [appLocalSessions] for the host behind [serviceKey].
  /// [runningOnly] requests a lightweight inventory of active writers.
  Future<List<LocalSession>> metaSessions(
    String serviceKey, {
    bool runningOnly = false,
  });

  /// Remote analogue of [appSessionLiveness].
  Future<SessionLiveness> metaSessionLiveness(
    String serviceKey,
    String threadId,
  );

  /// Live read-only transcript + ownership updates for a session held by
  /// another app-server. The stream's first event is a complete snapshot.
  Stream<SessionFollowUpdate> metaSessionEvents(
    String serviceKey,
    String threadId,
  );

  /// Remote analogue of [appLocalSessionTranscript].
  Future<List<ThreadItem>> metaSessionTranscript(
    String serviceKey,
    String threadId,
  );

  /// Remote analogue of [appForceResume]: the host evicts the rollout's holders
  /// and resumes it into its colocated app-server. Gate on explicit
  /// confirmation; do not call while a turn is actively running.
  Future<ForceResumeReport> metaForceResume(String serviceKey, String threadId);

  /// Open session [threadId] of a host-access engine (ACP, OpenCode) for
  /// writing when that is safe now: a released original session is acquired
  /// and loaded in this call. Never forks, copies or creates a session; a
  /// refused open answers follow/blocked access instead of failing.
  Future<SessionAccess> appSessionOpen(String serviceKey, String threadId);

  /// Where this controller stands with [threadId] now, with no side effects.
  Future<SessionAccess> appSessionAccess(String serviceKey, String threadId);

  /// Hand this host's idle writer of [snapshot]'s session back to other
  /// clients; a replaced host or generation is refused.
  Future<SessionAccess> appSessionRelease(
    String serviceKey,
    SessionAccess snapshot,
  );

  /// Native slash commands of the engine behind [serviceKey]
  /// ([AppCapabilities.commands]) offered in project [directory].
  Future<List<AgentCommand>> appCommandListForProject(
    String serviceKey,
    String? directory,
  );

  /// Capture the current destination context before reading attachment bytes.
  String metaUploadContext();

  /// Reject a context change before transmitting attachment bytes.
  Future<String> metaUploadFileScoped(
    String serviceKey,
    String fileName,
    Uint8List bytes,
    String context,
  );

  /// Read a thread's persisted config from the host behind [serviceKey]
  /// (all-null when none stored).
  Future<ThreadConfig> metaThreadConfigGet(String serviceKey, String threadId);

  /// Persist a thread's config on the host behind [serviceKey]; returns the
  /// stored value.
  Future<ThreadConfig> metaThreadConfigSet(
    String serviceKey,
    String threadId,
    ThreadConfig config,
  );

  /// Read the project-folder config (roots + default) of the host behind
  /// [serviceKey] — what a new session's folder browser starts from.
  Future<ProjectConfig> metaProjectConfig(String serviceKey);

  /// Read the project settings, including the effective (possibly automatic)
  /// default folder. Never creates the folder. Works on `meta:` and `host:`
  /// keys; a legacy host answers with a null revision.
  Future<ProjectSettings> metaProjectSettings(String serviceKey);

  /// Save reviewed project settings; new hosts require [expectedRevision].
  /// A null [defaultProject] restores the automatic folder.
  Future<ProjectSettings> metaSaveProjectSettings(
    String serviceKey, {
    required List<String> projectRoots,
    String? defaultProject,
    String? expectedRevision,
  });

  // --- remote device management (kind `host`) ---

  /// This computer's own remote-management switch.
  Future<DeviceManagement> deviceManagementStatus();

  /// Enable or disable remote management of this computer only.
  Future<DeviceManagement> deviceManagementSetEnabled(bool enabled);

  /// Authoritative boot-restore choices; a failure must block restore.
  Future<List<HostRestoreOverride>> hostRestoreOverrides();

  /// Restore one host under revalidation. Pass the override's revision, or
  /// (for a legacy preference with no override) null and its runtime.
  Future<void> hostRestoreService(
    String name, {
    int? expectedRevision,
    HostRuntime? legacyRuntime,
  });

  /// A restart-persistence warning for [name] (any host when null), separate
  /// from whether its start/apply succeeded.
  Future<String?> hostRestoreProblem({String? name});

  /// Read a remote computer's management view.
  Future<ManagedDevice> managedDevice(String serviceKey);

  /// Create and start a remote service. A lost response needs a refresh,
  /// never an automatic retry.
  Future<ManagedHost> managedHostStart(
    String serviceKey, {
    required String name,
    required ManagedRuntime runtime,
    required HostConfigInput input,
    required HostKeyAction keyAction,
    String? key,
  });

  /// Save a reviewed revision without touching the live service.
  Future<ManagedHost> managedHostSave(
    String serviceKey,
    String name,
    ManagedHostEdit edit,
  );

  /// Apply the reviewed revision to the observed incarnation.
  Future<HostApplyResult> managedHostApply(
    String serviceKey,
    String name, {
    required String expectedIncarnation,
    required int expectedRevision,
  });

  /// Stop the observed incarnation; the computer stays manageable.
  Future<void> managedHostStop(
    String serviceKey,
    String name, {
    required String expectedIncarnation,
  });

  /// Start a stopped service from its reviewed saved revision.
  Future<ManagedHost> managedHostStartSaved(
    String serviceKey,
    String name, {
    required int expectedRevision,
  });

  /// Runtimes installed on the remote computer. Never installs or signs in.
  Future<List<ManagedRuntimeAvailability>> managedRuntimes(String serviceKey);

  /// Read (or, with [start], explicitly begin) Claude adapter preparation on
  /// the remote computer.
  Future<AcpClaudeSetup> managedClaudePrepare(
    String serviceKey, {
    required bool start,
  });

  /// List the sub-directories of [path] on the host behind [serviceKey], for
  /// the remote project-folder browser. Throws if [path] is outside the host's
  /// configured project roots.
  Future<List<HostDirEntry>> metaListDir(String serviceKey, String path);

  /// List the files (not sub-directories) in [path] on the host behind
  /// [serviceKey], for the file-transfer panel. Throws if [path] is outside the
  /// host's configured project roots.
  Future<List<HostFileEntry>> metaListFiles(String serviceKey, String path);

  /// Download a host file's raw bytes (root-confined) from the host behind
  /// [serviceKey], for saving to local disk. Throws if [path] is outside the
  /// configured roots.
  Future<Uint8List> metaReadFile(String serviceKey, String path);

  /// Verify that the selected host shares this device's filesystem.
  Future<bool> metaHostIsLocal(String serviceKey);

  /// Read up to 8 MiB after an explicit Preview action.
  Future<FilePreviewData> metaFilePreview(
    String serviceKey,
    String? threadId,
    String href,
  );

  /// Stream into a new staging file on this device after a Download action.
  Future<void> metaFileDownload(
    String serviceKey,
    String? threadId,
    String href,
    String destination,
  );

  /// Read an image that [threadId]'s transcript already references, so it can
  /// render inline. Unlike [metaReadFile] this is not root-confined — the host
  /// authorises user attachments and typed generated artifacts, which is what
  /// makes a pasted screenshot in the OS temp directory visible to a remote
  /// controller without granting it a general file read. Throws for a path the
  /// transcript never mentioned, and on a host too old to serve the route.
  Future<Uint8List> metaReadThreadImage(
    String serviceKey,
    String threadId,
    String path,
  );

  /// Upload local [bytes] as [fileName] into host directory [dir]
  /// (root-confined) on the host behind [serviceKey]; returns the absolute HOST
  /// path where it landed. Throws if [dir] is outside the roots, or on a name
  /// collision (the host never overwrites).
  Future<String> metaWriteFile(
    String serviceKey,
    String dir,
    String fileName,
    Uint8List bytes,
  );
}
