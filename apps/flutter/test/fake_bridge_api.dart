import 'dart:async';

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
    }
    return AccountPoll(
      status: accountPollStatus,
      login: accountUser?.login,
      accountId: accountUser?.accountId,
    );
  }

  @override
  Future<AccountUser?> accountCurrentUser() async => accountUser;

  @override
  Future<void> accountLogout() async => accountUser = null;

  @override
  Future<List<AccountService>> accountServices() async => accountServiceList;

  /// Records the last [accountDeregisterService] call for assertions.
  String? lastDeregistered;

  @override
  Future<void> accountDeregisterService({
    required String device,
    required String kind,
    required String name,
  }) async {
    lastDeregistered = 'pcx:$device:$kind:$name';
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

  /// Path returned by [codexLocate] (set null to simulate "codex not found").
  String? codexPath = '/usr/local/bin/codex';

  @override
  Future<AppServeResult> appServeStart({
    required int port,
    String? binaryOverride,
    String? name,
    String? proxy,
  }) async {
    lastServePort = port;
    lastServeBinary = binaryOverride;
    lastServeName = name;
    lastServeProxy = proxy;
    final n = name ?? 'default';
    final key = 'pcx:local:app:$n';
    serveHosts
      ..removeWhere((h) => h.name == n)
      ..add(
        AppServeStatus(
          running: true,
          alive: true,
          pid: 4242,
          listenAddr: '127.0.0.1:$port',
          device: 'local',
          name: n,
          serviceKey: key,
        ),
      );
    return AppServeResult(
      device: 'local',
      name: n,
      serviceKey: key,
      listenAddr: '127.0.0.1:$port',
      pid: 4242,
      reused: false,
    );
  }

  @override
  Future<List<AppServeStatus>> appServeStatus() async =>
      List.unmodifiable(serveHosts);

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

  @override
  Future<bool> apiProbe(String serviceKey) async =>
      reachable[serviceKey] ?? true;

  @override
  Stream<AppEvent> appEvents(String serviceKey) => _appEvents
      .putIfAbsent(serviceKey, StreamController<AppEvent>.broadcast)
      .stream;

  /// Inject a server event into [serviceKey]'s stream (test helper).
  void pushEvent(String serviceKey, AppEvent event) =>
      _appEvents[serviceKey]?.add(event);

  /// When true, the next [appThreadList] throws (simulating a stale/closed
  /// socket), then resets — to exercise the picker's reconnect-and-retry path.
  bool failNextThreadList = false;

  @override
  Future<List<ThreadMeta>> appThreadList(String serviceKey) async {
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

  @override
  Future<String> appGitDiff(String serviceKey, String threadId) async =>
      gitDiffText;

  /// Records whether [appCompact] was called.
  bool compacted = false;

  @override
  Future<void> appCompact(String serviceKey, String threadId) async =>
      compacted = true;

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

  @override
  Future<void> appThreadResume(String serviceKey, String threadId) async =>
      lastResumed = threadId;

  /// Seedable history for resume tests.
  ThreadHistory readResult = const ThreadHistory(items: [], running: false);

  @override
  Future<ThreadHistory> appThreadRead(
    String serviceKey,
    String threadId,
  ) async => readResult;

  /// Per-turn override of the model recorded for assertions.
  String? lastTurnModel;

  /// Last prompt text passed to [appTurnStart].
  String? lastTurnText;

  /// Last collaboration mode passed to [appTurnStart] ("plan"/"default"/null).
  String? lastCollaborationMode;

  /// Last reasoning effort passed to [appTurnStart] ("low"/"medium"/"high"/null).
  String? lastReasoningEffort;

  /// Echoes a streamed agent reply so widget tests can assert rendering.
  @override
  Future<void> appTurnStart(
    String serviceKey,
    String threadId,
    String text, {
    String? model,
    String? approvalPolicy,
    String? sandbox,
    String? collaborationMode,
    String? reasoningEffort,
  }) async {
    lastTurnModel = model;
    lastTurnText = text;
    lastApproval = approvalPolicy;
    lastSandbox = sandbox;
    lastCollaborationMode = collaborationMode;
    lastReasoningEffort = reasoningEffort;
    final c = _appEvents[serviceKey];
    if (c == null) return;
    c.add(AppEvent(kind: 'turn/started', threadId: threadId, raw: '{}'));
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
}
