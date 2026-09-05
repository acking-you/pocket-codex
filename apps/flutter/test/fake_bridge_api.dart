import 'dart:async';
import 'dart:typed_data';

import 'package:pocket_codex/src/bridge_api.dart';

/// In-memory [BridgeApi] for widget/provider tests. Seed [config] and
/// services per test; records subscribe/unsubscribe calls.
class FakeBridgeApi implements BridgeApi {
  /// Creates a fake seeded with an optional [config] and [services].
  FakeBridgeApi({ConfigInfo? config, List<ServiceEntry>? services})
    : _config = config ?? const ConfigInfo(relay: null, hasKey: false),
      _services = List.of(services ?? const []);

  ConfigInfo _config;
  final List<ServiceEntry> _services;
  final Map<String, SubInfo> _subs = {};

  // App-server session simulation.
  final Set<String> _appConnected = {};
  final Map<String, StreamController<AppEvent>> _appEvents = {};
  final List<ThreadMeta> appThreads = [];
  int _threadSeq = 0;

  /// Make [discoverServices] throw, to exercise error states.
  Object? discoverError;

  /// Mutable view of the discovery list, so a test can make a service appear
  /// (or vanish) after construction — e.g. the home screen's auto-retry.
  List<ServiceEntry> get services => _services;

  @override
  Future<ConfigInfo> getConfig() async => _config;

  @override
  Future<void> setRelay(String relay) async => _config = ConfigInfo(
    relay: relay,
    hasKey: _config.hasKey,
    locale: _config.locale,
  );

  @override
  Future<void> setKey(String key) async {
    if (key.length != 32) throw ArgumentError('key must be 32 bytes');
    _config = ConfigInfo(
      relay: _config.relay,
      hasKey: true,
      locale: _config.locale,
    );
  }

  @override
  Future<String> importConfig(String text) async {
    if (!text.startsWith('pcx1:')) {
      throw const FormatException('not a pcx1 string');
    }
    _config = const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true);
    return 'lb7666.top:7666';
  }

  @override
  Future<String> exportConfig() async => 'pcx1:ZmFrZQ';

  @override
  Future<List<ServiceEntry>> discoverServices() async {
    if (discoverError != null) throw discoverError!;
    return _services;
  }

  @override
  Future<SubInfo> apiSubscribe(String serviceKey, int localPort) async {
    final s = SubInfo(
      key: serviceKey,
      localAddr: '127.0.0.1:$localPort',
      alive: true,
    );
    _subs[serviceKey] = s;
    return s;
  }

  @override
  Future<void> apiUnsubscribe(String serviceKey) async =>
      _subs.remove(serviceKey);

  @override
  Future<List<SubInfo>> subscriptions() async => _subs.values.toList();

  @override
  Future<void> setLocale(String locale) async => _config = ConfigInfo(
    relay: _config.relay,
    hasKey: _config.hasKey,
    locale: locale.isEmpty ? null : locale,
  );

  // --- 自带 codex bootstrap ---

  /// Seedable codex setup status returned by [codexSetupStatus].
  CodexSetupStatus codexStatus = const CodexSetupStatus(
    codexHome: '/fake/.codex',
    hasConfig: false,
    hasAuth: false,
    hasCustomProvider: false,
    needsSetup: true,
    promptVariant: 'default',
  );

  /// Records the last provider config written via [codexSetupProvider].
  ({String baseUrl, String apiKey, String? model})? lastProvider;

  /// Seedable auth status returned by [codexAuthStatus].
  CodexAuthStatus codexAuth = const CodexAuthStatus(authenticated: false);

  @override
  Future<CodexSetupStatus> codexSetupStatus() async => codexStatus;

  @override
  Future<void> codexSetupProvider({
    required String baseUrl,
    required String apiKey,
    String? model,
  }) async {
    lastProvider = (baseUrl: baseUrl, apiKey: apiKey, model: model);
    codexStatus = CodexSetupStatus(
      codexHome: codexStatus.codexHome,
      hasConfig: true,
      hasAuth: codexStatus.hasAuth,
      hasCustomProvider: true,
      authMode: codexStatus.authMode,
      needsSetup: false,
      promptVariant: codexStatus.promptVariant,
    );
  }

  @override
  Future<String> codexPromptVariant() async => codexStatus.promptVariant;

  @override
  Future<void> codexSetPromptVariant(String variant) async {
    codexStatus = CodexSetupStatus(
      codexHome: codexStatus.codexHome,
      hasConfig: codexStatus.hasConfig,
      hasAuth: codexStatus.hasAuth,
      hasCustomProvider: codexStatus.hasCustomProvider,
      authMode: codexStatus.authMode,
      needsSetup: codexStatus.needsSetup,
      promptVariant: variant,
    );
  }

  @override
  Future<CodexLoginStart> codexLoginChatgptStart(String serviceKey) async =>
      const CodexLoginStart(
        mode: 'browser',
        loginId: 'fake-login',
        authUrl: 'https://auth.openai.com/fake',
      );

  @override
  Future<CodexAuthStatus> codexAuthStatus(String serviceKey) async => codexAuth;

  @override
  Future<void> codexLoginCancel(String serviceKey, String loginId) async {}

  @override
  Future<void> codexLogout(String serviceKey) async {}

  // --- Hosted account ---

  /// Seedable signed-in user returned by [accountCurrentUser] (null = signed out).
  AccountUser? accountUser;

  /// Seedable services returned by [accountServices].
  List<AccountService> accountServiceList = const [];

  /// Status [accountLoginPoll] returns (default `authorized`).
  String accountPollStatus = 'authorized';

  @override
  Future<DeviceCode> accountLoginStart({String? backend}) async => DeviceCode(
    userCode: 'ABCD-1234',
    verificationUri: 'https://github.com/login/device',
    pollHandle: 'handle',
    intervalSecs: 0,
    expiresInSecs: 900,
    backend: backend ?? 'https://backend.example',
  );

  @override
  Future<AccountPoll> accountLoginPoll(
    String pollHandle,
    String backend,
  ) async {
    if (accountPollStatus == 'authorized') {
      accountUser = const AccountUser(login: 'octocat', accountId: '42');
      _signIn(accountUser!);
    }
    return AccountPoll(
      status: accountPollStatus,
      login: accountUser?.login,
      accountId: accountUser?.accountId,
    );
  }

  /// Mirror what Rust does on a successful login: persist the token, which flips
  /// `mode` to `account`. Without this the fake left `mode: unconfigured`, so a
  /// test could never catch the UI reading a stale config after signing in.
  void _signIn(AccountUser user) {
    _config = ConfigInfo(
      relay: _config.relay,
      hasKey: _config.hasKey,
      locale: _config.locale,
      mode: 'account',
      accountLogin: user.login,
      hasAccountToken: true,
    );
  }

  /// Records the last [accountWebLoginStart] redirectUri for assertions.
  String? lastWebRedirectUri;

  @override
  Future<WebLoginStart> accountWebLoginStart({
    required String redirectUri,
    String? backend,
  }) async {
    lastWebRedirectUri = redirectUri;
    return WebLoginStart(
      authorizeUrl: 'https://github.com/login/oauth/authorize?state=s',
      state: 'fake-state',
      codeVerifier: 'fake-verifier',
      backend: backend ?? 'https://backend.example',
    );
  }

  @override
  Future<AccountUser> accountWebLoginExchange({
    required String exchangeCode,
    required String codeVerifier,
    required String backend,
  }) async {
    accountUser = const AccountUser(login: 'octocat', accountId: '42');
    _signIn(accountUser!);
    return accountUser!;
  }

  @override
  Future<AccountUser?> accountCurrentUser() async => accountUser;

  @override
  Future<void> accountLogout() async {
    accountUser = null;
    // The token goes with it, so the mode derives back off `account`.
    _config = ConfigInfo(
      relay: _config.relay,
      hasKey: _config.hasKey,
      locale: _config.locale,
      mode: _config.relay == null ? 'unconfigured' : 'self_host',
    );
  }

  @override
  Future<List<AccountService>> accountServices() async => accountServiceList;

  /// Records the last [accountDeregisterService] call for assertions.
  String? lastDeregistered;

  /// Every key passed to [accountDeregisterService], in order — so a batch
  /// removal can assert it dropped each selected key.
  final List<String> deregistered = [];

  /// When true, [accountDeregisterService] records the call but does NOT drop
  /// the entry from discovery — mirroring an orphaned/hollow relay key the
  /// backend can't force off. Lets tests exercise the durable client-side
  /// dismiss (which must hide it anyway).
  bool keepOnDeregister = false;

  @override
  Future<void> accountDeregisterService({
    required String device,
    required String kind,
    required String name,
  }) async {
    lastDeregistered = 'pcx:$device:$kind:$name';
    deregistered.add(lastDeregistered!);
    if (keepOnDeregister) return;
    _services.removeWhere(
      (s) => s.device == device && s.kind == kind && s.name == name,
    );
  }

  // --- Local hosting ---

  /// Live local hosts, keyed by name (mirrors the bridge's per-name map).
  final List<AppServeStatus> serveHosts = [];

  /// Records the last [appServeStart] args for assertions.
  int? lastServePort;
  String? lastServeBinary, lastServeName, lastServeProxy;
  bool? lastServeEmbedded;

  /// Path returned by [codexLocate] (set null to simulate "codex not found").
  String? codexPath = '/usr/local/bin/codex';

  @override
  Future<AppServeResult> appServeStart({
    required int port,
    String? binaryOverride,
    String? name,
    String? proxy,
    required bool embedded,
  }) async {
    lastServePort = port;
    lastServeBinary = binaryOverride;
    lastServeName = name;
    lastServeProxy = proxy;
    lastServeEmbedded = embedded;
    final n = name ?? 'default';
    const device = 'local';
    final appKey = 'pcx:$device:app:$n';
    final apiKey = 'pcx:$device:api:$n';
    final metaKey = 'pcx:$device:meta:$n';
    serveHosts
      ..removeWhere((h) => h.name == n)
      ..add(
        AppServeStatus(
          name: n,
          device: device,
          pid: 4242,
          alive: true,
          appListenAddr: '127.0.0.1:$port',
          appServiceKey: appKey,
          appRegistered: true,
          apiListenAddr: '127.0.0.1:18080',
          apiServiceKey: apiKey,
          apiRegistered: true,
          metaListenAddr: '127.0.0.1:18090',
          metaServiceKey: metaKey,
          metaRegistered: true,
        ),
      );
    // Hosting registers on the relay, so discovery finds the new tunnels —
    // mirror that (idempotently) for flows that re-discover after a start.
    if (!_services.any((s) => s.key == appKey)) {
      _services.add(
        ServiceEntry(device: device, kind: 'app', name: n, key: appKey),
      );
    }
    if (!_services.any((s) => s.key == apiKey)) {
      _services.add(
        ServiceEntry(device: device, kind: 'api', name: n, key: apiKey),
      );
    }
    return AppServeResult(
      device: device,
      name: n,
      appServiceKey: appKey,
      appListenAddr: '127.0.0.1:$port',
      apiServiceKey: apiKey,
      apiListenAddr: '127.0.0.1:18080',
      metaServiceKey: metaKey,
      metaListenAddr: '127.0.0.1:18090',
      pid: 4242,
      reused: false,
    );
  }

  AppServeStatus _withTunnel(AppServeStatus h, String kind, bool registered) =>
      AppServeStatus(
        name: h.name,
        device: h.device,
        pid: h.pid,
        alive: h.alive,
        appListenAddr: h.appListenAddr,
        appServiceKey: h.appServiceKey,
        appRegistered: kind == 'app' ? registered : h.appRegistered,
        apiListenAddr: h.apiListenAddr,
        apiServiceKey: h.apiServiceKey,
        apiRegistered: kind == 'api' ? registered : h.apiRegistered,
        metaListenAddr: h.metaListenAddr,
        metaServiceKey: h.metaServiceKey,
        metaRegistered: kind == 'meta' ? registered : h.metaRegistered,
      );

  @override
  Future<List<AppServeStatus>> appServeStatus() async =>
      List.unmodifiable(serveHosts);

  /// Seedable value returned by [embeddedCodexVersion] (default: a fake commit).
  String embeddedCodexCommit = 'deadbeef1234';

  @override
  Future<String> embeddedCodexVersion() async => embeddedCodexCommit;

  @override
  Future<void> appServeDeregister({
    required String name,
    required String kind,
  }) async {
    final i = serveHosts.indexWhere((h) => h.name == name);
    if (i < 0) return;
    serveHosts[i] = _withTunnel(serveHosts[i], kind, false);
  }

  @override
  Future<void> appServeReregister({
    required String name,
    required String kind,
  }) async {
    final i = serveHosts.indexWhere((h) => h.name == name);
    if (i < 0) return;
    serveHosts[i] = _withTunnel(serveHosts[i], kind, true);
  }

  @override
  Future<void> appServeStop(String name) async =>
      serveHosts.removeWhere((h) => h.name == name);

  @override
  Future<void> appServeStopAll() async => serveHosts.clear();

  @override
  Future<String?> codexLocate() async => codexPath;

  // --- App-server remote control ---

  /// Number of [appConnect] calls (asserts a reconnect actually happened).
  int appConnectCount = 0;

  @override
  Future<void> appConnect(String serviceKey, int localPort) async {
    appConnectCount++;
    _appConnected.add(serviceKey);
    _appEvents.putIfAbsent(serviceKey, StreamController<AppEvent>.broadcast);
  }

  @override
  bool appIsConnected(String serviceKey) => _appConnected.contains(serviceKey);

  @override
  Future<void> appDisconnect(String serviceKey) async {
    _appConnected.remove(serviceKey);
    await _appEvents.remove(serviceKey)?.close();
  }

  /// Seedable reachability returned by [appProbe] (default: reachable). A
  /// connected service is always reachable.
  final Map<String, bool> reachable = {};

  @override
  Future<bool> appProbe(String serviceKey) async =>
      _appConnected.contains(serviceKey) || (reachable[serviceKey] ?? true);

  /// Seedable failure reason for [appProbeReason]; null falls back to a generic
  /// one so an unreachable fake still exercises the "we know why" path.
  final Map<String, String> probeReason = {};

  @override
  Future<String?> appProbeReason(String serviceKey) async =>
      await appProbe(serviceKey)
      ? null
      : (probeReason[serviceKey] ?? 'probe: initialize timed out');

  @override
  Future<bool> apiProbe(String serviceKey) async =>
      reachable[serviceKey] ?? true;

  /// Seedable reachability for the loopback health checks ([appProbeLocal] /
  /// [apiProbeLocal]), keyed by the local `host:port` (default: reachable).
  final Map<String, bool> reachableLocal = {};

  @override
  Future<bool> appProbeLocal(String localAddr) async =>
      reachableLocal[localAddr] ?? true;

  @override
  Future<bool> apiProbeLocal(String localAddr) async =>
      reachableLocal[localAddr] ?? true;

  @override
  Stream<AppEvent> appEvents(String serviceKey) => _appEvents
      .putIfAbsent(serviceKey, StreamController<AppEvent>.broadcast)
      .stream;

  final StreamController<LogLine> _logEvents =
      StreamController<LogLine>.broadcast();

  @override
  Stream<LogLine> logEvents() => _logEvents.stream;

  final StreamController<RetryProgress> _retryEvents =
      StreamController<RetryProgress>.broadcast();

  @override
  Stream<RetryProgress> metaRetryEvents() => _retryEvents.stream;

  /// Inject a retry tick, so a test can assert the "retrying n/max" indicator.
  void pushRetry(int attempt, {int maxAttempts = 10}) => _retryEvents.add(
    RetryProgress(attempt: attempt, maxAttempts: maxAttempts),
  );

  /// Inject a log line into the live stream (test helper).
  void pushLog(LogLine line) => _logEvents.add(line);

  /// Inject a server event into [serviceKey]'s stream (test helper).
  void pushEvent(String serviceKey, AppEvent event) =>
      _appEvents[serviceKey]?.add(event);

  /// When true, the next [appThreadList] throws (simulating a stale/closed
  /// socket), then resets — to exercise the picker's reconnect-and-retry path.
  bool failNextThreadList = false;

  /// Simulate a backend whose handshake works but whose first RPC kills the link.
  bool disconnectOnThreadList = false;

  @override
  Future<List<ThreadMeta>> appThreadList(String serviceKey) async {
    if (disconnectOnThreadList) {
      _appConnected.remove(serviceKey);
      throw StateError(
        'request `thread/list` timed out; app-server connection closed',
      );
    }
    if (failNextThreadList) {
      failNextThreadList = false;
      throw StateError('Trying to work with closed connection');
    }
    return List.unmodifiable(appThreads);
  }

  /// Raw JSON returned by [appRateLimits] (tests can override).
  String rateLimitsJson = '{}';

  @override
  Future<String> appRateLimits(String serviceKey) async => rateLimitsJson;

  /// Unified diff returned by [appGitDiff] (tests can override).
  String gitDiffText = '';

  /// When set, [appGitDiff] waits on this before returning — so a test can hold
  /// the fetch open and assert on the loading state, then complete it.
  Completer<void>? gitDiffGate;

  @override
  Future<String> appGitDiff(String serviceKey, String threadId) async {
    final gate = gitDiffGate;
    if (gate != null) await gate.future;
    return gitDiffText;
  }

  /// Records whether [appCompact] was called.
  bool compacted = false;

  @override
  Future<void> appCompact(String serviceKey, String threadId) async =>
      compacted = true;

  /// Names set via [appSetThreadName], keyed by thread id.
  final Map<String, String> setNames = {};

  /// When true, [appSetThreadName] throws, to exercise the rollback path.
  bool failSetThreadName = false;

  /// Fails only the NEXT [appSetThreadName] call, so a test can interleave one
  /// failing rename with a succeeding one.
  bool failNextSetThreadName = false;

  /// When set, the next [appSetThreadName] parks until this completes — lets a
  /// test hold one rename in flight while issuing another.
  Completer<void>? renameGate;

  @override
  Future<void> appSetThreadName(
    String serviceKey,
    String threadId,
    String name,
  ) async {
    // Capture both switches before the await: they describe THIS call, and a
    // second call may arrive (and reset them) while this one is parked.
    final failThis = failSetThreadName || failNextSetThreadName;
    failNextSetThreadName = false;
    final gate = renameGate;
    if (gate != null) {
      renameGate = null;
      await gate.future;
    }
    if (failThis) throw Exception('rename refused');
    setNames[threadId] = name;
    final i = appThreads.indexWhere((t) => t.id == threadId);
    if (i >= 0) {
      appThreads[i] = appThreads[i].withName(name.isEmpty ? null : name);
    }
  }

  /// Summaries returned by [appThreadSummary], keyed by thread id. A thread
  /// with no entry summarizes to null, like one whose history has no agent
  /// reply yet.
  final Map<String, String> summaries = {};

  /// Thread ids [appThreadSummary] was called for, in call order — lets a test
  /// assert the activity view only fetches the rows it actually built.
  final List<String> summaryCalls = [];

  /// When set, every [appThreadSummary] parks on this, so a test can inspect
  /// the pre-summary row.
  Completer<void>? summaryGate;

  @override
  Future<String?> appThreadSummary(String serviceKey, String threadId) async {
    summaryCalls.add(threadId);
    final gate = summaryGate;
    if (gate != null) await gate.future;
    return summaries[threadId];
  }

  /// When true, [appModelList] returns no models, to exercise the
  /// "can't switch collaboration mode without a model" path.
  bool emptyModelList = false;

  @override
  Future<List<ModelInfo>> appModelList(String serviceKey) async =>
      emptyModelList
      ? const []
      : const [
          ModelInfo(
            id: 'gpt-5.5',
            displayName: 'GPT-5.5',
            description: 'default',
            supportedReasoningEfforts: ['low', 'medium', 'high', 'xhigh'],
            defaultReasoningEffort: 'medium',
          ),
          ModelInfo(
            id: 'gpt-5',
            displayName: 'GPT-5',
            description: '',
            supportedReasoningEfforts: ['minimal', 'low', 'medium', 'high'],
            defaultReasoningEffort: 'medium',
          ),
        ];

  /// Records the params of the last [appThreadStart] for assertions.
  String? lastModel, lastCwd, lastApproval, lastSandbox;

  @override
  Future<String> appThreadStart(
    String serviceKey, {
    String? model,
    String? cwd,
    String? approvalPolicy,
    String? sandbox,
  }) async {
    lastModel = model;
    lastCwd = cwd;
    lastApproval = approvalPolicy;
    lastSandbox = sandbox;
    final id = 'thread-${_threadSeq++}';
    appThreads.insert(
      0,
      ThreadMeta(id: id, preview: '', cwd: cwd ?? '', updatedAt: 0),
    );
    return id;
  }

  /// Records the last resumed thread id for assertions.
  String? lastResumed;

  /// Optional failure thrown by [appThreadResume].
  Object? appThreadResumeError;

  @override
  Future<void> appThreadResume(String serviceKey, String threadId) async {
    if (appThreadResumeError != null) throw appThreadResumeError!;
    lastResumed = threadId;
  }

  /// Seedable history for resume tests.
  ThreadHistory readResult = const ThreadHistory(items: [], running: false);

  @override
  Future<ThreadHistory> appThreadRead(
    String serviceKey,
    String threadId,
  ) async => readResult;

  /// Older pages a paginated thread hands back, oldest batch LAST — each call
  /// to [appThreadOlderPage] pops the last one, so seeding
  /// `[oldest, middle]` serves `middle` then `oldest`, the order the UI walks.
  List<List<ThreadItem>> olderPages = [];

  /// Turn ids passed to [appThreadOlderPage], in call order.
  int olderPageCalls = 0;

  @override
  Future<OlderPage> appThreadOlderPage(
    String serviceKey,
    String threadId,
  ) async {
    olderPageCalls++;
    if (olderPages.isEmpty) {
      return const OlderPage(items: [], hasOlder: false);
    }
    final items = olderPages.removeLast();
    return OlderPage(items: items, hasOlder: olderPages.isNotEmpty);
  }

  /// Items each turn hands back, keyed by turn id.
  Map<String, List<ThreadItem>> turnItems = {};

  /// Turn ids passed to [appThreadTurnItems], in call order.
  final List<String> turnItemCalls = [];

  @override
  Future<List<ThreadItem>> appThreadTurnItems(
    String serviceKey,
    String threadId,
    String turnId,
  ) async {
    turnItemCalls.add(turnId);
    return turnItems[turnId] ?? const [];
  }

  /// Seedable runtime config for the status-bar model indicator tests.
  ThreadRuntimeConfig? runtimeConfig;

  @override
  ThreadRuntimeConfig? appThreadRuntimeConfig(
    String serviceKey,
    String threadId,
  ) => runtimeConfig;

  /// Per-turn override of the model recorded for assertions.
  String? lastTurnModel;

  /// Last prompt text passed to [appTurnStart].
  String? lastTurnText;

  /// Last image data URLs passed to [appTurnStart].
  List<String> lastTurnImages = const [];

  /// Last collaboration mode passed to [appTurnStart] ("plan"/"default"/null).
  String? lastCollaborationMode;

  /// Last reasoning effort passed to [appTurnStart] ("low"/"medium"/"high"/null).
  String? lastReasoningEffort;

  /// Count of [appTurnStart] calls, so a test can assert a retry did NOT start a
  /// second server-side turn (the retry-safety guard reloaded instead).
  int turnStartCount = 0;

  /// When set, [appTurnStart] records its params then awaits this gate before
  /// emitting turn events / returning — so a test can act (e.g. change effort)
  /// while a turn is "in flight". Complete it to let the turn finish.
  Completer<void>? turnStartGate;

  /// When false, [appTurnStart] emits ONLY `turn/started` (no reply delta, no
  /// completion) — leaving the turn "streaming with no output yet" so a test can
  /// drive the rest (deltas / completion / interrupt) precisely via [pushEvent].
  /// Used to exercise the Esc state machine + message queue.
  bool autoCompleteTurn = true;

  /// Echoes a streamed agent reply so widget tests can assert rendering.
  @override
  Future<void> appTurnStart(
    String serviceKey,
    String threadId,
    String text, {
    List<String> images = const [],
    String? model,
    String? approvalPolicy,
    String? sandbox,
    String? collaborationMode,
    String? reasoningEffort,
  }) async {
    turnStartCount++;
    lastTurnModel = model;
    lastTurnText = text;
    lastTurnImages = images;
    lastApproval = approvalPolicy;
    lastSandbox = sandbox;
    lastCollaborationMode = collaborationMode;
    lastReasoningEffort = reasoningEffort;
    if (turnStartGate != null) await turnStartGate!.future;
    final c = _appEvents[serviceKey];
    if (c == null) return;
    c.add(AppEvent(kind: 'turn/started', threadId: threadId, raw: '{}'));
    // Leave the turn running with no output when the test wants to drive the
    // reply/completion itself (Esc state-machine + queue tests).
    if (!autoCompleteTurn) return;
    c.add(
      AppEvent(
        kind: 'item/agentMessage/delta',
        threadId: threadId,
        itemId: 'a1',
        itemType: 'agentMessage',
        text: 'echo: $text',
        raw: '{}',
      ),
    );
    c.add(AppEvent(kind: 'turn/completed', threadId: threadId, raw: '{}'));
  }

  /// Records the last turn id passed to [appTurnInterrupt].
  String? lastInterruptTurnId;
  bool interrupted = false;

  @override
  Future<void> appTurnInterrupt(
    String serviceKey,
    String threadId, {
    String? turnId,
  }) async {
    interrupted = true;
    lastInterruptTurnId = turnId;
  }

  /// Records the last approval decision for assertions.
  String? lastApprovalDecision;

  @override
  Future<void> appRespondApproval(
    String serviceKey,
    String requestId,
    String decision,
  ) async => lastApprovalDecision = decision;

  /// Records the last answers JSON passed to [appRespondUserInput].
  String? lastUserInputAnswers;

  @override
  Future<void> appRespondUserInput(
    String serviceKey,
    String requestId,
    String answersJson,
  ) async => lastUserInputAnswers = answersJson;

  // --- Local session takeover ---

  /// Seedable local sessions returned by [appLocalSessions].
  List<LocalSession> localSessions = const [];

  /// Seedable per-thread liveness returned by [appSessionLiveness].
  final Map<String, SessionLiveness> liveness = {};

  /// Seedable per-thread transcript returned by [appLocalSessionTranscript].
  final Map<String, List<ThreadItem>> transcripts = {};

  /// Records the last `(serviceKey, threadId)` passed to [appForceResume].
  String? lastForceResumedKey, lastForceResumedThread;

  /// Result returned by [appForceResume] (tests can override).
  ForceResumeReport forceResumeResult = const ForceResumeReport(
    killed: [],
    survived: [],
    stillHeld: false,
    resumed: true,
  );

  @override
  Future<List<LocalSession>> appLocalSessions() async => localSessions;

  @override
  Future<SessionLiveness> appSessionLiveness(String threadId) async =>
      liveness[threadId] ??
      SessionLiveness(
        threadId: threadId,
        turnState: 'completed',
        heldOpen: false,
        safety: 'resumable',
        allowsResume: true,
        requiresTakeover: false,
        holders: const [],
      );

  @override
  Future<ForceResumeReport> appForceResume(
    String serviceKey,
    String threadId,
  ) async {
    lastForceResumedKey = serviceKey;
    lastForceResumedThread = threadId;
    return forceResumeResult;
  }

  @override
  Future<List<ThreadItem>> appLocalSessionTranscript(String threadId) async =>
      transcripts[threadId] ?? const [];

  // --- Remote (meta service) sessions + per-thread config ---

  /// Per-service-key override for [metaSessions]; falls back to [localSessions]
  /// so a test that only seeds local sessions also sees them "remotely".
  final Map<String, List<LocalSession>> remoteSessions = {};

  /// Seedable per-thread config returned by [metaThreadConfigGet], keyed by
  /// thread id. [metaThreadConfigSet] writes here.
  final Map<String, ThreadConfig> threadConfigs = {};

  /// Records the last `(serviceKey, threadId)` passed to [metaThreadConfigGet] /
  /// [metaThreadConfigSet], so tests can assert the config wiring fired.
  String? lastConfigGetThread, lastConfigSetThread;

  /// Records the last `(serviceKey, threadId)` passed to [metaForceResume].
  String? lastMetaResumedKey, lastMetaResumedThread;

  /// Number of live session streams opened, for asserting that active-writer
  /// chat uses the subscription instead of periodic transcript requests.
  int metaSessionEventSubscriptions = 0;

  final Map<String, StreamController<SessionFollowUpdate>> _metaSessionEvents =
      {};

  @override
  Future<List<LocalSession>> metaSessions(String serviceKey) async =>
      remoteSessions[serviceKey] ?? localSessions;

  @override
  Future<SessionLiveness> metaSessionLiveness(
    String serviceKey,
    String threadId,
  ) async => appSessionLiveness(threadId);

  @override
  Stream<SessionFollowUpdate> metaSessionEvents(
    String serviceKey,
    String threadId,
  ) {
    metaSessionEventSubscriptions++;
    final existing = _metaSessionEvents[threadId];
    if (existing != null) return existing.stream;
    late final StreamController<SessionFollowUpdate> controller;
    controller = StreamController<SessionFollowUpdate>.broadcast(
      onListen: () => unawaited(() async {
        controller.add(
          SessionFollowUpdate(
            liveness: await appSessionLiveness(threadId),
            items: transcripts[threadId] ?? const [],
          ),
        );
      }()),
    );
    _metaSessionEvents[threadId] = controller;
    return controller.stream;
  }

  /// Emits one remote-writer snapshot to subscribers of [threadId].
  void pushMetaSessionUpdate(String threadId, SessionFollowUpdate update) {
    _metaSessionEvents
        .putIfAbsent(threadId, StreamController<SessionFollowUpdate>.broadcast)
        .add(update);
  }

  @override
  Future<List<ThreadItem>> metaSessionTranscript(
    String serviceKey,
    String threadId,
  ) async => transcripts[threadId] ?? const [];

  @override
  Future<ForceResumeReport> metaForceResume(
    String serviceKey,
    String threadId,
  ) async {
    lastMetaResumedKey = serviceKey;
    lastMetaResumedThread = threadId;
    return forceResumeResult;
  }

  /// Records the last [metaUploadFile] call for assertions.
  String? lastUploadName;

  /// Bytes passed to the last [metaUploadFile].
  Uint8List? lastUploadBytes;

  /// When set, [metaUploadFile] throws it instead of returning a path.
  Object? uploadError;

  @override
  Future<String> metaUploadFile(
    String serviceKey,
    String fileName,
    Uint8List bytes,
  ) async {
    if (uploadError != null) throw uploadError!;
    lastUploadName = fileName;
    lastUploadBytes = bytes;
    return '/host/uploads/123/$fileName';
  }

  @override
  Future<ThreadConfig> metaThreadConfigGet(
    String serviceKey,
    String threadId,
  ) async {
    lastConfigGetThread = threadId;
    return threadConfigs[threadId] ?? const ThreadConfig();
  }

  @override
  Future<ThreadConfig> metaThreadConfigSet(
    String serviceKey,
    String threadId,
    ThreadConfig config,
  ) async {
    lastConfigSetThread = threadId;
    threadConfigs[threadId] = config;
    return config;
  }

  /// Seedable per-service project config returned by [metaProjectConfig];
  /// [metaSetProjectConfig] writes here.
  final Map<String, ProjectConfig> projectConfigs = {};

  /// Seedable directory tree for [metaListDir], keyed by absolute host path →
  /// its immediate sub-directories. A path with no entry lists empty.
  final Map<String, List<HostDirEntry>> dirTree = {};

  /// Fails the next [metaProjectConfig] call, then resets — simulates the
  /// cold-open race where the meta tunnel isn't up yet.
  bool failNextProjectConfig = false;

  /// How many times [metaProjectConfig] has been called, so a test can assert
  /// a retry happened (and that it stops once answered).
  int projectConfigCalls = 0;

  @override
  Future<ProjectConfig> metaProjectConfig(String serviceKey) async {
    projectConfigCalls++;
    if (failNextProjectConfig) {
      failNextProjectConfig = false;
      throw StateError('meta tunnel not ready');
    }
    return projectConfigs[serviceKey] ?? const ProjectConfig();
  }

  @override
  Future<ProjectConfig> metaSetProjectConfig(
    String serviceKey,
    List<String> projectRoots,
    String? defaultProject,
  ) async {
    final cfg = ProjectConfig(
      projectRoots: projectRoots,
      defaultProject: defaultProject,
    );
    projectConfigs[serviceKey] = cfg;
    return cfg;
  }

  @override
  Future<List<HostDirEntry>> metaListDir(
    String serviceKey,
    String path,
  ) async => dirTree[path] ?? const [];

  /// Seedable file listing for [metaListFiles], keyed by absolute host dir path.
  final Map<String, List<HostFileEntry>> fileTree = {};

  /// Seedable file bytes for [metaReadFile], keyed by absolute host file path.
  final Map<String, Uint8List> fileBytes = {};

  /// Records the last [metaWriteFile] call for assertions.
  String? lastWriteDir;

  /// Name passed to the last [metaWriteFile].
  String? lastWriteName;

  @override
  Future<List<HostFileEntry>> metaListFiles(
    String serviceKey,
    String path,
  ) async => fileTree[path] ?? const [];

  @override
  Future<Uint8List> metaReadFile(String serviceKey, String path) async =>
      fileBytes[path] ?? Uint8List(0);

  /// Paths the fake host will serve to [metaReadThreadImage] — i.e. the ones
  /// its transcript references. Anything else throws, like a real host's 403.
  final Map<String, Uint8List> threadImageBytes = {};

  @override
  Future<Uint8List> metaReadThreadImage(
    String serviceKey,
    String threadId,
    String path,
  ) async {
    final bytes = threadImageBytes[path];
    if (bytes == null) {
      throw StateError('403 path is outside the configured project roots');
    }
    return bytes;
  }

  @override
  Future<String> metaWriteFile(
    String serviceKey,
    String dir,
    String fileName,
    Uint8List bytes,
  ) async {
    lastWriteDir = dir;
    lastWriteName = fileName;
    return '$dir/$fileName';
  }
}
