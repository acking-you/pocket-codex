import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/src/providers.dart';

/// First-run setup: import a `pcx1:` string or type relay + key.
class OnboardingScreen extends ConsumerStatefulWidget {
  /// Default constructor.
  const OnboardingScreen({super.key});
  @override
  ConsumerState<OnboardingScreen> createState() => _OnboardingState();
}

class _OnboardingState extends ConsumerState<OnboardingScreen> {
  final _import = TextEditingController();
  final _relay = TextEditingController();
  final _key = TextEditingController();
  String? _error;
  bool _busy = false;

  @override
  void dispose() {
    _import.dispose();
    _relay.dispose();
    _key.dispose();
    super.dispose();
  }

  Future<void> _run(Future<void> Function() op) async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await op();
      if (mounted) context.go('/');
    } catch (e) {
      setState(() => _error = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final api = ref.read(bridgeApiProvider);
    return Scaffold(
      body: SafeArea(
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 480),
            child: ListView(
              padding: const EdgeInsets.all(24),
              shrinkWrap: true,
              children: [
                Image.asset(
                  'assets/logo/poster.png',
                  height: 120,
                  key: const Key('onboarding-logo'),
                ),
                const SizedBox(height: 24),
                Text(
                  '连接到 pb-mapper relay',
                  style: Theme.of(context).textTheme.titleLarge,
                ),
                const SizedBox(height: 16),
                TextField(
                  controller: _import,
                  decoration: const InputDecoration(
                    labelText: 'pcx1: 分享串(一键导入)',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 8),
                FilledButton(
                  key: const Key('import-btn'),
                  onPressed: _busy
                      ? null
                      : () => _run(() => api.importConfig(_import.text)),
                  child: const Text('导入'),
                ),
                const Divider(height: 32),
                TextField(
                  controller: _relay,
                  decoration: const InputDecoration(
                    labelText: 'relay host:port',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 8),
                TextField(
                  controller: _key,
                  decoration: const InputDecoration(
                    labelText: 'MSG_HEADER_KEY (32 字节)',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 8),
                FilledButton.tonal(
                  key: const Key('manual-btn'),
                  onPressed: _busy
                      ? null
                      : () => _run(() async {
                          final key = _key.text.trim();
                          final relay = _relay.text.trim();
                          if (relay.isEmpty) {
                            throw const FormatException('relay 地址不能为空');
                          }
                          // Set key first: it validates 32 bytes and throws on
                          // bad input, so a relay is never persisted without a
                          // valid key (which would wrongly skip onboarding).
                          await api.setKey(key);
                          await api.setRelay(relay);
                        }),
                  child: const Text('保存'),
                ),
                if (_error != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 16),
                    child: Text(
                      _error!,
                      key: const Key('onboarding-error'),
                      style: TextStyle(
                        color: Theme.of(context).colorScheme.error,
                      ),
                    ),
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
