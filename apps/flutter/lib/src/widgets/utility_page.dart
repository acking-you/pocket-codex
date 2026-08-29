import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/widgets/window_title_bar.dart';

/// Shared shell for secondary, full-window pages.
///
/// Desktop pages use an origin breadcrumb (Conversation / Current page), keep
/// page-specific actions on the right, and move cross-page navigation into one
/// quiet overflow menu. Mobile retains the platform-standard implied back
/// button because it is the familiar compact-screen interaction.
class UtilityPage extends StatelessWidget {
  /// Creates a secondary page shell.
  const UtilityPage({
    super.key,
    required this.route,
    required this.title,
    required this.body,
    this.parent,
    this.actions = const [],
    this.bottomNavigationBar,
    this.floatingActionButton,
  });

  /// The top-level route represented by this page.
  final String route;

  /// Localized page title.
  final String title;

  /// The list this page drilled down from, for a detail page. Renders as a third
  /// breadcrumb (`Conversation / Services / <name>`) whose middle segment is the
  /// way back up; omit it on a top-level page.
  final UtilityParent? parent;

  /// Page content below the shared title bar.
  final Widget body;

  /// Actions specific to this page, rendered before the desktop page menu.
  final List<Widget> actions;

  /// Optional narrow-layout navigation supplied by the page.
  final Widget? bottomNavigationBar;

  /// Optional page floating action button.
  final Widget? floatingActionButton;

  @override
  Widget build(BuildContext context) {
    final scaffold = Scaffold(
      appBar: UtilityPageTitleBar(
        route: route,
        title: title,
        parent: parent,
        actions: actions,
      ),
      body: body,
      bottomNavigationBar: bottomNavigationBar,
      floatingActionButton: floatingActionButton,
    );
    return AppPageShortcuts(currentRoute: route, child: scaffold);
  }
}

/// The list a detail page sits under, named so the breadcrumb can lead back to
/// it. [route] is the top-level route the page menu should treat as current, so a
/// detail page highlights the section it belongs to.
@immutable
class UtilityParent {
  /// Creates a parent breadcrumb segment.
  const UtilityParent({required this.title, required this.route});

  /// Localized title of the list this page came from.
  final String title;

  /// Route that list lives at.
  final String route;
}

/// Installs app-level page shortcuts while leaving [child] unchanged.
///
/// Home uses route `/`, so a utility page is pushed above the live
/// conversation. Utility pages replace one another and retain that same
/// conversation beneath them. Compact platforms do not install desktop keys.
class AppPageShortcuts extends StatelessWidget {
  /// Creates a shortcut scope for [currentRoute].
  const AppPageShortcuts({
    super.key,
    required this.currentRoute,
    required this.child,
  });

  /// The top-level route rendered by [child].
  final String currentRoute;

  /// Content receiving the shortcut scope.
  final Widget child;

  @override
  Widget build(BuildContext context) {
    if (!isDesktop) return child;
    return CallbackShortcuts(
      bindings: _pageShortcuts(context, currentRoute),
      child: Focus(autofocus: true, child: child),
    );
  }
}

/// Shared title bar used by [UtilityPage].
class UtilityPageTitleBar extends ConsumerWidget
    implements PreferredSizeWidget {
  /// Creates a utility-page title bar.
  const UtilityPageTitleBar({
    super.key,
    required this.route,
    required this.title,
    this.parent,
    this.actions = const [],
  });

  /// The current top-level route.
  final String route;

  /// Localized current-page title.
  final String title;

  /// The list this page drilled down from, if any.
  final UtilityParent? parent;

  /// Actions that belong only to the current page.
  final List<Widget> actions;

  @override
  Size get preferredSize =>
      Size.fromHeight(kToolbarHeight + (isDesktop ? 1 : 0));

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    if (!isDesktop) {
      return WindowTitleBar(title: Text(title), actions: actions);
    }
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final compact = MediaQuery.sizeOf(context).width < 620;
    return WindowTitleBar(
      automaticallyImplyLeading: false,
      backgroundColor: surfacePanel(scheme),
      title: compact
          ? Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                IconButton.outlined(
                  key: const Key('utility-chat-origin'),
                  tooltip: l10n.utilityChat,
                  onPressed: () => _openUtilityRoute(context, route, '/'),
                  icon: const Icon(Icons.chat_bubble_outline, size: 16),
                  constraints: const BoxConstraints.tightFor(
                    width: 32,
                    height: 32,
                  ),
                  padding: EdgeInsets.zero,
                  style: IconButton.styleFrom(
                    foregroundColor: scheme.onSurfaceVariant,
                    backgroundColor: scheme.surfaceContainerLow,
                    side: BorderSide(color: scheme.outline),
                    visualDensity: VisualDensity.compact,
                  ),
                ),
                const SizedBox(width: 7),
                // Compact drops the parent's label but keeps its tap target, so
                // a detail page still has a way up when the bar can't spell the
                // chain out.
                if (parent != null) ...[
                  _BreadcrumbLink(
                    label: parent!.title,
                    onTap: () => _openParent(context, parent!),
                    compact: true,
                  ),
                  _breadcrumbSeparator(scheme),
                ],
                Flexible(
                  child: Text(
                    title,
                    overflow: TextOverflow.ellipsis,
                    style: Theme.of(context).textTheme.titleSmall?.copyWith(
                      fontSize: 15,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ),
              ],
            )
          : Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                OutlinedButton.icon(
                  key: const Key('utility-chat-origin'),
                  onPressed: () => _openUtilityRoute(context, route, '/'),
                  icon: const Icon(Icons.chat_bubble_outline, size: 17),
                  label: Text(l10n.utilityChat),
                  style: OutlinedButton.styleFrom(
                    foregroundColor: scheme.onSurfaceVariant,
                    backgroundColor: scheme.surfaceContainerLow,
                    side: BorderSide(color: scheme.outline),
                    padding: const EdgeInsets.symmetric(
                      horizontal: 9,
                      vertical: 7,
                    ),
                    visualDensity: VisualDensity.compact,
                  ),
                ),
                _breadcrumbSeparator(scheme),
                if (parent != null) ...[
                  _BreadcrumbLink(
                    label: parent!.title,
                    onTap: () => _openParent(context, parent!),
                  ),
                  _breadcrumbSeparator(scheme),
                ],
                Flexible(
                  child: Text(
                    title,
                    overflow: TextOverflow.ellipsis,
                    style: Theme.of(context).textTheme.titleSmall?.copyWith(
                      fontSize: 15,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ),
              ],
            ),
      actions: [
        ...actions,
        PopupMenuButton<String>(
          key: const Key('utility-page-menu'),
          tooltip: l10n.utilitySwitchPage,
          position: PopupMenuPosition.under,
          icon: const Icon(Icons.more_horiz),
          onSelected: (value) {
            if (value == _themeValue) {
              final dark = Theme.of(context).brightness == Brightness.dark;
              ref
                  .read(uiPrefsProvider.notifier)
                  .setThemeMode(dark ? 'light' : 'dark');
              return;
            }
            _openUtilityRoute(context, route, value);
          },
          // A detail page marks its parent section as current, so the menu shows
          // where you are rather than nothing at all.
          itemBuilder: (context) => _menuItems(context, parent?.route ?? route),
        ),
      ],
      bottom: PreferredSize(
        preferredSize: const Size.fromHeight(1),
        child: Divider(height: 1, color: scheme.outlineVariant),
      ),
    );
  }
}

const _themeValue = '__theme__';

/// Climb from a detail page to the list it came from.
///
/// Pops when that list is the route directly beneath — the usual case, since the
/// detail was pushed from it — which keeps the list's scroll position and
/// selection. Only when the detail was reached some other way (a deep link, or a
/// sibling swap that left no list underneath) does it navigate instead. Note this
/// can't go through `_openUtilityRoute`: a detail page reports its parent's
/// section as its own `route` so the page menu highlights correctly, so that
/// helper would see target == current and do nothing.
void _openParent(BuildContext context, UtilityParent parent) {
  final router = GoRouter.of(context);
  if (router.canPop()) {
    router.pop();
    return;
  }
  context.go(parent.route);
}

/// The `/` between breadcrumb segments — ink at 28%, well under the labels it
/// divides so the chain reads as one line rather than three words.
Widget _breadcrumbSeparator(ColorScheme scheme) => Padding(
  padding: const EdgeInsets.symmetric(horizontal: 7),
  child: Text(
    '/',
    style: TextStyle(
      color: scheme.onSurface.withValues(alpha: 0.28),
      fontWeight: FontWeight.w400,
    ),
  ),
);

/// A middle breadcrumb segment: the current page's parent, tappable to go up.
/// Quieter than the chat origin button (no border, no fill) because the origin
/// leaves the section entirely while this only climbs one level inside it.
class _BreadcrumbLink extends StatelessWidget {
  const _BreadcrumbLink({
    required this.label,
    required this.onTap,
    this.compact = false,
  });

  final String label;
  final VoidCallback onTap;

  /// Narrow bars show the icon-sized hit area without the label.
  final bool compact;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return TextButton(
      key: const Key('utility-parent-origin'),
      onPressed: onTap,
      style: TextButton.styleFrom(
        foregroundColor: scheme.onSurfaceVariant,
        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 7),
        minimumSize: Size.zero,
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
        visualDensity: VisualDensity.compact,
      ),
      child: compact
          ? const Icon(Icons.chevron_left, size: 17)
          : Text(
              label,
              overflow: TextOverflow.ellipsis,
              style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w400),
            ),
    );
  }
}

List<PopupMenuEntry<String>> _menuItems(BuildContext context, String route) {
  final l10n = AppLocalizations.of(context);
  final dark = Theme.of(context).brightness == Brightness.dark;
  final modifier = defaultTargetPlatform == TargetPlatform.macOS ? '⌘' : 'Ctrl';
  final pages =
      <({String route, IconData icon, String label, String shortcut})>[
        (
          route: '/',
          icon: Icons.chat_bubble_outline,
          label: l10n.utilityChat,
          shortcut: '$modifier 0',
        ),
        (
          route: '/manage',
          icon: Icons.dns_outlined,
          label: l10n.manageServices,
          shortcut: '$modifier 1',
        ),
        (
          route: '/sessions',
          icon: Icons.history,
          label: l10n.localSessionsTitle,
          shortcut: '$modifier 2',
        ),
        (
          route: '/logs',
          icon: Icons.article_outlined,
          label: l10n.logsTitle,
          shortcut: '$modifier 3',
        ),
        (
          route: '/settings',
          icon: Icons.settings_outlined,
          label: l10n.settingsTitle,
          shortcut: '$modifier ,',
        ),
      ];
  return [
    PopupMenuItem<String>(
      enabled: false,
      height: 32,
      child: Text(
        l10n.utilityPages,
        style: Theme.of(context).textTheme.labelSmall?.copyWith(
          color: Theme.of(context).colorScheme.onSurfaceVariant,
        ),
      ),
    ),
    for (final page in pages)
      PopupMenuItem<String>(
        value: page.route,
        enabled: page.route != route,
        child: _PageMenuRow(
          icon: page.icon,
          label: page.label,
          shortcut: page.shortcut,
          selected: page.route == route,
        ),
      ),
    const PopupMenuDivider(),
    PopupMenuItem<String>(
      value: _themeValue,
      child: _PageMenuRow(
        icon: dark ? Icons.light_mode_outlined : Icons.dark_mode_outlined,
        label: dark ? l10n.appearanceLight : l10n.appearanceDark,
      ),
    ),
  ];
}

class _PageMenuRow extends StatelessWidget {
  const _PageMenuRow({
    required this.icon,
    required this.label,
    this.shortcut,
    this.selected = false,
  });

  final IconData icon;
  final String label;
  final String? shortcut;
  final bool selected;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return SizedBox(
      width: 220,
      child: Row(
        children: [
          Icon(
            icon,
            size: 18,
            color: selected ? scheme.primary : scheme.onSurfaceVariant,
          ),
          const SizedBox(width: 11),
          Expanded(
            child: Text(
              label,
              style: TextStyle(
                fontWeight: selected ? FontWeight.w600 : FontWeight.w400,
              ),
            ),
          ),
          if (shortcut != null)
            Text(
              shortcut!,
              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                color: scheme.onSurface.withValues(alpha: 0.38),
              ),
            ),
        ],
      ),
    );
  }
}

void _openUtilityRoute(
  BuildContext context,
  String currentRoute,
  String targetRoute,
) {
  if (targetRoute == currentRoute) return;
  if (targetRoute == '/') {
    // The root route keeps a stable page key when it already sits below this
    // page, so `go` removes every utility/detail route above it without
    // rebuilding the conversation. It also behaves correctly for a direct
    // utility deep link whose stack has no conversation yet.
    context.go('/');
    return;
  }
  // Open above Home, then swap sibling utility pages without replacing the
  // live conversation route underneath.
  if (currentRoute == '/') {
    context.push(targetRoute);
  } else {
    context.pushReplacement(targetRoute);
  }
}

Map<ShortcutActivator, VoidCallback> _pageShortcuts(
  BuildContext context,
  String currentRoute,
) {
  void open(String targetRoute) {
    final current = ModalRoute.of(context);
    if (current != null && !current.isCurrent) return;
    _openUtilityRoute(context, currentRoute, targetRoute);
  }

  final meta = defaultTargetPlatform == TargetPlatform.macOS;
  ShortcutActivator shortcut(LogicalKeyboardKey key) =>
      SingleActivator(key, meta: meta, control: !meta);
  return {
    shortcut(LogicalKeyboardKey.digit0): () => open('/'),
    shortcut(LogicalKeyboardKey.digit1): () => open('/manage'),
    shortcut(LogicalKeyboardKey.digit2): () => open('/sessions'),
    shortcut(LogicalKeyboardKey.digit3): () => open('/logs'),
    shortcut(LogicalKeyboardKey.comma): () => open('/settings'),
  };
}
