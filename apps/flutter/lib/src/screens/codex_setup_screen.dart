import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/widgets/app_toast.dart';
import 'package:pocket_codex/src/widgets/code_row.dart';
import 'package:pocket_codex/src/widgets/group_card.dart';
import 'package:pocket_codex/src/widgets/icon_badge.dart';
import 'package:pocket_codex/src/widgets/links.dart';
import 'package:pocket_codex/src/widgets/status_dots.dart';
import 'package:pocket_codex/src/widgets/utility_page.dart';

/// Form fields on this page stand alone on the sheet rather than inside a
/// bordered toolbar, so they keep an outline — see `SearchField` for the filled,
/// borderless counterpart used in filters.
final _fieldBorder = OutlineInputBorder(
  borderRadius: BorderRadius.circular(kControlRadius),
);

/// Which access method a row represents. Both write to the same `CODEX_HOME`,
/// and codex honours whichever was configured last — so exactly one is ever
/// "in use", which is what makes them rows of one list rather than two
/// side-by-side sections.
enum _Method {
  /// ChatGPT OAuth, driven on the local app-server; codex writes `auth.json`.
  chatgpt,

  /// A Base URL + API key written into codex's `config.toml`.
  provider,
}

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
/// The two used to sit as equal, both-expanded "Option 1 / Option 2" sections,
/// which said nothing about which one codex is actually using and left three
/// unusable text fields in front of an already-signed-in user. They are now one
/// card of two rows: whichever is live sorts first and carries an "in use" pill;
/// the other collapses to a title and a way to switch.
///
/// 另外提供「非降智 system prompt」开关(见 openai/codex#30364),as its own
/// Advanced card — it is about how codex prompts the model, not about access.
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

  /// The method the user opened for editing, or null for "just show me where I
  /// stand". The live one is expanded on arrival so its state and controls are
  /// visible without a click; picking the other one swaps the expansion.
  _Method? _expanded;

  /// Whether the user has chosen an expansion themselves. Until they do, the
  /// expansion follows [_liveMethod], so a login completing mid-visit moves the
  /// disclosure with it instead of leaving a stale panel open.
  bool _expandedByUser = false;

  @override
  void initState() {
    super.initState();
    _loadStatus();
  }

  /// The method codex is actually set up with, or null when neither is.
  ///
  /// A credential wins over a custom provider when both exist: `hasAuth` means
  /// codex can authenticate on its own, and that is the path it takes.
  _Method? get _liveMethod {
    final status = _status;
    if (status == null) return null;
    if (status.hasAuth) return _Method.chatgpt;
    if (status.hasCustomProvider) return _Method.provider;
    return null;
  }

  /// Which method's panel is open. Defaults to the live one; with neither
  /// configured, the provider form leads, since it is the one that works without
  /// a running host.
  _Method get _openMethod => _expanded ?? _liveMethod ?? _Method.provider;

  /// The rows, live-first. With neither configured the provider comes first for
  /// the same reason it is the default expansion.
  List<_Method> get _orderedMethods {
    final live = _liveMethod;
    return switch (live) {
      _Method.chatgpt => const [_Method.chatgpt, _Method.provider],
      _Method.provider => const [_Method.provider, _Method.chatgpt],
      null => const [_Method.provider, _Method.chatgpt],
    };
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
        setState(() {
          _status = s;
          // Follow the live method until the user takes over the disclosure, so
          // a login that completes while this page is open moves the open panel
          // with it rather than leaving the old form expanded.
          if (!_expandedByUser) _expanded = null;
        });
        // Keep the chat's "codex needs setup" banner/send-guard in sync with
        // what the wizard just changed (configured a provider, logged in, …).
        ref.invalidate(codexSetupStatusProvider);
      }
    } catch (e) {
      if (mounted) setState(() => _error = friendlyError(e));
    }
  }

  /// A live JSON-RPC session to a locally-hosted app-server, which is what both
  /// codex-account operations run on. Null (with [_error] set) when nothing is
  /// hosted — codex's own login and logout live in the codex process, so there
  /// has to be one running to drive them.
  Future<String?> _connectedLocalHost(AppLocalizations l10n) async {
    final host = (await _api.appServeStatus())
        .where((h) => h.appServiceKey.isNotEmpty)
        .firstOrNull;
    if (host == null) {
      if (mounted) {
        setState(() {
          _busy = false;
          _error = l10n.codexSetupNeedHost;
        });
      }
      return null;
    }
    final key = host.appServiceKey;
    if (!_api.appIsConnected(key)) await _api.appConnect(key, 0);
    return key;
  }

  /// Sign the local codex out on the running host, then refresh.
  Future<void> _logout(AppLocalizations l10n) async {
    setState(() {
      _busy = true;
      _error = null;
      _info = null;
    });
    try {
      final key = await _connectedLocalHost(l10n);
      if (key == null) return;
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
      final key = await _connectedLocalHost(l10n);
      if (key == null) return;
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
    final scheme = theme.colorScheme;
    final status = _status;
    final nonDegraded = status?.promptVariant == 'non_degraded';
    return UtilityPage(
      route: '/settings',
      title: l10n.codexSetup,
      parent: UtilityParent(title: l10n.settingsTitle, route: '/settings'),
      body: SafeArea(
        child: Align(
          alignment: Alignment.topCenter,
          child: ConstrainedBox(
            // Matches the settings page this is pushed from, so the two read as
            // one column rather than two different page widths.
            constraints: const BoxConstraints(maxWidth: 620),
            child: ListView(
              padding: const EdgeInsets.fromLTRB(24, 18, 24, 24),
              children: [
                if (status != null) _statusCard(l10n, theme, status),
                const SizedBox(height: 10),
                GroupCard(
                  title: l10n.codexSetupMethodsTitle,
                  hint: l10n.codexSetupMethodsHint,
                  children: [
                    for (final method in _orderedMethods)
                      _methodRow(l10n, theme, method),
                  ],
                ),
                const SizedBox(height: 10),
                GroupCard(
                  title: l10n.codexSetupAdvanced,
                  children: [_nonDegradedRow(l10n, theme, nonDegraded)],
                ),
                if (_info != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 14),
                    child: _Notice(
                      noticeKey: const Key('codex-setup-info'),
                      icon: Icons.check_circle_outline,
                      message: _info!,
                      color: successColor(scheme),
                    ),
                  ),
                if (_error != null)
                  Padding(
                    padding: const EdgeInsets.only(top: 14),
                    child: _Notice(
                      noticeKey: const Key('codex-setup-error'),
                      icon: Icons.error_outline,
                      message: _error!,
                      color: scheme.error,
                    ),
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  /// One access method as a row of the methods card: always its title, status
  /// and the way in; its controls only when it is the open one.
  Widget _methodRow(AppLocalizations l10n, ThemeData theme, _Method method) {
    final scheme = theme.colorScheme;
    final live = _liveMethod == method;
    final open = _openMethod == method;
    final chatgpt = method == _Method.chatgpt;
    return _MethodRow(
      rowKey: Key('codex-method-${chatgpt ? 'chatgpt' : 'provider'}'),
      icon: chatgpt ? Icons.chat_bubble_outline : Icons.bolt_outlined,
      title: chatgpt
          ? l10n.codexSetupLoginSection
          : l10n.codexSetupProviderSection,
      subtitle: chatgpt
          ? l10n.codexSetupLoginDesc
          : l10n.codexSetupProviderDesc,
      status: live
          ? StatusChip(
              color: successColor(scheme),
              label: l10n.codexSetupInUse,
              filled: true,
            )
          : null,
      open: open,
      // Only the closed row offers a way in. Collapsing the open one too would
      // allow a card with two shut rows and no controls at all — a state with
      // nothing to do in it, reachable by clicking the only visible button.
      actionLabel: open ? null : l10n.codexSetupSwitchTo,
      onToggle: open
          ? null
          : () => setState(() {
              _expandedByUser = true;
              _expanded = method;
            }),
      detail: open
          ? (chatgpt ? _chatgptDetail(l10n, theme) : _providerDetail(l10n))
          : null,
    );
  }

  /// The custom-provider form: where codex should send requests, and with what.
  Widget _providerDetail(AppLocalizations l10n) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
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
        decoration: InputDecoration(labelText: 'API Key', border: _fieldBorder),
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
      Align(
        alignment: Alignment.centerLeft,
        child: FilledButton(
          key: const Key('codex-save-provider'),
          onPressed: _busy ? null : () => _saveProvider(l10n),
          child: Text(l10n.codexSetupSaveProvider),
        ),
      ),
    ],
  );

  /// The ChatGPT login body: sign in, or (once signed in) what we are signed in
  /// as and how to sign out. The device-code panel appears here when codex could
  /// not bind its local callback port.
  Widget _chatgptDetail(AppLocalizations l10n, ThemeData theme) {
    final scheme = theme.colorScheme;
    final status = _status;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (status?.hasAuth == true)
          // Signed in → what we are signed in as, and the way out. No login
          // button: pressing it would restart an OAuth flow for an account
          // already in place.
          Row(
            children: [
              Expanded(
                child: Text(
                  l10n.codexSetupStatusAuth(
                    status!.authMode ?? l10n.codexSetupCredentialExists,
                  ),
                  key: const Key('codex-login-done'),
                  style: TextStyle(color: scheme.onSurfaceVariant),
                ),
              ),
              OutlinedButton(
                key: const Key('codex-logout-btn'),
                style: OutlinedButton.styleFrom(
                  foregroundColor: scheme.error,
                  side: BorderSide(color: scheme.error),
                ),
                onPressed: _busy ? null : () => _logout(l10n),
                child: Text(l10n.accountSignOut),
              ),
            ],
          )
        else ...[
          Align(
            alignment: Alignment.centerLeft,
            child: FilledButton.icon(
              key: const Key('codex-login-chatgpt'),
              onPressed: (_busy || _loginPolling)
                  ? null
                  : () => _startChatgptLogin(l10n),
              icon: _loginPolling
                  ? const SizedBox(
                      width: 15,
                      height: 15,
                      child: CircularProgressIndicator(strokeWidth: 1.8),
                    )
                  : const Icon(Icons.open_in_new, size: 18),
              label: Text(
                _loginPolling
                    ? l10n.codexSetupLoginWaiting
                    : l10n.codexSetupLoginButton,
              ),
            ),
          ),
          const SizedBox(height: 10),
          // The proxy caveat used to trail the section description, where it
          // read as part of what this method IS. It is a precondition, so it
          // sits with the control it constrains.
          _InlineHint(text: l10n.codexSetupProxyNote),
        ],
        // Device-code flow: show the one-time code to enter on the opened
        // verification page (browser callback port unavailable).
        if (_deviceCode != null && _deviceCode!.isNotEmpty) ...[
          const SizedBox(height: 12),
          Container(
            width: double.infinity,
            padding: const EdgeInsets.all(14),
            decoration: BoxDecoration(
              color: scheme.surfaceContainerHighest,
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
      ],
    );
  }

  /// The prompt-variant switch, as an Advanced row. Its "what" and its "when"
  /// used to run together in one paragraph; the scope and the upstream issue are
  /// their own line now, the issue as a link rather than bare text.
  Widget _nonDegradedRow(
    AppLocalizations l10n,
    ThemeData theme,
    bool nonDegraded,
  ) {
    final scheme = theme.colorScheme;
    final status = _status;
    return Padding(
      padding: const EdgeInsets.fromLTRB(13, 11, 11, 12),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Padding(
            // Nudged down so the badge aligns with the title's cap height rather
            // than the block of wrapped text beneath it.
            padding: EdgeInsets.only(top: 2),
            child: IconBadge(icon: Icons.psychology_outlined),
          ),
          const SizedBox(width: 11),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  l10n.codexSetupNonDegradedTitle,
                  style: const TextStyle(fontWeight: FontWeight.w600),
                ),
                const SizedBox(height: 3),
                Text(
                  l10n.codexSetupNonDegradedDesc,
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: scheme.onSurfaceVariant,
                    height: 1.4,
                  ),
                ),
                const SizedBox(height: 5),
                // Scope and provenance on one quiet line: when it takes effect,
                // then the upstream issue as a real link.
                Row(
                  children: [
                    Text(
                      l10n.codexSetupNonDegradedScope,
                      style: theme.textTheme.bodySmall?.copyWith(
                        color: onSurfaceMuted(scheme),
                      ),
                    ),
                    Text(
                      '  ·  ',
                      style: TextStyle(color: onSurfaceMuted(scheme)),
                    ),
                    Flexible(
                      child: linkifyText(
                        context,
                        'https://github.com/${l10n.codexSetupNonDegradedIssue.replaceFirst('#', '/issues/')}',
                        style: theme.textTheme.bodySmall,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                  ],
                ),
              ],
            ),
          ),
          const SizedBox(width: 8),
          Switch(
            key: const Key('codex-nondegraded-toggle'),
            value: nonDegraded,
            onChanged: (status == null || _busy)
                ? null
                : (on) => _run(
                    () => _api.codexSetPromptVariant(
                      on ? 'non_degraded' : 'default',
                    ),
                  ),
          ),
        ],
      ),
    );
  }

  /// Where the user stands, in one card: whether codex can call a model at all,
  /// which method got it there, and where its config lives.
  ///
  /// The path used to lead with the raw `CODEX_HOME:` env-var name under a
  /// status line. It is now labelled in words and demoted to the footer — it is
  /// reference material, not the answer to "is this working".
  Widget _statusCard(
    AppLocalizations l10n,
    ThemeData theme,
    CodexSetupStatus s,
  ) {
    final scheme = theme.colorScheme;
    final ready = !s.needsSetup;
    final color = ready ? successColor(scheme) : scheme.error;
    final label = ready
        ? (s.hasAuth
              ? l10n.codexSetupStatusAuth(
                  s.authMode ?? l10n.codexSetupCredentialExists,
                )
              : l10n.codexSetupStatusProvider)
        : l10n.codexSetupStatusNeedSetup;
    return Card(
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(14, 13, 14, 13),
            child: Row(
              children: [
                Container(
                  width: 38,
                  height: 38,
                  decoration: BoxDecoration(
                    color: color.withValues(alpha: 0.14),
                    borderRadius: BorderRadius.circular(kControlRadius),
                  ),
                  child: Icon(
                    ready ? Icons.check_circle_outline : Icons.error_outline,
                    size: 20,
                    color: color,
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        label,
                        key: const Key('codex-setup-state'),
                        style: const TextStyle(fontWeight: FontWeight.w600),
                      ),
                      const SizedBox(height: 2),
                      Text(
                        ready
                            ? l10n.codexSetupStatusReadyHint
                            : l10n.codexSetupStatusNeedSetupHint,
                        style: theme.textTheme.bodySmall?.copyWith(
                          color: scheme.onSurfaceVariant,
                          height: 1.4,
                        ),
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
          Divider(height: 1, color: scheme.outlineVariant),
          Padding(
            padding: const EdgeInsets.fromLTRB(14, 6, 12, 7),
            // Unfilled: the card is already the ground, so a tinted row inside
            // it would read as a box in a box.
            child: CodeRow(
              value: s.codexHome,
              label: l10n.codexSetupCodexHomeLabel,
              filled: false,
              copyKey: const Key('codex-home-copy'),
            ),
          ),
        ],
      ),
    );
  }
}

/// One access-method row: identity and status always, controls when [open].
///
/// The closed form is deliberately a header only. Two methods both showing their
/// full controls is what made the old page unreadable — three text fields in
/// front of a signed-in user, with nothing saying which half codex obeys.
class _MethodRow extends StatelessWidget {
  const _MethodRow({
    required this.rowKey,
    required this.icon,
    required this.title,
    required this.subtitle,
    required this.open,
    this.status,
    this.actionLabel,
    this.onToggle,
    this.detail,
  });

  /// Identifies this row (not the widget's own `key`, so the animated container
  /// below can keep its identity across an expansion change).
  final Key rowKey;
  final IconData icon;
  final String title;
  final String subtitle;

  /// Whether this row's controls are shown.
  final bool open;

  /// The "in use" pill, on the method codex actually runs on.
  final Widget? status;

  /// Label for the way in. Null on the open row, which has nowhere to go.
  final String? actionLabel;
  final VoidCallback? onToggle;

  /// This method's controls, built only when [open].
  final Widget? detail;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final header = Row(
      children: [
        // Accented while open: the method being configured is the subject of the
        // card, the collapsed one is a label on a row.
        IconBadge(icon: icon, accent: open),
        const SizedBox(width: 11),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(title, style: const TextStyle(fontWeight: FontWeight.w600)),
              const SizedBox(height: 2),
              Text(
                subtitle,
                style: Theme.of(context).textTheme.bodySmall?.copyWith(
                  color: scheme.onSurfaceVariant,
                  height: 1.4,
                ),
              ),
            ],
          ),
        ),
        if (status != null) ...[const SizedBox(width: 8), status!],
        if (actionLabel != null) ...[
          const SizedBox(width: 8),
          OutlinedButton(onPressed: onToggle, child: Text(actionLabel!)),
        ],
      ],
    );
    final content = Padding(
      key: rowKey,
      padding: const EdgeInsets.fromLTRB(13, 11, 11, 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          header,
          if (detail != null) ...[
            const SizedBox(height: 14),
            // Indented to the text column so the controls read as belonging to
            // the title above them rather than to the card.
            Padding(padding: const EdgeInsets.only(left: 43), child: detail!),
          ],
        ],
      ),
    );
    // A closed row is a target; an open one is where you already are.
    if (open || onToggle == null) return content;
    return InkWell(mouseCursor: clickable, onTap: onToggle, child: content);
  }
}

/// A quiet caveat under the control it constrains.
class _InlineHint extends StatelessWidget {
  const _InlineHint({required this.text});

  final String text;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: const EdgeInsets.only(top: 1.5),
          child: Icon(
            Icons.info_outline,
            size: 14,
            color: onSurfaceMuted(scheme),
          ),
        ),
        const SizedBox(width: 6),
        Expanded(
          child: Text(
            text,
            style: Theme.of(context).textTheme.bodySmall?.copyWith(
              color: onSurfaceMuted(scheme),
              height: 1.4,
            ),
          ),
        ),
      ],
    );
  }
}

/// The page's outcome line — a tinted panel rather than bare coloured text, so a
/// result under a card stack reads as a result.
class _Notice extends StatelessWidget {
  const _Notice({
    required this.noticeKey,
    required this.icon,
    required this.message,
    required this.color,
  });

  final Key noticeKey;
  final IconData icon;
  final String message;
  final Color color;

  @override
  Widget build(BuildContext context) => Container(
    key: noticeKey,
    padding: const EdgeInsets.fromLTRB(12, 10, 12, 10),
    decoration: BoxDecoration(
      color: color.withValues(alpha: 0.10),
      borderRadius: BorderRadius.circular(kPanelRadius),
      border: Border.all(color: color.withValues(alpha: 0.35)),
    ),
    child: Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Icon(icon, size: 17, color: color),
        const SizedBox(width: 9),
        // Linkified: a provider URL or an upstream error often carries one, and
        // it was previously plain text the user had to retype.
        Expanded(
          child: linkifyText(
            context,
            message,
            style: TextStyle(color: color, height: 1.4),
          ),
        ),
      ],
    ),
  );
}
