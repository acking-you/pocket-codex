import 'dart:typed_data';

import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/rust/api/bridge.dart' as frb;

/// Real [BridgeApi] backed by the flutter_rust_bridge bindings.
class RustBridgeApi implements BridgeApi {
  /// Creates the real bridge.
  const RustBridgeApi();

  @override
  Future<ConfigInfo> getConfig() async {
    final c = await frb.getConfig();
    return ConfigInfo(
      relay: c.relay,
      hasKey: c.hasKey,
      locale: c.locale,
      mode: c.mode,
      accountLogin: c.accountLogin,
      hasAccountToken: c.hasAccountToken,
    );
  }

  @override
  Future<DeviceCode> accountLoginStart({String? backend}) async {
    final d = await frb.accountLoginStart(backend: backend);
    return DeviceCode(
      userCode: d.userCode,
      verificationUri: d.verificationUri,
      pollHandle: d.pollHandle,
      intervalSecs: d.intervalSecs.toInt(),
      expiresInSecs: d.expiresInSecs.toInt(),
      backend: d.backend,
    );
  }

  @override
  Future<AccountPoll> accountLoginPoll(
    String pollHandle,
    String backend,
  ) async {
    final p = await frb.accountLoginPoll(
      pollHandle: pollHandle,
      backend: backend,
    );
    return AccountPoll(
      status: p.status,
      login: p.login,
      accountId: p.accountId,
    );
  }

  @override
  Future<WebLoginStart> accountWebLoginStart({
    required String redirectUri,
    String? backend,
  }) async {
    final s = await frb.accountWebLoginStart(
      redirectUri: redirectUri,
      backend: backend,
    );
    return WebLoginStart(
      authorizeUrl: s.authorizeUrl,
      state: s.state,
      codeVerifier: s.codeVerifier,
      backend: s.backend,
    );
  }

  @override
  Future<AccountUser> accountWebLoginExchange({
    required String exchangeCode,
    required String codeVerifier,
    required String backend,
  }) async {
    final u = await frb.accountWebLoginExchange(
      exchangeCode: exchangeCode,
      codeVerifier: codeVerifier,
      backend: backend,
    );
    return AccountUser(login: u.login, accountId: u.accountId);
  }

  @override
  Future<AccountUser?> accountCurrentUser() async {
    final u = await frb.accountCurrentUser();
    return u == null
        ? null
        : AccountUser(login: u.login, accountId: u.accountId);
  }

  @override
  Future<void> accountLogout() => frb.accountLogout();

  @override
  Future<List<AccountService>> accountServices() async {
    final list = await frb.accountServices();
    return list
        .map(
          (s) => AccountService(device: s.device, kind: s.kind, name: s.name),
        )
        .toList();
  }

  @override
  Future<void> accountDeregisterService({
    required String device,
    required String kind,
    required String name,
  }) => frb.accountDeregisterService(device: device, kind: kind, name: name);

  @override
  Future<AppServeResult> appServeStart({
    required int port,
    String? binaryOverride,
    String? name,
    String? proxy,
    required bool embedded,
  }) async {
    final r = await frb.appServeStart(
      port: port,
      binaryOverride: binaryOverride,
      name: name,
      proxy: proxy,
      embedded: embedded,
    );
    return AppServeResult(
      device: r.device,
      name: r.name,
      appServiceKey: r.appServiceKey,
      appListenAddr: r.appListenAddr,
      apiServiceKey: r.apiServiceKey,
      apiListenAddr: r.apiListenAddr,
      metaServiceKey: r.metaServiceKey,
      metaListenAddr: r.metaListenAddr,
      pid: r.pid,
      reused: r.reused,
    );
  }

  @override
  Future<List<AppServeStatus>> appServeStatus() async {
    final list = await frb.appServeStatus();
    return list
        .map(
          (s) => AppServeStatus(
            name: s.name,
            device: s.device,
            pid: s.pid,
            alive: s.alive,
            appListenAddr: s.appListenAddr,
            appServiceKey: s.appServiceKey,
            appRegistered: s.appRegistered,
            apiListenAddr: s.apiListenAddr,
            apiServiceKey: s.apiServiceKey,
            apiRegistered: s.apiRegistered,
            metaListenAddr: s.metaListenAddr,
            metaServiceKey: s.metaServiceKey,
            metaRegistered: s.metaRegistered,
            embedded: s.embedded,
            codexBinary: s.codexBinary,
            proxy: s.proxy,
          ),
        )
        .toList();
  }

  @override
  Future<String> embeddedCodexVersion() => frb.embeddedCodexVersion();

  @override
  Future<void> appServeDeregister({
    required String name,
    required String kind,
  }) => frb.appServeDeregister(name: name, kind: kind);

  @override
  Future<void> appServeReregister({
    required String name,
    required String kind,
  }) => frb.appServeReregister(name: name, kind: kind);

  @override
  Future<void> appServeStop(String name) => frb.appServeStop(name: name);

  @override
  Future<void> appServeStopAll() => frb.appServeStopAll();

  @override
  Future<String?> codexLocate() => frb.codexLocate();

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

  // --- 自带 codex bootstrap: provider setup, ChatGPT login, system prompt ---

  @override
  Future<CodexSetupStatus> codexSetupStatus() async {
    final s = await frb.codexSetupStatus();
    return CodexSetupStatus(
      codexHome: s.codexHome,
      hasConfig: s.hasConfig,
      hasAuth: s.hasAuth,
      hasCustomProvider: s.hasCustomProvider,
      authMode: s.authMode,
      needsSetup: s.needsSetup,
      promptVariant: s.promptVariant,
    );
  }

  @override
  Future<void> codexSetupProvider({
    required String baseUrl,
    required String apiKey,
    String? model,
  }) => frb.codexSetupProvider(baseUrl: baseUrl, apiKey: apiKey, model: model);

  @override
  Future<String> codexPromptVariant() => frb.codexPromptVariant();

  @override
  Future<void> codexSetPromptVariant(String variant) =>
      frb.codexSetPromptVariant(variant: variant);

  @override
  Future<CodexLoginStart> codexLoginChatgptStart(String serviceKey) async {
    final s = await frb.codexLoginChatgptStart(serviceKey: serviceKey);
    return CodexLoginStart(
      mode: s.mode,
      loginId: s.loginId,
      authUrl: s.authUrl,
      verificationUrl: s.verificationUrl,
      userCode: s.userCode,
    );
  }

  @override
  Future<CodexAuthStatus> codexAuthStatus(String serviceKey) async {
    final s = await frb.codexAuthStatus(serviceKey: serviceKey);
    return CodexAuthStatus(authenticated: s.authenticated, method: s.method);
  }

  @override
  Future<void> codexLoginCancel(String serviceKey, String loginId) =>
      frb.codexLoginCancel(serviceKey: serviceKey, loginId: loginId);

  @override
  Future<void> codexLogout(String serviceKey) =>
      frb.codexLogout(serviceKey: serviceKey);

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
  Future<bool> appProbe(String serviceKey) =>
      frb.appProbe(serviceKey: serviceKey);

  @override
  Future<bool> apiProbe(String serviceKey) =>
      frb.apiProbe(serviceKey: serviceKey);

  @override
  Future<bool> appProbeLocal(String localAddr) =>
      frb.appProbeLocal(localAddr: localAddr);

  @override
  Future<bool> apiProbeLocal(String localAddr) =>
      frb.apiProbeLocal(localAddr: localAddr);

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
          images: e.images,
          requestId: e.requestId,
          raw: e.raw,
        ),
      );

  @override
  Stream<LogLine> logEvents() => frb.logEvents().map(
    (l) => LogLine(
      level: l.level,
      target: l.target,
      message: l.message,
      timestampMs: l.timestampMs,
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
              images: i.images,
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
      model: h.model,
      modelProvider: h.modelProvider,
      approvalPolicy: h.approvalPolicy,
      sandboxMode: h.sandboxMode,
      configConfirmed: h.configConfirmed,
    );
  }

  @override
  ThreadRuntimeConfig? appThreadRuntimeConfig(
    String serviceKey,
    String threadId,
  ) {
    final c = frb.appThreadRuntimeConfig(
      serviceKey: serviceKey,
      threadId: threadId,
    );
    if (c == null) return null;
    return ThreadRuntimeConfig(
      model: c.model,
      modelProvider: c.modelProvider,
      reasoningEffort: c.reasoningEffort,
      approvalPolicy: c.approvalPolicy,
      sandboxMode: c.sandboxMode,
      collaborationMode: c.collaborationMode,
      confirmedByUpdate: c.confirmedByUpdate,
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
    List<String> images = const [],
    String? model,
    String? approvalPolicy,
    String? sandbox,
    String? collaborationMode,
    String? reasoningEffort,
  }) => frb.appTurnStart(
    serviceKey: serviceKey,
    threadId: threadId,
    text: text,
    images: images,
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

  @override
  Future<void> appRespondUserInput(
    String serviceKey,
    String requestId,
    String answersJson,
  ) => frb.appRespondUserInput(
    serviceKey: serviceKey,
    requestId: requestId,
    answersJson: answersJson,
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
            images: i.images,
          ),
        )
        .toList();
  }

  // --- Remote (meta service) sessions + per-thread config ---

  @override
  Future<List<LocalSession>> metaSessions(String serviceKey) async {
    final list = await frb.metaSessions(serviceKey: serviceKey);
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
  Future<SessionLiveness> metaSessionLiveness(
    String serviceKey,
    String threadId,
  ) async {
    final v = await frb.metaSessionLiveness(
      serviceKey: serviceKey,
      threadId: threadId,
    );
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
  Future<List<ThreadItem>> metaSessionTranscript(
    String serviceKey,
    String threadId,
  ) async {
    final items = await frb.metaSessionTranscript(
      serviceKey: serviceKey,
      threadId: threadId,
    );
    return items
        .map(
          (i) => ThreadItem(
            id: i.id,
            itemType: i.itemType,
            title: i.title,
            text: i.text,
            images: i.images,
          ),
        )
        .toList();
  }

  @override
  Future<ForceResumeReport> metaForceResume(
    String serviceKey,
    String threadId,
  ) async {
    final r = await frb.metaForceResume(
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
  Future<String> metaUploadFile(
    String serviceKey,
    String fileName,
    Uint8List bytes,
  ) => frb.metaUploadFile(
    serviceKey: serviceKey,
    fileName: fileName,
    bytes: bytes,
  );

  @override
  Future<ThreadConfig> metaThreadConfigGet(
    String serviceKey,
    String threadId,
  ) async {
    final c = await frb.metaThreadConfigGet(
      serviceKey: serviceKey,
      threadId: threadId,
    );
    return _threadConfig(c);
  }

  @override
  Future<ThreadConfig> metaThreadConfigSet(
    String serviceKey,
    String threadId,
    ThreadConfig config,
  ) async {
    final c = await frb.metaThreadConfigSet(
      serviceKey: serviceKey,
      threadId: threadId,
      config: frb.ThreadConfigDto(
        model: config.model,
        reasoningEffort: config.reasoningEffort,
        permissionMode: config.permissionMode,
        planMode: config.planMode,
      ),
    );
    return _threadConfig(c);
  }

  ThreadConfig _threadConfig(frb.ThreadConfigDto c) => ThreadConfig(
    model: c.model,
    reasoningEffort: c.reasoningEffort,
    permissionMode: c.permissionMode,
    planMode: c.planMode,
  );

  @override
  Future<ProjectConfig> metaProjectConfig(String serviceKey) async =>
      _projectConfig(await frb.metaProjectConfig(serviceKey: serviceKey));

  @override
  Future<ProjectConfig> metaSetProjectConfig(
    String serviceKey,
    List<String> projectRoots,
    String? defaultProject,
  ) async => _projectConfig(
    await frb.metaSetProjectConfig(
      serviceKey: serviceKey,
      projectRoots: projectRoots,
      defaultProject: defaultProject,
    ),
  );

  @override
  Future<List<HostDirEntry>> metaListDir(String serviceKey, String path) async {
    final entries = await frb.metaListDir(serviceKey: serviceKey, path: path);
    return entries
        .map(
          (e) =>
              HostDirEntry(name: e.name, path: e.path, isGitRepo: e.isGitRepo),
        )
        .toList(growable: false);
  }

  @override
  Future<List<HostFileEntry>> metaListFiles(
    String serviceKey,
    String path,
  ) async {
    final files = await frb.metaListFiles(serviceKey: serviceKey, path: path);
    return files
        .map(
          (e) => HostFileEntry(
            name: e.name,
            path: e.path,
            size: e.size.toInt(),
            mtime: e.mtime.toInt(),
          ),
        )
        .toList(growable: false);
  }

  @override
  Future<Uint8List> metaReadFile(String serviceKey, String path) =>
      frb.metaReadFile(serviceKey: serviceKey, path: path);

  @override
  Future<Uint8List> metaReadThreadImage(
    String serviceKey,
    String threadId,
    String path,
  ) => frb.metaReadThreadImage(
    serviceKey: serviceKey,
    threadId: threadId,
    path: path,
  );

  @override
  Future<String> metaWriteFile(
    String serviceKey,
    String dir,
    String fileName,
    Uint8List bytes,
  ) => frb.metaWriteFile(
    serviceKey: serviceKey,
    dir: dir,
    fileName: fileName,
    bytes: bytes,
  );

  ProjectConfig _projectConfig(frb.ProjectConfigDto c) => ProjectConfig(
    projectRoots: c.projectRoots,
    defaultProject: c.defaultProject,
  );
}
