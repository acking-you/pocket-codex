import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/providers.dart';
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
        if (mounted) context.go('/');
        return;
      }
    } on PlatformException catch (e) {
      // Android Custom Tabs have no timeout: when GitHub won't load (a flaky /
      // proxied network where the in-app browser tab can't reach github.com),
      // the user closes the tab and we get CANCELED. Don't fail silently —
      // point them at the device code, which reaches GitHub through the backend
      // and stays reliable when the in-app browser can't.
      failure = e.code == 'CANCELED'
          ? l10n.accountWebTrouble
          : friendlyError(e);
    } catch (e) {
      failure = friendlyError(e);
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
            if (mounted) context.go('/');
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
      appBar: AppBar(title: Text(l10n.accountSignInTitle)),
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
                if (device == null) ...[
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
