import 'dart:async';

import 'package:flutter/foundation.dart'
    show defaultTargetPlatform, kIsWeb, TargetPlatform, visibleForTesting;
import 'package:flutter/material.dart';
import 'package:pocket_codex/src/widgets/window_title_bar.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/dismissed_services.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/widgets/brand_logo.dart';
import 'package:pocket_codex/src/widgets/local_host_dialog.dart';

/// Chat-first home: resolves the right app service (last used → locally
/// hosted → first reachable), connects, picks the latest conversation, and
/// lands the user straight in the chat — the phone opens ready to talk, the
/// desktop opens as a two-pane chat with every session in the sidebar.
///
/// When nothing is connectable it degrades to a branded hero with the ONE
/// action that fixes it (desktop: start hosting; phone: pointer to the
/// desktop) plus shortcuts to the management page, and keeps re-checking in
/// the background so it enters the chat by itself once a host appears.
class HomeScreen extends ConsumerStatefulWidget {
  /// Default constructor.
  const HomeScreen({super.key});

  @override
  ConsumerState<HomeScreen> createState() => _HomeScreenState();

  /// Test-only: re-arm the once-per-run auto-host restore, which is a static
  /// so a later visit to the home route can't resurrect hosting the user
  /// stopped on purpose.
  @visibleForTesting
  static void debugResetAutoHost() =>
      _HomeScreenState._autoHostAttempted = false;
}

/// Which UI the home is showing.
enum _Phase {
  /// Discovering / probing / connecting.
  resolving,

  /// Connected — the chat is on screen.
  ready,

  /// Discovery worked but there is no connectable app service.
  noService,

  /// Discovery itself failed (relay/broker unreachable, not configured…).
  discoverFailed,
}

class _HomeScreenState extends ConsumerState<HomeScreen>
    with WidgetsBindingObserver {
  /// Cadence for re-checking while stuck on a fallback screen, so the moment
  /// the user's desktop host comes up the phone slides into the chat on its
  /// own. Matches the manage page's re-probe cadence.
  static const _retryInterval = Duration(seconds: 15);

  /// How long an in-flight resolve may run before the self-heal tick is
  /// allowed to supersede it (a wedged tunnel connect would otherwise disable
  /// the retry loop forever — the generation guard makes superseding safe).
  static const _stuckAfter = Duration(seconds: 45);

  /// Auto-restoring the last hosting is attempted once per app run — a user
  /// who stops hosting afterwards must not have it resurrected behind their
  /// back by a later visit to the home route.
  static bool _autoHostAttempted = false;

  _Phase _phase = _Phase.resolving;
  String? _serviceKey;
  String? _threadId;
  String? _cwd;
  List<ServiceEntry> _candidates = const [];
  String? _error;
  // True while auto-restoring the previous hosting (shown on the splash).
  bool _rehosting = false;
  // Generation guard: a service switch / retry supersedes in-flight resolves.
  int _generation = 0;
  Timer? _retryTimer;
  bool _resolving = false;
  DateTime? _resolveStartedAt;
  bool _switching = false;

  bool get _isDesktop =>
      !kIsWeb &&
      (defaultTargetPlatform == TargetPlatform.windows ||
          defaultTargetPlatform == TargetPlatform.macOS ||
          defaultTargetPlatform == TargetPlatform.linux);

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    // Deferred so the invalidate isn't a during-build provider write. A fresh
    // discovery on mount matters after onboarding/sign-in: the provider may
    // cache a pre-login fetch (or its error), and first impressions shouldn't
    // wait for the 15s self-heal tick.
    Future.microtask(() {
      if (!mounted) return;
      ref.invalidate(servicesProvider);
      _resolve();
    });
    _retryTimer = Timer.periodic(_retryInterval, (_) => _selfHeal());
  }

  /// Re-check a failure state in place (no splash flash), and rescue a
  /// wedged resolve after [_stuckAfter].
  void _selfHeal() {
    if (!mounted || _phase == _Phase.ready) return;
    final startedAt = _resolveStartedAt;
    final stuck =
        _resolving &&
        startedAt != null &&
        DateTime.now().difference(startedAt) > _stuckAfter;
    if (_resolving && !stuck) return;
    ref.invalidate(servicesProvider);
    _resolve(background: true);
  }

  @override
  void dispose() {
    _retryTimer?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    // Coming back to the foreground on a fallback screen: re-check right away
    // (the desktop host may have been started while we were backgrounded).
    if (state == AppLifecycleState.resumed &&
        mounted &&
        _phase != _Phase.ready &&
        !_resolving) {
      ref.invalidate(servicesProvider);
      _resolve(background: true);
    }
  }

  /// Resolve → connect → land in the chat. [forceKey] pins the service (the
  /// sidebar switcher); otherwise ranking picks one. [background] re-checks
  /// without flipping the current fallback UI to the splash — the hero stays
  /// put and only a changed outcome repaints (no 15s flicker).
  Future<void> _resolve({String? forceKey, bool background = false}) async {
    final gen = ++_generation;
    _resolving = true;
    _resolveStartedAt = DateTime.now();
    if (!background) {
      setState(() {
        _phase = _Phase.resolving;
        _error = null;
      });
    }
    try {
      final api = ref.read(bridgeApiProvider);

      // 1. Discover.
      List<ServiceEntry> services;
      try {
        services = await ref.read(servicesProvider.future);
      } catch (e) {
        if (!mounted || gen != _generation) return;
        setState(() {
          _phase = _Phase.discoverFailed;
          _error = friendlyError(e);
        });
        return;
      }
      if (!mounted || gen != _generation) return;
      final dismissed = await _dismissed();
      if (!mounted || gen != _generation) return;
      List<ServiceEntry> candidates() => services
          .where((s) => s.kind == 'app' && !dismissed.contains(s.key))
          .toList(growable: false);

      // 2. Desktop: restore the hosting the user left running last time, so a
      // freshly booted desktop is immediately chattable (and phone-reachable)
      // even when OTHER hosts are discoverable. Gated on THIS machine not
      // hosting yet; best-effort — failures fall through to the hero/remote.
      var apps = candidates();
      if (_isDesktop && !_autoHostAttempted) {
        final prefs = await _prefs();
        final host = prefs.autoHost;
        var locallyHosting = false;
        if (host != null) {
          try {
            locallyHosting = (await api.appServeStatus()).isNotEmpty;
          } catch (_) {
            // Unknown local state: treat as not hosting and let the attempt
            // (or its failure) settle it.
          }
        }
        if (!mounted || gen != _generation) return;
        if (host != null && !locallyHosting) {
          // Burn the once-per-run flag only for a real attempt, so a slow
          // prefs load on the first pass doesn't forfeit the restore.
          _autoHostAttempted = true;
          if (!background) setState(() => _rehosting = true);
          try {
            await api.appServeStart(
              port: host.port,
              binaryOverride: host.binaryOverride,
              name: host.name,
              proxy: host.proxy,
              embedded: host.embedded,
            );
            ref.invalidate(localServeListProvider);
            ref.invalidate(servicesProvider);
            services = await ref.read(servicesProvider.future);
            apps = candidates();
          } catch (_) {
            // The hero (with its start-hosting action) is the fallback.
          } finally {
            if (mounted && gen == _generation && _rehosting) {
              setState(() => _rehosting = false);
            }
          }
        }
      }
      if (!mounted || gen != _generation) return;
      if (apps.isEmpty) {
        setState(() {
          _phase = _Phase.noService;
          _candidates = const [];
          if (background) _error = null;
        });
        return;
      }

      // 3. Rank: pinned switch > last used > hosted on this machine > rest.
      final prefs = await _prefs();
      final localKeys = <String>{};
      try {
        for (final h in await api.appServeStatus()) {
          localKeys.add(h.appServiceKey);
        }
      } catch (_) {
        // Local-host lookup is desktop-only sugar; ignore failures.
      }
      if (!mounted || gen != _generation) return;
      int rank(ServiceEntry s) {
        if (s.key == forceKey) return 0;
        if (s.key == prefs.lastServiceKey) return 1;
        if (localKeys.contains(s.key)) return 2;
        return 3;
      }

      final ranked = [...apps]..sort((a, b) => rank(a).compareTo(rank(b)));

      // 4. First reachable wins. An already-connected session short-circuits
      // (appProbe returns true for it without a network round-trip).
      ServiceEntry? target;
      String? lastError;
      for (final s in ranked) {
        try {
          if (await api.appProbe(s.key)) {
            target = s;
            break;
          }
        } catch (e) {
          lastError = friendlyError(e);
        }
        if (!mounted || gen != _generation) return;
      }
      if (!mounted || gen != _generation) return;
      if (target == null) {
        setState(() {
          _phase = _Phase.noService;
          _candidates = ranked;
          _error = lastError ?? AppLocalizations.of(context).unreachableReason;
        });
        return;
      }

      // 5. Connect (reuse a live session; one clean reconnect for a stale
      // half-open tunnel — same recovery the project screen uses).
      if (!api.appIsConnected(target.key)) {
        try {
          await api.appConnect(target.key, appLocalPort);
        } catch (_) {
          try {
            await api.appDisconnect(target.key);
            await api.appConnect(target.key, appLocalPort);
          } catch (e) {
            if (!mounted || gen != _generation) return;
            setState(() {
              _phase = _Phase.noService;
              _candidates = ranked;
              _error = friendlyError(e);
            });
            return;
          }
        }
      }
      if (!mounted || gen != _generation) return;

      // 6. Land in the most recent conversation (prefer the one the user last
      // had open); none → a fresh conversation with the starter guidance.
      final pick = await _pickThread(target.key, prefs);
      if (!mounted || gen != _generation) return;

      ref.read(uiPrefsProvider.notifier).setLastService(target.key);
      setState(() {
        _phase = _Phase.ready;
        _serviceKey = target!.key;
        _threadId = pick?.id;
        _cwd = pick?.cwd;
        _candidates = ranked;
      });
    } finally {
      if (gen == _generation) {
        _resolving = false;
        _resolveStartedAt = null;
      }
    }
  }

  /// The conversation the chat should open on [serviceKey]: the one the user
  /// last had open when still listed, else the most recently updated, else
  /// null (fresh conversation).
  Future<ThreadMeta?> _pickThread(String serviceKey, UiPrefs prefs) async {
    List<ThreadMeta> threads = const [];
    try {
      threads = await ref.read(bridgeApiProvider).appThreadList(serviceKey);
    } catch (_) {
      // A failed list just means we open a new conversation.
    }
    final last = prefs.lastThreadByService[serviceKey];
    for (final t in threads) {
      if (t.id == last) return t;
    }
    if (threads.isEmpty) return null;
    return threads.reduce((a, b) => a.updatedAt >= b.updatedAt ? a : b);
  }

  /// The prefs snapshot. Prefers what's already in memory; briefly waits for
  /// the initial file load otherwise (bounded — resolving the home must never
  /// hang on disk I/O), degrading to defaults.
  Future<UiPrefs> _prefs() async {
    final snap = ref.read(uiPrefsProvider).valueOrNull;
    if (snap != null) return snap;
    try {
      return await ref
          .read(uiPrefsProvider.future)
          .timeout(const Duration(seconds: 2));
    } catch (_) {
      return const UiPrefs();
    }
  }

  /// The dismissed-service keys, waiting briefly for the initial file load so
  /// the boot resolve doesn't pick an entry the user durably hid.
  Future<Set<String>> _dismissed() async {
    final snap = ref.read(dismissedServicesProvider).valueOrNull;
    if (snap != null) return snap;
    try {
      return await ref
          .read(dismissedServicesProvider.future)
          .timeout(const Duration(seconds: 2));
    } catch (_) {
      return const <String>{};
    }
  }

  /// Switch the chat to another host, keeping the current chat on screen
  /// until the target is known-good: probe + connect FIRST, and on failure
  /// stay put with a snackbar instead of tearing the conversation down.
  Future<void> _switchService(String key) async {
    if (key == _serviceKey || _switching) return;
    final l10n = AppLocalizations.of(context);
    final messenger = ScaffoldMessenger.of(context);
    // Supersede any background resolve; this switch owns the outcome now.
    final gen = ++_generation;
    setState(() => _switching = true);
    try {
      final api = ref.read(bridgeApiProvider);
      var ok = false;
      try {
        ok = await api.appProbe(key);
      } catch (_) {
        ok = false;
      }
      if (ok && !api.appIsConnected(key)) {
        try {
          await api.appConnect(key, appLocalPort);
        } catch (_) {
          try {
            await api.appDisconnect(key);
            await api.appConnect(key, appLocalPort);
          } catch (_) {
            ok = false;
          }
        }
      }
      if (!mounted || gen != _generation) return;
      if (!ok) {
        messenger.showSnackBar(
          SnackBar(content: Text(l10n.switchServiceFailed)),
        );
        return;
      }
      final pick = await _pickThread(key, await _prefs());
      if (!mounted || gen != _generation) return;
      ref.read(uiPrefsProvider.notifier).setLastService(key);
      setState(() {
        _phase = _Phase.ready;
        _serviceKey = key;
        _threadId = pick?.id;
        _cwd = pick?.cwd;
      });
    } finally {
      // The switch superseded any resolve, so clear its in-flight markers too
      // (or the self-heal tick would think a resolve is still running).
      _resolving = false;
      _resolveStartedAt = null;
      if (mounted) {
        setState(() => _switching = false);
      } else {
        _switching = false;
      }
    }
  }

  Future<void> _startHosting() async {
    await showDialog<void>(
      context: context,
      builder: (_) => const LocalHostDialog(),
    );
    if (!mounted) return;
    ref.invalidate(servicesProvider);
    _resolve();
  }

  void _retry() {
    ref.invalidate(servicesProvider);
    ref.invalidate(appReachableProvider);
    _resolve();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    switch (_phase) {
      case _Phase.ready:
        // The switcher list tracks live discovery (a host added/removed on the
        // manage page shows up without re-resolving); reachability is probed
        // on switch, so no per-entry filtering here.
        final live = ref.watch(servicesProvider).valueOrNull;
        final dismissed =
            ref.watch(dismissedServicesProvider).valueOrNull ??
            const <String>{};
        final candidates = live == null
            ? _candidates
            : live
                  .where((s) => s.kind == 'app' && !dismissed.contains(s.key))
                  .toList(growable: false);
        // The chat IS the home. Keyed by service so a switch rebuilds the
        // session state from scratch (connections stay alive underneath).
        return AppSessionScreen(
          key: ValueKey('home-$_serviceKey'),
          serviceKey: _serviceKey!,
          threadId: _threadId,
          cwd: _cwd,
          home: true,
          services: candidates,
          onSwitchService: _switchService,
        );
      case _Phase.resolving:
        return _splash(l10n);
      case _Phase.noService:
      case _Phase.discoverFailed:
        return _hero(l10n);
    }
  }

  /// App bar with the manage/settings escape hatches, shared by the splash
  /// and the hero so no home state is ever a dead end. Text only — logo tiles
  /// in a title bar read as clutter; the brand mark lives in the body hero.
  PreferredSizeWidget _homeAppBar(AppLocalizations l10n) => WindowTitleBar(
    title: Text(l10n.appTitle, overflow: TextOverflow.ellipsis),
    actions: [
      IconButton(
        key: const Key('home-manage-btn'),
        icon: const Icon(Icons.dns_outlined),
        tooltip: l10n.manageServices,
        onPressed: () => context.push('/manage'),
      ),
      IconButton(
        key: const Key('home-settings-btn'),
        icon: const Icon(Icons.settings_outlined),
        tooltip: l10n.settingsTitle,
        onPressed: () => context.push('/settings'),
      ),
    ],
  );

  /// Branded connecting splash (cold start / explicit retry).
  Widget _splash(AppLocalizations l10n) => Scaffold(
    appBar: _homeAppBar(l10n),
    body: Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const BrandLogo(size: 72),
          const SizedBox(height: 18),
          Text(l10n.appTitle, style: Theme.of(context).textTheme.titleLarge),
          const SizedBox(height: 22),
          const SizedBox(
            width: 200,
            child: LinearProgressIndicator(minHeight: 3),
          ),
          const SizedBox(height: 14),
          Text(
            _rehosting ? l10n.homeRestoringHost : l10n.homeConnecting,
            key: const Key('home-splash-status'),
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ],
      ),
    ),
  );

  /// Fallback hero: what's wrong + the one action that fixes it, and the
  /// management/settings escape hatches.
  Widget _hero(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    final discoverFailed = _phase == _Phase.discoverFailed;
    final config = ref.watch(configProvider).valueOrNull;
    final account = config?.mode == 'account';
    // Hosting from the hero mirrors the manage page's gate (desktop + account).
    final canHost = _isDesktop && account;
    final hint = discoverFailed
        ? (_error ?? '')
        : canHost
        ? l10n.homeNoServiceDesktopHint
        : _isDesktop
        // Self-host desktop: in-app hosting is account-only, so point at
        // the CLI (and the account path) instead of a button that isn't
        // there / a "use your computer" hint that IS this computer.
        ? l10n.homeNoServiceSelfHostHint
        : l10n.homeNoServiceMobileHint;
    return Scaffold(
      appBar: _homeAppBar(l10n),
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 440),
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                const BrandLogo(size: 72),
                const SizedBox(height: 20),
                Text(
                  discoverFailed
                      ? l10n.discoverFailed
                      : l10n.homeNoServiceTitle,
                  key: const Key('home-hero-title'),
                  style: Theme.of(context).textTheme.titleMedium,
                  textAlign: TextAlign.center,
                ),
                const SizedBox(height: 10),
                Text(
                  hint,
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: scheme.onSurfaceVariant,
                  ),
                  textAlign: TextAlign.center,
                ),
                if (!discoverFailed && _error != null) ...[
                  const SizedBox(height: 8),
                  Text(
                    _error!,
                    key: const Key('home-hero-error'),
                    style: TextStyle(fontSize: 12, color: scheme.error),
                    textAlign: TextAlign.center,
                  ),
                ],
                const SizedBox(height: 22),
                if (canHost && !discoverFailed) ...[
                  FilledButton.icon(
                    key: const Key('home-start-hosting-btn'),
                    onPressed: _startHosting,
                    icon: const Icon(Icons.rocket_launch_outlined),
                    label: Text(l10n.startHosting),
                  ),
                  const SizedBox(height: 10),
                ],
                Row(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    OutlinedButton.icon(
                      key: const Key('home-retry-btn'),
                      onPressed: _retry,
                      icon: const Icon(Icons.refresh, size: 18),
                      label: Text(l10n.retry),
                    ),
                    const SizedBox(width: 10),
                    TextButton(
                      key: const Key('home-open-manage-btn'),
                      onPressed: () => context.push('/manage'),
                      child: Text(l10n.manageServices),
                    ),
                  ],
                ),
                const SizedBox(height: 16),
                Text(
                  l10n.homeAutoRetryNote,
                  style: TextStyle(fontSize: 11.5, color: scheme.outline),
                  textAlign: TextAlign.center,
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
