import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/api_service_screen.dart';

/// Home screen: lists discovered services on the configured relay.
class ServicesScreen extends ConsumerWidget {
  /// Default constructor.
  const ServicesScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final servicesAsync = ref.watch(servicesProvider);
    final config = ref.watch(configProvider).valueOrNull;
    final selectedKey = ref.watch(selectedApiKeyProvider);
    return Scaffold(
      appBar: AppBar(
        title: const Text('Pocket-Codex'),
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
          message: '$e',
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
                        ? const Center(child: Text('选择一个 API 服务'))
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
    final api = services.where((s) => s.kind == 'api').toList();
    final app = services.where((s) => s.kind == 'app').toList();
    return ListView(
      children: [
        ListTile(
          leading: const Icon(Icons.dns, color: Colors.green),
          title: Text(relay ?? '(未配置 relay)'),
          subtitle: const Text('relay'),
        ),
        if (api.isNotEmpty) const _SectionHeader('API 服务'),
        ...api.map(
          (s) => ListTile(
            key: Key('svc-${s.key}'),
            leading: const Icon(Icons.api),
            title: Text(s.name),
            subtitle: Text(s.device),
            onTap: () => onTapApi(s.key),
          ),
        ),
        if (app.isNotEmpty) const _SectionHeader('App-server 服务'),
        ...app.map(
          (s) => ListTile(
            key: Key('svc-${s.key}'),
            leading: const Icon(Icons.computer),
            title: Text(s.name),
            subtitle: Text('${s.device} · 会话功能见 P2'),
            enabled: false,
          ),
        ),
        if (services.isEmpty)
          const Padding(
            padding: EdgeInsets.all(32),
            child: Center(child: Text('该 relay 上没有发现服务')),
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
  const _ErrorState({required this.message, required this.onRetry});
  final String message;
  final VoidCallback onRetry;
  @override
  Widget build(BuildContext context) => Center(
    child: Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(
          message,
          key: const Key('services-error'),
          textAlign: TextAlign.center,
        ),
        const SizedBox(height: 12),
        FilledButton(onPressed: onRetry, child: const Text('重试')),
      ],
    ),
  );
}
