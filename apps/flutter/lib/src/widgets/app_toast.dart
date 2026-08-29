import 'package:flutter/material.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/theme.dart';

/// Transient confirmations and failures, in the app's own surface language.
///
/// These are `ScaffoldMessenger` snack bars underneath — `MaterialApp` provides
/// one above the router, so a toast survives the navigation that often follows
/// the action that raised it (signing in, then leaving for the chat). Only the
/// presentation is ours: Material's default is an `inverseSurface` slab with its
/// own elevation and type, which reads as a different product than the raised
/// cards everywhere else.
///
/// Three entry points rather than one with a kind argument, because the call
/// site should say what happened: `showToastOk(context, l10n.copied)`.

/// What a toast is reporting. Drives the leading glyph, its colour, and how long
/// the toast stays: an error takes longer to read than "Copied".
enum _Tone { neutral, ok, error }

/// A neutral notice — something happened, and it is neither good news nor bad.
void showToast(BuildContext context, String message) =>
    _show(context, message, _Tone.neutral);

/// A confirmation: copied, saved, uploaded, signed in.
void showToastOk(BuildContext context, String message) =>
    _show(context, message, _Tone.ok);

/// A failure. Shown longer, since the message carries a reason worth reading.
void showToastError(BuildContext context, String message) =>
    _show(context, message, _Tone.error);

/// A toast raised on a messenger captured before an `await`, for reporting the
/// outcome of work whose own `BuildContext` may be gone by the time it finishes.
/// Needs the [theme] read alongside the messenger, since the styling comes from
/// the scheme and there is no live context to look it up on.
///
/// Prefer the context forms; this exists for the download/upload paths that
/// already hold a messenger for exactly this reason.
class ToastMessenger {
  /// Captures the messenger and theme for [context].
  ToastMessenger.of(BuildContext context)
    : _messenger = ScaffoldMessenger.maybeOf(context),
      _scheme = Theme.of(context).colorScheme,
      _textTheme = Theme.of(context).textTheme,
      _wide = MediaQuery.sizeOf(context).width >= _insetBreakpoint;

  final ScaffoldMessengerState? _messenger;
  final ColorScheme _scheme;
  final TextTheme _textTheme;
  final bool _wide;

  /// A confirmation.
  void ok(String message) => _raise(message, _Tone.ok);

  /// A failure.
  void error(String message) => _raise(message, _Tone.error);

  /// A neutral notice.
  void notice(String message) => _raise(message, _Tone.neutral);

  void _raise(String message, _Tone tone) {
    final messenger = _messenger;
    if (messenger == null) return;
    messenger.hideCurrentSnackBar();
    messenger.showSnackBar(
      _bar(
        message: message,
        tone: tone,
        scheme: _scheme,
        textTheme: _textTheme,
        wide: _wide,
      ),
    );
  }
}

/// The design's toast: a raised card 17px glyph + 10 from its label, padded
/// 13/11, on the panel radius — the same lift as the composer and the floating
/// clusters, so a notice reads as part of the app rather than an OS overlay.
const double _glyphSize = 17;
const double _glyphGap = 10;
const EdgeInsets _cardPadding = EdgeInsets.fromLTRB(13, 11, 13, 11);

/// Wide enough for a sentence, narrow enough that "Copied" doesn't stretch a
/// 1300px window. Below this the toast just insets from both edges instead.
const double _desktopWidth = 380;
const double _insetBreakpoint = 520;

const Duration _briefly = Duration(seconds: 2);
const Duration _longEnoughToRead = Duration(seconds: 4);

void _show(BuildContext context, String message, _Tone tone) {
  final messenger = ScaffoldMessenger.maybeOf(context);
  if (messenger == null) return;
  final theme = Theme.of(context);
  // Tapping a copy button twice should replace the confirmation, not queue a
  // second one behind it.
  messenger.hideCurrentSnackBar();
  messenger.showSnackBar(
    _bar(
      message: message,
      tone: tone,
      scheme: theme.colorScheme,
      textTheme: theme.textTheme,
      wide: MediaQuery.sizeOf(context).width >= _insetBreakpoint,
    ),
  );
}

SnackBar _bar({
  required String message,
  required _Tone tone,
  required ColorScheme scheme,
  required TextTheme textTheme,
  required bool wide,
}) {
  final (IconData icon, Color color) = switch (tone) {
    _Tone.ok => (Icons.check_circle_outline, successColor(scheme)),
    _Tone.error => (Icons.error_outline, scheme.error),
    _Tone.neutral => (Icons.info_outline, infoColor(scheme)),
  };
  return SnackBar(
    // The SnackBar is only the positioning and the timing; every pixel of the
    // visible surface is the card below.
    backgroundColor: Colors.transparent,
    elevation: 0,
    padding: EdgeInsets.zero,
    behavior: SnackBarBehavior.floating,
    duration: tone == _Tone.error ? _longEnoughToRead : _briefly,
    width: wide ? _desktopWidth : null,
    margin: wide ? null : const EdgeInsets.all(12),
    content: DecoratedBox(
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(kPanelRadius),
        boxShadow: panelShadow(scheme),
      ),
      child: Container(
        padding: _cardPadding,
        decoration: BoxDecoration(
          color: surfacePanel(scheme),
          borderRadius: BorderRadius.circular(kPanelRadius),
          border: Border.all(color: scheme.outline),
        ),
        child: Row(
          children: [
            Icon(icon, size: _glyphSize, color: color),
            const SizedBox(width: _glyphGap),
            Expanded(
              child: Text(
                message,
                style: textTheme.bodyMedium?.copyWith(color: scheme.onSurface),
              ),
            ),
          ],
        ),
      ),
    ),
  );
}
