import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/web_authenticator.dart';
import 'package:pocket_codex/src/widgets/app_toast.dart';
import 'package:pocket_codex/src/widgets/brand_logo.dart';
import 'package:pocket_codex/src/widgets/window_title_bar.dart';
import 'package:url_launcher/url_launcher.dart';

/// Default first-run experience: sign in to a hosted account. The convenient
/// browser-redirect login is the default ("Sign in with GitHub" opens a browser
/// and returns automatically); a device-code fallback (enter a code on GitHub)
/// stays one tap away. The self-host relay setup remains behind "Advanced".
class AccountOnboardingScreen extends ConsumerStatefulWidget {
  /// Default constructor.
  const AccountOnboardingScreen({super.key, this.sessionExpired = false});

  /// Whether this sign-in was opened because a saved session cannot renew.
  final bool sessionExpired;

  @override
  ConsumerState<AccountOnboardingScreen> createState() =>
      _AccountOnboardingState();
}

class _AccountOnboardingState extends ConsumerState<AccountOnboardingScreen> {
  DeviceCode? _device;
  String? _error;
  bool _busy = false;
  bool _polling = false;
  bool _advanced = false;
  final _backend = TextEditingController();

  @override
  void dispose() {
    _polling = false; // stop the poll loop if the screen goes away
    _backend.dispose();
    super.dispose();
  }

  /// The convenient default: open a browser, let GitHub authorize, and come back
  /// automatically. The backend brokers GitHub's authorization-code flow; we only
  /// ever hold a one-time exchange code (+ a PKCE verifier the backend never sees).
  Future<void> _startWeb() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    final api = ref.read(bridgeApiProvider);
    final authenticator = ref.read(webAuthenticatorProvider);
    final l10n = AppLocalizations.of(context);
    String? failure;
    try {
      // Blank backend → the built-in default (lb7666.top); a self-deployed
      // backend can be entered under "Advanced".
      final override = _backend.text.trim();
      final cb = webAuthCallback();
      final start = await api.accountWebLoginStart(
        redirectUri: cb.redirectUri,
        backend: override.isEmpty ? null : override,
      );
      final result = await authenticator.authenticate(
        url: start.authorizeUrl,
        callbackUrlScheme: cb.callbackScheme,
      );
      final params = Uri.parse(result).queryParameters;
      final err = params['error'];
      final code = params['exchange_code'];
      if (err != null && err.isNotEmpty) {
        failure = err == 'access_denied'
            ? l10n.accountDenied
            : l10n.accountWebFailed;
      } else if (params['state'] != start.state ||
          code == null ||
          code.isEmpty) {
        // A mismatched state or missing code means the redirect wasn't ours.
        failure = l10n.accountWebFailed;
      } else {
        final user = await api.accountWebLoginExchange(
          exchangeCode: code,
          codeVerifier: start.codeVerifier,
          backend: start.backend,
        );
        _showSignedIn(user.login);
        await _goAfterSignIn();
        return;
      }
    } on PlatformException catch (e) {
      // The browser hand-off didn't come back. CANCELED is the user dismissing
      // the tab — on Android that is also what a GitHub page that won't load
      // looks like, since a Custom Tab has no timeout of its own. Either way the
      // remedy is the same, and it is the one thing that still works when the
      // in-app browser can't reach GitHub: the device code, which goes through
      // the backend.
      failure = e.code == 'CANCELED'
          ? l10n.accountWebTrouble
          : friendlyError(e);
    } catch (e) {
      // Includes the desktop loopback listener giving up (see the timeout in
      // web_authenticator.dart) and a redirect that never arrived because the
      // browser opened a profile that isn't signed in. Lead with the remedy, but
      // keep the cause: a raw transport error is what makes a real bug
      // diagnosable when someone reports it.
      failure = '${l10n.accountWebTrouble}\n\n${friendlyError(e)}';
    }
    if (mounted) {
      setState(() {
        _error = failure;
        _busy = false;
      });
    }
  }

  /// Fallback for environments without a usable browser hand-off: show a code to
  /// type at github.com/login/device, then poll until authorized.
  Future<void> _startDevice() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    final api = ref.read(bridgeApiProvider);
    try {
      // Blank backend → the built-in default (lb7666.top); a self-deployed
      // backend can be entered under "Advanced".
      final override = _backend.text.trim();
      final device = await api.accountLoginStart(
        backend: override.isEmpty ? null : override,
      );
      if (!mounted) return;
      setState(() {
        _device = device;
        _busy = false;
      });
      unawaited(_poll(device));
    } catch (e) {
      if (mounted) {
        setState(() {
          _error = friendlyError(e);
          _busy = false;
        });
      }
    }
  }

  Future<void> _poll(DeviceCode device) async {
    _polling = true;
    final api = ref.read(bridgeApiProvider);
    final interval = Duration(
      seconds: device.intervalSecs < 1 ? 5 : device.intervalSecs,
    );
    // Client-side expiry: stop once the device code's lifetime elapses even if
    // the backend never answers, so the spinner can't spin forever.
    final deadline = DateTime.now().add(
      Duration(seconds: device.expiresInSecs < 1 ? 900 : device.expiresInSecs),
    );
    var delay = interval;
    while (_polling && mounted) {
      await Future<void>.delayed(delay);
      if (!_polling || !mounted) return;
      if (DateTime.now().isAfter(deadline)) {
        _polling = false;
        if (mounted) {
          setState(() {
            _device = null;
            _error = AppLocalizations.of(context).accountCodeExpired;
          });
        }
        return;
      }
      // Resolve l10n fresh each iteration so a locale change mid-poll shows the
      // terminal message in the current language.
      final l10n = AppLocalizations.of(context);
      try {
        final poll = await api.accountLoginPoll(
          device.pollHandle,
          device.backend,
        );
        switch (poll.status) {
          case 'authorized':
            _polling = false;
            _showSignedIn(poll.login);
            await _goAfterSignIn();
            return;
          case 'slow_down':
            delay = interval + const Duration(seconds: 5);
          case 'expired':
            _polling = false;
            if (mounted) {
              setState(() {
                _device = null;
                _error = l10n.accountCodeExpired;
              });
            }
            return;
          case 'denied':
            _polling = false;
            if (mounted) {
              setState(() {
                _device = null;
                _error = l10n.accountDenied;
              });
            }
            return;
          default: // pending / unknown: keep polling at the base interval
            delay = interval;
        }
      } catch (_) {
        // Transient network error: keep polling at the base interval.
        delay = interval;
      }
    }
  }

  /// Land the signed-in user: the first sign-in on this device gets the
  /// focused welcome guide (one-click hosting setup on desktop / the
  /// what-to-do-on-the-computer steps on a phone); every later sign-in goes
  /// straight to the chat. Waits (bounded) for the prefs file when its load is
  /// still in flight, degrading to "not seen" — showing the guide twice is a
  /// far smaller cost than never showing it.
  Future<void> _goAfterSignIn() async {
    // The token was just written to config.toml by Rust, but `configProvider`
    // is a FutureProvider that already resolved — without invalidating it every
    // screen we navigate to keeps reading the pre-login snapshot, so the app
    // still believes nobody is signed in (`mode` stays off `account`, the
    // Settings account section stays hidden, and the guide has no account to
    // show). Await the refreshed value so the next route builds from it.
    ref.invalidate(configProvider);
    try {
      await ref.read(configProvider.future).timeout(const Duration(seconds: 2));
    } catch (_) {
      // A slow/failed re-read must not strand the user on the login screen;
      // the destination re-reads config itself.
    }
    if (!mounted) return;
    // Service discovery and reachability were resolved against the old
    // (tokenless) config too, so they'd report nothing available.
    ref.invalidate(servicesProvider);
    var seen = ref.read(uiPrefsProvider).valueOrNull?.guideSeen;
    if (seen == null) {
      try {
        final prefs = await ref
            .read(uiPrefsProvider.future)
            .timeout(const Duration(seconds: 2));
        seen = prefs.guideSeen;
      } catch (_) {
        seen = false;
      }
    }
    if (mounted) context.go(seen ? '/' : '/welcome');
  }

  /// Confirm a successful sign-in. Raised on the app-level ScaffoldMessenger, so
  /// it survives the immediate `context.go('/')`.
  void _showSignedIn(String? login) {
    if (!mounted) return;
    final l10n = AppLocalizations.of(context);
    showToastOk(
      context,
      (login != null && login.isNotEmpty)
          ? l10n.accountSignedInAs(login)
          : l10n.accountSignedIn,
    );
  }

  Future<void> _openVerification(String url) async {
    final uri = Uri.tryParse(url);
    if (uri != null) {
      await launchUrl(uri, mode: LaunchMode.externalApplication);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final theme = Theme.of(context);
    final device = _device;
    return Scaffold(
      appBar: WindowTitleBar(title: Text(l10n.accountSignInTitle)),
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 420),
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(24),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                const Center(child: BrandLogo(size: 72)),
                const SizedBox(height: 24),
                if (widget.sessionExpired) ...[
                  DecoratedBox(
                    key: const Key('account-session-expired'),
                    decoration: BoxDecoration(
                      color: theme.colorScheme.errorContainer,
                      borderRadius: BorderRadius.circular(kControlRadius),
                    ),
                    child: Padding(
                      padding: const EdgeInsets.all(12),
                      child: Row(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Icon(
                            Icons.lock_clock_outlined,
                            size: 20,
                            color: theme.colorScheme.onErrorContainer,
                          ),
                          const SizedBox(width: 10),
                          Expanded(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text(
                                  l10n.accountSessionExpiredTitle,
                                  style: theme.textTheme.titleSmall?.copyWith(
                                    color: theme.colorScheme.onErrorContainer,
                                  ),
                                ),
                                const SizedBox(height: 4),
                                Text(
                                  l10n.accountSessionExpiredMessage,
                                  style: theme.textTheme.bodySmall?.copyWith(
                                    color: theme.colorScheme.onErrorContainer,
                                  ),
                                ),
                              ],
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                  const SizedBox(height: 16),
                ],
                if (_error != null) ...[
                  Text(
                    _error!,
                    textAlign: TextAlign.center,
                    style: TextStyle(color: theme.colorScheme.error),
                  ),
                  const SizedBox(height: 16),
                ],
                // Which sign-in leads depends on the platform, because the two
                // idioms don't cost the same on each. A phone has one browser,
                // shared cookies and a working app-redirect, so the tap-through
                // is fewest steps. A desktop's redirect must round-trip through
                // a loopback listener and often lands in a different browser
                // profile than the one signed into GitHub; the device code has
                // no redirect at all and reaches GitHub via the backend, which
                // is why it stays reliable behind a proxy or VPN.
                if (device == null) ...[
                  if (prefersDeviceCodeLogin) ...[
                    FilledButton.icon(
                      onPressed: _busy ? null : _startDevice,
                      icon: const Icon(Icons.pin_outlined),
                      label: Text(l10n.accountUseDeviceCode),
                    ),
                    const SizedBox(height: 4),
                    TextButton(
                      onPressed: _busy ? null : _startWeb,
                      child: Text(l10n.accountSignInButton),
                    ),
                  ] else ...[
                    FilledButton.icon(
                      onPressed: _busy ? null : _startWeb,
                      icon: const Icon(Icons.login),
                      label: Text(l10n.accountSignInButton),
                    ),
                    const SizedBox(height: 4),
                    TextButton(
                      onPressed: _busy ? null : _startDevice,
                      child: Text(l10n.accountUseDeviceCode),
                    ),
                  ],
                  if (_busy) ...[
                    const SizedBox(height: 12),
                    const Center(
                      child: SizedBox(
                        width: 18,
                        height: 18,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      ),
                    ),
                  ],
                ] else ...[
                  // Three ordered steps, because the code alone doesn't say what
                  // to do with it: take the code, enter it at GitHub, come back.
                  _Step(
                    number: 1,
                    caption: l10n.accountEnterCode,
                    child: _DeviceCodeCard(code: device.userCode),
                  ),
                  const SizedBox(height: 14),
                  _Step(
                    number: 2,
                    child: FilledButton.icon(
                      key: const Key('account-open-github'),
                      onPressed: () =>
                          _openVerification(device.verificationUri),
                      icon: const Icon(Icons.open_in_new, size: 18),
                      label: Text(l10n.accountOpenGitHub),
                    ),
                  ),
                  const SizedBox(height: 14),
                  _Step(
                    number: 3,
                    child: Row(
                      children: [
                        const SizedBox(
                          width: 15,
                          height: 15,
                          child: CircularProgressIndicator(strokeWidth: 1.8),
                        ),
                        const SizedBox(width: 10),
                        Expanded(
                          child: Text(
                            l10n.accountWaiting,
                            style: theme.textTheme.bodySmall?.copyWith(
                              color: theme.colorScheme.onSurfaceVariant,
                            ),
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
                const SizedBox(height: 24),
                // Unobtrusive escape hatch: a self-deployed backend URL or the
                // legacy self-hosted relay. Hidden behind one quiet toggle so
                // the default is simply "Sign in with GitHub".
                if (!_advanced)
                  TextButton(
                    onPressed: () => setState(() => _advanced = true),
                    child: Text(l10n.accountAdvanced),
                  )
                else ...[
                  TextField(
                    controller: _backend,
                    enabled: device == null,
                    keyboardType: TextInputType.url,
                    autocorrect: false,
                    decoration: InputDecoration(
                      labelText: l10n.accountBackendHint,
                      isDense: true,
                    ),
                  ),
                  TextButton(
                    onPressed: () => context.go('/onboarding/self-host'),
                    child: Text(l10n.accountAdvancedSelfHost),
                  ),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// One numbered step of the device-code flow.
///
/// The number is what makes the three read as an order rather than three
/// unrelated controls; [caption] explains a step whose content can't speak for
/// itself (the bare code), and is omitted where the control already says it.
class _Step extends StatelessWidget {
  const _Step({required this.number, required this.child, this.caption});

  final int number;
  final Widget child;
  final String? caption;

  /// A 20px disc, the same tinted-glyph treatment the capability rows use, with
  /// its content indented to clear it.
  static const double _discSize = 20;
  static const double _discGap = 10;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Container(
          width: _discSize,
          height: _discSize,
          alignment: Alignment.center,
          decoration: BoxDecoration(
            color: scheme.primaryContainer,
            shape: BoxShape.circle,
          ),
          child: Text(
            '$number',
            style: TextStyle(
              fontSize: 11.5,
              fontWeight: FontWeight.w600,
              color: scheme.onPrimaryContainer,
            ),
          ),
        ),
        const SizedBox(width: _discGap),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (caption != null) ...[
                Padding(
                  padding: const EdgeInsets.only(top: 1, bottom: 7),
                  child: Text(
                    caption!,
                    style: theme.textTheme.bodySmall?.copyWith(
                      color: scheme.onSurfaceVariant,
                    ),
                  ),
                ),
              ],
              child,
            ],
          ),
        ),
      ],
    );
  }
}

/// The device code, as one tap target that copies it.
///
/// Copying is the whole point of showing the code, so the card itself is the
/// button rather than hanging a separate control underneath — and it confirms,
/// because a silent clipboard write leaves the user unsure whether to retype the
/// code by hand. Monospace with the digits spaced out, since the next thing that
/// happens is a human transcribing it into another window.
class _DeviceCodeCard extends StatelessWidget {
  const _DeviceCodeCard({required this.code});

  final String code;

  static const double _codeSize = 25;
  static const double _codeTracking = 4;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    return Tooltip(
      message: l10n.accountCopyCode,
      child: Material(
        color: scheme.surfaceContainerLowest,
        borderRadius: BorderRadius.circular(kPanelRadius),
        child: InkWell(
          key: const Key('account-code-copy'),
          mouseCursor: clickable,
          borderRadius: BorderRadius.circular(kPanelRadius),
          onTap: () {
            Clipboard.setData(ClipboardData(text: code));
            showToastOk(context, l10n.copied);
          },
          child: Container(
            padding: const EdgeInsets.fromLTRB(14, 12, 12, 12),
            decoration: BoxDecoration(
              borderRadius: BorderRadius.circular(kPanelRadius),
              border: Border.all(color: scheme.outline),
            ),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    code,
                    style: TextStyle(
                      fontFamily: monoFontFamily,
                      fontFamilyFallback: monoCjkFallback,
                      fontSize: _codeSize,
                      letterSpacing: _codeTracking,
                      fontWeight: FontWeight.w600,
                      color: scheme.onSurface,
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                Icon(
                  Icons.copy_outlined,
                  size: 18,
                  color: scheme.onSurfaceVariant,
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
