import 'dart:math' as math;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';

/// Browser-style vertical auto-scroll for a mouse-equipped transcript.
///
/// Click the middle button, then move above or below the anchor to scroll.
/// Another click, Escape, the wheel, or leaving the viewport stops scrolling.
/// Holding the middle button while moving also works and stops on release.
class MiddleClickScroll extends StatefulWidget {
  /// Wraps the viewport driven by [controller].
  const MiddleClickScroll({
    super.key,
    required this.controller,
    required this.child,
  });

  /// The vertical transcript's scroll controller.
  final ScrollController controller;

  /// The transcript viewport and its overlays.
  final Widget child;

  @override
  State<MiddleClickScroll> createState() => _MiddleClickScrollState();
}

class _MiddleClickScrollState extends State<MiddleClickScroll>
    with SingleTickerProviderStateMixin, WidgetsBindingObserver {
  static const _deadZone = 12.0;
  late final Ticker _ticker;
  Offset? _anchor;
  Offset _pointer = Offset.zero;
  Duration? _elapsed;
  bool _held = false;
  bool _dragged = false;

  @override
  void initState() {
    super.initState();
    _ticker = createTicker(_tick);
    WidgetsBinding.instance.addObserver(this);
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    if (_anchor != null) HardwareKeyboard.instance.removeHandler(_onKey);
    _ticker.dispose();
    super.dispose();
  }

  @override
  void didUpdateWidget(MiddleClickScroll oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller) _stop();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state != AppLifecycleState.resumed) _stop();
  }

  void _down(PointerDownEvent event) {
    if (_anchor != null) {
      _stop();
      return;
    }
    if (event.kind != PointerDeviceKind.mouse ||
        event.buttons != kMiddleMouseButton ||
        widget.controller.positions.length != 1) {
      return;
    }
    final position = widget.controller.position;
    if (!position.hasContentDimensions ||
        position.maxScrollExtent <= position.minScrollExtent) {
      return;
    }
    setState(() {
      _anchor = _pointer = event.localPosition;
      _held = true;
      _dragged = false;
      _elapsed = null;
    });
    HardwareKeyboard.instance.addHandler(_onKey);
    _ticker.start();
  }

  void _move(PointerEvent event) {
    final anchor = _anchor;
    if (anchor == null) return;
    _pointer = event.localPosition;
    if (_held && (_pointer - anchor).distance > _deadZone) _dragged = true;
  }

  void _up(PointerUpEvent event) {
    if (_held && _dragged) _stop();
    _held = false;
  }

  void _signal(PointerSignalEvent event) {
    if (_anchor == null) return;
    _stop();
    // The active overlay catches this first wheel event; pass it through the
    // normal resolver so switching back to wheel scrolling loses no movement.
    if (event is PointerScrollEvent) {
      GestureBinding.instance.pointerSignalResolver.register(event, (_) {
        if (widget.controller.positions.length == 1) {
          widget.controller.position.pointerScroll(event.scrollDelta.dy);
        }
      });
    }
  }

  bool _onKey(KeyEvent event) {
    if (event is! KeyDownEvent ||
        event.logicalKey != LogicalKeyboardKey.escape) {
      return false;
    }
    _stop();
    return true;
  }

  void _stop() {
    if (_anchor == null) return;
    _ticker.stop();
    HardwareKeyboard.instance.removeHandler(_onKey);
    setState(() => _anchor = null);
    _held = false;
  }

  void _tick(Duration elapsed) {
    final previous = _elapsed;
    _elapsed = elapsed;
    if (previous == null || _anchor == null) return;
    if (widget.controller.positions.length != 1) {
      _stop();
      return;
    }
    final offset = _pointer.dy - _anchor!.dy;
    final distance = math.max(0.0, offset.abs() - _deadZone);
    if (distance == 0) return;
    final speed = math.min(2400.0, distance * 8 + distance * distance / 32);
    // A delayed frame must not turn into a large jump after a window resumes.
    final seconds = ((elapsed - previous).inMicroseconds / 1000000).clamp(
      0.0,
      0.05,
    );
    widget.controller.position.pointerScroll(offset.sign * speed * seconds);
  }

  @override
  Widget build(BuildContext context) {
    final anchor = _anchor;
    final scheme = Theme.of(context).colorScheme;
    return Listener(
      behavior: HitTestBehavior.translucent,
      onPointerDown: _down,
      onPointerMove: _move,
      onPointerUp: _up,
      onPointerCancel: (_) => _stop(),
      onPointerSignal: _signal,
      child: MouseRegion(
        onHover: _move,
        onExit: (_) => _stop(),
        child: Stack(
          fit: StackFit.passthrough,
          children: [
            widget.child,
            if (anchor != null) ...[
              const Positioned.fill(
                child: MouseRegion(
                  cursor: SystemMouseCursors.allScroll,
                  child: ColoredBox(color: Colors.transparent),
                ),
              ),
              Positioned(
                left: anchor.dx - 14,
                top: anchor.dy - 14,
                child: IgnorePointer(
                  child: ExcludeSemantics(
                    child: Container(
                      key: const Key('middle-click-scroll-anchor'),
                      width: 28,
                      height: 28,
                      decoration: BoxDecoration(
                        shape: BoxShape.circle,
                        color: scheme.surface,
                        border: Border.all(color: scheme.outline),
                      ),
                      child: Icon(
                        Icons.unfold_more,
                        size: 20,
                        color: scheme.onSurface,
                      ),
                    ),
                  ),
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }
}
