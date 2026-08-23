import 'package:flutter/material.dart';
import 'package:pocket_codex/src/widgets/window_title_bar.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/widgets/links.dart';

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
  // Subscriber listener default. Deliberately differs from the server-side
  // `api serve` default (18180) so running both on one host does not collide;
  // matches the CLI's `api connect` default.
  final _port = TextEditingController(text: '28180');
  SubInfo? _sub;
  String? _error;
  bool _busy = false;

  @override
  void dispose() {
    _port.dispose();
    super.dispose();
  }

  Future<void> _subscribe() async {
    final l10n = AppLocalizations.of(context);
    final port = int.tryParse(_port.text);
    if (port == null || port < 1 || port > 65535) {
      setState(() => _error = l10n.portRangeError);
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      _sub = await ref
          .read(bridgeApiProvider)
          .apiSubscribe(widget.serviceKey, port);
      ref.invalidate(subscriptionsProvider);
    } catch (e) {
      _error = '${l10n.subscribeFailed}\n${friendlyError(e)}';
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
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    // `pcx:<device>:api:<name>` (or the account-mode `pcxu:…` variant): the
    // human-facing bits are the trailing name and the device before the kind.
    final parts = widget.serviceKey.split(':');
    final name = parts.isNotEmpty ? parts.last : widget.serviceKey;
    final device = parts.length >= 3 ? parts[parts.length - 3] : '';
    final body = ListView(
      padding: const EdgeInsets.all(20),
      children: [
        // Header: what this service is, in words — the raw relay key follows
        // as a copyable code row instead of leading the page.
        Row(
          children: [
            Container(
              width: 40,
              height: 40,
              decoration: BoxDecoration(
                color: scheme.secondaryContainer,
                borderRadius: BorderRadius.circular(10),
              ),
              child: Icon(
                Icons.api,
                size: 20,
                color: scheme.onSecondaryContainer,
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    name,
                    style: Theme.of(context).textTheme.titleMedium?.copyWith(
                      fontWeight: FontWeight.w600,
                    ),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                  if (device.isNotEmpty) ...[
                    const SizedBox(height: 1),
                    Text(
                      device,
                      style: Theme.of(context).textTheme.bodySmall?.copyWith(
                        color: scheme.onSurfaceVariant,
                      ),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ],
                ],
              ),
            ),
          ],
        ),
        const SizedBox(height: 12),
        Container(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
          decoration: BoxDecoration(
            color: scheme.surfaceContainerLow,
            borderRadius: BorderRadius.circular(8),
          ),
          child: Row(
            children: [
              Expanded(
                child: SelectableText(
                  widget.serviceKey,
                  style: TextStyle(
                    fontFamily: monoFontFamily,
                    fontFamilyFallback: monoCjkFallback,
                    fontSize: 12,
                    color: scheme.onSurfaceVariant,
                  ),
                ),
              ),
              IconButton(
                icon: const Icon(Icons.copy, size: 16),
                visualDensity: VisualDensity.compact,
                onPressed: () =>
                    Clipboard.setData(ClipboardData(text: widget.serviceKey)),
              ),
            ],
          ),
        ),
        const SizedBox(height: 20),
        if (_sub == null) ...[
          TextField(
            controller: _port,
            keyboardType: TextInputType.number,
            decoration: InputDecoration(
              labelText: l10n.localPortLabel,
              border: const OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 16),
          FilledButton.icon(
            key: const Key('subscribe-btn'),
            onPressed: _busy ? null : _subscribe,
            icon: const Icon(Icons.play_arrow_rounded, size: 20),
            label: Text(l10n.startSubscription),
            style: FilledButton.styleFrom(
              minimumSize: const Size.fromHeight(44),
            ),
          ),
        ] else ...[
          Card(
            child: ListTile(
              title: KeyedSubtree(
                key: const Key('base-url'),
                child: linkifyText(
                  context,
                  'http://${_sub!.localAddr}/v1',
                  selectable: true,
                ),
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
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: Text(l10n.noAuthWarning),
            ),
          ),
          const SizedBox(height: 12),
          OutlinedButton(
            key: const Key('stop-btn'),
            onPressed: _stop,
            child: Text(l10n.stop),
          ),
        ],
        if (_error != null)
          Padding(
            padding: const EdgeInsets.only(top: 16),
            child: KeyedSubtree(
              key: const Key('api-error'),
              child: linkifyText(
                context,
                _error!,
                style: TextStyle(color: Theme.of(context).colorScheme.error),
              ),
            ),
          ),
      ],
    );
    if (widget.embedded) return body;
    return Scaffold(
      appBar: WindowTitleBar(title: Text(l10n.apiServiceTitle)),
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
        'name = "PocketCodex API"\n'
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
            linkifyText(
              context,
              snippet,
              selectable: true,
              style: const TextStyle(
                fontFamily: monoFontFamily,
                fontFamilyFallback: monoCjkFallback,
                fontSize: 12,
              ),
            ),
          ],
        ),
      ),
    );
  }
}
