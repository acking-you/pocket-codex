import 'package:flutter/material.dart';

/// The desktop design layer.
///
/// The app stays on Material everywhere — this doesn't fork the widget tree or
/// adopt a native toolkit (which would be wrong on two of the three desktop
/// OSes we ship). Instead it re-tunes Material itself for a desktop language:
/// denser, quieter, bordered surfaces rather than tonal elevation, tighter
/// radii, crisp inputs, hover feedback instead of touch ripples. Applied only
/// on desktop (see `theme.dart`), so mobile keeps stock Material untouched.
///
/// The feel to aim at is VS Code / Linear / Zed — an app design language, not a
/// skin of any one OS. [DesktopTokens] carries the handful of values shared
/// widgets read so a bespoke desktop primitive stays consistent with the theme.

/// The desktop corner radius. Small enough to read as a tool, not a phone card.
const double kDesktopRadius = 7.0;

/// Values a desktop-flavored widget reads to stay in step with the theme. Only
/// present in the tree on desktop; `context.desktop` is null on mobile.
@immutable
class DesktopTokens extends ThemeExtension<DesktopTokens> {
  const DesktopTokens({
    required this.radius,
    required this.panel,
    required this.border,
    required this.hover,
  });

  /// Shared corner radius.
  final double radius;

  /// A quiet raised surface (a panel, a menu) — a hair above the background
  /// rather than a tonally-elevated Material card.
  final Color panel;

  /// Hairline separator / outline colour.
  final Color border;

  /// Pointer-hover wash for an interactive row or button.
  final Color hover;

  /// Tokens derived from a colour scheme.
  factory DesktopTokens.of(ColorScheme s) => DesktopTokens(
    radius: kDesktopRadius,
    panel: Color.alphaBlend(
      s.surfaceContainerHighest.withValues(alpha: 0.5),
      s.surface,
    ),
    border: s.outlineVariant,
    hover: s.onSurface.withValues(alpha: 0.05),
  );

  @override
  DesktopTokens copyWith({
    double? radius,
    Color? panel,
    Color? border,
    Color? hover,
  }) => DesktopTokens(
    radius: radius ?? this.radius,
    panel: panel ?? this.panel,
    border: border ?? this.border,
    hover: hover ?? this.hover,
  );

  @override
  DesktopTokens lerp(ThemeExtension<DesktopTokens>? other, double t) {
    if (other is! DesktopTokens) return this;
    return DesktopTokens(
      radius: radius + (other.radius - radius) * t,
      panel: Color.lerp(panel, other.panel, t)!,
      border: Color.lerp(border, other.border, t)!,
      hover: Color.lerp(hover, other.hover, t)!,
    );
  }
}

/// Read the desktop tokens, or null on mobile (where they aren't installed).
extension DesktopThemeX on BuildContext {
  DesktopTokens? get desktop => Theme.of(this).extension<DesktopTokens>();
}

/// Re-tune [base] for the desktop design language. Same colour scheme, denser
/// and flatter chrome.
ThemeData desktopize(ThemeData base) {
  final scheme = base.colorScheme;
  final border = BorderSide(color: scheme.outlineVariant, width: 1);
  final radius = BorderRadius.circular(kDesktopRadius);
  final shape = RoundedRectangleBorder(borderRadius: radius);
  final borderedShape = RoundedRectangleBorder(
    borderRadius: radius,
    side: border,
  );

  return base.copyWith(
    // Denser rows, buttons, list tiles — desktop packs more per screen.
    visualDensity: VisualDensity.compact,
    // Hover is the desktop's primary affordance; the ripple is a touch idiom.
    splashFactory: NoSplash.splashFactory,
    hoverColor: scheme.onSurface.withValues(alpha: 0.05),
    highlightColor: scheme.onSurface.withValues(alpha: 0.06),
    dividerTheme: base.dividerTheme.copyWith(
      color: scheme.outlineVariant,
      thickness: 1,
      space: 1,
    ),
    // Surfaces read as bordered panels, not tonally-raised cards.
    cardTheme: base.cardTheme.copyWith(
      elevation: 0,
      color: scheme.surface,
      surfaceTintColor: Colors.transparent,
      shape: borderedShape,
      margin: EdgeInsets.zero,
    ),
    dialogTheme: base.dialogTheme.copyWith(
      elevation: 1,
      surfaceTintColor: Colors.transparent,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(10),
        side: border,
      ),
    ),
    popupMenuTheme: base.popupMenuTheme.copyWith(
      elevation: 2,
      surfaceTintColor: Colors.transparent,
      shape: borderedShape,
    ),
    menuTheme: MenuThemeData(
      style: MenuStyle(
        elevation: const WidgetStatePropertyAll(2),
        surfaceTintColor: const WidgetStatePropertyAll(Colors.transparent),
        backgroundColor: WidgetStatePropertyAll(scheme.surface),
        shape: WidgetStatePropertyAll(borderedShape),
      ),
    ),
    // A quick, quiet tooltip — no long delay, no heavy chrome.
    tooltipTheme: base.tooltipTheme.copyWith(
      waitDuration: const Duration(milliseconds: 400),
      decoration: BoxDecoration(
        color: scheme.inverseSurface.withValues(alpha: 0.92),
        borderRadius: BorderRadius.circular(6),
      ),
      textStyle: TextStyle(color: scheme.onInverseSurface, fontSize: 12),
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: FilledButton.styleFrom(
        shape: shape,
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
      ),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: OutlinedButton.styleFrom(
        shape: shape,
        side: border,
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
      ),
    ),
    textButtonTheme: TextButtonThemeData(
      style: TextButton.styleFrom(shape: shape),
    ),
    // Inputs are left to the widgets: the composer wants a borderless field
    // inside its own frame, the search pills want a filled borderless look, and
    // dialogs want the default outline. A global input theme fights all three,
    // so a desktop-bordered field is an opt-in (a future primitive), not a
    // blanket override. Just tighten the default density.
    inputDecorationTheme: base.inputDecorationTheme.copyWith(isDense: true),
    extensions: [DesktopTokens.of(scheme)],
  );
}
