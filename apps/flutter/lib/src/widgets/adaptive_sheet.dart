import 'package:flutter/material.dart';

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
  double maxWidth = 460,
}) {
  final size = MediaQuery.sizeOf(context);
  Widget body(BuildContext c) {
    final child = builder(c);
    return scrollable ? SingleChildScrollView(child: child) : child;
  }

  if (size.width < kDesktopPanelWidth) {
    return showModalBottomSheet<T>(
      context: context,
      showDragHandle: true,
      isScrollControlled: true,
      constraints: BoxConstraints(maxHeight: size.height * 0.85),
      builder: (c) => SafeArea(child: body(c)),
    );
  }
  return showDialog<T>(
    context: context,
    builder: (c) => Dialog(
      clipBehavior: Clip.antiAlias,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(14)),
      child: ConstrainedBox(
        constraints: BoxConstraints(
          maxWidth: maxWidth,
          maxHeight: size.height * 0.8,
        ),
        child: body(c),
      ),
    ),
  );
}
