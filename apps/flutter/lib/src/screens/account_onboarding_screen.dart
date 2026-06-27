import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/widgets/brand_logo.dart';
import 'package:url_launcher/url_launcher.dart';

/// Default first-run experience: sign in to a hosted account via GitHub device
/// flow. The self-host relay setup remains available behind an "Advanced" link.
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

  Future<void> _start() async {
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
    final l10n = AppLocalizations.of(context);
    final interval = Duration(
      seconds: device.intervalSecs < 1 ? 5 : device.intervalSecs,
    );
    var delay = interval;
    while (_polling && mounted) {
      await Future<void>.delayed(delay);
      if (!_polling || !mounted) return;
      try {
        final poll = await api.accountLoginPoll(
          device.pollHandle,
          device.backend,
        );
        switch (poll.status) {
          case 'authorized':
            _polling = false;
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
                if (device == null)
                  FilledButton.icon(
                    onPressed: _busy ? null : _start,
                    icon: const Icon(Icons.login),
                    label: Text(l10n.accountSignInButton),
                  )
                else ...[
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
                    onPressed: () => Clipboard.setData(
                      ClipboardData(text: device.userCode),
                    ),
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
