import 'package:flutter/material.dart';
import 'package:pocket_codex/src/widgets/window_title_bar.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/ui_prefs.dart';

/// Settings: language, relay/key, subscription status, export.
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
    final l10n = AppLocalizations.of(context);
    final api = ref.read(bridgeApiProvider);
    final config = ref.watch(configProvider).valueOrNull;
    final locale = ref.watch(localeProvider);
    final subs =
        ref.watch(subscriptionsProvider).valueOrNull ?? const <SubInfo>[];
    return Scaffold(
      appBar: WindowTitleBar(title: Text(l10n.settingsTitle)),
      body: ListView(
        children: [
          ListTile(
            key: const Key('language-btn'),
            title: Text(l10n.language),
            subtitle: Text(_languageLabel(l10n, locale)),
            trailing: const Icon(Icons.language),
            onTap: () => _pickLanguage(api),
          ),
          ListTile(
            key: const Key('appearance-btn'),
            title: Text(l10n.appearance),
            subtitle: Text(
              _appearanceLabel(
                l10n,
                ref.watch(uiPrefsProvider).valueOrNull?.themeMode,
              ),
            ),
            trailing: const Icon(Icons.brightness_6_outlined),
            onTap: _pickAppearance,
          ),
          ListTile(
            key: const Key('codex-setup-btn'),
            leading: const Icon(Icons.tune),
            title: Text(l10n.codexSetup),
            subtitle: Text(l10n.codexSetupSettingsSubtitle),
            trailing: const Icon(Icons.chevron_right),
            onTap: () => context.push('/setup/codex'),
          ),
          const Divider(),
          if (config?.mode == 'account') ...[
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 8, 16, 4),
              child: Text(l10n.accountSection),
            ),
            ListTile(
              leading: const Icon(Icons.account_circle),
              title: Text('@${config?.accountLogin ?? ''}'),
            ),
            ListTile(
              key: const Key('sign-out-btn'),
              title: Text(l10n.accountSignOut),
              trailing: const Icon(Icons.logout),
              onTap: () => _signOut(api),
            ),
            const Divider(),
          ],
          ListTile(
            title: Text(l10n.relayRow),
            subtitle: Text(config?.relay ?? l10n.notConfigured),
            trailing: const Icon(Icons.edit),
            onTap: () => _editRelay(api),
          ),
          ListTile(
            title: Text(l10n.keyRow),
            subtitle: Text(
              config?.hasKey == true ? l10n.keySet : l10n.keyNotSet,
            ),
            trailing: const Icon(Icons.edit),
            onTap: () => _editKey(api),
          ),
          const Divider(),
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 8, 16, 4),
            child: Text(l10n.activeSubscriptions),
          ),
          if (subs.isEmpty)
            ListTile(dense: true, title: Text(l10n.none))
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
            title: Text(l10n.exportShareString),
            trailing: const Icon(Icons.copy),
            onTap: () async {
              final s = await api.exportConfig();
              await Clipboard.setData(ClipboardData(text: s));
              setState(() => _msg = l10n.copiedShareString);
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

  String _languageLabel(AppLocalizations l10n, Locale? locale) {
    switch (locale?.languageCode) {
      case 'zh':
        return l10n.languageChinese;
      case 'en':
        return l10n.languageEnglish;
      default:
        return l10n.languageSystem;
    }
  }

  String _appearanceLabel(AppLocalizations l10n, String? mode) {
    switch (mode) {
      case 'light':
        return l10n.appearanceLight;
      case 'dark':
        return l10n.appearanceDark;
      default:
        return l10n.appearanceSystem;
    }
  }

  Future<void> _pickAppearance() async {
    final l10n = AppLocalizations.of(context);
    final current =
        ref.read(uiPrefsProvider).valueOrNull?.themeMode ?? 'system';
    final choice = await showDialog<String>(
      context: context,
      builder: (c) => SimpleDialog(
        title: Text(l10n.appearance),
        children: [
          _choiceOption(c, 'system', l10n.appearanceSystem, current),
          _choiceOption(c, 'light', l10n.appearanceLight, current),
          _choiceOption(c, 'dark', l10n.appearanceDark, current),
        ],
      ),
    );
    if (choice == null) return;
    ref
        .read(uiPrefsProvider.notifier)
        .setThemeMode(choice == 'system' ? null : choice);
  }

  Future<void> _pickLanguage(BridgeApi api) async {
    final l10n = AppLocalizations.of(context);
    final current = ref.read(localeProvider)?.languageCode ?? 'system';
    final choice = await showDialog<String>(
      context: context,
      builder: (c) => SimpleDialog(
        title: Text(l10n.language),
        children: [
          _choiceOption(c, 'system', l10n.languageSystem, current),
          _choiceOption(c, 'zh', l10n.languageChinese, current),
          _choiceOption(c, 'en', l10n.languageEnglish, current),
        ],
      ),
    );
    if (choice == null) return;
    ref.read(localeProvider.notifier).state = choice == 'system'
        ? null
        : Locale(choice);
    await api.setLocale(choice == 'system' ? '' : choice);
  }

  Widget _choiceOption(
    BuildContext c,
    String value,
    String label,
    String current,
  ) {
    return SimpleDialogOption(
      onPressed: () => Navigator.pop(c, value),
      child: Row(
        children: [
          Icon(value == current ? Icons.check : null, size: 18),
          const SizedBox(width: 12),
          Text(label),
        ],
      ),
    );
  }

  Future<void> _signOut(BridgeApi api) async {
    await api.accountLogout();
    ref.invalidate(configProvider);
    if (mounted) context.go('/onboarding');
  }

  Future<void> _editRelay(BridgeApi api) async {
    final l10n = AppLocalizations.of(context);
    final ctrl = TextEditingController(
      text: ref.read(configProvider).valueOrNull?.relay ?? '',
    );
    final ok = await _prompt(l10n.relayFieldLabel, ctrl);
    if (ok == true) {
      final relay = ctrl.text.trim();
      if (relay.isEmpty) {
        setState(() => _msg = l10n.relayEmpty);
        return;
      }
      await api.setRelay(relay);
      ref.invalidate(configProvider);
      ref.invalidate(servicesProvider);
    }
  }

  Future<void> _editKey(BridgeApi api) async {
    final l10n = AppLocalizations.of(context);
    final ctrl = TextEditingController();
    final ok = await _prompt(l10n.keyFieldLabel, ctrl, obscure: true);
    if (ok == true) {
      final key = ctrl.text.trim();
      if (key.length != 32) {
        setState(() => _msg = l10n.keyLengthError);
        return;
      }
      await api.setKey(key);
      ref.invalidate(configProvider);
    }
  }

  Future<bool?> _prompt(
    String label,
    TextEditingController ctrl, {
    bool obscure = false,
  }) {
    final l10n = AppLocalizations.of(context);
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
            child: Text(l10n.cancel),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(c, true),
            child: Text(l10n.save),
          ),
        ],
      ),
    );
  }
}
