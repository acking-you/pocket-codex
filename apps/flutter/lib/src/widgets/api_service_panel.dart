import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/service_key.dart';
import 'package:pocket_codex/src/widgets/adaptive_sheet.dart';
import 'package:pocket_codex/src/widgets/app_toast.dart';
import 'package:pocket_codex/src/widgets/code_row.dart';
import 'package:pocket_codex/src/widgets/icon_badge.dart';
import 'package:pocket_codex/src/widgets/links.dart';

/// Subscribe to an API capability and expose it on a local OpenAI-compatible
/// port, as a transient panel over the capability list.
///
/// This used to be a whole route (`/api/:key`) reached by drilling
/// Chat → Services → this, for one port field and one button. A panel keeps the
/// capability row it acts on in view and costs no navigation.
Future<void> showApiServicePanel(BuildContext context, String serviceKey) =>
    showAdaptivePanel<void>(
      context: context,
      insetTop: false,
      builder: (_) => ApiServicePanel(serviceKey: serviceKey),
    );

/// The panel body: the relay key, then either the subscribe form or the live
/// endpoint's `base_url` + config snippet.
class ApiServicePanel extends ConsumerStatefulWidget {
  /// [serviceKey] is the full `pcx:<device>:api:<name>` key.
  const ApiServicePanel({super.key, required this.serviceKey});

  /// Full relay key of the API service.
  final String serviceKey;

  @override
  ConsumerState<ApiServicePanel> createState() => _ApiServicePanelState();
}

class _ApiServicePanelState extends ConsumerState<ApiServicePanel> {
  // Subscriber listener default. Deliberately differs from the server-side
  // `api serve` default (18180) so running both on one host does not collide;
  // matches the CLI's `api connect` default.
  final _port = TextEditingController(text: '28180');
  SubInfo? _sub;
  String? _error;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    // A key already subscribed must open showing its endpoint, not an empty
    // form that would refuse to bind the port a second time. The route this
    // replaces could ignore that: it was pushed fresh each time and the user
    // never saw the two states side by side in one list.
    final live = ref
        .read(subscriptionsProvider)
        .valueOrNull
        ?.where((sub) => sub.key == widget.serviceKey)
        .firstOrNull;
    if (live != null) _sub = live;
  }

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

  void _copy(String text) {
    Clipboard.setData(ClipboardData(text: text));
    showToastOk(context, AppLocalizations.of(context).copied);
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final name = serviceKeyName(widget.serviceKey);
    final device = serviceKeyDevice(widget.serviceKey);
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 16, 20, 20),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              const IconBadge(
                icon: Icons.bolt_outlined,
                size: 36,
                accent: true,
              ),
              const SizedBox(width: 11),
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
                    if (device.isNotEmpty)
                      Text(
                        device,
                        style: Theme.of(context).textTheme.bodySmall?.copyWith(
                          color: scheme.onSurfaceVariant,
                        ),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                  ],
                ),
              ),
              IconButton(
                key: const Key('api-panel-close'),
                tooltip: l10n.cancel,
                icon: const Icon(Icons.close, size: 18),
                onPressed: () => Navigator.of(context).pop(),
              ),
            ],
          ),
          const SizedBox(height: 12),
          CodeRow(value: widget.serviceKey),
          const SizedBox(height: 16),
          if (_sub == null) ..._form(l10n, scheme) else ..._live(l10n, scheme),
          if (_error != null)
            Padding(
              padding: const EdgeInsets.only(top: 14),
              child: KeyedSubtree(
                key: const Key('api-error'),
                child: linkifyText(
                  context,
                  _error!,
                  style: TextStyle(color: scheme.error),
                ),
              ),
            ),
        ],
      ),
    );
  }

  /// Not yet subscribed: the local port to bind, and the one action.
  List<Widget> _form(AppLocalizations l10n, ColorScheme scheme) => [
    TextField(
      controller: _port,
      keyboardType: TextInputType.number,
      decoration: InputDecoration(
        labelText: l10n.localPortLabel,
        filled: true,
        fillColor: scheme.surfaceContainerHighest,
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(kControlRadius),
          borderSide: BorderSide.none,
        ),
      ),
    ),
    const SizedBox(height: 14),
    FilledButton.icon(
      key: const Key('subscribe-btn'),
      onPressed: _busy ? null : _subscribe,
      icon: const Icon(Icons.play_arrow_rounded, size: 20),
      label: Text(l10n.startSubscription),
      style: FilledButton.styleFrom(minimumSize: const Size.fromHeight(44)),
    ),
  ];

  /// Subscribed: what to paste where, the no-auth caveat, and how to stop.
  List<Widget> _live(AppLocalizations l10n, ColorScheme scheme) => [
    Card(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(14, 11, 8, 11),
        child: Row(
          children: [
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'base_url',
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: scheme.onSurfaceVariant,
                    ),
                  ),
                  const SizedBox(height: 2),
                  KeyedSubtree(
                    key: const Key('base-url'),
                    child: linkifyText(
                      context,
                      'http://${_sub!.localAddr}/v1',
                      selectable: true,
                    ),
                  ),
                ],
              ),
            ),
            IconButton(
              tooltip: l10n.copy,
              icon: const Icon(Icons.copy, size: 17),
              onPressed: () => _copy('http://${_sub!.localAddr}/v1'),
            ),
          ],
        ),
      ),
    ),
    const SizedBox(height: 8),
    _ProviderSnippet(localAddr: _sub!.localAddr),
    const SizedBox(height: 8),
    // The proxy listens with no auth; say so where the base_url is copied
    // from, not in a footnote.
    Container(
      padding: const EdgeInsets.fromLTRB(12, 10, 12, 10),
      decoration: BoxDecoration(
        color: scheme.errorContainer,
        borderRadius: BorderRadius.circular(kPanelRadius),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(
            Icons.warning_amber_rounded,
            size: 17,
            color: scheme.onErrorContainer,
          ),
          const SizedBox(width: 9),
          Expanded(
            child: Text(
              l10n.noAuthWarning,
              style: TextStyle(color: scheme.onErrorContainer),
            ),
          ),
        ],
      ),
    ),
    const SizedBox(height: 12),
    OutlinedButton(
      key: const Key('stop-btn'),
      onPressed: _stop,
      child: Text(l10n.stop),
    ),
  ];
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
                  tooltip: AppLocalizations.of(context).copy,
                  icon: const Icon(Icons.copy, size: 17),
                  onPressed: () {
                    Clipboard.setData(ClipboardData(text: snippet));
                    showToastOk(context, AppLocalizations.of(context).copied);
                  },
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
