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
class ServicesScreen extends ConsumerWidget {
  /// Default constructor.
  const ServicesScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
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
    required this.services,
    required this.onTapApi,
    required this.onTapApp,
    this.highlightKey,
  });
  final String? relay;
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
    final api = services.where((s) => s.kind == 'api').toList();
    final app = services.where((s) => s.kind == 'app').toList();
    // Live subscription health, keyed by service key (alive/dead). A discovered
    // service is by definition currently registered on the relay → "online".
    final subs = {
      for (final s in ref.watch(subscriptionsProvider).valueOrNull ?? const [])
        s.key: s,
    };
    final bridge = ref.watch(bridgeApiProvider);

    // Per-API status: subscribed+alive, subscribed+dropped, or just online.
    StatusChip apiStatus(ServiceEntry s) {
      final sub = subs[s.key];
      if (sub != null) {
        return sub.alive
            ? StatusChip(color: online, label: l10n.subscribedAlive, filled: true)
            : StatusChip(
                color: scheme.error,
                label: l10n.subscribedDead,
                filled: true,
              );
      }
      return StatusChip(color: online, label: l10n.statusOnline, filled: true);
    }

    return ListView(
      padding: const EdgeInsets.fromLTRB(12, 12, 12, 16),
      children: [
        _RelayBanner(relay: relay ?? l10n.relayNotConfigured, online: online),
        if (api.isNotEmpty) _SectionHeader(l10n.apiServicesSection),
        ...api.map(
          (s) => _ServiceCard(
            key: Key('svc-${s.key}'),
            icon: Icons.api,
            iconBg: scheme.secondaryContainer,
            iconFg: scheme.onSecondaryContainer,
            title: s.name,
            subtitle: s.device,
            selected: s.key == highlightKey,
            status: apiStatus(s),
            onTap: () => onTapApi(s.key),
          ),
        ),
        if (app.isNotEmpty) _SectionHeader(l10n.appServerServices),
        ...app.map((s) {
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
            icon: Icons.computer,
            iconBg: scheme.tertiaryContainer,
            iconFg: scheme.onTertiaryContainer,
            title: s.name,
            subtitle: l10n.appServerSubtitle(s.device),
            reason: reason,
            chevron: true,
            status: StatusChip(
              color: statusColor,
              label: statusLabel,
              filled: true,
            ),
            onTap: () => onTapApp(s.key),
          );
        }),
        if (services.isEmpty)
          Padding(
            padding: const EdgeInsets.all(32),
            child: Center(child: Text(l10n.noServicesFound)),
          ),
      ],
    );
  }
}

/// The relay header: a tinted banner showing the configured relay and its
/// (implicitly online — the list only renders once discovery succeeded) status.
class _RelayBanner extends StatelessWidget {
  const _RelayBanner({required this.relay, required this.online});
  final String relay;
  final Color online;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
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
            icon: Icons.dns,
            bg: scheme.primaryContainer,
            fg: scheme.onPrimaryContainer,
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  relay,
                  style: Theme.of(context).textTheme.titleSmall,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
                const SizedBox(height: 1),
                Text(
                  l10n.relayRow,
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
                          style: Theme.of(context).textTheme.bodySmall
                              ?.copyWith(color: scheme.error),
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
