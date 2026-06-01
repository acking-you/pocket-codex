import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/rust/api/bridge.dart' as frb;

/// Real [BridgeApi] backed by the flutter_rust_bridge bindings.
class RustBridgeApi implements BridgeApi {
  /// Creates the real bridge.
  const RustBridgeApi();

  @override
  Future<ConfigInfo> getConfig() async {
    final c = await frb.getConfig();
    return ConfigInfo(relay: c.relay, hasKey: c.hasKey);
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
}
