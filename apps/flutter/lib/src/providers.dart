import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/src/app_modes.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/bridge_api_rust.dart';

/// The engine API. Overridden with a FakeBridgeApi in tests.
final bridgeApiProvider = Provider<BridgeApi>((ref) => const RustBridgeApi());

/// The model / permission mode / plan mode / reasoning effort the user last
/// chose, so a brand-new conversation inherits them instead of resetting to
/// hard defaults. Updated whenever the user picks a setting; read when a new
/// conversation is started. Held in memory for the app's lifetime (not yet
/// persisted to disk — survives navigating and the "new conversation" button,
/// not a full app restart).
class SessionDefaults {
  /// Creates the defaults (all optional; sensible fallbacks).
  const SessionDefaults({
    this.model,
    this.mode = PermissionMode.auto,
    this.plan = false,
    this.effort,
  });

  /// Last-chosen model (null = the server default).
  final ModelInfo? model;

  /// Last-chosen permission preset.
  final PermissionMode mode;

  /// Whether plan mode was last on.
  final bool plan;

  /// Last-chosen reasoning effort (null = model default).
  final ReasoningEffort? effort;
}

/// Holds the [SessionDefaults] a new conversation inherits, keyed by app
/// service. Per-service because a model is only valid on the service it was
/// picked from (each exposes its own model list) — a global store would leak a
/// foreign model id onto another service's first turn. Seeded on each user pick
/// (model / mode / plan / effort) in the session screen.
final sessionDefaultsProvider = StateProvider.family<SessionDefaults, String>(
  (ref, serviceKey) => const SessionDefaults(),
);

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

/// Whether an app-server service's backend is actually REACHABLE — it answers a
/// handshake — rather than merely registered on the relay. A `pb-register`
/// worker stays registered (so the relay lists the key) even when the codex
/// app-server it forwards to has died, which would otherwise show a false
/// "online". Probed lazily per service via a transient tunnel: the AsyncValue
/// is `loading` while in flight and `data(false)` for a registered-but-dead
/// backend. The services-screen refresh invalidates this to re-probe.
final appReachableProvider = FutureProvider.family<bool, String>((
  ref,
  serviceKey,
) async {
  return ref.watch(bridgeApiProvider).appProbe(serviceKey);
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
