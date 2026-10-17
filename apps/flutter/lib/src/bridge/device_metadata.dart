part of '../bridge_api.dart';

/// Metadata supplied by the process that owns a registered service.
class DeviceMetadata {
  /// Creates publisher metadata.
  const DeviceMetadata({
    required this.os,
    required this.arch,
    required this.hostname,
    required this.instanceId,
    this.environment,
    this.hostId,
    this.sourceName,
  });

  /// Publisher OS, independent of the controller's OS.
  final String os;

  /// Publisher executable architecture.
  final String arch;

  /// Runtime environment, currently `wsl`.
  final String? environment;

  /// Display hostname; never an ownership check.
  final String hostname;

  /// Stable hosting installation identity, independent of process restarts.
  final String? hostId;

  /// OS distribution or WSL distribution name reported by the publisher.
  final String? sourceName;

  /// Opaque process identity, renewed on restart.
  final String instanceId;

  /// Human-readable platform, including WSL where the host reports it.
  String get platform => [
    switch (os) {
      'windows' => 'Windows',
      'macos' => 'macOS',
      'linux' => 'Linux',
      'android' => 'Android',
      'ios' => 'iOS',
      _ => os,
    },
    if (environment == 'wsl') 'WSL',
    ?sourceName,
    arch,
  ].where((part) => part.isNotEmpty).join(' · ');

  /// Platform and a short stable instance label for selecting the right host.
  String get instanceLabel {
    final id = hostId?.replaceAll('-', '');
    return id == null || id.isEmpty
        ? platform
        : '$platform · ${id.substring(0, id.length < 8 ? id.length : 8)}';
  }
}
