import 'package:flutter/material.dart';

/// A small solid status dot used to convey availability at a glance (e.g. a
/// service is online / a subscription is alive). [color] carries the meaning;
/// use [PulsingDot] instead for an active / running state.
class StatusDot extends StatelessWidget {
  /// Creates a status dot of [size] px in [color].
  const StatusDot({super.key, required this.color, this.size = 9});

  /// Dot colour (green = healthy, red = down, etc.).
  final Color color;

  /// Diameter in logical pixels.
  final double size;

  @override
  Widget build(BuildContext context) => Container(
    width: size,
    height: size,
    decoration: BoxDecoration(
      color: color,
      shape: BoxShape.circle,
      boxShadow: [
        BoxShadow(
          color: color.withValues(alpha: 0.45),
          blurRadius: 4,
          spreadRadius: 0.3,
        ),
      ],
    ),
  );
}

/// A continuously pulsing dot, used to signal an in-flight / running state so
/// active sessions are perceptible before they're opened. Several rendered at
/// once read as "many things happening".
class PulsingDot extends StatefulWidget {
  /// Creates a pulsing dot of [size] px in [color].
  const PulsingDot({super.key, required this.color, this.size = 9});

  /// Dot colour (defaults to the caller's choice; usually the "working" colour).
  final Color color;

  /// Diameter in logical pixels.
  final double size;

  @override
  State<PulsingDot> createState() => _PulsingDotState();
}

class _PulsingDotState extends State<PulsingDot>
    with SingleTickerProviderStateMixin {
  late final AnimationController _c = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 900),
  )..repeat(reverse: true);

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: _c,
    builder: (context, _) {
      final t = Curves.easeInOut.transform(_c.value);
      return Container(
        width: widget.size,
        height: widget.size,
        decoration: BoxDecoration(
          color: Color.lerp(
            widget.color.withValues(alpha: 0.4),
            widget.color,
            t,
          ),
          shape: BoxShape.circle,
          boxShadow: [
            BoxShadow(
              color: widget.color.withValues(alpha: 0.2 + 0.5 * t),
              blurRadius: 2 + 5 * t,
              spreadRadius: 0.4 + t,
            ),
          ],
        ),
      );
    },
  );
}

/// A dot + label pill for service availability rows. Set [pulsing] for an
/// active state (renders a [PulsingDot] instead of a [StatusDot]); set [filled]
/// to wrap the dot+label in a status-tinted rounded pill (used on the card-style
/// service rows, where a bare dot+text would float without weight).
class StatusChip extends StatelessWidget {
  /// Creates a status chip.
  const StatusChip({
    super.key,
    required this.color,
    required this.label,
    this.pulsing = false,
    this.filled = false,
  });

  /// Colour shared by the dot and (subtly) the label.
  final Color color;

  /// Short status text (localised by the caller).
  final String label;

  /// Whether to animate the dot (running / in-flight states).
  final bool pulsing;

  /// Whether to draw a status-tinted rounded background behind the dot+label.
  final bool filled;

  @override
  Widget build(BuildContext context) {
    final muted = Theme.of(context).colorScheme.onSurfaceVariant;
    final row = Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        pulsing
            ? PulsingDot(color: color, size: 8)
            : StatusDot(color: color, size: 8),
        const SizedBox(width: 5),
        Text(label, style: TextStyle(fontSize: 11.5, color: muted)),
      ],
    );
    if (!filled) return row;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 4),
      decoration: BoxDecoration(
        // A faint wash of the status colour: enough to read as a pill in both
        // themes, neutral enough that the label (onSurfaceVariant) stays legible.
        color: color.withValues(alpha: 0.13),
        borderRadius: BorderRadius.circular(20),
      ),
      child: row,
    );
  }
}
