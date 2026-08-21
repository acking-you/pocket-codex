import 'package:flutter/material.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';

/// An anchored project switcher — the desktop idiom for "which checkout is
/// this conversation about". Opens directly under whatever [builder] renders
/// (a title word, a chip), not as a centred modal: the answer belongs next to
/// the thing being changed.
///
/// The list is search-filtered, the current project carries a check, and the
/// two escape hatches from the reference sit at the bottom: browse for a
/// folder the list doesn't know yet, or work with no project at all.
class ProjectMenu extends StatefulWidget {
  /// Creates a project switcher anchored to [builder]'s widget.
  const ProjectMenu({
    super.key,
    required this.projects,
    required this.current,
    required this.onPick,
    required this.onBrowse,
    required this.onClear,
    required this.builder,
  });

  /// Absolute host paths to offer, already de-duplicated and ordered (most
  /// recently used first).
  final List<String> projects;

  /// The project in effect, checked in the list. Null = no project.
  final String? current;

  /// A project was chosen from the list.
  final ValueChanged<String> onPick;

  /// "New project" — hand off to the host folder browser.
  final VoidCallback onBrowse;

  /// "Work outside a project" — clear the working directory.
  final VoidCallback onClear;

  /// Renders the trigger. Call `controller.open()` / `close()` from it.
  final Widget Function(BuildContext context, MenuController controller)
  builder;

  @override
  State<ProjectMenu> createState() => _ProjectMenuState();
}

class _ProjectMenuState extends State<ProjectMenu> {
  final MenuController _menu = MenuController();
  final TextEditingController _query = TextEditingController();
  final FocusNode _queryFocus = FocusNode();

  @override
  void dispose() {
    _query.dispose();
    _queryFocus.dispose();
    super.dispose();
  }

  /// Leaf folder name of an absolute path — what the user actually calls the
  /// project.
  static String leafOf(String path) {
    final segs = path.split(RegExp(r'[\\/]'))..removeWhere((s) => s.isEmpty);
    return segs.isEmpty ? path : segs.last;
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final q = _query.text.trim().toLowerCase();
    // Match on the folder name AND the full path — a user who remembers only
    // "rust_pro" should find every project under it.
    final matches = q.isEmpty
        ? widget.projects
        : widget.projects
              .where((p) => p.toLowerCase().contains(q))
              .toList(growable: false);

    Widget action(Key key, IconData icon, String label, VoidCallback onTap) =>
        InkWell(
          mouseCursor: clickable,
          key: key,
          onTap: () {
            _menu.close();
            onTap();
          },
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 9),
            child: Row(
              children: [
                Icon(icon, size: 16, color: scheme.onSurfaceVariant),
                const SizedBox(width: 10),
                Text(label, style: const TextStyle(fontSize: 13)),
              ],
            ),
          ),
        );

    return MenuAnchor(
      controller: _menu,
      alignmentOffset: const Offset(0, 6),
      // The search box is the point of the menu, so put the caret in it as it
      // opens; reset the filter so a reopen never shows a stale result set.
      onOpen: () {
        _query.clear();
        WidgetsBinding.instance.addPostFrameCallback(
          (_) => _queryFocus.requestFocus(),
        );
      },
      style: MenuStyle(
        padding: WidgetStateProperty.all(
          const EdgeInsets.symmetric(vertical: 6),
        ),
      ),
      menuChildren: [
        SizedBox(
          width: 300,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(10, 2, 10, 8),
                child: TextField(
                  key: const Key('project-menu-search'),
                  controller: _query,
                  focusNode: _queryFocus,
                  onChanged: (_) => setState(() {}),
                  style: const TextStyle(fontSize: 13),
                  decoration: InputDecoration(
                    isDense: true,
                    prefixIcon: const Icon(Icons.search, size: 16),
                    prefixIconConstraints: const BoxConstraints(
                      minWidth: 30,
                      minHeight: 30,
                    ),
                    hintText: l10n.searchProjects,
                    hintStyle: TextStyle(
                      fontSize: 13,
                      color: scheme.onSurfaceVariant,
                    ),
                    filled: true,
                    fillColor: scheme.surfaceContainerHighest,
                    contentPadding: const EdgeInsets.symmetric(vertical: 8),
                    border: OutlineInputBorder(
                      borderRadius: BorderRadius.circular(8),
                      borderSide: BorderSide.none,
                    ),
                  ),
                ),
              ),
              // Bounded so a machine with thirty projects scrolls instead of
              // growing a menu taller than the window. Deliberately NOT a lazy
              // ListView: a menu panel measures its intrinsic width, which a
              // shrink-wrapped viewport refuses to report. The list is short.
              if (matches.isNotEmpty)
                ConstrainedBox(
                  constraints: const BoxConstraints(maxHeight: 220),
                  // `primary: false`: the menu panel already owns the primary
                  // scroll controller, and a second attach breaks its scrollbar.
                  child: SingleChildScrollView(
                    primary: false,
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        for (final p in matches)
                          InkWell(
                            mouseCursor: clickable,
                            key: Key('project-menu-item-$p'),
                            onTap: () {
                              _menu.close();
                              widget.onPick(p);
                            },
                            child: Padding(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 12,
                                vertical: 9,
                              ),
                              child: Row(
                                children: [
                                  Icon(
                                    Icons.folder_outlined,
                                    size: 16,
                                    color: scheme.onSurfaceVariant,
                                  ),
                                  const SizedBox(width: 10),
                                  Expanded(
                                    child: Tooltip(
                                      message: p,
                                      child: Text(
                                        leafOf(p),
                                        maxLines: 1,
                                        overflow: TextOverflow.ellipsis,
                                        style: const TextStyle(fontSize: 13),
                                      ),
                                    ),
                                  ),
                                  if (p == widget.current)
                                    Icon(
                                      Icons.check,
                                      size: 16,
                                      color: scheme.primary,
                                    ),
                                ],
                              ),
                            ),
                          ),
                      ],
                    ),
                  ),
                )
              else
                Padding(
                  padding: const EdgeInsets.fromLTRB(12, 4, 12, 10),
                  child: Text(
                    l10n.noMatchingProjects,
                    style: TextStyle(fontSize: 12.5, color: scheme.outline),
                  ),
                ),
              const Divider(height: 9),
              action(
                const Key('project-menu-new'),
                Icons.add,
                l10n.newProject,
                widget.onBrowse,
              ),
              action(
                const Key('project-menu-none'),
                Icons.close,
                l10n.workOutsideProject,
                widget.onClear,
              ),
            ],
          ),
        ),
      ],
      builder: (context, controller, _) => widget.builder(context, controller),
    );
  }
}
