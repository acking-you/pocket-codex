import 'dart:async';

import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/src/app_modes.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/bridge_api_rust.dart';
import 'package:pocket_codex/src/web_authenticator.dart';

/// The engine API. Overridden with a FakeBridgeApi in tests.
final bridgeApiProvider = Provider<BridgeApi>((ref) => const RustBridgeApi());

/// Drives the browser-redirect login tab. Overridden with a fake in tests so the
/// onboarding flow runs without the real platform-channel plugin.
final webAuthenticatorProvider = Provider<WebAuthenticator>(
  (ref) => const FlutterWebAuthenticator(),
);

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

/// Whether an API proxy is actually reachable (its host answers a minimal HTTP
/// request) vs merely registered on the relay — the API analogue of
/// [`appReachableProvider`], so a dead-but-registered proxy reads unreachable.
final apiReachableProvider = FutureProvider.family<bool, String>((
  ref,
  serviceKey,
) async {
  return ref.watch(bridgeApiProvider).apiProbe(serviceKey);
});

/// Whether an app-server THIS machine hosts itself actually answers, probed by
/// its loopback app-listen `host:port` (no relay hop). Unlike a host's bare
/// port-open `alive` flag this is a real `initialize` handshake, so a wedged /
/// half-open local codex reads `false` instead of a false "running"; and it is
/// fast (loopback), so it flips green the instant the backend is up. Keyed by
/// local address so it re-probes per host. Invalidated on the same cadence as
/// [`appReachableProvider`].
final appReachableLocalProvider = FutureProvider.family<bool, String>((
  ref,
  localAddr,
) async {
  return ref.watch(bridgeApiProvider).appProbeLocal(localAddr);
});

/// Loopback-direct reachability of an API proxy THIS machine hosts (the API
/// analogue of [`appReachableLocalProvider`]): a real HTTP probe with no relay
/// round-trip, so a local host's API tunnel reads "online" immediately.
final apiReachableLocalProvider = FutureProvider.family<bool, String>((
  ref,
  localAddr,
) async {
  return ref.watch(bridgeApiProvider).apiProbeLocal(localAddr);
});

/// Every locally-hosted codex app-server (the app's own `serve` hosts), for the
/// desktop local-hosting block. Invalidated by the services-screen re-probe
/// timer + the refresh button, and after start/stop.
final localServeListProvider = FutureProvider<List<AppServeStatus>>((
  ref,
) async {
  return ref.watch(bridgeApiProvider).appServeStatus();
});

/// The 自带 codex's config/credential status on THIS machine (config.toml /
/// auth.json / custom provider). The chat uses it to guide the user to the
/// setup wizard when the local host's codex can't make model calls yet. Refresh
/// via `ref.invalidate` after the setup wizard changes anything.
final codexSetupStatusProvider = FutureProvider<CodexSetupStatus>((ref) async {
  return ref.watch(bridgeApiProvider).codexSetupStatus();
});

/// Service keys for one of OUR OWN local tunnels the user just deregistered or
/// stopped, hidden from the service lists optimistically so the entry vanishes
/// at once. Only such keys go here — they reliably leave the relay — and each is
/// cleared once discovery confirms it ABSENT (so a key the relay hasn't finished
/// dropping doesn't flicker back). A foreign best-effort 注销 is NOT added here
/// (a still-running host re-registers, and hiding it would strand a live entry).
final pendingRemovalProvider = StateProvider<Set<String>>((ref) => {});

/// A conversation's one-line summary, fetched on demand and cached.
///
/// Keyed by `serviceKey|threadId`. Deliberately NOT autoDispose: a row scrolled
/// out and back must not re-read the thread, and the whole point of fetching
/// lazily is to pay for each conversation at most once per session. The read is
/// expensive (a full thread history server-side), so the activity view asks for
/// these only for rows it actually renders.
///
/// A failure yields null rather than an error state: a missing gist is a row
/// with one less line, not something to interrupt the list for.
final threadSummaryProvider = FutureProvider.family<String?, String>((
  ref,
  key,
) async {
  final sep = key.indexOf('|');
  if (sep <= 0) return null;
  final serviceKey = key.substring(0, sep);
  final threadId = key.substring(sep + 1);
  try {
    return await ref
        .watch(bridgeApiProvider)
        .appThreadSummary(serviceKey, threadId);
  } catch (_) {
    return null;
  }
});

/// Cache key for [threadSummaryProvider].
String threadSummaryKey(String serviceKey, String threadId) =>
    '$serviceKey|$threadId';

/// A host meta request currently being retried, or null when nothing is.
///
/// Drives the "retrying 2/10" line in `ErrorRetry`. Each tick auto-clears after
/// a moment: the bridge only reports that an attempt FAILED, never that the
/// retry finally succeeded (success arrives as the request's own result), so a
/// sticky value would leave a stale "retrying" line on screen forever.
final metaRetryProvider = StreamProvider<RetryProgress?>((ref) {
  final api = ref.watch(bridgeApiProvider);
  final out = StreamController<RetryProgress?>();
  Timer? clear;
  final sub = api.metaRetryEvents().listen((p) {
    out.add(p);
    clear?.cancel();
    // Comfortably longer than the backoff's own ceiling, so consecutive
    // attempts keep the line up, and a resolved request drops it.
    clear = Timer(const Duration(seconds: 4), () => out.add(null));
  }, onError: (_) {});
  ref.onDispose(() {
    clear?.cancel();
    sub.cancel();
    out.close();
  });
  return out.stream;
});

/// Threads with a turn currently running on [serviceKey], as a live set,
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
  // Cancellable re-subscribe backoff. A plain `Future.delayed` would leave a
  // pending timer when the provider is torn down (container disposal in tests,
  // or invalidation), so gate the wait on a Timer we cancel in onDispose.
  var disposed = false;
  Timer? backoff;
  ref.onDispose(() {
    disposed = true;
    backoff?.cancel();
  });
  yield const <String>{};
  while (!disposed) {
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
    if (disposed) break;
    final gate = Completer<void>();
    backoff = Timer(const Duration(seconds: 1), () {
      if (!gate.isCompleted) gate.complete();
    });
    await gate.future;
  }
});

/// Service key selected in the wide-layout master-detail pane (null = none,
/// falls back to the first API service). Unused on narrow layouts, which push
/// a detail route instead.
final selectedApiKeyProvider = StateProvider<String?>((ref) => null);

/// Selected home-screen section, as a stable key (not an index, so adding/hiding
/// a section — e.g. Local hosting only on desktop — never mis-selects). Rendered
/// as a bottom `NavigationBar` on mobile and a side `NavigationRail` on desktop.
enum ServicesSection {
  /// Responses-API proxy services.
  api,

  /// App-server (remote codex control) services.
  appServer,

  /// Remote-viewable host sessions (pick a connected app-server, then browse its
  /// host's CODEX_HOME sessions over the meta tunnel).
  sessions,

  /// This machine's locally-hosted codex servers (desktop + account only).
  hosting,
}

/// The app-server service key selected in the Sessions tab's host picker (null =
/// fall back to the first connected app-server). Its host's sessions are listed
/// over that service's meta tunnel.
final sessionsHostKeyProvider = StateProvider<String?>((ref) => null);

/// The currently-selected home-screen section tab.
final servicesSectionProvider = StateProvider<ServicesSection>(
  (ref) => ServicesSection.api,
);

/// Multi-select state for batch-removing inactive service entries. `active`
/// gates the checkbox UI; `keys` are the currently-ticked service keys. Only
/// unreachable, non-local entries are ever selectable — reachable or locally
/// hosted services must never be dismissed (a dismissal is keyed on being
/// unreachable, and hiding a live one would strand it), mirroring the
/// single-entry 注销 rule.
@immutable
class ServiceSelection {
  /// Creates a selection state (inactive + empty by default).
  const ServiceSelection({this.active = false, this.keys = const {}});

  /// Whether multi-select mode is on (checkboxes shown, taps toggle).
  final bool active;

  /// The ticked service keys.
  final Set<String> keys;

  /// Whether [key] is currently ticked.
  bool contains(String key) => keys.contains(key);

  /// Number of ticked keys.
  int get count => keys.length;

  /// Selection with [key] toggled (and mode forced on).
  ServiceSelection toggled(String key) {
    final next = {...keys};
    if (!next.remove(key)) next.add(key);
    return ServiceSelection(active: true, keys: next);
  }

  /// Selection with exactly [all] ticked (and mode on) — the "select all".
  ServiceSelection withAll(Iterable<String> all) =>
      ServiceSelection(active: true, keys: {...all});

  /// Selection with [removed] keys dropped, keeping mode + the rest.
  ServiceSelection without(Iterable<String> removed) =>
      ServiceSelection(active: active, keys: {...keys}..removeAll(removed));
}

/// Per-section multi-select state, so entering select mode in one tab (API vs
/// app-server) never shows checkboxes in another. Not autoDispose (tiny state,
/// preserved across a glance at another tab), matching [pendingRemovalProvider].
final serviceSelectionProvider =
    StateProvider.family<ServiceSelection, ServicesSection>(
      (ref, section) => const ServiceSelection(),
    );

/// Active UI locale (`null` = follow system). Seeded at boot from the
/// persisted config via a ProviderScope override, then changed by the
/// settings language picker (which also persists through `setLocale`).
final localeProvider = StateProvider<Locale?>((ref) => null);
