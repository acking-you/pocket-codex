import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart';

/// True on the desktop platforms that run frameless (macOS + Windows). Linux
/// keeps its native title bar for now. Gated on [defaultTargetPlatform] (NOT
/// `dart:io`) so `flutter test` — forced to android — takes the mobile branch
/// and never exercises the desktop insets/drag.
bool get isFramelessDesktop =>
    !kIsWeb &&
    (defaultTargetPlatform == TargetPlatform.macOS ||
        defaultTargetPlatform == TargetPlatform.windows);

/// An [AppBar] replacement for the frameless desktop window.
///
/// The native title bar is hidden (see `desktop_tray.dart`), so this bar sits
/// flush at the window's top edge and IS the title bar: its empty space drags
/// the window ([DragToMoveArea]), and it insets its edges so the native window
/// buttons — macOS traffic lights (top-left), Windows caption buttons
/// (top-right) — don't overlap the leading/actions. The theme's `appBarTheme`
/// flattens tint/elevation so it blends into the content below.
///
/// On mobile (and Linux, which keeps native decorations) it is a plain
/// [AppBar]. Implements [PreferredSizeWidget] so it drops into
/// `Scaffold.appBar` in place of an `AppBar`.
class WindowTitleBar extends StatelessWidget implements PreferredSizeWidget {
  /// Creates a window title bar with the usual [AppBar] slots.
  const WindowTitleBar({
    super.key,
    this.leading,
    this.title,
    this.actions,
    this.bottom,
    this.automaticallyImplyLeading = true,
  });

  /// Leading widget (e.g. a hamburger or back button), as on [AppBar].
  final Widget? leading;

  /// Title widget, as on [AppBar].
  final Widget? title;

  /// Trailing action widgets, as on [AppBar].
  final List<Widget>? actions;

  /// Bottom widget (e.g. a warning strip or tab bar), as on [AppBar].
  final PreferredSizeWidget? bottom;

  /// Whether to imply a leading widget when none is given, as on [AppBar].
  final bool automaticallyImplyLeading;

  /// Width the macOS traffic lights need reserved at the leading edge.
  static const double _macLeadingInset = 68;

  /// Width the Windows caption buttons need reserved at the trailing edge.
  static const double _winTrailingInset = 138;

  @override
  Size get preferredSize =>
      Size.fromHeight(kToolbarHeight + (bottom?.preferredSize.height ?? 0));

  @override
  Widget build(BuildContext context) {
    if (!isFramelessDesktop) {
      return AppBar(
        leading: leading,
        title: title,
        actions: actions,
        bottom: bottom,
        automaticallyImplyLeading: automaticallyImplyLeading,
      );
    }

    final isMac = defaultTargetPlatform == TargetPlatform.macOS;
    final leadInset = isMac ? _macLeadingInset : 0.0;
    final trailInset = isMac ? 0.0 : _winTrailingInset;

    // Push a leading widget right past the traffic lights; when there is none,
    // inset the title instead so it doesn't start under them.
    final leadingWidget = leading == null
        ? null
        : Padding(
            padding: EdgeInsets.only(left: leadInset),
            child: leading,
          );

    return AppBar(
      automaticallyImplyLeading: automaticallyImplyLeading,
      leadingWidth: leading == null ? null : leadInset + kToolbarHeight,
      leading: leadingWidget,
      titleSpacing: leading == null
          ? leadInset + NavigationToolbar.kMiddleSpacing
          : NavigationToolbar.kMiddleSpacing,
      title: title,
      actions: [
        ...?actions,
        SizedBox(width: trailInset),
      ],
      // The bar's empty space drags the window; the leading/title/actions sit
      // above this layer and keep their own taps.
      flexibleSpace: const DragToMoveArea(child: SizedBox.expand()),
      bottom: bottom,
    );
  }
}
