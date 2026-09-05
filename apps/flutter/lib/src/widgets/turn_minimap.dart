import 'dart:math' as math;

import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/theme.dart';

/// One turn on the minimap: where to jump, and what to show while hovering it.
@immutable
class TurnMinimapItem {
  /// Creates a minimap entry.
  const TurnMinimapItem({
    required this.rowIndex,
    required this.userText,
    this.assistantText,
    this.turnId = '',
  });

  /// Index of this turn's user message in the transcript's row list — what the
  /// list controller is asked to scroll to. `-1` for a turn the transcript
  /// hasn't loaded: the tick still marks where the turn sits in the
  /// conversation, but there is no row to scroll to until its items arrive.
  final int rowIndex;

  /// Id of the turn, so selecting a tick whose [rowIndex] is `-1` can fetch it.
  /// Empty when the caller derived entries from rows alone.
  final String turnId;

  /// The user's own message, one line, whitespace already collapsed.
  final String userText;

  /// The turn's final reply, for the preview's second block. Null when the turn
  /// produced no prose (tool calls only, or still running).
  final String? assistantText;
}

/// Below this many turns the rail says nothing a scrollbar doesn't, and the
/// corner arrows are the better affordance.
///
/// The rail earns its place by showing a conversation's *shape* — how many turns
/// there are and where you sit among them. At two or three ticks there is no
/// shape to show, and what is left is a hover-only target that steps one turn at
/// a time, which is exactly what the arrows already do with a visible label and a
/// touch-sized target. So below this, the arrows come back.
const int kTurnMinimapMinItems = 4;

/// Nominal spacing between ticks. The rail's natural height is this times the
/// gaps, then capped to the space available — past that the ticks compress and
/// the rail stops growing.
const double _kTickSpacing = 8;

/// Left inset of the rail within the gutter. Exported because the transcript
/// needs it to tell whether the gutter can hold the rail at all — with less than
/// this there is nowhere for it to sit and the corner arrows keep the job.
const double kTurnMinimapRailInset = 12;

/// Widest the resting hit strip may be. Capped against the real gutter too, so
/// the strip can never reach over the centred column and swallow a selection.
const double _kHitStripMaxWidth = 40;

/// How wide the pointer region grows while a preview is open, so travelling
/// toward the card doesn't leave the strip and dismiss it.
const double _kExpandedHitWidth = 352;

/// Gutter at or above which the rail simply stays visible. Narrower than this
/// there isn't room for it to rest without crowding the text, so it fades in on
/// approach instead.
const double kTurnMinimapPersistentGutter = 48;

/// Preferred width of the hover preview card, the narrowest it may shrink to
/// before it stops being worth showing, and how far it may reach past the gutter
/// over the conversation.
///
/// It has to overhang somewhat — a readable card does not fit in a 60 px margin —
/// but a card that buries the text you are scanning defeats its own purpose, so
/// the overhang is bounded and the card narrows within that budget.
const double _kPreviewWidth = 300;
const double _kMinPreviewWidth = 150;
const double _kMaxPreviewOverhang = 160;

/// Extra width a tick gets for being on screen, so the rail's shape shows where
/// you are and reshapes as you scroll — the thumb of a scrollbar, drawn as a
/// bulge in the scale.
const double _kInViewBulge = 6;

/// Tick widths by distance from the hovered one: the pointed-at tick, its
/// neighbours, then the rest. A falloff rather than a single highlight, so the
/// rail reads as one object responding to the cursor instead of a row of
/// independent marks.
///
/// [inView] adds [_kInViewBulge] on top. The two cues compose deliberately: the
/// pointer's falloff is much larger, so a hovered tick still stands out from the
/// on-screen band it may sit inside.
double _tickWidth(int? distance, {bool inView = false}) {
  final base = switch (distance) {
    0 => 22.0,
    1 => 15.0,
    2 => 10.0,
    _ => 7.0,
  };
  return inView ? base + _kInViewBulge : base;
}

/// A left-gutter rail of one tick per conversation turn: hover a tick to preview
/// that turn, click to jump to it.
///
/// This replaces a pair of prev/next arrows in the bottom-right corner. Those
/// could only step, one turn at a time, with no indication of how many turns
/// there were, where in them you currently sat, or what you would land on — so
/// finding a particular exchange in a long conversation meant clicking blind and
/// reading after each jump. The rail shows the whole conversation's shape at
/// once and makes any turn one click away.
///
/// Pointer-only and desktop-only by construction: it is a hover affordance, and
/// a 2 px tick is not a touch target. Compact layouts keep the arrows.
class TurnMinimap extends StatefulWidget {
  /// Creates the rail.
  const TurnMinimap({
    super.key,
    required this.items,
    required this.visibleRange,
    required this.gutterWidth,
    required this.onSelect,
    this.onPreview,
  });

  /// The turns, in transcript order.
  final List<TurnMinimapItem> items;

  /// The transcript's currently visible row range, as `(first, last)`. Listened
  /// to rather than read so scrolling repaints only the ticks — the transcript
  /// itself must not rebuild on every scroll frame.
  final ValueListenable<(int, int)?> visibleRange;

  /// Space between the window edge and the centred conversation column. The rail
  /// lives here; when there is none it goes inert.
  final double gutterWidth;

  /// Jump to this turn.
  final ValueChanged<TurnMinimapItem> onSelect;

  /// Called when the pointer rests on a turn, before any click. Lets the
  /// transcript start fetching a turn it hasn't loaded so the jump is instant
  /// when the click comes. Optional: the preview itself needs no fetch, since
  /// the entry already carries its text.
  final ValueChanged<TurnMinimapItem>? onPreview;

  @override
  State<TurnMinimap> createState() => _TurnMinimapState();
}

class _TurnMinimapState extends State<TurnMinimap> {
  /// The tick the pointer (or the keyboard) is on, or null when neither is.
  /// Drives the width falloff and the preview.
  int? _active;

  /// Whether the pointer is anywhere near the rail. Only used to fade the rail
  /// in on a window too narrow to keep it resting.
  bool _hovering = false;

  final _focus = FocusNode(debugLabel: 'turn-minimap');
  final _previewKey = GlobalKey();

  @override
  void dispose() {
    _focus.dispose();
    super.dispose();
  }

  @override
  void didUpdateWidget(TurnMinimap old) {
    super.didUpdateWidget(old);
    // A turn was removed (a rewind, a reload) — an index past the end would
    // otherwise resolve to nothing and leave a stuck preview.
    _active = _clampIndex(_active);
  }

  /// [index] pulled back inside the current turn list, or null when there are no
  /// turns left to point at.
  int? _clampIndex(int? index) {
    if (index == null || index < widget.items.length) return index;
    return widget.items.isEmpty ? null : widget.items.length - 1;
  }

  /// Where the rail sits: [kTurnMinimapRailInset] from the window's left edge,
  /// whatever the gutter is doing.
  ///
  /// It used to be placed relative to the conversation column instead, hugging
  /// its left edge — which tracked the column outward as the window widened and
  /// left the rail floating in the middle of empty margin. This is an index of
  /// the whole conversation, so it belongs at the frame where a scrollbar would
  /// be, not attached to the prose.
  double get _railLeft => kTurnMinimapRailInset;

  /// Rest width of the pointer region: the gutter minus the rail's inset, capped.
  /// Zero (inert) when the gutter can't hold it.
  ///
  /// The cap is what keeps the strip off the text: the column is centred, so a
  /// strip that grew with the gutter would eventually reach under the prose and
  /// swallow clicks meant for it.
  double get _hitWidth => math.max(
    0,
    math.min(
      _kHitStripMaxWidth,
      widget.gutterWidth.floorToDouble() - _railLeft,
    ),
  );

  bool get _persistent => widget.gutterWidth >= kTurnMinimapPersistentGutter;

  /// Where tick [index] sits, as a fraction of the rail's height.
  double _fractionOf(int index) {
    final count = widget.items.length;
    if (count <= 1) return 0;
    return index.clamp(0, count - 1) / (count - 1);
  }

  /// The tick nearest [localY] on a rail of [railHeight].
  int? _indexAt(double localY, double railHeight) {
    final count = widget.items.length;
    if (count <= 0 || railHeight <= 0) return null;
    if (count == 1) return 0;
    final progress = (localY / railHeight).clamp(0.0, 1.0);
    return (progress * (count - 1)).round().clamp(0, count - 1);
  }

  void _move(int delta) {
    setState(() {
      final base = _active ?? 0;
      _active = (base + delta).clamp(0, widget.items.length - 1);
    });
  }

  void _select(int index) {
    final item = widget.items.elementAtOrNull(index);
    if (item == null) return;
    widget.onSelect(item);
    // Drop focus after a jump so the preview doesn't hang over the place the
    // user just navigated to. The visible range keeps marking their position.
    _focus.unfocus();
    setState(() {
      _active = null;
    });
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent && event is! KeyRepeatEvent) {
      return KeyEventResult.ignored;
    }
    switch (event.logicalKey) {
      case LogicalKeyboardKey.arrowDown:
        _move(1);
      case LogicalKeyboardKey.arrowUp:
        _move(-1);
      case LogicalKeyboardKey.home:
        setState(() => _active = 0);
      case LogicalKeyboardKey.end:
        setState(() => _active = widget.items.length - 1);
      case LogicalKeyboardKey.enter:
      case LogicalKeyboardKey.space:
        final active = _active;
        if (active != null) _select(active);
      default:
        return KeyEventResult.ignored;
    }
    return KeyEventResult.handled;
  }

  @override
  Widget build(BuildContext context) {
    if (widget.items.length < kTurnMinimapMinItems) {
      return const SizedBox.shrink();
    }
    final hitWidth = _hitWidth;
    // No gutter to live in: rather than reach over the text, the rail stands
    // down entirely and the transcript keeps its own scrollbar.
    if (hitWidth <= 0) return const SizedBox.shrink();

    return LayoutBuilder(
      builder: (context, constraints) {
        // The rail is centred vertically and grows with the turn count until it
        // runs out of room, after which the ticks pack tighter instead.
        final available = math.max(0.0, constraints.maxHeight - 96);
        final natural = math.max(
          1.0,
          (widget.items.length - 1) * _kTickSpacing,
        );
        final railHeight = math.min(natural, available);
        final open = _active != null;
        return Align(
          alignment: Alignment.centerLeft,
          child: Padding(
            // At the frame, where a scrollbar sits — see `_railLeft`.
            padding: EdgeInsets.only(left: _railLeft),
            child: AnimatedOpacity(
              // A wide window has room to show the rail at rest; a narrow one
              // would have it crowding the prose, so there it waits for the
              // pointer to come looking.
              opacity: _persistent || _hovering || open ? 1 : 0,
              duration: const Duration(milliseconds: 150),
              child: SizedBox(
                key: const Key('turn-minimap-rail'),
                height: railHeight,
                // Only the resting strip takes the pointer; the preview extends
                // past it and is allowed to overhang.
                width: open ? _kExpandedHitWidth : hitWidth,
                child: _rail(railHeight, hitWidth),
              ),
            ),
          ),
        );
      },
    );
  }

  Widget _rail(double railHeight, double hitWidth) {
    final scheme = Theme.of(context).colorScheme;
    return Focus(
      focusNode: _focus,
      onKeyEvent: _onKey,
      onFocusChange: (has) =>
          setState(() => _active = has ? (_active ?? 0) : null),
      child: MouseRegion(
        cursor: clickable,
        onEnter: (_) => setState(() => _hovering = true),
        onExit: (_) => _leave(),
        onHover: (event) {
          if (event.localPosition.dx > hitWidth) {
            final card = _previewKey.currentContext?.findRenderObject();
            // The expanded box also covers empty space beside other ticks.
            // Only the actual card should keep a preview open off the rail.
            if (card is RenderBox &&
                card.hasSize &&
                (Offset.zero & card.size).contains(
                  card.globalToLocal(event.position),
                )) {
              return;
            }
            _leave();
            return;
          }
          final next = _indexAt(event.localPosition.dy, railHeight);
          if (next != _active) {
            setState(() => _active = next);
            if (next != null && next < widget.items.length) {
              widget.onPreview?.call(widget.items[next]);
            }
          }
        },
        child: GestureDetector(
          behavior: HitTestBehavior.translucent,
          onTapDown: (details) {
            if (details.localPosition.dx > hitWidth) return;
            final index = _indexAt(details.localPosition.dy, railHeight);
            if (index != null) _select(index);
          },
          // No spine behind the ticks: it read as a stray vertical rule against
          // the page's left edge, and the ticks already line up into a scale on
          // their own.
          child: Stack(
            clipBehavior: Clip.none,
            children: [..._ticks(railHeight, scheme), ?_preview(railHeight)],
          ),
        ),
      ),
    );
  }

  void _leave() {
    if (!_hovering && _active == null) return;
    setState(() {
      _hovering = false;
      _active = null;
    });
  }

  /// The ticks. Each repaints on scroll through [TurnMinimap.visibleRange]
  /// alone, so following a streaming reply never rebuilds the transcript.
  List<Widget> _ticks(double railHeight, ColorScheme scheme) {
    final active = _active;
    return [
      for (var i = 0; i < widget.items.length; i++)
        Positioned(
          left: 0,
          top: railHeight * _fractionOf(i) - 1,
          child: ValueListenableBuilder<(int, int)?>(
            valueListenable: widget.visibleRange,
            builder: (context, range, _) {
              final row = widget.items[i].rowIndex;
              final inView =
                  range != null && row >= range.$1 && row <= range.$2;
              final distance = active == null ? null : (i - active).abs();
              return AnimatedContainer(
                duration: const Duration(milliseconds: 150),
                curve: Curves.easeOutCubic,
                height: 2,
                // Scrolling reshapes the rail, it does not only re-ink it. The
                // on-screen turns bulge outward, so the rail carries a visible
                // "you are here" band that travels as you scroll — which is the
                // job a scrollbar thumb does, and the reason this sits where a
                // scrollbar would. Ink alone was too quiet to read in passing.
                width: _tickWidth(distance, inView: inView),
                decoration: BoxDecoration(
                  // Width and ink both track position; the hovered tick is the
                  // widest thing on the rail, so the two cues stay legible
                  // together rather than competing.
                  color: inView
                      ? scheme.onSurface.withValues(alpha: 0.85)
                      : distance == 0
                      ? scheme.onSurfaceVariant
                      : scheme.onSurfaceVariant.withValues(alpha: 0.4),
                  borderRadius: BorderRadius.circular(1),
                ),
              );
            },
          ),
        ),
    ];
  }

  /// The hover card, anchored to its tick rather than to the cursor, so it holds
  /// still while the pointer travels into it.
  Widget? _preview(double railHeight) {
    final active = _active;
    if (active == null) return null;
    final item = widget.items.elementAtOrNull(active);
    if (item == null) return null;
    // A turn with neither a message nor a reply to show gets no card: an empty
    // one would only cover the conversation it is meant to help you find.
    if (item.userText.trim().isEmpty &&
        (item.assistantText?.trim().isEmpty ?? true)) {
      return null;
    }
    final fraction = _fractionOf(active);
    // The card would overflow the rail at the ends, so its anchor slides: the
    // first turn hangs below its tick, the last above it, the rest straddle.
    final align = active == 0
        ? 0.0
        : active == widget.items.length - 1
        ? -1.0
        : -0.5;
    // Clear of the widest tick, so the card never sits on the mark it describes.
    final left = _tickWidth(0) + 10;
    // The card is allowed to overhang the gutter — it has to be readable, and a
    // 300 px card cannot fit a 60 px margin — but not by so much that it buries
    // the conversation. Past this it narrows instead, and if it cannot stay
    // legible at all the tick simply doesn't preview.
    final room = widget.gutterWidth - _railLeft - left + _kMaxPreviewOverhang;
    final width = math.min(_kPreviewWidth, room);
    if (width < _kMinPreviewWidth) return null;
    return Positioned(
      left: left,
      top: railHeight * fraction,
      child: FractionalTranslation(
        translation: Offset(0, align),
        child: _TurnPreviewCard(key: _previewKey, item: item, width: width),
      ),
    );
  }
}

/// The floating preview: what the user asked, and how the turn answered.
class _TurnPreviewCard extends StatelessWidget {
  const _TurnPreviewCard({super.key, required this.item, required this.width});

  final TurnMinimapItem item;

  /// Resolved width — the preferred one, or less where the gutter is tight.
  final double width;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final reply = item.assistantText?.trim() ?? '';
    final question = item.userText.trim();
    return Container(
      key: const Key('turn-minimap-preview'),
      width: width,
      padding: const EdgeInsets.fromLTRB(12, 10, 12, 11),
      decoration: BoxDecoration(
        color: surfacePanel(scheme),
        borderRadius: BorderRadius.circular(kPanelRadius),
        // A hairline as well as a shadow: this floats over prose, where a
        // shadow alone leaves a light card on a light page with no edge.
        border: Border.all(color: scheme.outlineVariant),
        boxShadow: panelShadow(scheme, blur: 18),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          // An attachment-only turn has no words of its own to head the card, so
          // the reply stands alone rather than under an empty line.
          if (question.isNotEmpty)
            Text(
              question,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(
                fontWeight: FontWeight.w600,
                fontFamilyFallback: cjkFontFallback,
                color: scheme.onSurface,
              ),
            ),
          if (reply.isNotEmpty) ...[
            if (question.isNotEmpty) const SizedBox(height: 4),
            Text(
              reply,
              maxLines: 3,
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.bodySmall?.copyWith(
                color: scheme.onSurfaceVariant,
                height: 1.4,
                fontFamilyFallback: cjkFontFallback,
              ),
            ),
          ],
        ],
      ),
    );
  }
}
