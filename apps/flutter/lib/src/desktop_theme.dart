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

/// The design's corner radii. Spacing is deliberately not tokenised — each
/// widget bakes its own metrics from the design — but these recur often enough
/// that a literal at every site drifts. They live here rather than in
/// `theme.dart` because that file imports this one.

/// Cards, panels, code blocks, message bubbles, menus.
const double kPanelRadius = 12.0;

/// The composer card and other large raised frames.
const double kComposerRadius = 24.0;

/// Small controls: chips, pills, suggestion rows, hover chips.
const double kControlRadius = 8.0;

/// The desktop corner radius. Follows the design's control radius so a
/// desktop-tuned button and a chat-surface chip round the same amount.
const double kDesktopRadius = kControlRadius;

/// Every lift in the design is a single offsetless shadow of the ink at a low
/// alpha — there is no Material `elevation` anywhere. A wider blur is for
/// content that floats over scrolling material.
List<BoxShadow> panelShadow(ColorScheme scheme, {double blur = 12}) => [
  BoxShadow(color: scheme.onSurface.withValues(alpha: 0.02), blurRadius: blur),
];

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

  /// Tokens derived from a colour scheme. The panel is the design's raised
  /// card — opaque in both themes, so it lifts off the page rather than
  /// tinting it.
  factory DesktopTokens.of(ColorScheme s) => DesktopTokens(
    radius: kDesktopRadius,
    panel: s.surfaceBright,
    border: s.outlineVariant,
    hover: s.surfaceContainer,
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

/// A pointing hand while enabled, the plain arrow while disabled — what a
/// desktop pointer should say about a control.
///
/// Flutter's own `WidgetStateMouseCursor.adaptiveClickable` resolves to a hand
/// ONLY on web; on a native desktop build it returns `basic`, so every button,
/// row and chip hovers as if it were inert text. The button/list themes below
/// substitute this, and [clickable] is the same value for the ink widgets that
/// can't be reached from a theme at all — `InkWell` hardcodes
/// `adaptiveClickable` (see ink_well.dart), so it has to be passed explicitly.
const WidgetStateMouseCursor clickable = WidgetStateMouseCursor.resolveWith(
  _clickableCursor,
  debugDescription: 'desktopClickable',
);

MouseCursor _clickableCursor(Set<WidgetState> states) =>
    states.contains(WidgetState.disabled)
    ? SystemMouseCursors.basic
    : SystemMouseCursors.click;

/// Re-tune [base] for the desktop design language. Same colour scheme, denser
/// and flatter chrome.
ThemeData desktopize(ThemeData base) {
  final scheme = base.colorScheme;
  final border = BorderSide(color: scheme.outlineVariant, width: 1);
  final radius = BorderRadius.circular(kDesktopRadius);
  final shape = RoundedRectangleBorder(borderRadius: radius);
  // Menus float over arbitrary content, so they sit on the opaque raised card
  // rather than a wash — a translucent menu would let text read through it.
  final menuColor = scheme.surfaceBright;
  // A menu's hairline is a step firmer than a panel's, for the same reason.
  final menuShape = RoundedRectangleBorder(
    borderRadius: BorderRadius.circular(kPanelRadius),
    side: BorderSide(color: scheme.outline),
  );

  return base.copyWith(
    // Denser rows, buttons, list tiles — desktop packs more per screen.
    visualDensity: VisualDensity.compact,
    // Hover is the desktop's primary affordance; the ripple is a touch idiom.
    // Both are ink washes off the container ladder, so they composite correctly
    // on the page and on a raised card alike.
    splashFactory: NoSplash.splashFactory,
    hoverColor: scheme.surfaceContainer,
    highlightColor: scheme.surfaceContainerHigh,
    // A pointing hand over anything clickable. Flutter's default for ink
    // widgets (`WidgetStateMouseCursor.adaptiveClickable`) resolves to a click
    // cursor ONLY on web — on a native desktop build it returns `basic`, so
    // every button, list row and chip in the app hovered as if it were inert
    // content. Set once here rather than per widget: 90-odd call sites would
    // each have had to remember, and a missed one is invisible until someone
    // notices the arrow never changes.
    //
    // Text and text-like surfaces are NOT affected — `TextField`,
    // `SelectionArea` and links resolve their own cursor (`textable` /
    // per-span), so an I-beam still wins where the content is selectable.
    iconButtonTheme: IconButtonThemeData(
      style: ButtonStyle(mouseCursor: clickable),
    ),
    listTileTheme: base.listTileTheme.copyWith(mouseCursor: clickable),
    checkboxTheme: base.checkboxTheme.copyWith(mouseCursor: clickable),
    radioTheme: base.radioTheme.copyWith(mouseCursor: clickable),
    switchTheme: base.switchTheme.copyWith(mouseCursor: clickable),
    segmentedButtonTheme: base.segmentedButtonTheme.copyWith(
      style: ButtonStyle(mouseCursor: clickable),
    ),
    dividerTheme: base.dividerTheme.copyWith(
      color: scheme.outlineVariant,
      thickness: 1,
      space: 1,
    ),
    // Cards keep the base theme's panel look (raised card + panel radius);
    // the lift is a soft shadow, not a Material elevation, on desktop too.
    dialogTheme: base.dialogTheme.copyWith(
      elevation: 0,
      backgroundColor: scheme.surfaceBright,
      surfaceTintColor: Colors.transparent,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(kPanelRadius),
        side: BorderSide(color: scheme.outline),
      ),
    ),
    popupMenuTheme: base.popupMenuTheme.copyWith(
      elevation: 0,
      color: menuColor,
      surfaceTintColor: Colors.transparent,
      shape: menuShape,
    ),
    menuTheme: MenuThemeData(
      style: MenuStyle(
        elevation: const WidgetStatePropertyAll(0),
        surfaceTintColor: const WidgetStatePropertyAll(Colors.transparent),
        backgroundColor: WidgetStatePropertyAll(menuColor),
        shape: WidgetStatePropertyAll(menuShape),
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
      ).copyWith(mouseCursor: clickable),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: OutlinedButton.styleFrom(
        shape: shape,
        side: border,
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
      ).copyWith(mouseCursor: clickable),
    ),
    textButtonTheme: TextButtonThemeData(
      style: TextButton.styleFrom(
        shape: shape,
      ).copyWith(mouseCursor: clickable),
    ),
    elevatedButtonTheme: ElevatedButtonThemeData(
      style: const ButtonStyle(mouseCursor: clickable),
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
