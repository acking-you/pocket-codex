import 'package:pocket_codex/src/bridge_api.dart';

/// In-memory [BridgeApi] for widget/provider tests. Seed [config] and
/// services per test; records subscribe/unsubscribe calls.
class FakeBridgeApi implements BridgeApi {
  /// Creates a fake seeded with an optional [config] and [services].
  FakeBridgeApi({ConfigInfo? config, List<ServiceEntry>? services})
    : _config = config ?? const ConfigInfo(relay: null, hasKey: false),
      _services = services ?? const [];

  ConfigInfo _config;
  final List<ServiceEntry> _services;
  final Map<String, SubInfo> _subs = {};

  /// Make [discoverServices] throw, to exercise error states.
  Object? discoverError;

  @override
  Future<ConfigInfo> getConfig() async => _config;

  @override
  Future<void> setRelay(String relay) async =>
      _config = ConfigInfo(relay: relay, hasKey: _config.hasKey);

  @override
  Future<void> setKey(String key) async {
    if (key.length != 32) throw ArgumentError('key must be 32 bytes');
    _config = ConfigInfo(relay: _config.relay, hasKey: true);
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
}
