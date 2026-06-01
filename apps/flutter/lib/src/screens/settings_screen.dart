import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';

/// Settings: relay/key, subscription status, export, about.
class SettingsScreen extends ConsumerStatefulWidget {
  /// Default constructor.
  const SettingsScreen({super.key});
  @override
  ConsumerState<SettingsScreen> createState() => _SettingsState();
}

class _SettingsState extends ConsumerState<SettingsScreen> {
  String? _msg;

  @override
  Widget build(BuildContext context) {
    final api = ref.read(bridgeApiProvider);
    final config = ref.watch(configProvider).valueOrNull;
    final subs =
        ref.watch(subscriptionsProvider).valueOrNull ?? const <SubInfo>[];
    return Scaffold(
      appBar: AppBar(title: const Text('设置')),
      body: ListView(
        children: [
          ListTile(
            title: const Text('relay'),
            subtitle: Text(config?.relay ?? '(未配置)'),
            trailing: const Icon(Icons.edit),
            onTap: () => _editRelay(api),
          ),
          ListTile(
            title: const Text('MSG_HEADER_KEY'),
            subtitle: Text(config?.hasKey == true ? '•••••••• (已设置)' : '(未设置)'),
            trailing: const Icon(Icons.edit),
            onTap: () => _editKey(api),
          ),
          const Divider(),
          const Padding(
            padding: EdgeInsets.fromLTRB(16, 8, 16, 4),
            child: Text('活跃订阅'),
          ),
          if (subs.isEmpty)
            const ListTile(dense: true, title: Text('(无)'))
          else
            ...subs.map(
              (s) => ListTile(
                dense: true,
                leading: Icon(
                  Icons.circle,
                  size: 12,
                  color: s.alive ? Colors.green : Colors.red,
                ),
                title: Text(s.key),
                subtitle: Text(s.localAddr),
              ),
            ),
          const Divider(),
          ListTile(
            key: const Key('export-btn'),
            title: const Text('导出 pcx1: 分享串'),
            trailing: const Icon(Icons.copy),
            onTap: () async {
              final s = await api.exportConfig();
              await Clipboard.setData(ClipboardData(text: s));
              setState(() => _msg = '已复制 pcx1: 分享串');
            },
          ),
          if (_msg != null)
            Padding(
              padding: const EdgeInsets.all(16),
              child: Text(_msg!, key: const Key('settings-msg')),
            ),
        ],
      ),
    );
  }

  Future<void> _editRelay(BridgeApi api) async {
    final ctrl = TextEditingController(
      text: ref.read(configProvider).valueOrNull?.relay ?? '',
    );
    final ok = await _prompt(context, 'relay host:port', ctrl);
    if (ok == true) {
      final relay = ctrl.text.trim();
      if (relay.isEmpty) {
        setState(() => _msg = 'relay 地址不能为空');
        return;
      }
      await api.setRelay(relay);
      ref.invalidate(configProvider);
      ref.invalidate(servicesProvider);
    }
  }

  Future<void> _editKey(BridgeApi api) async {
    final ctrl = TextEditingController();
    final ok = await _prompt(
      context,
      'MSG_HEADER_KEY (32 字节)',
      ctrl,
      obscure: true,
    );
    if (ok == true) {
      try {
        await api.setKey(ctrl.text);
        ref.invalidate(configProvider);
      } catch (e) {
        setState(() => _msg = '$e');
      }
    }
  }

  Future<bool?> _prompt(
    BuildContext context,
    String label,
    TextEditingController ctrl, {
    bool obscure = false,
  }) {
    return showDialog<bool>(
      context: context,
      builder: (c) => AlertDialog(
        content: TextField(
          controller: ctrl,
          obscureText: obscure,
          decoration: InputDecoration(labelText: label),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(c, false),
            child: const Text('取消'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(c, true),
            child: const Text('保存'),
          ),
        ],
      ),
    );
  }
}
