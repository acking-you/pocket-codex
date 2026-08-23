import 'package:flutter/material.dart';
import 'package:pocket_codex/src/desktop_theme.dart';

/// Width at or above which a transient panel is shown as a centred dialog
/// rather than a bottom sheet. Matches the transcript's document-layout
/// breakpoint so the whole window changes idiom at once.
const double kDesktopPanelWidth = 720;

/// Show [builder] as the right kind of transient panel for the window.
///
/// A bottom sheet is a thumb idiom: it rises from the edge the hand is near.
/// On a desktop window it reads as a phone control bolted to a big screen and
/// puts its content as far from the pointer as possible, so wide windows get a
/// centred dialog instead.
///
/// Both forms are height-bounded and scrollable. The sheet in particular MUST
/// be `isScrollControlled` — without it Flutter caps the sheet at 9/16 of the
/// screen and simply clips whatever doesn't fit, with no way to reach it.
Future<T?> showAdaptivePanel<T>({
  required BuildContext context,
  required WidgetBuilder builder,

  /// Set false when the body brings its own scrollable (a `ListView`), which
  /// cannot be nested inside another one without an unbounded-height error.
  bool scrollable = true,

  /// Set false when the body draws its own top edge (its own grab bar or
  /// header), so the panel doesn't add a second inset above it.
  bool insetTop = true,
  double maxWidth = 460,
}) {
  final size = MediaQuery.sizeOf(context);
  final scheme = Theme.of(context).colorScheme;
  Widget body(BuildContext c) {
    final child = builder(c);
    return scrollable ? SingleChildScrollView(child: child) : child;
  }

  if (size.width < kDesktopPanelWidth) {
    return showModalBottomSheet<T>(
      context: context,
      showDragHandle: true,
      isScrollControlled: true,
      backgroundColor: scheme.surfaceBright,
      // Lighter than Material's default: the panel is already lifted by its own
      // ground and hairline, so the page only needs dimming, not hiding.
      barrierColor: scheme.onSurface.withValues(alpha: 0.28),
      constraints: BoxConstraints(maxHeight: size.height * 0.85),
      // The drag handle occupies the top edge, so the body starts right under it.
      builder: (c) => SafeArea(
        child: Padding(
          padding: EdgeInsets.only(top: insetTop ? 4 : 0),
          child: body(c),
        ),
      ),
    );
  }
  return showDialog<T>(
    context: context,
    barrierColor: scheme.onSurface.withValues(alpha: 0.28),
    builder: (c) => Dialog(
      clipBehavior: Clip.antiAlias,
      backgroundColor: scheme.surfaceBright,
      // A hairline instead of a shadow-only edge: on a light page a white card
      // on a light scrim has nothing to hold its boundary.
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(kPanelRadius),
        side: BorderSide(color: scheme.outline),
      ),
      child: ConstrainedBox(
        constraints: BoxConstraints(
          maxWidth: maxWidth,
          maxHeight: size.height * 0.8,
        ),
        // No drag handle here, so the body needs its own breathing room at the
        // top — without it a title sits hard against the card's edge.
        child: Padding(
          padding: EdgeInsets.only(top: insetTop ? 20 : 0),
          child: body(c),
        ),
      ),
    ),
  );
}
