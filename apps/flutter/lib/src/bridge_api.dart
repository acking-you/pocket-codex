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
  const SubInfo({required this.key, required this.localAddr, required this.alive});

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
  const ConfigInfo({required this.relay, required this.hasKey});

  /// Configured relay `host:port`, if any.
  final String? relay;

  /// Whether a 32-byte key is stored.
  final bool hasKey;
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
}
