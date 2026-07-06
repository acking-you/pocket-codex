import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
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
/// the window ([DragToMoveArea]). Platform differ on the window buttons: macOS
/// keeps its native traffic lights (top-left, floating over content), so we
/// only inset the leading edge to clear them; Windows loses ALL native caption
/// buttons with `TitleBarStyle.hidden`, so this bar draws its own
/// minimize/maximize/close ([_WindowCaptionButtons]) at the trailing edge. The
/// theme's `appBarTheme` flattens tint/elevation so it blends into the content.
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
        // macOS keeps its native traffic lights (top-left); Windows lost its
        // native caption buttons to TitleBarStyle.hidden, so draw our own at
        // the trailing edge.
        if (!isMac) const _WindowCaptionButtons(),
      ],
      // The bar's empty space drags the window; the leading/title/actions sit
      // above this layer and keep their own taps.
      flexibleSpace: const DragToMoveArea(child: SizedBox.expand()),
      bottom: bottom,
    );
  }
}

/// The minimize / maximize-restore / close buttons drawn at the trailing edge
/// of the frameless Windows title bar (macOS uses its native traffic lights, so
/// these are Windows-only). Stateful to swap the maximize glyph for a restore
/// glyph while the window is maximized, tracked via [WindowListener].
class _WindowCaptionButtons extends StatefulWidget {
  const _WindowCaptionButtons();

  @override
  State<_WindowCaptionButtons> createState() => _WindowCaptionButtonsState();
}

class _WindowCaptionButtonsState extends State<_WindowCaptionButtons>
    with WindowListener {
  bool _maximized = false;

  @override
  void initState() {
    super.initState();
    windowManager.addListener(this);
    _syncMaximized();
  }

  @override
  void dispose() {
    windowManager.removeListener(this);
    super.dispose();
  }

  Future<void> _syncMaximized() async {
    try {
      final m = await windowManager.isMaximized();
      if (mounted) setState(() => _maximized = m);
    } catch (_) {
      // Best-effort (and no-op under `flutter test`, which has no real window).
    }
  }

  @override
  void onWindowMaximize() {
    if (mounted) setState(() => _maximized = true);
  }

  @override
  void onWindowUnmaximize() {
    if (mounted) setState(() => _maximized = false);
  }

  Future<void> _toggleMaximize() async {
    try {
      if (await windowManager.isMaximized()) {
        await windowManager.unmaximize();
      } else {
        await windowManager.maximize();
      }
    } catch (_) {}
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        _CaptionButton(
          icon: Icons.remove,
          tooltip: l10n.windowMinimize,
          onTap: () => windowManager.minimize(),
        ),
        _CaptionButton(
          icon: _maximized ? Icons.filter_none : Icons.crop_square,
          tooltip: _maximized ? l10n.windowRestore : l10n.windowMaximize,
          // filter_none (two stacked squares) reads better a touch smaller.
          iconSize: _maximized ? 13 : 15,
          onTap: _toggleMaximize,
        ),
        _CaptionButton(
          icon: Icons.close,
          tooltip: l10n.windowClose,
          isClose: true,
          // Honors setPreventClose → hides to tray, matching the native button.
          onTap: () => windowManager.close(),
        ),
      ],
    );
  }
}

/// A single Windows-style caption button: full-bar-height, fixed-width, with a
/// hover highlight (red for [isClose], a subtle wash otherwise).
class _CaptionButton extends StatefulWidget {
  const _CaptionButton({
    required this.icon,
    required this.tooltip,
    required this.onTap,
    this.isClose = false,
    this.iconSize = 16,
  });

  final IconData icon;
  final String tooltip;
  final VoidCallback onTap;
  final bool isClose;
  final double iconSize;

  /// Each button's width; three of them ≈ the classic 138 px Windows caption.
  static const double width = 46;

  @override
  State<_CaptionButton> createState() => _CaptionButtonState();
}

class _CaptionButtonState extends State<_CaptionButton> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final Color bg;
    final Color fg;
    if (_hover && widget.isClose) {
      bg = const Color(0xFFC42B1C); // Windows close-button red
      fg = Colors.white;
    } else if (_hover) {
      bg = scheme.onSurface.withValues(alpha: 0.08);
      fg = scheme.onSurface;
    } else {
      bg = Colors.transparent;
      fg = scheme.onSurfaceVariant;
    }
    return Tooltip(
      message: widget.tooltip,
      child: MouseRegion(
        onEnter: (_) => setState(() => _hover = true),
        onExit: (_) => setState(() => _hover = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onTap,
          child: Container(
            width: _CaptionButton.width,
            height: kToolbarHeight,
            color: bg,
            alignment: Alignment.center,
            child: Icon(widget.icon, size: widget.iconSize, color: fg),
          ),
        ),
      ),
    );
  }
}
