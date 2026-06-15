import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/bridge_api_rust.dart';

/// The engine API. Overridden with a FakeBridgeApi in tests.
final bridgeApiProvider = Provider<BridgeApi>((ref) => const RustBridgeApi());

/// Current persisted config (relay + whether a key is set).
final configProvider = FutureProvider<ConfigInfo>((ref) async {
  return ref.watch(bridgeApiProvider).getConfig();
});

/// Discovered services on the configured relay. Re-run by invalidating.
final servicesProvider = FutureProvider<List<ServiceEntry>>((ref) async {
  return ref.watch(bridgeApiProvider).discoverServices();
});

/// Active local subscriptions.
final subscriptionsProvider = FutureProvider<List<SubInfo>>((ref) async {
  return ref.watch(bridgeApiProvider).subscriptions();
});

/// The set of thread ids on [serviceKey] that currently have an in-flight turn,
/// derived purely from the live event stream: `turn/started` adds a thread,
/// `turn/completed` / `turn/failed` removes it. Lets the session lists show a
/// running indicator BEFORE a session is opened, and animate when several run
/// at once. Subscribing here is safe alongside the session screen's own
/// listener — each `appEvents` call gets an independent broadcast receiver.
/// Errors (e.g. not connected yet) surface as an AsyncError; consumers treat a
/// missing value as the empty set.
///
/// Deliberately NOT autoDispose: the running set is accumulated across events,
/// and tearing the provider down between rebuilds (e.g. while navigating
/// picker↔session) would reset it and drop the badge.
///
/// Self-healing: `appEvents` errors if the service isn't connected yet (the
/// picker watches this while it's still connecting), and the stream closes on
/// disconnect. Either way we wait briefly and re-subscribe, so the badge
/// recovers once the connection is up rather than getting stuck empty.
final runningThreadsProvider = StreamProvider.family<Set<String>, String>((
  ref,
  serviceKey,
) async* {
  final api = ref.watch(bridgeApiProvider);
  final running = <String>{};
  yield const <String>{};
  while (true) {
    try {
      await for (final e in api.appEvents(serviceKey)) {
        final tid = e.threadId;
        if (tid == null || tid.isEmpty) continue;
        if (e.kind == 'turn/started') {
          running.add(tid);
          yield Set<String>.unmodifiable(running);
        } else if (e.kind == 'turn/completed' || e.kind == 'turn/failed') {
          running.remove(tid);
          yield Set<String>.unmodifiable(running);
        }
      }
    } catch (_) {
      // Not connected yet / transient drop — fall through to re-subscribe.
    }
    await Future<void>.delayed(const Duration(seconds: 1));
  }
});

/// Service key selected in the wide-layout master-detail pane (null = none,
/// falls back to the first API service). Unused on narrow layouts, which push
/// a detail route instead.
final selectedApiKeyProvider = StateProvider<String?>((ref) => null);

/// Active UI locale (`null` = follow system). Seeded at boot from the
/// persisted config via a ProviderScope override, then changed by the
/// settings language picker (which also persists through `setLocale`).
final localeProvider = StateProvider<Locale?>((ref) => null);
