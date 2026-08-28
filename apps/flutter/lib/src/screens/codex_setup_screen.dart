import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/widgets/app_toast.dart';
import 'package:pocket_codex/src/widgets/utility_page.dart';

/// Form fields on this page stand alone on the sheet rather than inside a
/// bordered toolbar, so they keep an outline — see `SearchField` for the filled,
/// borderless counterpart used in filters.
final _fieldBorder = OutlineInputBorder(
  borderRadius: BorderRadius.circular(kControlRadius),
);

/// 自带 codex 首次配置向导。
///
/// 当本机 `CODEX_HOME` 既没有凭证(`auth.json`)也没有自定义 provider 时,codex
/// 无法发起任何模型调用。这个页面用两种方式引导用户配好:
///
/// 1. 自定义 provider(最小可用):填 Base URL + API Key,写入 codex 的
///    `config.toml`,无需 `codex login`。
/// 2. codex 官方 ChatGPT 登录:在本机运行中的 app-server 上驱动
///    `account/login/start`,浏览器完成 OAuth,codex 自己写 `auth.json`。
///
/// 另外提供「非降智 system prompt」开关(见 openai/codex#30364)。
class CodexSetupScreen extends ConsumerStatefulWidget {
  /// Default constructor.
  const CodexSetupScreen({super.key});

  @override
  ConsumerState<CodexSetupScreen> createState() => _CodexSetupScreenState();
}

class _CodexSetupScreenState extends ConsumerState<CodexSetupScreen> {
  final _baseUrl = TextEditingController();
  final _apiKey = TextEditingController();
  final _model = TextEditingController();

  CodexSetupStatus? _status;
  bool _busy = false;
  bool _loginPolling = false;
  String? _error;
  String? _info;
  // Device-code login: the one-time code the user enters on the opened page,
  // and that page's URL (so they can reopen it). Null for the browser flow.
  String? _deviceCode;
  String? _deviceUrl;

  @override
  void initState() {
    super.initState();
    _loadStatus();
  }

  @override
  void dispose() {
    _baseUrl.dispose();
    _apiKey.dispose();
    _model.dispose();
    super.dispose();
  }

  BridgeApi get _api => ref.read(bridgeApiProvider);

  Future<void> _loadStatus() async {
    try {
      final s = await _api.codexSetupStatus();
      if (mounted) {
        setState(() => _status = s);
        // Keep the chat's "codex needs setup" banner/send-guard in sync with
        // what the wizard just changed (configured a provider, logged in, …).
        ref.invalidate(codexSetupStatusProvider);
      }
    } catch (e) {
      if (mounted) setState(() => _error = friendlyError(e));
    }
  }

  /// Sign the local codex out on the running host, then refresh.
  Future<void> _logout(AppLocalizations l10n) async {
    setState(() {
      _busy = true;
      _error = null;
      _info = null;
    });
    try {
      final hosts = await _api.appServeStatus();
      final candidates = hosts
          .where((h) => h.appServiceKey.isNotEmpty)
          .toList();
      final host = candidates.isEmpty ? null : candidates.first;
      if (host == null) {
        setState(() {
          _busy = false;
          _error = l10n.codexSetupNeedHost;
        });
        return;
      }
      final key = host.appServiceKey;
      if (!_api.appIsConnected(key)) {
        await _api.appConnect(key, 0);
      }
      await _api.codexLogout(key);
      await _loadStatus();
      if (mounted) setState(() => _busy = false);
    } catch (e) {
      if (mounted) {
        setState(() {
          _busy = false;
          _error = friendlyError(e);
        });
      }
    }
  }

  Future<void> _run(Future<void> Function() op, {String? okMessage}) async {
    setState(() {
      _busy = true;
      _error = null;
      _info = null;
    });
    try {
      await op();
      await _loadStatus();
      if (mounted && okMessage != null) setState(() => _info = okMessage);
    } catch (e) {
      if (mounted) setState(() => _error = friendlyError(e));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  void _saveProvider(AppLocalizations l10n) {
    final base = _baseUrl.text.trim();
    final key = _apiKey.text.trim();
    if (base.isEmpty || key.isEmpty) {
      setState(() => _error = l10n.codexSetupNeedFields);
      return;
    }
    _run(
      () => _api.codexSetupProvider(
        baseUrl: base,
        apiKey: key,
        model: _model.text.trim().isEmpty ? null : _model.text.trim(),
      ),
      okMessage: l10n.codexSetupProviderSaved,
    );
  }

  /// codex 官方登录:在本机运行中的 app-server 上驱动 ChatGPT OAuth。需要先启动
  /// 本机托管(有一个运行中的 host),否则提示用户先托管。
  Future<void> _startChatgptLogin(AppLocalizations l10n) async {
    setState(() {
      _busy = true;
      _error = null;
      _info = null;
      _deviceCode = null;
      _deviceUrl = null;
    });
    try {
      final hosts = await _api.appServeStatus();
      final candidates = hosts
          .where((h) => h.appServiceKey.isNotEmpty)
          .toList();
      final host = candidates.isEmpty ? null : candidates.first;
      if (host == null) {
        setState(() {
          _busy = false;
          _error = l10n.codexSetupNeedHost;
        });
        return;
      }
      final key = host.appServiceKey;
      // Ensure a live JSON-RPC session to the local app-server before driving
      // the login RPC on it.
      if (!_api.appIsConnected(key)) {
        await _api.appConnect(key, 0);
      }
      final start = await _api.codexLoginChatgptStart(key);
      final isDevice = start.mode == 'device';
      // Browser flow opens authUrl; device flow (codex couldn't bind its local
      // callback port) opens the verification page and shows a code to enter.
      final url = isDevice ? start.verificationUrl : start.authUrl;
      if (url != null && url.isNotEmpty) {
        await launchUrl(Uri.parse(url), mode: LaunchMode.externalApplication);
      }
      if (!mounted) return;
      setState(() {
        _busy = false;
        _loginPolling = true;
        _deviceCode = isDevice ? start.userCode : null;
        _deviceUrl = isDevice ? start.verificationUrl : null;
        _info = isDevice
            ? l10n.codexSetupDeviceOpened
            : l10n.codexSetupLoginOpened;
      });
      await _pollUntilAuthenticated(l10n, key, start.loginId);
    } catch (e) {
      if (mounted) {
        setState(() {
          _busy = false;
          _loginPolling = false;
          _error = friendlyError(e);
        });
      }
    }
  }

  /// Poll auth status until the browser login completes (bounded), then refresh.
  Future<void> _pollUntilAuthenticated(
    AppLocalizations l10n,
    String serviceKey,
    String loginId,
  ) async {
    // ~5 minutes at 2s cadence, matching codex's login window.
    for (var i = 0; i < 150; i++) {
      await Future<void>.delayed(const Duration(seconds: 2));
      if (!mounted || !_loginPolling) return;
      try {
        // Detect completion two ways, because the app-server's getAuthStatus RPC
        // caches and can lag behind the auth.json codex just wrote (esp. the
        // device-code flow): the RPC (authoritative for a REMOTE host) OR the
        // local auth.json on disk via codexSetupStatus (flips first for a LOCAL
        // host — this is what "re-enter the screen" was picking up).
        final rpc = await _api.codexAuthStatus(serviceKey);
        final disk = await _api.codexSetupStatus();
        if (rpc.authenticated || disk.hasAuth) {
          if (mounted) {
            setState(() {
              _loginPolling = false;
              _deviceCode = null;
              _deviceUrl = null;
              _info = l10n.codexSetupLoginSuccess(
                rpc.method ?? disk.authMode ?? 'chatgpt',
              );
            });
          }
          await _loadStatus();
          return;
        }
      } catch (_) {
        // Transient (host reconnecting); keep polling.
      }
    }
    if (mounted) {
      setState(() {
        _loginPolling = false;
        _error = l10n.codexSetupLoginTimeout;
      });
      try {
        await _api.codexLoginCancel(serviceKey, loginId);
      } catch (_) {}
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final theme = Theme.of(context);
    final status = _status;
    final nonDegraded = status?.promptVariant == 'non_degraded';
    return UtilityPage(
      route: '/settings',
      title: l10n.codexSetup,
      parent: UtilityParent(title: l10n.settingsTitle, route: '/settings'),
      body: SafeArea(
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 560),
            child: ListView(
              padding: const EdgeInsets.all(20),
              children: [
                if (status != null) _statusCard(l10n, theme, status),
                const SizedBox(height: 16),

                // --- 1. 自定义 provider ---
                Text(
                  l10n.codexSetupProviderSection,
                  style: theme.textTheme.titleMedium,
                ),
                const SizedBox(height: 4),
                Text(
                  l10n.codexSetupProviderDesc,
                  style: theme.textTheme.bodySmall,
                ),
                const SizedBox(height: 12),
                TextField(
                  key: const Key('codex-base-url'),
                  controller: _baseUrl,
                  keyboardType: TextInputType.url,
                  decoration: InputDecoration(
                    labelText: 'Base URL',
                    hintText: 'https://example.com/v1',
                    border: _fieldBorder,
                  ),
                ),
                const SizedBox(height: 10),
                TextField(
                  key: const Key('codex-api-key'),
                  controller: _apiKey,
                  obscureText: true,
                  decoration: InputDecoration(
                    labelText: 'API Key',
                    border: _fieldBorder,
                  ),
                ),
                const SizedBox(height: 10),
                TextField(
                  controller: _model,
                  decoration: InputDecoration(
                    labelText: l10n.codexSetupModelLabel,
                    hintText: 'gpt-5.5',
                    border: _fieldBorder,
                  ),
                ),
                const SizedBox(height: 12),
                FilledButton(
                  key: const Key('codex-save-provider'),
                  onPressed: _busy ? null : () => _saveProvider(l10n),
                  child: Text(l10n.codexSetupSaveProvider),
                ),

                const Divider(height: 40),

                // --- 2. 官方 ChatGPT 登录 ---
                Text(
                  l10n.codexSetupLoginSection,
                  style: theme.textTheme.titleMedium,
                ),
                const SizedBox(height: 4),
                Text(
                  l10n.codexSetupLoginDesc,
                  style: theme.textTheme.bodySmall,
                ),
                const SizedBox(height: 12),
                if (status?.hasAuth == true)
                  // Signed in → show the state + a sign-out instead of a login
                  // button (the status card above turns green too).
                  Row(
                    children: [
                      Icon(
                        Icons.check_circle,
                        size: 18,
                        color: theme.colorScheme.primary,
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          l10n.codexSetupStatusAuth(
                            status!.authMode ?? l10n.codexSetupCredentialExists,
                          ),
                          key: const Key('codex-login-done'),
                          style: TextStyle(color: theme.colorScheme.primary),
                        ),
                      ),
                      TextButton(
                        key: const Key('codex-logout-btn'),
                        onPressed: _busy ? null : () => _logout(l10n),
                        child: Text(l10n.accountSignOut),
                      ),
                    ],
                  )
                else
                  FilledButton.tonal(
                    key: const Key('codex-login-chatgpt'),
                    onPressed: (_busy || _loginPolling)
                        ? null
                        : () => _startChatgptLogin(l10n),
                    child: Text(
                      _loginPolling
                          ? l10n.codexSetupLoginWaiting
                          : l10n.codexSetupLoginButton,
                    ),
                  ),
                // Device-code flow: show the one-time code to enter on the
                // opened verification page (browser callback port unavailable).
                if (_deviceCode != null && _deviceCode!.isNotEmpty) ...[
                  const SizedBox(height: 12),
                  Container(
                    width: double.infinity,
                    padding: const EdgeInsets.all(14),
                    decoration: BoxDecoration(
                      color: theme.colorScheme.surfaceContainerHighest,
                      borderRadius: BorderRadius.circular(kPanelRadius),
                    ),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          l10n.codexSetupDeviceCodeLabel,
                          style: theme.textTheme.labelMedium,
                        ),
                        const SizedBox(height: 4),
                        Row(
                          children: [
                            Expanded(
                              child: SelectableText(
                                _deviceCode!,
                                key: const Key('codex-device-code'),
                                style: theme.textTheme.headlineSmall?.copyWith(
                                  letterSpacing: 3,
                                  fontWeight: FontWeight.w600,
                                ),
                              ),
                            ),
                            IconButton(
                              key: const Key('codex-device-code-copy'),
                              tooltip: l10n.accountCopyCode,
                              icon: const Icon(Icons.copy, size: 20),
                              onPressed: () async {
                                final messenger = ToastMessenger.of(context);
                                await Clipboard.setData(
                                  ClipboardData(text: _deviceCode!),
                                );
                                messenger.ok(l10n.copied);
                              },
                            ),
                          ],
                        ),
                        if (_deviceUrl != null && _deviceUrl!.isNotEmpty)
                          Align(
                            alignment: Alignment.centerLeft,
                            child: TextButton.icon(
                              onPressed: () => launchUrl(
                                Uri.parse(_deviceUrl!),
                                mode: LaunchMode.externalApplication,
                              ),
                              icon: const Icon(Icons.open_in_new, size: 16),
                              label: Text(l10n.codexSetupDeviceReopen),
                            ),
                          ),
                      ],
                    ),
                  ),
                ],

                const Divider(height: 40),

                // --- 3. 非降智 system prompt ---
                SwitchListTile(
                  key: const Key('codex-nondegraded-toggle'),
                  contentPadding: EdgeInsets.zero,
                  title: Text(l10n.codexSetupNonDegradedTitle),
                  subtitle: Text(l10n.codexSetupNonDegradedDesc),
                  value: nonDegraded,
                  onChanged: (status == null || _busy)
                      ? null
                      : (on) => _run(
                          () => _api.codexSetPromptVariant(
                            on ? 'non_degraded' : 'default',
                          ),
                        ),
                ),

                if (_info != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 16),
                    child: Text(
                      _info!,
                      key: const Key('codex-setup-info'),
                      style: TextStyle(color: theme.colorScheme.primary),
                    ),
                  ),
                if (_error != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 16),
                    child: Text(
                      _error!,
                      key: const Key('codex-setup-error'),
                      style: TextStyle(color: theme.colorScheme.error),
                    ),
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _statusCard(
    AppLocalizations l10n,
    ThemeData theme,
    CodexSetupStatus s,
  ) {
    final ready = !s.needsSetup;
    final color = ready ? theme.colorScheme.primary : theme.colorScheme.error;
    final label = ready
        ? (s.hasCustomProvider
              ? l10n.codexSetupStatusProvider
              : l10n.codexSetupStatusAuth(
                  s.authMode ?? l10n.codexSetupCredentialExists,
                ))
        : l10n.codexSetupStatusNeedSetup;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Row(
          children: [
            Icon(
              ready ? Icons.check_circle : Icons.error_outline,
              color: color,
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(label, style: theme.textTheme.titleSmall),
                  const SizedBox(height: 2),
                  Text(
                    'CODEX_HOME: ${s.codexHome}',
                    style: theme.textTheme.bodySmall,
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}
