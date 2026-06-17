import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/rust/api/bridge.dart' as frb;

/// Real [BridgeApi] backed by the flutter_rust_bridge bindings.
class RustBridgeApi implements BridgeApi {
  /// Creates the real bridge.
  const RustBridgeApi();

  @override
  Future<ConfigInfo> getConfig() async {
    final c = await frb.getConfig();
    return ConfigInfo(relay: c.relay, hasKey: c.hasKey, locale: c.locale);
  }

  @override
  Future<void> setRelay(String relay) => frb.setRelay(relay: relay);

  @override
  Future<void> setKey(String key) => frb.setKey(key: key);

  @override
  Future<String> importConfig(String text) => frb.importConfig(text: text);

  @override
  Future<String> exportConfig() => frb.exportConfig();

  @override
  Future<List<ServiceEntry>> discoverServices() async {
    final list = await frb.discoverServices();
    return list
        .map(
          (s) => ServiceEntry(
            device: s.device,
            kind: s.kind,
            name: s.name,
            key: s.key,
          ),
        )
        .toList();
  }

  @override
  Future<SubInfo> apiSubscribe(String serviceKey, int localPort) async {
    final s = await frb.apiSubscribe(
      serviceKey: serviceKey,
      localPort: localPort,
    );
    return SubInfo(key: s.key, localAddr: s.localAddr, alive: s.alive);
  }

  @override
  Future<void> apiUnsubscribe(String serviceKey) =>
      frb.apiUnsubscribe(serviceKey: serviceKey);

  @override
  Future<List<SubInfo>> subscriptions() async {
    final list = await frb.subscriptions();
    return list
        .map((s) => SubInfo(key: s.key, localAddr: s.localAddr, alive: s.alive))
        .toList();
  }

  @override
  Future<void> setLocale(String locale) => frb.setLocale(locale: locale);

  // --- App-server remote control ---

  @override
  Future<void> appConnect(String serviceKey, int localPort) {
    // The port crosses the bridge as a u16; reject anything that can't fit so a
    // bad value fails here rather than as an opaque FRB codec error. `0` is the
    // documented sentinel for "let the bridge pick a free port" (see
    // [appLocalPort]), so it's allowed.
    if (localPort < 0 || localPort > 65535) {
      throw RangeError.range(localPort, 0, 65535, 'localPort');
    }
    return frb.appConnect(serviceKey: serviceKey, localPort: localPort);
  }

  @override
  bool appIsConnected(String serviceKey) =>
      frb.appIsConnected(serviceKey: serviceKey);

  @override
  Future<void> appDisconnect(String serviceKey) =>
      frb.appDisconnect(serviceKey: serviceKey);

  @override
  Stream<AppEvent> appEvents(String serviceKey) => frb
      .appEvents(serviceKey: serviceKey)
      .map(
        (e) => AppEvent(
          kind: e.kind,
          threadId: e.threadId,
          itemId: e.itemId,
          itemType: e.itemType,
          title: e.title,
          text: e.text,
          requestId: e.requestId,
          raw: e.raw,
        ),
      );

  @override
  Future<List<ThreadMeta>> appThreadList(String serviceKey) async {
    final list = await frb.appThreadList(serviceKey: serviceKey);
    return list
        .map(
          (t) => ThreadMeta(
            id: t.id,
            preview: t.preview,
            cwd: t.cwd,
            updatedAt: t.updatedAt.toInt(),
          ),
        )
        .toList();
  }

  @override
  Future<List<ModelInfo>> appModelList(String serviceKey) async {
    final list = await frb.appModelList(serviceKey: serviceKey);
    return list
        .map(
          (m) => ModelInfo(
            id: m.id,
            displayName: m.displayName,
            description: m.description,
            supportedReasoningEfforts: m.supportedReasoningEfforts,
            defaultReasoningEffort: m.defaultReasoningEffort,
          ),
        )
        .toList();
  }

  @override
  Future<String> appThreadStart(
    String serviceKey, {
    String? model,
    String? cwd,
    String? approvalPolicy,
    String? sandbox,
  }) => frb.appThreadStart(
    serviceKey: serviceKey,
    model: model,
    cwd: cwd,
    approvalPolicy: approvalPolicy,
    sandbox: sandbox,
  );

  @override
  Future<void> appThreadResume(String serviceKey, String threadId) =>
      frb.appThreadResume(serviceKey: serviceKey, threadId: threadId);

  @override
  Future<ThreadHistory> appThreadRead(
    String serviceKey,
    String threadId,
  ) async {
    final h = await frb.appThreadRead(
      serviceKey: serviceKey,
      threadId: threadId,
    );
    return ThreadHistory(
      items: h.items
          .map(
            (i) => ThreadItem(
              id: i.id,
              itemType: i.itemType,
              title: i.title,
              text: i.text,
            ),
          )
          .toList(),
      running: h.running,
      branch: h.branch,
      cwd: h.cwd,
      tokensUsed: h.tokensUsed?.toInt(),
      contextWindow: h.contextWindow?.toInt(),
      collaborationMode: h.collaborationMode,
      reasoningEffort: h.reasoningEffort,
    );
  }

  @override
  Future<String> appRateLimits(String serviceKey) =>
      frb.appRateLimits(serviceKey: serviceKey);

  @override
  Future<String> appGitDiff(String serviceKey, String cwd) =>
      frb.appGitDiff(serviceKey: serviceKey, cwd: cwd);

  @override
  Future<void> appCompact(String serviceKey, String threadId) =>
      frb.appCompact(serviceKey: serviceKey, threadId: threadId);

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
  }) => frb.appTurnStart(
    serviceKey: serviceKey,
    threadId: threadId,
    text: text,
    model: model,
    approvalPolicy: approvalPolicy,
    sandbox: sandbox,
    collaborationMode: collaborationMode,
    reasoningEffort: reasoningEffort,
  );

  @override
  Future<void> appTurnInterrupt(
    String serviceKey,
    String threadId, {
    String? turnId,
  }) => frb.appTurnInterrupt(
    serviceKey: serviceKey,
    threadId: threadId,
    turnId: turnId,
  );

  @override
  Future<void> appRespondApproval(
    String serviceKey,
    String requestId,
    String decision,
  ) => frb.appRespondApproval(
    serviceKey: serviceKey,
    requestId: requestId,
    decision: decision,
  );

  // --- Local session takeover ---

  @override
  Future<List<LocalSession>> appLocalSessions() async {
    final list = await frb.appLocalSessions();
    return list
        .map(
          (s) => LocalSession(
            threadId: s.threadId,
            cwd: s.cwd,
            preview: s.preview,
            source: s.source,
            updatedAt: s.updatedAt.toInt(),
            turnState: s.turnState,
            heldOpen: s.heldOpen,
            safety: s.safety,
            allowsResume: s.allowsResume,
            requiresTakeover: s.requiresTakeover,
          ),
        )
        .toList();
  }

  @override
  Future<SessionLiveness> appSessionLiveness(String threadId) async {
    final v = await frb.appSessionLiveness(threadId: threadId);
    return SessionLiveness(
      threadId: v.threadId,
      turnState: v.turnState,
      heldOpen: v.heldOpen,
      safety: v.safety,
      allowsResume: v.allowsResume,
      requiresTakeover: v.requiresTakeover,
      holders: v.holders
          .map((h) => Holder(pid: h.pid.toInt(), name: h.name))
          .toList(),
    );
  }

  @override
  Future<ForceResumeReport> appForceResume(
    String serviceKey,
    String threadId,
  ) async {
    final r = await frb.appForceResume(
      serviceKey: serviceKey,
      threadId: threadId,
    );
    Holder map(frb.HolderDto h) => Holder(pid: h.pid.toInt(), name: h.name);
    return ForceResumeReport(
      killed: r.killed.map(map).toList(),
      survived: r.survived.map(map).toList(),
      stillHeld: r.stillHeld,
      resumed: r.resumed,
      resumeError: r.resumeError,
    );
  }

  @override
  Future<List<ThreadItem>> appLocalSessionTranscript(String threadId) async {
    final items = await frb.appLocalSessionTranscript(threadId: threadId);
    return items
        .map(
          (i) => ThreadItem(
            id: i.id,
            itemType: i.itemType,
            title: i.title,
            text: i.text,
          ),
        )
        .toList();
  }
}
