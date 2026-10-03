import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/error_format.dart';

/// User-facing text of an ACP hub, installer or management error: the
/// localized wording for the `acp.<code>` codes the UI explains itself (TRD
/// §6), otherwise the message after the code prefix.
String acpErrorMessage(AppLocalizations l10n, Object error) =>
    acpCodeMessage(l10n, acpErrorCode(error), friendlyError(error));

/// [acpErrorMessage] for a code reported separately (e.g. a job's
/// `errorCode`), falling back to [message].
String acpCodeMessage(AppLocalizations l10n, String? code, String message) =>
    switch (code) {
      'acp.remote_management_disabled' => l10n.acpRemoteDisabled,
      'acp.unsupported_platform' => l10n.acpUnsupportedPlatform,
      'acp.engine_missing' => l10n.acpEngineMissing,
      'acp.session_not_loadable' => l10n.acpSessionNotLoadable,
      'acp.auth_required' => l10n.acpAuthRequired,
      'acp.host_only' => l10n.acpLoginOnHost,
      _ => message,
    };
