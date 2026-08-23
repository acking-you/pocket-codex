import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/ui_prefs.dart';

/// The light/dark toggle, for a window's own controls.
///
/// Shared rather than per-screen: it belongs next to the window buttons on every
/// full-window surface, and two copies would drift.
class ThemeToggle extends ConsumerWidget {
  /// Creates the toggle.
  const ThemeToggle({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = AppLocalizations.of(context);
    // Keyed off what is actually ON SCREEN, not the stored preference: while
    // following the system there is no stored value, and a button that reads
    // "switch to light" over an already-light UI would be nonsense. This also
    // makes the first tap out of follow-system do the obvious thing — flip to
    // the opposite of what the user is looking at.
    final dark = Theme.of(context).brightness == Brightness.dark;
    final next = dark ? 'light' : 'dark';
    return IconButton(
      key: const Key('theme-toggle-btn'),
      // Cross-fade + rotate rather than a hard swap: the whole UI is mid-
      // transition for 200 ms, so an icon that jumped would be the one thing
      // in the window that didn't move.
      icon: AnimatedSwitcher(
        duration: const Duration(milliseconds: 250),
        transitionBuilder: (child, animation) => RotationTransition(
          turns: Tween(begin: 0.75, end: 1.0).animate(animation),
          child: FadeTransition(opacity: animation, child: child),
        ),
        child: Icon(
          dark ? Icons.dark_mode_outlined : Icons.light_mode_outlined,
          // The key is what makes the switcher animate: same type + no key
          // reads as the same widget and swaps silently.
          key: ValueKey(dark),
          size: 20,
        ),
      ),
      tooltip: dark ? l10n.appearanceLight : l10n.appearanceDark,
      visualDensity: VisualDensity.compact,
      onPressed: () => ref.read(uiPrefsProvider.notifier).setThemeMode(next),
    );
  }
}
