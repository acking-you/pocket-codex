import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/semantics.dart';
import 'package:flutter/services.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/motion.dart';
import 'package:pocket_codex/src/widgets/app_toast.dart';

/// A quiet project heading with pointer, keyboard and touch actions.
class ProjectSectionHeader extends StatefulWidget {
  /// Creates a collapsible heading whose new-conversation action is contextual.
  const ProjectSectionHeader({
    super.key,
    required this.path,
    required this.name,
    required this.collapsed,
    required this.count,
    required this.onToggle,
    required this.onNewConversation,
  });

  /// Full host-side working directory, also used to disambiguate names.
  final String path;

  /// Short project label.
  final String name;

  /// Whether the project's conversations are hidden.
  final bool collapsed;

  /// Total conversations, displayed when collapsed.
  final int count;

  /// Expand or collapse this project.
  final VoidCallback onToggle;

  /// Open this project's unsent conversation draft.
  final VoidCallback onNewConversation;

  @override
  State<ProjectSectionHeader> createState() => _ProjectSectionHeaderState();
}

enum _ProjectAction { create, copyPath }

class _ProjectSectionHeaderState extends State<ProjectSectionHeader> {
  bool _hovered = false;
  bool _focused = false;
  bool _menuOpen = false;

  @override
  void initState() {
    super.initState();
    FocusManager.instance.addHighlightModeListener(_highlightChanged);
  }

  void _highlightChanged(FocusHighlightMode mode) => setState(() {});

  @override
  void dispose() {
    FocusManager.instance.removeHighlightModeListener(_highlightChanged);
    super.dispose();
  }

  Future<void> _perform(_ProjectAction? action) async {
    // A live sidebar refresh may remove this heading while its menu is open.
    if (action == null) return;
    switch (action) {
      case _ProjectAction.create:
        widget.onNewConversation();
      case _ProjectAction.copyPath:
        await Clipboard.setData(ClipboardData(text: widget.path));
        if (mounted) showToastOk(context, AppLocalizations.of(context).copied);
    }
  }

  Future<void> _showActions({Offset? position}) async {
    if (_menuOpen) return;
    _menuOpen = true;
    final l10n = AppLocalizations.of(context);
    final label = l10n.newConversationInProject(widget.name);
    _ProjectAction? result;
    try {
      if (position != null) {
        final overlay =
            Overlay.of(context).context.findRenderObject()! as RenderBox;
        final local = overlay.globalToLocal(position);
        result = await showMenu<_ProjectAction>(
          context: context,
          position: RelativeRect.fromSize(
            Rect.fromLTWH(local.dx, local.dy, 0, 0),
            overlay.size,
          ),
          items: [
            PopupMenuItem(value: _ProjectAction.create, child: Text(label)),
            if (widget.path.isNotEmpty)
              PopupMenuItem(
                value: _ProjectAction.copyPath,
                child: Text(l10n.copyProjectPath),
              ),
          ],
        );
      } else {
        result = await showModalBottomSheet<_ProjectAction>(
          context: context,
          showDragHandle: true,
          isScrollControlled: true,
          constraints: BoxConstraints(
            maxHeight: MediaQuery.sizeOf(context).height * 0.8,
            maxWidth: 480,
          ),
          builder: (ctx) => SafeArea(
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Padding(
                    padding: const EdgeInsets.fromLTRB(20, 0, 20, 12),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          widget.name,
                          style: Theme.of(ctx).textTheme.titleMedium,
                        ),
                        if (widget.path.isNotEmpty) ...[
                          const SizedBox(height: 4),
                          Text(
                            widget.path,
                            style: Theme.of(ctx).textTheme.bodySmall,
                          ),
                        ],
                      ],
                    ),
                  ),
                  ListTile(
                    leading: const Icon(Icons.add),
                    title: Text(label),
                    onTap: () => Navigator.pop(ctx, _ProjectAction.create),
                  ),
                  if (widget.path.isNotEmpty)
                    ListTile(
                      leading: const Icon(Icons.copy_outlined),
                      title: Text(l10n.copyProjectPath),
                      onTap: () => Navigator.pop(ctx, _ProjectAction.copyPath),
                    ),
                ],
              ),
            ),
          ),
        );
      }
      await _perform(result);
    } finally {
      _menuOpen = false;
    }
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final l10n = AppLocalizations.of(context);
    final visible =
        _hovered ||
        (_focused &&
            FocusManager.instance.highlightMode ==
                FocusHighlightMode.traditional);
    final createLabel = l10n.newConversationInProject(widget.name);
    // Desktop headings are a 30 px source-list group; touch keeps 48 px.
    final dense = isDesktop;
    return Padding(
      padding: EdgeInsets.only(top: dense ? 8 : 6, bottom: dense ? 1 : 2),
      child: Focus(
        skipTraversal: true,
        onFocusChange: (focused) => setState(() => _focused = focused),
        onKeyEvent: (node, event) {
          if (event is KeyDownEvent &&
              (event.logicalKey == LogicalKeyboardKey.contextMenu ||
                  (event.logicalKey == LogicalKeyboardKey.f10 &&
                      HardwareKeyboard.instance.isShiftPressed))) {
            final box = context.findRenderObject()! as RenderBox;
            _showActions(
              position: box.localToGlobal(Offset(0, box.size.height)),
            );
            return KeyEventResult.handled;
          }
          return KeyEventResult.ignored;
        },
        child: MouseRegion(
          onEnter: (event) {
            if (event.kind == PointerDeviceKind.mouse) {
              setState(() => _hovered = true);
            }
          },
          onExit: (_) => setState(() => _hovered = false),
          child: Semantics(
            customSemanticsActions: {
              CustomSemanticsAction(label: createLabel):
                  widget.onNewConversation,
            },
            child: Material(
              color: Colors.transparent,
              child: Row(
                children: [
                  Expanded(
                    child: Tooltip(
                      message: widget.path,
                      triggerMode: TooltipTriggerMode.manual,
                      child: InkWell(
                        key: Key('project-header-${widget.path}'),
                        mouseCursor: clickable,
                        borderRadius: BorderRadius.circular(kRowRadius),
                        onTap: widget.onToggle,
                        onLongPress: () => _showActions(),
                        onSecondaryTapUp: (event) =>
                            _showActions(position: event.globalPosition),
                        child: ConstrainedBox(
                          constraints: BoxConstraints(
                            minHeight: dense ? 30 : 48,
                          ),
                          child: Padding(
                            padding: const EdgeInsets.only(left: 6, right: 4),
                            child: Row(
                              children: [
                                // The folder is the heading's identity; the
                                // chevron after the name says it folds. Rows
                                // below indent to the name, under the folder.
                                Icon(
                                  widget.collapsed
                                      ? Icons.folder_outlined
                                      : Icons.folder_open_outlined,
                                  size: 15,
                                  color: scheme.onSurfaceVariant,
                                ),
                                const SizedBox(width: 7),
                                Flexible(
                                  child: Text(
                                    widget.name,
                                    maxLines: 1,
                                    overflow: TextOverflow.ellipsis,
                                    style: TextStyle(
                                      fontSize: dense ? 12.5 : 14,
                                      fontWeight: FontWeight.w600,
                                      color: scheme.onSurfaceVariant,
                                    ),
                                  ),
                                ),
                                const SizedBox(width: 2),
                                // One glyph that turns, so the fold reads as
                                // a motion; with motion off it still lands on
                                // the right direction.
                                AnimatedRotation(
                                  turns: widget.collapsed ? 0 : 0.25,
                                  duration: Motion.of(context, Motion.fast),
                                  curve: Motion.move,
                                  child: Icon(
                                    Icons.keyboard_arrow_right,
                                    size: 15,
                                    color: scheme.onSurfaceVariant,
                                  ),
                                ),
                                const Spacer(),
                                if (widget.collapsed)
                                  Text(
                                    '${widget.count}',
                                    style: TextStyle(
                                      fontSize: 11.5,
                                      color: scheme.onSurfaceVariant,
                                      fontFeatures: const [
                                        FontFeature.tabularFigures(),
                                      ],
                                    ),
                                  ),
                              ],
                            ),
                          ),
                        ),
                      ),
                    ),
                  ),
                  // Keep the action's space and keyboard target when hidden.
                  ExcludeSemantics(
                    excluding: !visible,
                    child: IgnorePointer(
                      ignoring: !visible,
                      child: AnimatedOpacity(
                        opacity: visible ? 1 : 0,
                        duration: Motion.of(context, Motion.fast),
                        child: IconButton(
                          key: Key('project-new-${widget.path}'),
                          mouseCursor: clickable,
                          tooltip: createLabel,
                          icon: const Icon(Icons.add, size: 16),
                          color: scheme.onSurfaceVariant,
                          style: dense
                              ? IconButton.styleFrom(
                                  minimumSize: const Size(26, 26),
                                  fixedSize: const Size(26, 26),
                                  padding: EdgeInsets.zero,
                                  shape: RoundedRectangleBorder(
                                    borderRadius: BorderRadius.circular(
                                      kRowRadius,
                                    ),
                                  ),
                                )
                              : null,
                          onPressed: widget.onNewConversation,
                        ),
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
