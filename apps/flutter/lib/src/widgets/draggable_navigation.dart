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
  Offset? _offset;
  _NavigationPosition _position = _NavigationPosition(null);

  @override
  Widget build(BuildContext context) {
    _position = _NavigationPosition(_offset)..actual = _position.actual;
    return CustomSingleChildLayout(
      delegate: _position,
      child: Semantics(
        label: widget.label,
        child: GestureDetector(
          key: const Key('draggable-turn-navigation'),
          behavior: HitTestBehavior.opaque,
          onPanUpdate: (details) => setState(() {
            _offset = _position.actual + details.delta;
          }),
          child: widget.child,
        ),
      ),
    );
  }
}

class _NavigationPosition extends SingleChildLayoutDelegate {
  _NavigationPosition(this.offset);
  final Offset? offset;
  Offset actual = Offset.zero;
  static const margin = 12.0;

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
    return actual = Offset(
      (offset?.dx ?? right).clamp(margin, right),
      (offset?.dy ?? bottom).clamp(margin, bottom),
    );
  }

  @override
  bool shouldRelayout(_NavigationPosition oldDelegate) =>
      offset != oldDelegate.offset;
}
