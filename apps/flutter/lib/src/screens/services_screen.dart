import 'dart:async';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/api_service_screen.dart';
import 'package:pocket_codex/src/widgets/brand_logo.dart';
import 'package:pocket_codex/src/widgets/loading.dart';
import 'package:pocket_codex/src/widgets/status_dots.dart';

/// Home screen: lists discovered services on the configured relay.
class ServicesScreen extends ConsumerStatefulWidget {
  /// Default constructor.
  const ServicesScreen({super.key});

  @override
  ConsumerState<ServicesScreen> createState() => _ServicesScreenState();
}

class _ServicesScreenState extends ConsumerState<ServicesScreen>
    with WidgetsBindingObserver {
  /// Cadence for re-probing every app-server's reachability so a server that
  /// came back online flips from "unreachable" to "online" on its own — the
  /// manual refresh button stays as a fallback. Kept in the same order of
  /// magnitude as the session keepalive while staying gentle enough to avoid
  /// probe churn against the remote app-server.
  static const _reprobeInterval = Duration(seconds: 15);

  Timer? _reprobeTimer;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _reprobeTimer = Timer.periodic(_reprobeInterval, (_) {
      // Re-probe each service's reachability (app-server + API proxy), not the
      // full discovery: cheap, and the thing that goes stale when a remote
      // server is restarted out from under us.
      if (mounted) {
        ref.invalidate(appReachableProvider);
        ref.invalidate(apiReachableProvider);
        ref.invalidate(localServeListProvider);
      }
    });
  }

  @override
  void dispose() {
    _reprobeTimer?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    // Returning to the foreground: re-probe once immediately so a server that
    // recovered while we were backgrounded shows online without waiting a tick.
    if (state == AppLifecycleState.resumed && mounted) {
      ref.invalidate(appReachableProvider);
      ref.invalidate(apiReachableProvider);
      // Refresh local hosts too (a host's codex/tunnels may have changed while
      // backgrounded) — same as the periodic timer + the refresh button.
      ref.invalidate(localServeListProvider);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final servicesAsync = ref.watch(servicesProvider);
    final config = ref.watch(configProvider).valueOrNull;
    final selectedKey = ref.watch(selectedApiKeyProvider);
    return Scaffold(
      appBar: AppBar(
        title: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            const BrandLogo(size: 26, plated: false),
            const SizedBox(width: 10),
            Flexible(
              child: Text(l10n.appTitle, overflow: TextOverflow.ellipsis),
            ),
          ],
        ),
        actions: [
          IconButton(
            key: const Key('refresh-btn'),
            icon: const Icon(Icons.refresh),
            tooltip: l10n.refreshStatus,
            // Re-discover services, re-read subscription health, and re-probe
            // every app-server's backend reachability, then rebuild so each
            // status re-evaluates.
            onPressed: () {
              ref.invalidate(servicesProvider);
              ref.invalidate(subscriptionsProvider);
              ref.invalidate(appReachableProvider);
              ref.invalidate(apiReachableProvider);
              ref.invalidate(localServeListProvider);
            },
          ),
          IconButton(
            key: const Key('local-sessions-btn'),
            icon: const Icon(Icons.history),
            tooltip: l10n.localSessions,
            onPressed: () => context.push('/sessions'),
          ),
          IconButton(
            key: const Key('settings-btn'),
            icon: const Icon(Icons.settings),
            onPressed: () => context.push('/settings'),
          ),
        ],
      ),
      body: AnimatedSwitcher(
        duration: const Duration(milliseconds: 250),
        child: servicesAsync.when(
          loading: () =>
              const ListLoadingSkeleton(key: ValueKey('svc-loading')),
          error: (e, _) => KeyedSubtree(
            key: const ValueKey('svc-error'),
            child: _ErrorState(
              detail: friendlyError(e),
              onRetry: () => ref.invalidate(servicesProvider),
            ),
          ),
          data: (services) => RefreshIndicator(
            key: const ValueKey('svc-data'),
            onRefresh: () async => ref.invalidate(servicesProvider),
            child: LayoutBuilder(
              builder: (context, c) {
                final wide = c.maxWidth >= 600;
                final apiServices = services
                    .where((s) => s.kind == 'api')
                    .toList();
                final selected =
                    apiServices
                        .where((s) => s.key == selectedKey)
                        .firstOrNull ??
                    apiServices.firstOrNull;
                final list = _ServiceList(
                  relay: config?.relay,
                  accountLogin: config?.mode == 'account'
                      ? config?.accountLogin
                      : null,
                  services: services,
                  // Only the inline detail pane has a "current" service worth
                  // highlighting; in narrow mode tapping pushes a route instead.
                  highlightKey: wide ? selected?.key : null,
                  onTapApi: (key) {
                    if (wide) {
                      ref.read(selectedApiKeyProvider.notifier).state = key;
                    } else {
                      context.push('/api/$key');
                    }
                  },
                  // App-server sessions are a full-screen chat; always push a
                  // route (no embedded pane) regardless of layout width.
                  onTapApp: (key) =>
                      context.push('/app/${Uri.encodeComponent(key)}'),
                );
                if (!wide) return list;
                return Row(
                  children: [
                    SizedBox(width: 360, child: list),
                    const VerticalDivider(width: 1),
                    Expanded(
                      child: selected == null
                          ? Center(child: Text(l10n.selectApiService))
                          // Keep the detail readable instead of stretching the
                          // form across a wide pane: a centred, capped column.
                          : Center(
                              child: ConstrainedBox(
                                constraints: const BoxConstraints(
                                  maxWidth: 460,
                                ),
                                child: ApiServiceScreen(
                                  key: ValueKey(selected.key),
                                  serviceKey: selected.key,
                                  embedded: true,
                                ),
                              ),
                            ),
                    ),
                  ],
                );
              },
            ),
          ),
        ),
      ),
    );
  }
}

class _ServiceList extends ConsumerWidget {
  const _ServiceList({
    required this.relay,
    required this.accountLogin,
    required this.services,
    required this.onTapApi,
    required this.onTapApp,
    this.highlightKey,
  });
  final String? relay;

  /// The signed-in GitHub login in account mode (null in self-host mode), shown
  /// in the header card instead of a relay address.
  final String? accountLogin;
  final List<ServiceEntry> services;
  final void Function(String key) onTapApi;
  final void Function(String key) onTapApp;

  /// The service key to render selected (a 2px accent border) — the inline
  /// detail pane's current service in the wide layout; null otherwise.
  final String? highlightKey;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final online = Colors.green.shade600;
    // Optimistically hide a just-removed service so it vanishes at once. When
    // fresh discovery data lands, stop hiding keys that are now ABSENT (confirmed
    // gone) — keys still present stay hidden, so a deregister that the relay
    // hasn't finished processing doesn't flicker back as 不可达.
    final pending = ref.watch(pendingRemovalProvider);
    ref.listen(servicesProvider, (_, next) {
      final data = next.valueOrNull;
      if (data == null) return;
      final present = {for (final s in data) s.key};
      final current = ref.read(pendingRemovalProvider);
      final stillHidden = current.intersection(present);
      if (stillHidden.length != current.length) {
        ref.read(pendingRemovalProvider.notifier).state = stillHidden;
      }
    });
    final api = services
        .where((s) => s.kind == 'api' && !pending.contains(s.key))
        .toList();
    final app = services
        .where((s) => s.kind == 'app' && !pending.contains(s.key))
        .toList();
    // Tunnels this machine hosts itself (its own `serve`): each host publishes
    // an app + an api tunnel. Used to relabel their discovery cards as 本地托管
    // and to route a card 注销 to a reversible serve-deregister (vs the backend
    // force-drop for someone else's service).
    final localHosts =
        ref.watch(localServeListProvider).valueOrNull ??
        const <AppServeStatus>[];
    final localTunnels = <String, ({String name, String kind})>{
      for (final h in localHosts) ...{
        h.appServiceKey: (name: h.name, kind: 'app'),
        h.apiServiceKey: (name: h.name, kind: 'api'),
      },
    };
    // Live subscription health, keyed by service key (alive/dead). A discovered
    // service is by definition currently registered on the relay → "online".
    final subs = {
      for (final s in ref.watch(subscriptionsProvider).valueOrNull ?? const [])
        s.key: s,
    };
    final bridge = ref.watch(bridgeApiProvider);

    // Per-API status. Subscribed → the live tunnel's alive/dropped flag.
    // Otherwise PROBE the proxy (like the app-server) so a registered-but-dead
    // api-proxy reads "unreachable" instead of a false green "online", with the
    // reason spelled out.
    (StatusChip, String?) apiStatus(ServiceEntry s) {
      final sub = subs[s.key];
      if (sub != null) {
        return sub.alive
            ? (
                StatusChip(
                  color: online,
                  label: l10n.subscribedAlive,
                  filled: true,
                ),
                null,
              )
            : (
                StatusChip(
                  color: scheme.error,
                  label: l10n.subscribedDead,
                  filled: true,
                ),
                null,
              );
      }
      return ref
          .watch(apiReachableProvider(s.key))
          .when(
            data: (ok) => ok
                ? (
                    StatusChip(
                      color: online,
                      label: l10n.statusOnline,
                      filled: true,
                    ),
                    null,
                  )
                : (
                    StatusChip(
                      color: scheme.error,
                      label: l10n.statusUnreachable,
                      filled: true,
                    ),
                    l10n.apiUnreachableReason,
                  ),
            loading: () => (
              StatusChip(
                color: scheme.outline,
                label: l10n.statusChecking,
                filled: true,
              ),
              null,
            ),
            error: (_, _) => (
              StatusChip(
                color: scheme.error,
                label: l10n.statusUnreachable,
                filled: true,
              ),
              l10n.apiUnreachableReason,
            ),
          );
    }

    return ListView(
      padding: const EdgeInsets.fromLTRB(12, 12, 12, 16),
      children: [
        _RelayBanner(
          relay: relay ?? l10n.relayNotConfigured,
          accountLogin: accountLogin,
          online: online,
        ),
        if (api.isNotEmpty) _SectionHeader(l10n.apiServicesSection),
        ...api.map((s) {
          final (status, reason) = apiStatus(s);
          return _ServiceCard(
            key: Key('svc-${s.key}'),
            icon: Icons.api,
            iconBg: scheme.secondaryContainer,
            iconFg: scheme.onSecondaryContainer,
            title: s.name,
            subtitle: s.device,
            selected: s.key == highlightKey,
            reason: reason,
            status: status,
            onTap: () => onTapApi(s.key),
            onDeregister: () => _confirmDeregister(
              context,
              ref,
              s,
              localTunnel: localTunnels[s.key],
            ),
          );
        }),
        if (app.isNotEmpty) _SectionHeader(l10n.appServerServices),
        ...app.map((s) {
          // An app-server we host locally also shows here (to drive it); label
          // it "本地托管" with the host icon instead of "远程控制", so it reads
          // as the same instance the 本地托管 section manages, not a separate one.
          final isLocal = localTunnels.containsKey(s.key);
          final connected = bridge.appIsConnected(s.key);
          // "Registered on the relay" is NOT "reachable": a pb-register worker
          // can outlive the codex app-server it forwards to, leaving a hollow
          // registration. Probe the real backend so a dead one reads
          // "unreachable" instead of a false green "online".
          final reach = ref.watch(appReachableProvider(s.key));
          // `reason` is non-null only when unreachable: the backend probe failed
          // even though the relay still lists the registration, so spell out
          // that the dead link is the remote app-server, not the relay.
          final (
            Color statusColor,
            String statusLabel,
            String? reason,
          ) = connected
              ? (online, l10n.statusConnected, null)
              : reach.when(
                  data: (ok) => ok
                      ? (online, l10n.statusOnline, null)
                      : (
                          scheme.error,
                          l10n.statusUnreachable,
                          l10n.unreachableReason,
                        ),
                  loading: () => (scheme.outline, l10n.statusChecking, null),
                  error: (_, _) => (
                    scheme.error,
                    l10n.statusUnreachable,
                    l10n.unreachableReason,
                  ),
                );
          return _ServiceCard(
            key: Key('svc-${s.key}'),
            icon: isLocal ? Icons.dns : Icons.computer,
            iconBg: scheme.tertiaryContainer,
            iconFg: scheme.onTertiaryContainer,
            title: s.name,
            subtitle: isLocal
                ? l10n.appServerSubtitleLocal(s.device)
                : l10n.appServerSubtitle(s.device),
            reason: reason,
            chevron: true,
            status: StatusChip(
              color: statusColor,
              label: statusLabel,
              filled: true,
            ),
            onTap: () => onTapApp(s.key),
            onDeregister: () => _confirmDeregister(
              context,
              ref,
              s,
              localTunnel: localTunnels[s.key],
            ),
          );
        }),
        // Desktop + account mode: host this machine's codex app-server(s) from
        // the app (the in-app equivalent of `pocket-codex serve`). Several can
        // run at once. Hidden on mobile (no local codex) and in self-host mode
        // (account-only for now).
        if (_hostingSupported && accountLogin != null) ...[
          _SectionHeader(l10n.localHostingSection),
          ...localHosts.map(
            (h) => _LocalHostCard(key: Key('local-host-${h.name}'), host: h),
          ),
          const _AddLocalHostCard(),
        ],
        if (services.isEmpty)
          Padding(
            padding: const EdgeInsets.all(32),
            child: Center(child: Text(l10n.noServicesFound)),
          ),
      ],
    );
  }
}

/// Local hosting spawns a local `codex` binary + child processes — desktop only.
bool get _hostingSupported =>
    !kIsWeb &&
    (defaultTargetPlatform == TargetPlatform.windows ||
        defaultTargetPlatform == TargetPlatform.macOS ||
        defaultTargetPlatform == TargetPlatform.linux);

/// Confirm, then take a service's tunnel off the relay. For one of *our* local
/// hosts ([localTunnel] set) this is a reversible unpublish — the codex / API
/// proxy keep running and the 本地托管 card can re-register it. For someone
/// else's service it asks the backend to force-drop the relay key (best-effort —
/// a still-running host re-registers). The key is hidden at once via
/// [pendingRemovalProvider].
Future<void> _confirmDeregister(
  BuildContext context,
  WidgetRef ref,
  ServiceEntry s, {
  ({String name, String kind})? localTunnel,
}) async {
  final l10n = AppLocalizations.of(context);
  final scheme = Theme.of(context).colorScheme;
  final isLocal = localTunnel != null;
  final ok = await showDialog<bool>(
    context: context,
    builder: (_) => AlertDialog(
      key: const Key('deregister-dialog'),
      title: Text(l10n.deregisterTitle),
      content: Text(
        isLocal
            ? l10n.deregisterLocalWarning(s.name)
            : l10n.deregisterWarning(s.name),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(false),
          child: Text(l10n.cancel),
        ),
        FilledButton(
          key: const Key('deregister-confirm-btn'),
          style: FilledButton.styleFrom(backgroundColor: scheme.error),
          onPressed: () => Navigator.of(context).pop(true),
          child: Text(l10n.deregister),
        ),
      ],
    ),
  );
  if (ok != true) return;
  try {
    if (localTunnel != null) {
      // Reversible: stop this tunnel's register task (codex/proxy keep running);
      // serve_deregister also best-effort force-drops the relay key. Our own
      // tunnel reliably leaves discovery, so optimistically hide it at once.
      await ref
          .read(bridgeApiProvider)
          .appServeDeregister(name: localTunnel.name, kind: localTunnel.kind);
      ref
          .read(pendingRemovalProvider.notifier)
          .update((set) => {...set, s.key});
      ref.invalidate(localServeListProvider);
    } else {
      // Someone else's service: best-effort ask the backend to drop the relay
      // key. Do NOT optimistically hide it — a still-running host re-registers
      // within seconds, and hiding would strand a live service off the list.
      await ref
          .read(bridgeApiProvider)
          .accountDeregisterService(
            device: s.device,
            kind: s.kind,
            name: s.name,
          );
    }
    ref.invalidate(servicesProvider);
  } catch (e) {
    if (context.mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('${l10n.deregisterFailed}: ${friendlyError(e)}'),
        ),
      );
    }
  }
}

/// The header card: in account mode the signed-in GitHub identity; in self-host
/// mode the configured relay. Status is implicitly online — the list only
/// renders once discovery (which needs a valid session/relay) has succeeded.
class _RelayBanner extends StatelessWidget {
  const _RelayBanner({
    required this.relay,
    required this.accountLogin,
    required this.online,
  });
  final String relay;

  /// Non-null in account mode: the signed-in GitHub login (shown instead of a
  /// relay address, so an account user never sees "(no relay configured)").
  final String? accountLogin;
  final Color online;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final account = accountLogin != null;
    return Container(
      margin: const EdgeInsets.only(bottom: 8),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: scheme.primaryContainer.withValues(alpha: 0.45),
        borderRadius: BorderRadius.circular(16),
        border: Border.all(color: scheme.primary.withValues(alpha: 0.18)),
      ),
      child: Row(
        children: [
          _IconBadge(
            icon: account ? Icons.account_circle : Icons.dns,
            bg: scheme.primaryContainer,
            fg: scheme.onPrimaryContainer,
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  account ? '@$accountLogin' : relay,
                  style: Theme.of(context).textTheme.titleSmall,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
                const SizedBox(height: 1),
                Text(
                  account ? l10n.accountSection : l10n.relayRow,
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: scheme.onSurfaceVariant,
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(width: 8),
          StatusChip(color: online, label: l10n.statusOnline, filled: true),
        ],
      ),
    );
  }
}

/// A discovered service, rendered as a tappable card: tinted icon badge, name +
/// device, an optional unreachable reason, and a filled status pill.
class _ServiceCard extends StatelessWidget {
  const _ServiceCard({
    super.key,
    required this.icon,
    required this.iconBg,
    required this.iconFg,
    required this.title,
    required this.subtitle,
    required this.status,
    required this.onTap,
    this.reason,
    this.chevron = false,
    this.selected = false,
    this.onDeregister,
  });

  final IconData icon;
  final Color iconBg;
  final Color iconFg;
  final String title;
  final String subtitle;

  /// Non-null only for an unreachable app-server: the "why" line under the
  /// subtitle (relay registration up, remote backend down).
  final String? reason;
  final StatusChip status;
  final VoidCallback onTap;
  final bool chevron;
  final bool selected;

  /// When set, an overflow menu offers a 注销 (deregister) action.
  final VoidCallback? onDeregister;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Material(
        color: scheme.surfaceContainerLow,
        borderRadius: BorderRadius.circular(14),
        child: InkWell(
          onTap: onTap,
          borderRadius: BorderRadius.circular(14),
          child: Container(
            decoration: BoxDecoration(
              borderRadius: BorderRadius.circular(14),
              border: Border.all(
                color: selected ? scheme.primary : scheme.outlineVariant,
                width: selected ? 2 : 1,
              ),
            ),
            padding: const EdgeInsets.all(12),
            child: Row(
              children: [
                _IconBadge(icon: icon, bg: iconBg, fg: iconFg),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        title,
                        style: Theme.of(context).textTheme.titleSmall,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                      const SizedBox(height: 2),
                      Text(
                        subtitle,
                        style: Theme.of(context).textTheme.bodySmall?.copyWith(
                          color: scheme.onSurfaceVariant,
                        ),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                      if (reason != null) ...[
                        const SizedBox(height: 3),
                        Text(
                          reason!,
                          style: Theme.of(
                            context,
                          ).textTheme.bodySmall?.copyWith(color: scheme.error),
                        ),
                      ],
                    ],
                  ),
                ),
                const SizedBox(width: 8),
                status,
                if (chevron) ...[
                  const SizedBox(width: 2),
                  Icon(Icons.chevron_right, color: scheme.outline),
                ],
                if (onDeregister != null)
                  PopupMenuButton<String>(
                    icon: Icon(Icons.more_vert, color: scheme.outline),
                    padding: EdgeInsets.zero,
                    onSelected: (_) => onDeregister!(),
                    itemBuilder: (context) => [
                      PopupMenuItem(
                        value: 'deregister',
                        child: Row(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            Icon(Icons.link_off, size: 18, color: scheme.error),
                            const SizedBox(width: 8),
                            Text(AppLocalizations.of(context).deregister),
                          ],
                        ),
                      ),
                    ],
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// A rounded, theme-tinted icon tile — the visual anchor on relay/service rows.
class _IconBadge extends StatelessWidget {
  const _IconBadge({required this.icon, required this.bg, required this.fg});
  final IconData icon;
  final Color bg;
  final Color fg;

  @override
  Widget build(BuildContext context) => Container(
    width: 42,
    height: 42,
    decoration: BoxDecoration(
      color: bg,
      borderRadius: BorderRadius.circular(12),
    ),
    child: Icon(icon, size: 22, color: fg),
  );
}

class _SectionHeader extends StatelessWidget {
  const _SectionHeader(this.text);
  final String text;
  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.fromLTRB(6, 16, 6, 6),
    child: Text(
      text,
      style: Theme.of(context).textTheme.labelMedium?.copyWith(
        letterSpacing: .5,
        color: Theme.of(context).colorScheme.onSurfaceVariant,
      ),
    ),
  );
}

/// One locally-hosted host: a codex app-server + an in-app API proxy, each
/// published through its own tunnel. The card shows codex's liveness and both
/// tunnels' publish state, with a per-tunnel 注销 / 重新注册 toggle. Tapping the
/// header opens [_LocalHostDialog] for the full 停止托管 + details.
class _LocalHostCard extends ConsumerWidget {
  const _LocalHostCard({super.key, required this.host});

  final AppServeStatus host;

  /// Synthesize the discovery entry for one of this host's tunnels, so the
  /// shared [_confirmDeregister] flow (confirm + optimistic hide) can run.
  ServiceEntry _entry(String kind) => ServiceEntry(
    device: host.device,
    kind: kind,
    name: host.name,
    key: kind == 'api' ? host.apiServiceKey : host.appServiceKey,
  );

  Future<void> _reregister(WidgetRef ref, String kind) async {
    final key = kind == 'api' ? host.apiServiceKey : host.appServiceKey;
    await ref
        .read(bridgeApiProvider)
        .appServeReregister(name: host.name, kind: kind);
    // Make sure it isn't still optimistically hidden, then re-discover.
    ref
        .read(pendingRemovalProvider.notifier)
        .update((s) => s.difference({key}));
    ref.invalidate(localServeListProvider);
    ref.invalidate(servicesProvider);
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final online = Colors.green.shade600;
    final codexChip = StatusChip(
      color: host.alive ? online : scheme.tertiary,
      label: host.alive ? l10n.localHostRunning : l10n.localHostStarting,
      filled: true,
    );
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Material(
        color: scheme.surfaceContainerLow,
        borderRadius: BorderRadius.circular(14),
        // The whole card opens the host dialog (stop + details); the per-tunnel
        // buttons inside absorb their own taps.
        child: InkWell(
          borderRadius: BorderRadius.circular(14),
          onTap: () => showDialog<void>(
            context: context,
            builder: (_) => _LocalHostDialog(existing: host),
          ),
          child: Container(
            decoration: BoxDecoration(
              borderRadius: BorderRadius.circular(14),
              border: Border.all(color: scheme.outlineVariant),
            ),
            child: Column(
              children: [
                Padding(
                  padding: const EdgeInsets.all(12),
                  child: Row(
                    children: [
                      _IconBadge(
                        icon: Icons.dns,
                        bg: scheme.tertiaryContainer,
                        fg: scheme.onTertiaryContainer,
                      ),
                      const SizedBox(width: 12),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              host.name,
                              style: Theme.of(context).textTheme.titleSmall,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                            ),
                            const SizedBox(height: 2),
                            Text(
                              host.device,
                              style: Theme.of(context).textTheme.bodySmall
                                  ?.copyWith(color: scheme.onSurfaceVariant),
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                            ),
                          ],
                        ),
                      ),
                      const SizedBox(width: 8),
                      codexChip,
                      const SizedBox(width: 2),
                      Icon(Icons.chevron_right, color: scheme.outline),
                    ],
                  ),
                ),
                Divider(height: 1, color: scheme.outlineVariant),
                _TunnelRow(
                  label: l10n.tunnelAppLabel,
                  addr: host.appListenAddr,
                  registered: host.appRegistered,
                  onDeregister: () => _confirmDeregister(
                    context,
                    ref,
                    _entry('app'),
                    localTunnel: (name: host.name, kind: 'app'),
                  ),
                  onReregister: () => _reregister(ref, 'app'),
                ),
                Divider(height: 1, color: scheme.outlineVariant),
                _TunnelRow(
                  label: l10n.tunnelApiLabel,
                  addr: host.apiListenAddr,
                  registered: host.apiRegistered,
                  onDeregister: () => _confirmDeregister(
                    context,
                    ref,
                    _entry('api'),
                    localTunnel: (name: host.name, kind: 'api'),
                  ),
                  onReregister: () => _reregister(ref, 'api'),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// One tunnel row inside a [_LocalHostCard]: kind label, listen address, a
/// published/offline pill, and a 注销 (when published) / 重新注册 (when offline)
/// toggle.
class _TunnelRow extends StatelessWidget {
  const _TunnelRow({
    required this.label,
    required this.addr,
    required this.registered,
    required this.onDeregister,
    required this.onReregister,
  });

  final String label;
  final String addr;
  final bool registered;
  final VoidCallback onDeregister;
  final VoidCallback onReregister;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final online = Colors.green.shade600;
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 6, 6, 6),
      child: Row(
        children: [
          Icon(
            registered ? Icons.cloud_done_outlined : Icons.cloud_off_outlined,
            size: 18,
            color: registered ? online : scheme.outline,
          ),
          const SizedBox(width: 8),
          SizedBox(
            width: 78,
            child: Text(
              label,
              style: Theme.of(context).textTheme.labelMedium,
              overflow: TextOverflow.ellipsis,
            ),
          ),
          Expanded(
            child: Text(
              registered ? addr : l10n.tunnelOffline,
              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                color: registered ? scheme.onSurfaceVariant : scheme.outline,
              ),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
          ),
          if (registered)
            TextButton(
              key: Key('tunnel-deregister-$label'),
              onPressed: onDeregister,
              style: TextButton.styleFrom(foregroundColor: scheme.error),
              child: Text(l10n.deregister),
            )
          else
            TextButton(
              key: Key('tunnel-reregister-$label'),
              onPressed: onReregister,
              child: Text(l10n.reregister),
            ),
        ],
      ),
    );
  }
}

/// The "+ host another" entry that opens [_LocalHostDialog] in new-host mode.
class _AddLocalHostCard extends StatelessWidget {
  const _AddLocalHostCard();

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: OutlinedButton.icon(
        key: const Key('add-local-host-card'),
        onPressed: () => showDialog<void>(
          context: context,
          builder: (_) => const _LocalHostDialog(),
        ),
        icon: const Icon(Icons.add),
        label: Text(l10n.addLocalHost),
        style: OutlinedButton.styleFrom(
          minimumSize: const Size.fromHeight(44),
          alignment: Alignment.centerLeft,
        ),
      ),
    );
  }
}

/// Manage one local host. With [existing] set it shows that host's listen
/// address + service key and a Stop button. Otherwise it's the "new host" form
/// (codex path, port, instance name, proxy) with a Start button. codex is
/// auto-detected (with a "change path" override) or picked when not on PATH.
class _LocalHostDialog extends ConsumerStatefulWidget {
  const _LocalHostDialog({this.existing});

  /// The running host this dialog manages, or null to host a new one.
  final AppServeStatus? existing;

  @override
  ConsumerState<_LocalHostDialog> createState() => _LocalHostDialogState();
}

class _LocalHostDialogState extends ConsumerState<_LocalHostDialog> {
  final _port = TextEditingController(text: '18080');
  final _path = TextEditingController();
  final _name = TextEditingController(text: 'default');
  // Codex needs a proxy to reach chatgpt.com on most networks, so hosting
  // defaults to a proxy (a local HTTP proxy on :11111) unless the user opts out.
  final _proxy = TextEditingController(text: 'http://127.0.0.1:11111');
  bool _useProxy = true;
  bool _overridePath = false; // user chose to customize the codex path
  String? _codexPath; // auto-detected codex (config → PATH), null = not found
  bool _codexChecked = false;
  bool _busy = false;
  String? _error;

  bool get _isExisting => widget.existing != null;
  bool get _codexFound => _codexPath != null;

  @override
  void initState() {
    super.initState();
    if (_isExisting) return;
    // Auto-detect codex: when found we just show "available" (with a "change
    // path" override); when not, the user picks a path (persisted on start).
    Future.microtask(() async {
      final found = await ref.read(bridgeApiProvider).codexLocate();
      if (!mounted) return;
      setState(() {
        _codexPath = found;
        _codexChecked = true;
        if (found != null) _path.text = found; // prefill the override field
      });
    });
  }

  @override
  void dispose() {
    _port.dispose();
    _path.dispose();
    _name.dispose();
    _proxy.dispose();
    super.dispose();
  }

  Future<void> _browseCodex() async {
    final file = await openFile();
    if (file != null && mounted) setState(() => _path.text = file.path);
  }

  Future<void> _start() async {
    final l10n = AppLocalizations.of(context);
    final port = int.tryParse(_port.text.trim());
    // 0 is allowed (the engine picks an ephemeral port); reject out-of-range,
    // incl. negatives, which would otherwise wrap silently to a u16.
    if (port == null || port < 0 || port > 65535) {
      setState(() => _error = l10n.localHostPort);
      return;
    }
    // codex: auto-detected and not overridden → let the bridge resolve it;
    // otherwise the path the user typed / picked.
    final manual = !_codexFound || _overridePath;
    final override = manual ? _path.text.trim() : '';
    if (manual && override.isEmpty) {
      setState(() => _error = l10n.codexPathRequired);
      return;
    }
    // A proxy is mandatory unless the user explicitly turned it off.
    final proxy = _useProxy ? _proxy.text.trim() : null;
    if (_useProxy && (proxy == null || proxy.isEmpty)) {
      setState(() => _error = l10n.proxyRequired);
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final name = _name.text.trim();
      await ref
          .read(bridgeApiProvider)
          .appServeStart(
            port: port,
            binaryOverride: override.isEmpty ? null : override,
            name: name.isEmpty ? null : name,
            proxy: proxy,
          );
      ref.invalidate(localServeListProvider);
      ref.invalidate(servicesProvider);
      if (mounted) Navigator.of(context).pop();
    } catch (e) {
      if (mounted) setState(() => _error = friendlyError(e));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _stop() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final host = widget.existing!;
      // Full stop: kills codex + the API proxy, aborts both tunnels, and
      // force-drops both relay keys.
      await ref.read(bridgeApiProvider).appServeStop(host.name);
      // Optimistically hide both discovery entries so they leave at once.
      ref
          .read(pendingRemovalProvider.notifier)
          .update((set) => {...set, host.appServiceKey, host.apiServiceKey});
      ref.invalidate(localServeListProvider);
      ref.invalidate(servicesProvider);
      if (mounted) Navigator.of(context).pop();
    } catch (e) {
      if (mounted) setState(() => _error = friendlyError(e));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final small = Theme.of(context).textTheme.bodySmall;
    final existing = widget.existing;
    final children = <Widget>[Text(l10n.localHostHint)];
    if (existing != null) {
      // Two tunnels under one name: show each kind's listen address + relay key.
      children
        ..add(const SizedBox(height: 12))
        ..add(Text(l10n.tunnelAppLabel, style: small))
        ..add(
          Text(l10n.localHostListening(existing.appListenAddr), style: small),
        )
        ..add(
          SelectableText(
            existing.appServiceKey,
            style: small?.copyWith(color: scheme.onSurfaceVariant),
          ),
        )
        ..add(const SizedBox(height: 8))
        ..add(Text(l10n.tunnelApiLabel, style: small))
        ..add(
          Text(l10n.localHostListening(existing.apiListenAddr), style: small),
        )
        ..add(
          SelectableText(
            existing.apiServiceKey,
            style: small?.copyWith(color: scheme.onSurfaceVariant),
          ),
        );
    } else {
      children.add(const SizedBox(height: 16));
      // --- codex availability ---
      if (!_codexChecked) {
        children.add(const LinearProgressIndicator());
      } else if (_codexFound && !_overridePath) {
        children.add(
          Row(
            children: [
              Icon(Icons.check_circle, size: 18, color: Colors.green.shade600),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  l10n.codexFoundAt(_codexPath!),
                  style: small,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              TextButton(
                key: const Key('customize-codex-btn'),
                onPressed: _busy
                    ? null
                    : () => setState(() => _overridePath = true),
                child: Text(l10n.customizeCodexPath),
              ),
            ],
          ),
        );
      } else {
        if (!_codexFound) {
          children
            ..add(
              Text(l10n.codexNotFound, style: TextStyle(color: scheme.error)),
            )
            ..add(const SizedBox(height: 8));
        }
        children.add(
          Row(
            crossAxisAlignment: CrossAxisAlignment.end,
            children: [
              Expanded(
                child: TextField(
                  key: const Key('codex-path-field'),
                  controller: _path,
                  decoration: InputDecoration(labelText: l10n.codexBinaryPath),
                ),
              ),
              const SizedBox(width: 8),
              OutlinedButton(
                key: const Key('browse-codex-btn'),
                onPressed: _busy ? null : _browseCodex,
                child: Text(l10n.chooseCodexPath),
              ),
            ],
          ),
        );
      }
      // --- port + name ---
      children
        ..add(const SizedBox(height: 12))
        ..add(
          TextField(
            controller: _port,
            decoration: InputDecoration(labelText: l10n.localHostPort),
            keyboardType: TextInputType.number,
          ),
        )
        ..add(const SizedBox(height: 12))
        ..add(
          TextField(
            controller: _name,
            decoration: InputDecoration(labelText: l10n.localHostName),
          ),
        )
        // --- proxy (mandatory unless turned off) ---
        ..add(
          SwitchListTile(
            key: const Key('use-proxy-switch'),
            contentPadding: EdgeInsets.zero,
            title: Text(l10n.useProxy),
            value: _useProxy,
            onChanged: _busy ? null : (v) => setState(() => _useProxy = v),
          ),
        );
      if (_useProxy) {
        children.add(
          TextField(
            key: const Key('proxy-field'),
            controller: _proxy,
            decoration: InputDecoration(labelText: l10n.proxyLabel),
          ),
        );
      } else {
        children.add(
          Text(
            l10n.noProxyWarning,
            style: TextStyle(color: Colors.orange.shade800),
          ),
        );
      }
    }
    if (_error != null) {
      children
        ..add(const SizedBox(height: 12))
        ..add(Text(_error!, style: TextStyle(color: scheme.error)));
    }
    return AlertDialog(
      title: Text(l10n.localHostDialogTitle),
      content: SizedBox(
        width: 380,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: children,
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: _busy ? null : () => Navigator.of(context).pop(),
          child: Text(l10n.cancel),
        ),
        if (existing != null)
          FilledButton(
            key: const Key('stop-hosting-btn'),
            onPressed: _busy ? null : _stop,
            child: Text(l10n.stopHosting),
          )
        else
          FilledButton(
            key: const Key('start-hosting-btn'),
            onPressed: _busy ? null : _start,
            child: Text(l10n.startHosting),
          ),
      ],
    );
  }
}

class _ErrorState extends StatelessWidget {
  const _ErrorState({required this.detail, required this.onRetry});

  /// Raw engine error string, shown as secondary diagnostic detail.
  final String detail;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(
            l10n.discoverFailed,
            key: const Key('services-error'),
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: 4),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 24),
            child: Text(
              detail,
              textAlign: TextAlign.center,
              style: Theme.of(context).textTheme.bodySmall,
            ),
          ),
          const SizedBox(height: 12),
          FilledButton(onPressed: onRetry, child: Text(l10n.retry)),
        ],
      ),
    );
  }
}
