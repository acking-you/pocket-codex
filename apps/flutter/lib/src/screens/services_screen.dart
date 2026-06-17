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
            // Re-discover services + re-read subscription health, and rebuild so
            // each app-server's connected/online dot re-evaluates.
            onPressed: () {
              ref.invalidate(servicesProvider);
              ref.invalidate(subscriptionsProvider);
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
          loading: () => const ListLoadingSkeleton(key: ValueKey('svc-loading')),
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
              final list = _ServiceList(
                relay: config?.relay,
                services: services,
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
              final apiServices = services
                  .where((s) => s.kind == 'api')
                  .toList();
              final selected =
                  apiServices.where((s) => s.key == selectedKey).firstOrNull ??
                  apiServices.firstOrNull;
              return Row(
                children: [
                  SizedBox(width: 360, child: list),
                  const VerticalDivider(width: 1),
                  Expanded(
                    child: selected == null
                        ? Center(child: Text(l10n.selectApiService))
                        : ApiServiceScreen(
                            key: ValueKey(selected.key),
                            serviceKey: selected.key,
                            embedded: true,
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
  });
  final String? relay;
  final List<ServiceEntry> services;
  final void Function(String key) onTapApi;
  final void Function(String key) onTapApp;

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
    Widget apiStatus(ServiceEntry s) {
      final sub = subs[s.key];
      if (sub != null) {
        return sub.alive
            ? StatusChip(color: online, label: l10n.subscribedAlive)
            : StatusChip(color: scheme.error, label: l10n.subscribedDead);
      }
      return StatusChip(color: online, label: l10n.statusOnline);
    }

    return ListView(
      children: [
        ListTile(
          leading: const Icon(Icons.dns),
          title: Text(relay ?? l10n.relayNotConfigured),
          subtitle: Text(l10n.relayRow),
          // The list only renders once discovery succeeded, so the relay is
          // reachable here; the error/offline case is the screen's error state.
          trailing: StatusChip(color: online, label: l10n.statusOnline),
        ),
        if (api.isNotEmpty) _SectionHeader(l10n.apiServicesSection),
        ...api.map(
          (s) => ListTile(
            key: Key('svc-${s.key}'),
            leading: const Icon(Icons.api),
            title: Text(s.name),
            subtitle: Text(s.device),
            trailing: apiStatus(s),
            onTap: () => onTapApi(s.key),
          ),
        ),
        if (app.isNotEmpty) _SectionHeader(l10n.appServerServices),
        ...app.map((s) {
          final connected = bridge.appIsConnected(s.key);
          return ListTile(
            key: Key('svc-${s.key}'),
            leading: const Icon(Icons.computer),
            title: Text(s.name),
            subtitle: Text(l10n.appServerSubtitle(s.device)),
            trailing: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                StatusChip(
                  color: online,
                  label: connected ? l10n.statusConnected : l10n.statusOnline,
                ),
                const SizedBox(width: 4),
                const Icon(Icons.chevron_right),
              ],
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

class _SectionHeader extends StatelessWidget {
  const _SectionHeader(this.text);
  final String text;
  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.fromLTRB(16, 16, 16, 4),
    child: Text(
      text,
      style: Theme.of(
        context,
      ).textTheme.labelSmall?.copyWith(letterSpacing: .5),
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
