import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';

/// Metadata of the selected publisher only. The provider owns the in-flight
/// read and is discarded on account/relay changes or leaving the device.
final serviceDeviceMetadataProvider = FutureProvider.autoDispose
    .family<DeviceMetadata?, String>((ref, key) {
      ref.watch(configProvider);
      return ref.watch(bridgeApiProvider).serviceDeviceMetadata(key);
    });

/// A remote computer's management view, by its `host:` service key. Reads
/// only: invalidating it never starts or stops anything.
final managedDeviceProvider = FutureProvider.autoDispose
    .family<ManagedDevice, String>((ref, serviceKey) {
      ref.watch(configProvider);
      return ref.watch(bridgeApiProvider).managedDevice(serviceKey);
    });

/// Runtimes installed on a remote computer. Discovery never installs or
/// signs in.
final managedRuntimesProvider = FutureProvider.autoDispose
    .family<List<ManagedRuntimeAvailability>, String>(
      (ref, serviceKey) =>
          ref.watch(bridgeApiProvider).managedRuntimes(serviceKey),
    );

/// This computer's remote-management switch.
final deviceManagementProvider = FutureProvider<DeviceManagement>(
  (ref) => ref.watch(bridgeApiProvider).deviceManagementStatus(),
);

/// This computer's restart-persistence warning for one host (any host when
/// null). Separate from whether a start or apply succeeded.
final hostRestoreProblemProvider = FutureProvider.autoDispose
    .family<String?, String?>(
      (ref, name) =>
          ref.watch(bridgeApiProvider).hostRestoreProblem(name: name),
    );

/// Why boot restore was skipped entirely (its authoritative records could
/// not be read); null when it ran.
final hostRestoreBlockedProvider = StateProvider<String?>((_) => null);
