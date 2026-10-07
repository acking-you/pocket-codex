import 'package:flutter/material.dart';

/// A navigation control that stays inside its transcript viewport when moved,
/// resized, rotated, or constrained by the keyboard. Drag state is isolated so
/// moving the control does not rebuild the conversation.
class DraggableNavigation extends StatefulWidget {
  const DraggableNavigation({
    super.key,
    required this.child,
    required this.label,
  });

  final Widget child;
  final String label;

  @override
  State<DraggableNavigation> createState() => _DraggableNavigationState();
}

class _DraggableNavigationState extends State<DraggableNavigation> {
  // The drag never calls setState: the delegate listens to this notifier, so
  // a pointer move re-lays out one box instead of rebuilding the child (the
  // whole navigation cluster) every frame. That rebuild is what made the
  // control trail the finger.
  final _offset = ValueNotifier<Offset?>(null);
  late final _NavigationPosition _position = _NavigationPosition(_offset);

  @override
  void dispose() {
    _offset.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return CustomSingleChildLayout(
      delegate: _position,
      child: Semantics(
        label: widget.label,
        child: GestureDetector(
          key: const Key('draggable-turn-navigation'),
          behavior: HitTestBehavior.opaque,
          // Start from where the control actually is (it may sit at its
          // default corner with no offset yet), then follow the pointer 1:1.
          onPanStart: (_) => _offset.value = _position.actual,
          onPanUpdate: (details) =>
              _offset.value = _position.clamp(
                (_offset.value ?? _position.actual) + details.delta,
              ),
          // Its own layer: moving it then composites one small picture
          // instead of repainting the transcript underneath every frame.
          child: RepaintBoundary(child: widget.child),
        ),
      ),
    );
  }
}

class _NavigationPosition extends SingleChildLayoutDelegate {
  _NavigationPosition(this.offset) : super(relayout: offset);
  final ValueNotifier<Offset?> offset;

  /// Where the child was last placed, and the box it was clamped to.
  Offset actual = Offset.zero;
  Rect _bounds = Rect.zero;
  static const margin = 12.0;

  /// [value] kept inside the area the child may occupy, so a drag past the
  /// edge stops at it instead of accumulating an offset the user must undo.
  Offset clamp(Offset value) => _bounds.isEmpty
      ? value
      : Offset(
          value.dx.clamp(_bounds.left, _bounds.right),
          value.dy.clamp(_bounds.top, _bounds.bottom),
        );

  @override
  BoxConstraints getConstraintsForChild(BoxConstraints constraints) =>
      constraints.loosen().deflate(const EdgeInsets.all(margin));

  @override
  Offset getPositionForChild(Size size, Size childSize) {
    final right = (size.width - childSize.width - margin).clamp(
      margin,
      double.infinity,
    );
    final bottom = (size.height - childSize.height - margin).clamp(
      margin,
      double.infinity,
    );
    _bounds = Rect.fromLTRB(margin, margin, right, bottom);
    final value = offset.value;
    return actual = Offset(
      (value?.dx ?? right).clamp(margin, right),
      (value?.dy ?? bottom).clamp(margin, bottom),
    );
  }

  @override
  bool shouldRelayout(_NavigationPosition oldDelegate) =>
      !identical(offset, oldDelegate.offset);
}
