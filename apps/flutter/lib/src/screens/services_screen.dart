import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/api_service_screen.dart';

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
        title: Text(l10n.appTitle),
        actions: [
          IconButton(
            key: const Key('settings-btn'),
            icon: const Icon(Icons.settings),
            onPressed: () => context.push('/settings'),
          ),
        ],
      ),
      body: servicesAsync.when(
        loading: () => const Center(child: CircularProgressIndicator()),
        error: (e, _) => _ErrorState(
          detail: '$e',
          onRetry: () => ref.invalidate(servicesProvider),
        ),
        data: (services) => RefreshIndicator(
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
    );
  }
}

class _ServiceList extends StatelessWidget {
  const _ServiceList({
    required this.relay,
    required this.services,
    required this.onTapApi,
  });
  final String? relay;
  final List<ServiceEntry> services;
  final void Function(String key) onTapApi;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final api = services.where((s) => s.kind == 'api').toList();
    final app = services.where((s) => s.kind == 'app').toList();
    return ListView(
      children: [
        ListTile(
          leading: const Icon(Icons.dns, color: Colors.green),
          title: Text(relay ?? l10n.relayNotConfigured),
          subtitle: Text(l10n.relayRow),
        ),
        if (api.isNotEmpty) _SectionHeader(l10n.apiServicesSection),
        ...api.map(
          (s) => ListTile(
            key: Key('svc-${s.key}'),
            leading: const Icon(Icons.api),
            title: Text(s.name),
            subtitle: Text(s.device),
            onTap: () => onTapApi(s.key),
          ),
        ),
        if (app.isNotEmpty) _SectionHeader(l10n.appServerServices),
        ...app.map(
          (s) => ListTile(
            key: Key('svc-${s.key}'),
            leading: const Icon(Icons.computer),
            title: Text(s.name),
            subtitle: Text(l10n.appServerSubtitle(s.device)),
            enabled: false,
          ),
        ),
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
