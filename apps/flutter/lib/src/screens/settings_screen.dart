import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/widgets/status_dots.dart';
import 'package:pocket_codex/src/widgets/utility_page.dart';

/// Settings: language, relay/key, subscription status, export.
class SettingsScreen extends ConsumerStatefulWidget {
  /// Default constructor.
  const SettingsScreen({super.key});
  @override
  ConsumerState<SettingsScreen> createState() => _SettingsState();
}

class _SettingsState extends ConsumerState<SettingsScreen> {
  String? _msg;
  int _selectedSection = 0;
  bool _scrollingToSection = false;
  final _sectionKeys = List<GlobalKey>.generate(5, (_) => GlobalKey());
  final _settingsScrollController = ScrollController();
  final _settingsScrollViewportKey = GlobalKey();

  @override
  void initState() {
    super.initState();
    _settingsScrollController.addListener(_syncSelectedSection);
  }

  @override
  void dispose() {
    _settingsScrollController
      ..removeListener(_syncSelectedSection)
      ..dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final api = ref.read(bridgeApiProvider);
    final config = ref.watch(configProvider).valueOrNull;
    final locale = ref.watch(localeProvider);
    final subs =
        ref.watch(subscriptionsProvider).valueOrNull ?? const <SubInfo>[];
    final themeMode = ref.watch(uiPrefsProvider).valueOrNull?.themeMode;
    final desktop = isDesktop && MediaQuery.sizeOf(context).width >= 840;
    return UtilityPage(
      route: '/settings',
      title: l10n.settingsTitle,
      body: desktop
          ? _desktopBody(l10n, api, config, locale, subs, themeMode)
          : _compactBody(l10n, api, config, locale, subs, themeMode),
    );
  }

  Widget _compactBody(
    AppLocalizations l10n,
    BridgeApi api,
    ConfigInfo? config,
    Locale? locale,
    List<SubInfo> subs,
    String? themeMode,
  ) => ListView(
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
        subtitle: Text(_appearanceLabel(l10n, themeMode)),
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
        title: Text(
          config?.mode == 'account'
              ? l10n.settingsSelfHostedRelay
              : l10n.relayRow,
        ),
        subtitle: Text(_relayLabel(l10n, config)),
        trailing: const Icon(Icons.edit),
        onTap: () => _editRelay(api),
      ),
      ListTile(
        title: Text(
          config?.mode == 'account' ? l10n.settingsSelfHostedKey : l10n.keyRow,
        ),
        subtitle: Text(_keyLabel(l10n, config)),
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
        subtitle: Text(
          _canExport(config)
              ? l10n.settingsExportDescription
              : l10n.settingsExportUnavailable,
        ),
        trailing: const Icon(Icons.copy),
        onTap: _canExport(config) ? () => _export(api) : null,
      ),
      if (_msg != null)
        Padding(
          padding: const EdgeInsets.all(16),
          child: Text(_msg!, key: const Key('settings-msg')),
        ),
    ],
  );

  Widget _desktopBody(
    AppLocalizations l10n,
    BridgeApi api,
    ConfigInfo? config,
    Locale? locale,
    List<SubInfo> subs,
    String? themeMode,
  ) {
    final scheme = Theme.of(context).colorScheme;
    final sections = [
      (
        icon: Icons.settings_outlined,
        title: l10n.settingsGeneral,
        subtitle: l10n.settingsGeneralDescription,
      ),
      (
        icon: Icons.auto_awesome_outlined,
        title: 'Codex',
        subtitle: l10n.settingsCodexDescription,
      ),
      (
        icon: Icons.person_outline,
        title: l10n.settingsAccountConnection,
        subtitle: l10n.settingsAccountConnectionDescription,
      ),
      (
        icon: Icons.hub_outlined,
        title: l10n.settingsServicesSubscriptions,
        subtitle: l10n.settingsServicesSubscriptionsDescription,
      ),
      (
        icon: Icons.tune_outlined,
        title: l10n.settingsAdvanced,
        subtitle: l10n.settingsAdvancedDescription,
      ),
    ];
    return Align(
      alignment: Alignment.topCenter,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 1080),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(24, 18, 24, 18),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              SizedBox(
                width: 218,
                child: Card(
                  child: ListView(
                    padding: const EdgeInsets.all(8),
                    children: [
                      for (var i = 0; i < sections.length; i++)
                        _SettingsNavItem(
                          key: Key('settings-nav-$i'),
                          icon: sections[i].icon,
                          title: sections[i].title,
                          subtitle: sections[i].subtitle,
                          selected: i == _selectedSection,
                          onTap: () => _scrollToSection(i),
                        ),
                    ],
                  ),
                ),
              ),
              const SizedBox(width: 18),
              Expanded(
                child: SingleChildScrollView(
                  key: _settingsScrollViewportKey,
                  controller: _settingsScrollController,
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      KeyedSubtree(
                        key: _sectionKeys[0],
                        child: _SettingsGroupCard(
                          title: l10n.settingsGeneral,
                          subtitle: l10n.settingsGeneralDescription,
                          children: [
                            Padding(
                              key: const Key('appearance-btn'),
                              padding: const EdgeInsets.fromLTRB(
                                13,
                                12,
                                13,
                                10,
                              ),
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  Text(
                                    l10n.appearance,
                                    style: const TextStyle(
                                      fontWeight: FontWeight.w600,
                                    ),
                                  ),
                                  const SizedBox(height: 2),
                                  Text(
                                    l10n.settingsAppearanceDescription,
                                    style: Theme.of(context).textTheme.bodySmall
                                        ?.copyWith(
                                          color: scheme.onSurfaceVariant,
                                        ),
                                  ),
                                  const SizedBox(height: 10),
                                  Row(
                                    children: [
                                      Expanded(
                                        child: _ThemeChoice(
                                          key: const Key('theme-system'),
                                          icon: Icons.brightness_auto_outlined,
                                          label: l10n.appearanceSystem,
                                          selected: themeMode == null,
                                          onTap: () => ref
                                              .read(uiPrefsProvider.notifier)
                                              .setThemeMode(null),
                                        ),
                                      ),
                                      const SizedBox(width: 8),
                                      Expanded(
                                        child: _ThemeChoice(
                                          key: const Key('theme-light'),
                                          icon: Icons.light_mode_outlined,
                                          label: l10n.appearanceLight,
                                          selected: themeMode == 'light',
                                          onTap: () => ref
                                              .read(uiPrefsProvider.notifier)
                                              .setThemeMode('light'),
                                        ),
                                      ),
                                      const SizedBox(width: 8),
                                      Expanded(
                                        child: _ThemeChoice(
                                          key: const Key('theme-dark'),
                                          icon: Icons.dark_mode_outlined,
                                          label: l10n.appearanceDark,
                                          selected: themeMode == 'dark',
                                          onTap: () => ref
                                              .read(uiPrefsProvider.notifier)
                                              .setThemeMode('dark'),
                                        ),
                                      ),
                                    ],
                                  ),
                                ],
                              ),
                            ),
                            _SettingsRow(
                              key: const Key('language-btn'),
                              icon: Icons.language,
                              title: l10n.language,
                              subtitle: l10n.settingsLanguageDescription,
                              value: _languageLabel(l10n, locale),
                              onTap: () => _pickLanguage(api),
                            ),
                          ],
                        ),
                      ),
                      const SizedBox(height: 10),
                      KeyedSubtree(
                        key: _sectionKeys[1],
                        child: _SettingsGroupCard(
                          title: 'Codex',
                          subtitle: l10n.settingsCodexDescription,
                          children: [
                            _SettingsRow(
                              key: const Key('codex-setup-btn'),
                              icon: Icons.auto_awesome_outlined,
                              title: l10n.codexSetup,
                              subtitle: l10n.codexSetupSettingsSubtitle,
                              onTap: () => context.push('/setup/codex'),
                            ),
                          ],
                        ),
                      ),
                      const SizedBox(height: 10),
                      KeyedSubtree(
                        key: _sectionKeys[2],
                        child: Column(
                          children: [
                            _SettingsGroupCard(
                              title: l10n.settingsAccountConnection,
                              subtitle:
                                  l10n.settingsAccountConnectionDescription,
                              children: [
                                if (config?.mode == 'account')
                                  _SettingsRow(
                                    icon: Icons.account_circle_outlined,
                                    title: '@${config?.accountLogin ?? ''}',
                                    subtitle: l10n.servicesAccountMode,
                                  ),
                                _SettingsRow(
                                  icon: Icons.dns_outlined,
                                  title: config?.mode == 'account'
                                      ? l10n.settingsSelfHostedRelay
                                      : l10n.relayRow,
                                  subtitle: _relayLabel(l10n, config),
                                  actionLabel: l10n.settingsConfigure,
                                  onTap: () => _editRelay(api),
                                ),
                                _SettingsRow(
                                  icon: Icons.key_outlined,
                                  title: config?.mode == 'account'
                                      ? l10n.settingsSelfHostedKey
                                      : l10n.keyRow,
                                  subtitle: _keyLabel(l10n, config),
                                  actionLabel: config?.hasKey == true
                                      ? l10n.settingsEdit
                                      : l10n.save,
                                  onTap: () => _editKey(api),
                                ),
                              ],
                            ),
                            if (config?.mode == 'account') ...[
                              const SizedBox(height: 10),
                              _DangerRow(
                                onPressed: () => _signOut(api),
                                title: l10n.accountSignOut,
                                subtitle: l10n.settingsRemoveCredentials,
                              ),
                            ],
                          ],
                        ),
                      ),
                      const SizedBox(height: 10),
                      KeyedSubtree(
                        key: _sectionKeys[3],
                        child: _SettingsGroupCard(
                          title: l10n.settingsServicesSubscriptions,
                          subtitle:
                              l10n.settingsServicesSubscriptionsDescription,
                          children: [
                            if (subs.isEmpty)
                              _SettingsRow(
                                icon: Icons.link_off_outlined,
                                title: l10n.activeSubscriptions,
                                subtitle: l10n.none,
                              )
                            else
                              for (final sub in subs)
                                _SettingsRow(
                                  icon: Icons.link_outlined,
                                  title: sub.key,
                                  subtitle: sub.localAddr,
                                  status: StatusChip(
                                    color: sub.alive
                                        ? Colors.green.shade600
                                        : scheme.error,
                                    label: sub.alive
                                        ? l10n.subscribedAlive
                                        : l10n.subscribedDead,
                                    filled: true,
                                  ),
                                ),
                          ],
                        ),
                      ),
                      const SizedBox(height: 10),
                      KeyedSubtree(
                        key: _sectionKeys[4],
                        child: _SettingsGroupCard(
                          title: l10n.settingsAdvanced,
                          subtitle: l10n.settingsAdvancedDescription,
                          children: [
                            _SettingsRow(
                              key: const Key('export-btn'),
                              icon: Icons.ios_share_outlined,
                              title: l10n.exportShareString,
                              subtitle: _canExport(config)
                                  ? l10n.settingsExportDescription
                                  : l10n.settingsExportUnavailable,
                              actionLabel: l10n.copy,
                              onTap: _canExport(config)
                                  ? () => _export(api)
                                  : null,
                            ),
                            _SettingsRow(
                              icon: Icons.article_outlined,
                              title: l10n.settingsDiagnostics,
                              subtitle: l10n.logsTitle,
                              onTap: () => context.pushReplacement('/logs'),
                            ),
                          ],
                        ),
                      ),
                      if (_msg != null)
                        Padding(
                          padding: const EdgeInsets.all(16),
                          child: Text(
                            _msg!,
                            key: const Key('settings-msg'),
                            style: TextStyle(color: scheme.primary),
                          ),
                        ),
                    ],
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  Future<void> _scrollToSection(int index) async {
    setState(() => _selectedSection = index);
    final target = _sectionKeys[index].currentContext;
    if (target == null) return;
    _scrollingToSection = true;
    await Scrollable.ensureVisible(
      target,
      duration: const Duration(milliseconds: 250),
      curve: Curves.easeOut,
      alignment: 0.02,
    );
    _scrollingToSection = false;
  }

  void _syncSelectedSection() {
    if (!mounted ||
        _scrollingToSection ||
        !_settingsScrollController.hasClients) {
      return;
    }
    final viewport = _settingsScrollViewportKey.currentContext
        ?.findRenderObject();
    if (viewport is! RenderBox) return;
    if (_settingsScrollController.position.maxScrollExtent <= 1) return;

    var next = 0;
    if (_settingsScrollController.position.extentAfter <= 1) {
      next = _sectionKeys.length - 1;
    } else {
      final viewportTop = viewport.localToGlobal(Offset.zero).dy;
      for (var i = 0; i < _sectionKeys.length; i++) {
        final section = _sectionKeys[i].currentContext?.findRenderObject();
        if (section is RenderBox &&
            section.localToGlobal(Offset.zero).dy <= viewportTop + 48) {
          next = i;
        }
      }
    }
    if (next != _selectedSection) setState(() => _selectedSection = next);
  }

  String _relayLabel(AppLocalizations l10n, ConfigInfo? config) {
    final value = config?.relay?.trim() ?? '';
    if (config?.mode == 'account') {
      return value.isEmpty
          ? l10n.settingsUsingAccountBroker
          : l10n.settingsUsingAccountBrokerWithRelay(value);
    }
    if (value.isNotEmpty) return value;
    return l10n.notConfigured;
  }

  String _keyLabel(AppLocalizations l10n, ConfigInfo? config) {
    if (config?.mode == 'account') {
      return config?.hasKey == true
          ? l10n.settingsSelfHostedKeySet
          : l10n.settingsSelfHostedKeyNotSet;
    }
    return config?.hasKey == true ? l10n.keySet : l10n.keyNotSet;
  }

  bool _canExport(ConfigInfo? config) =>
      (config?.relay?.trim().isNotEmpty ?? false) && config?.hasKey == true;

  Future<void> _export(BridgeApi api) async {
    try {
      final value = await api.exportConfig();
      await Clipboard.setData(ClipboardData(text: value));
      if (mounted) {
        _showMessage(AppLocalizations.of(context).copiedShareString);
      }
    } catch (error) {
      if (mounted) {
        final l10n = AppLocalizations.of(context);
        _showMessage(l10n.settingsOperationFailed(friendlyError(error)));
      }
    }
  }

  void _showMessage(String message) {
    if (!mounted) return;
    setState(() => _msg = message);
    if (isDesktop && MediaQuery.sizeOf(context).width >= 840) {
      final messenger = ScaffoldMessenger.of(context);
      messenger.hideCurrentSnackBar();
      messenger.showSnackBar(SnackBar(content: Text(message)));
    }
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
    try {
      await api.setLocale(choice == 'system' ? '' : choice);
      if (!mounted) return;
      ref.read(localeProvider.notifier).state = choice == 'system'
          ? null
          : Locale(choice);
      setState(() => _msg = null);
    } catch (error) {
      if (mounted) {
        _showMessage(l10n.settingsOperationFailed(friendlyError(error)));
      }
    }
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
    final l10n = AppLocalizations.of(context);
    try {
      await api.accountLogout();
      ref.invalidate(configProvider);
      if (mounted) context.go('/onboarding');
    } catch (error) {
      if (mounted) {
        _showMessage(l10n.settingsOperationFailed(friendlyError(error)));
      }
    }
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
        _showMessage(l10n.relayEmpty);
        return;
      }
      try {
        await api.setRelay(relay);
        if (mounted) setState(() => _msg = null);
        ref.invalidate(configProvider);
        ref.invalidate(servicesProvider);
      } catch (error) {
        if (mounted) {
          _showMessage(l10n.settingsOperationFailed(friendlyError(error)));
        }
      }
    }
  }

  Future<void> _editKey(BridgeApi api) async {
    final l10n = AppLocalizations.of(context);
    final ctrl = TextEditingController();
    final ok = await _prompt(l10n.keyFieldLabel, ctrl, obscure: true);
    if (ok == true) {
      final key = ctrl.text.trim();
      if (key.length != 32) {
        _showMessage(l10n.keyLengthError);
        return;
      }
      try {
        await api.setKey(key);
        if (mounted) setState(() => _msg = null);
        ref.invalidate(configProvider);
      } catch (error) {
        if (mounted) {
          _showMessage(l10n.settingsOperationFailed(friendlyError(error)));
        }
      }
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

class _SettingsNavItem extends StatelessWidget {
  const _SettingsNavItem({
    super.key,
    required this.icon,
    required this.title,
    required this.subtitle,
    required this.selected,
    required this.onTap,
  });

  final IconData icon;
  final String title;
  final String subtitle;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: Material(
        color: selected ? scheme.primaryContainer : Colors.transparent,
        borderRadius: BorderRadius.circular(9),
        child: InkWell(
          onTap: onTap,
          borderRadius: BorderRadius.circular(9),
          child: ConstrainedBox(
            constraints: const BoxConstraints(minHeight: 56),
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
              child: Row(
                children: [
                  Icon(
                    icon,
                    size: 18,
                    color: selected
                        ? scheme.onPrimaryContainer
                        : scheme.onSurfaceVariant,
                  ),
                  const SizedBox(width: 10),
                  Expanded(
                    child: Column(
                      mainAxisAlignment: MainAxisAlignment.center,
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          title,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            fontWeight: FontWeight.w600,
                            color: selected
                                ? scheme.onPrimaryContainer
                                : scheme.onSurface,
                          ),
                        ),
                        const SizedBox(height: 2),
                        Text(
                          subtitle,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: Theme.of(context).textTheme.bodySmall
                              ?.copyWith(color: scheme.onSurfaceVariant),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _SettingsGroupCard extends StatelessWidget {
  const _SettingsGroupCard({
    required this.title,
    required this.subtitle,
    required this.children,
  });

  final String title;
  final String subtitle;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Card(
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(14, 11, 14, 10),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  title,
                  style: const TextStyle(fontWeight: FontWeight.w600),
                ),
                const SizedBox(height: 2),
                Text(
                  subtitle,
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: scheme.onSurfaceVariant,
                  ),
                ),
              ],
            ),
          ),
          for (var i = 0; i < children.length; i++) ...[
            Divider(height: 1, color: scheme.outlineVariant),
            children[i],
          ],
        ],
      ),
    );
  }
}

class _SettingsRow extends StatelessWidget {
  const _SettingsRow({
    super.key,
    required this.icon,
    required this.title,
    required this.subtitle,
    this.value,
    this.status,
    this.actionLabel,
    this.onTap,
  });

  final IconData icon;
  final String title;
  final String subtitle;
  final String? value;
  final Widget? status;
  final String? actionLabel;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final content = Padding(
      padding: const EdgeInsets.fromLTRB(13, 8, 8, 8),
      child: Row(
        children: [
          Container(
            width: 32,
            height: 32,
            decoration: BoxDecoration(
              color: scheme.surfaceContainer,
              borderRadius: BorderRadius.circular(8),
            ),
            child: Icon(icon, size: 17, color: scheme.onSurfaceVariant),
          ),
          const SizedBox(width: 11),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  title,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(fontWeight: FontWeight.w600),
                ),
                const SizedBox(height: 1),
                Text(
                  subtitle,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: scheme.onSurfaceVariant,
                  ),
                ),
              ],
            ),
          ),
          if (value != null)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 10),
              child: Text(
                value!,
                style: TextStyle(color: scheme.onSurfaceVariant),
              ),
            ),
          if (status != null) ...[status!, const SizedBox(width: 8)],
          if (actionLabel != null)
            OutlinedButton(onPressed: onTap, child: Text(actionLabel!))
          else if (onTap != null)
            Icon(Icons.chevron_right, color: scheme.onSurfaceVariant),
        ],
      ),
    );
    if (onTap == null || actionLabel != null) return content;
    return InkWell(onTap: onTap, child: content);
  }
}

class _ThemeChoice extends StatelessWidget {
  const _ThemeChoice({
    super.key,
    required this.icon,
    required this.label,
    required this.selected,
    required this.onTap,
  });

  final IconData icon;
  final String label;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Material(
      color: selected ? scheme.primaryContainer : Colors.transparent,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(9),
        side: BorderSide(color: selected ? scheme.primary : scheme.outline),
      ),
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(9),
        child: SizedBox(
          height: 48,
          child: Stack(
            children: [
              Center(
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Icon(
                      icon,
                      size: 18,
                      color: selected
                          ? scheme.onPrimaryContainer
                          : scheme.onSurfaceVariant,
                    ),
                    const SizedBox(width: 7),
                    Text(label),
                  ],
                ),
              ),
              if (selected)
                Positioned(
                  top: 6,
                  right: 6,
                  child: Icon(
                    Icons.check,
                    size: 13,
                    color: scheme.onPrimaryContainer,
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}

class _DangerRow extends StatelessWidget {
  const _DangerRow({
    required this.onPressed,
    required this.title,
    required this.subtitle,
  });

  final VoidCallback onPressed;
  final String title;
  final String subtitle;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 13, vertical: 9),
      decoration: BoxDecoration(
        color: scheme.errorContainer.withValues(alpha: 0.35),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: scheme.error.withValues(alpha: 0.3)),
      ),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  title,
                  style: TextStyle(
                    color: scheme.error,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                const SizedBox(height: 2),
                Text(
                  subtitle,
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: scheme.onSurfaceVariant,
                  ),
                ),
              ],
            ),
          ),
          OutlinedButton(
            key: const Key('sign-out-btn'),
            onPressed: onPressed,
            style: OutlinedButton.styleFrom(
              foregroundColor: scheme.error,
              side: BorderSide(color: scheme.error),
            ),
            child: Text(title),
          ),
        ],
      ),
    );
  }
}
