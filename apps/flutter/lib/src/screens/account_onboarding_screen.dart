import 'dart:async';

import 'package:flutter/material.dart';
import 'package:pocket_codex/src/widgets/window_title_bar.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/web_authenticator.dart';
import 'package:pocket_codex/src/widgets/brand_logo.dart';
import 'package:url_launcher/url_launcher.dart';

/// Default first-run experience: sign in to a hosted account. The convenient
/// browser-redirect login is the default ("Sign in with GitHub" opens a browser
/// and returns automatically); a device-code fallback (enter a code on GitHub)
/// stays one tap away. The self-host relay setup remains behind "Advanced".
class AccountOnboardingScreen extends ConsumerStatefulWidget {
  /// Default constructor.
  const AccountOnboardingScreen({super.key});

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

  /// Confirm a successful sign-in with a toast. Shown via the root
  /// ScaffoldMessenger so it survives the immediate `context.go('/')`.
  void _showSignedIn(String? login) {
    if (!mounted) return;
    final l10n = AppLocalizations.of(context);
    final message = (login != null && login.isNotEmpty)
        ? l10n.accountSignedInAs(login)
        : l10n.accountSignedIn;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(message),
        behavior: SnackBarBehavior.floating,
        duration: const Duration(seconds: 3),
      ),
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
                  Text(l10n.accountEnterCode, textAlign: TextAlign.center),
                  const SizedBox(height: 12),
                  SelectableText(
                    device.userCode,
                    textAlign: TextAlign.center,
                    style: theme.textTheme.headlineMedium?.copyWith(
                      letterSpacing: 4,
                    ),
                  ),
                  const SizedBox(height: 4),
                  TextButton.icon(
                    onPressed: () =>
                        Clipboard.setData(ClipboardData(text: device.userCode)),
                    icon: const Icon(Icons.copy, size: 16),
                    label: Text(l10n.accountCopyCode),
                  ),
                  const SizedBox(height: 12),
                  FilledButton.icon(
                    onPressed: () => _openVerification(device.verificationUri),
                    icon: const Icon(Icons.open_in_new),
                    label: Text(l10n.accountOpenGitHub),
                  ),
                  const SizedBox(height: 20),
                  const Center(
                    child: SizedBox(
                      width: 18,
                      height: 18,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    ),
                  ),
                  const SizedBox(height: 8),
                  Text(
                    l10n.accountWaiting,
                    textAlign: TextAlign.center,
                    style: theme.textTheme.bodySmall,
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
