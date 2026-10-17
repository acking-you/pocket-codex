import 'dart:typed_data';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart'
    show PlatformInt64Util;

import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/rust/api/bridge.dart' as frb;
import 'package:pocket_codex/src/rust/api/bridge/codex_setup.dart' as frb;
import 'package:pocket_codex/src/rust/api/bridge/connection.dart' as frb;
import 'package:pocket_codex/src/rust/api/bridge/device_management.dart' as frb;
import 'package:pocket_codex/src/rust/api/bridge/device_metadata.dart' as frb;
import 'package:pocket_codex/src/rust/api/bridge/events.dart' as frb;
import 'package:pocket_codex/src/rust/api/bridge/host_config.dart' as frb;
import 'package:pocket_codex/src/rust/api/bridge/host_sessions.dart' as frb;
import 'package:pocket_codex/src/rust/api/bridge/native_sessions.dart' as frb;
import 'package:pocket_codex/src/rust/api/bridge/hosted_account.dart' as frb;
import 'package:pocket_codex/src/rust/api/bridge/hosting.dart' as frb;
import 'package:pocket_codex/src/rust/api/bridge/services.dart' as frb;
import 'package:pocket_codex/src/rust/api/bridge/session_config.dart' as frb;
import 'package:pocket_codex/src/rust/api/bridge/session_access.dart' as frb;
import 'package:pocket_codex/src/rust/api/bridge/threads.dart' as frb;

part 'bridge/rust_hosting.dart';
part 'bridge/rust_threads.dart';
part 'bridge/rust_host_sessions.dart';
part 'bridge/rust_management.dart';

/// Real [BridgeApi] backed by generated Rust bindings.
class RustBridgeApi
    with _RustHosting, _RustThreads, _RustHostSessions, _RustManagement
    implements BridgeApi {
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
      accountId: c.accountId,
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
  Future<void> accountLogout() => frb.accountLogout();

  @override
  Future<void> accountDeregisterService({
    required String device,
    required String kind,
    required String name,
  }) => frb.accountDeregisterService(device: device, kind: kind, name: name);

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

  // --- external Codex bootstrap: provider setup, ChatGPT login, system prompt ---

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
      selectedProvider: s.selectedProvider,
      configError: s.configError,
      effectiveSource: s.effectiveSource,
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
  Future<String?> appProbeReason(String serviceKey) =>
      frb.appProbeReason(serviceKey: serviceKey);

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
  Stream<RetryProgress> metaRetryEvents() => frb.metaRetryEvents().map(
    (p) => RetryProgress(attempt: p.attempt, maxAttempts: p.maxAttempts),
  );
}
