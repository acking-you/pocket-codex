import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';

/// API-service detail: subscribe and expose a local OpenAI-compatible port.
class ApiServiceScreen extends ConsumerStatefulWidget {
  /// [serviceKey] is the full `pcx:<device>:api:<name>` key. [embedded] omits
  /// the Scaffold so it can nest in the wide-layout right pane.
  const ApiServiceScreen({
    super.key,
    required this.serviceKey,
    this.embedded = false,
  });

  /// Full relay key of the API service.
  final String serviceKey;

  /// Whether to render without a Scaffold (nested in a wide-layout pane).
  final bool embedded;
  @override
  ConsumerState<ApiServiceScreen> createState() => _ApiServiceState();
}

class _ApiServiceState extends ConsumerState<ApiServiceScreen> {
  final _port = TextEditingController(text: '18180');
  SubInfo? _sub;
  String? _error;
  bool _busy = false;

  @override
  void dispose() {
    _port.dispose();
    super.dispose();
  }

  Future<void> _subscribe() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final port = int.tryParse(_port.text);
      if (port == null) throw const FormatException('端口必须是数字');
      _sub = await ref
          .read(bridgeApiProvider)
          .apiSubscribe(widget.serviceKey, port);
      ref.invalidate(subscriptionsProvider);
    } catch (e) {
      _error = '$e';
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _stop() async {
    await ref.read(bridgeApiProvider).apiUnsubscribe(widget.serviceKey);
    ref.invalidate(subscriptionsProvider);
    setState(() => _sub = null);
  }

  @override
  Widget build(BuildContext context) {
    final body = ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Text(widget.serviceKey, style: Theme.of(context).textTheme.bodySmall),
        const SizedBox(height: 16),
        if (_sub == null) ...[
          TextField(
            controller: _port,
            keyboardType: TextInputType.number,
            decoration: const InputDecoration(
              labelText: '本地端口',
              border: OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 12),
          FilledButton(
            key: const Key('subscribe-btn'),
            onPressed: _busy ? null : _subscribe,
            child: const Text('启动订阅'),
          ),
        ] else ...[
          Card(
            child: ListTile(
              title: SelectableText(
                'http://${_sub!.localAddr}/v1',
                key: const Key('base-url'),
              ),
              subtitle: const Text('base_url'),
              trailing: IconButton(
                icon: const Icon(Icons.copy),
                onPressed: () => Clipboard.setData(
                  ClipboardData(text: 'http://${_sub!.localAddr}/v1'),
                ),
              ),
            ),
          ),
          const SizedBox(height: 8),
          _ProviderSnippet(localAddr: _sub!.localAddr),
          const SizedBox(height: 8),
          Card(
            color: Theme.of(context).colorScheme.errorContainer,
            child: const Padding(
              padding: EdgeInsets.all(12),
              child: Text('⚠ 本地端点无鉴权,仅监听 127.0.0.1。仅在 App 前台存活。'),
            ),
          ),
          const SizedBox(height: 12),
          OutlinedButton(
            key: const Key('stop-btn'),
            onPressed: _stop,
            child: const Text('停止'),
          ),
        ],
        if (_error != null)
          Padding(
            padding: const EdgeInsets.only(top: 16),
            child: Text(
              _error!,
              key: const Key('api-error'),
              style: TextStyle(color: Theme.of(context).colorScheme.error),
            ),
          ),
      ],
    );
    if (widget.embedded) return body;
    return Scaffold(
      appBar: AppBar(title: const Text('API 服务')),
      body: body,
    );
  }
}

class _ProviderSnippet extends StatelessWidget {
  const _ProviderSnippet({required this.localAddr});
  final String localAddr;
  @override
  Widget build(BuildContext context) {
    final snippet =
        'model_provider = "pocket-codex-api"\n\n'
        '[model_providers.pocket-codex-api]\n'
        'name = "Pocket-Codex API"\n'
        'base_url = "http://$localAddr/v1"\n'
        'wire_api = "responses"\n'
        'requires_openai_auth = false\n'
        'supports_websockets = true';
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              mainAxisAlignment: MainAxisAlignment.spaceBetween,
              children: [
                const Text('~/.codex/config.toml'),
                IconButton(
                  icon: const Icon(Icons.copy),
                  onPressed: () =>
                      Clipboard.setData(ClipboardData(text: snippet)),
                ),
              ],
            ),
            SelectableText(
              snippet,
              style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
            ),
          ],
        ),
      ),
    );
  }
}
