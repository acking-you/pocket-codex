import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:pocket_codex/src/motion.dart';

/// A live waveform: a row of rounded bars driven by a sound [level] (0..1).
///
/// The level is one number per poll, so the bars carry history instead: each
/// new sample enters on the right and scrolls left, which reads as a voice
/// moving through time rather than a single bar pumping. A ticker scrolls
/// between samples so the motion is continuous at display rate.
///
/// With [active] false (or reduced motion) the bars rest as a flat line —
/// the microphone is open but nothing is being heard.
class Waveform extends StatefulWidget {
  /// Draws [level] as [bars] bars in [color].
  const Waveform({
    super.key,
    required this.level,
    required this.color,
    this.bars = 24,
    this.height = 22,
    this.barWidth = 2.5,
    this.gap = 2,
    this.active = true,
  });

  /// The sound level to show, 0..1.
  final ValueListenable<double> level;

  /// Bar colour.
  final Color color;

  /// How many bars of history to show.
  final int bars;

  /// Height of the tallest bar.
  final double height;

  /// Width of each bar.
  final double barWidth;

  /// Space between bars.
  final double gap;

  /// Whether sound is being taken in at all.
  final bool active;

  @override
  State<Waveform> createState() => _WaveformState();
}

class _WaveformState extends State<Waveform>
    with SingleTickerProviderStateMixin {
  /// One new sample per this long; the history scrolls by one bar per step.
  static const _step = Duration(milliseconds: 70);

  late List<double> _history = List.filled(widget.bars, 0);
  late final Ticker _ticker = createTicker(_tick);
  Duration _last = Duration.zero;
  double _phase = 0;

  @override
  void initState() {
    super.initState();
    widget.level.addListener(_onLevel);
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _sync();
  }

  @override
  void didUpdateWidget(Waveform old) {
    super.didUpdateWidget(old);
    if (old.level != widget.level) {
      old.level.removeListener(_onLevel);
      widget.level.addListener(_onLevel);
    }
    if (old.bars != widget.bars) {
      _history = List.filled(widget.bars, 0);
    }
    _sync();
  }

  void _sync() {
    final run = widget.active && Motion.ambientAllowed(context);
    if (run && !_ticker.isActive) {
      _last = Duration.zero;
      _ticker.start();
    } else if (!run && _ticker.isActive) {
      _ticker.stop();
      setState(() => _history = List.filled(widget.bars, 0));
    }
  }

  void _onLevel() {
    // Reduced motion: no scroll, but the newest level still shows, so a
    // talking user still sees that they are heard.
    if (!_ticker.isActive && widget.active) {
      setState(() {
        _history = [..._history.skip(1), widget.level.value];
      });
    }
  }

  void _tick(Duration elapsed) {
    final dt = elapsed - _last;
    _last = elapsed;
    _phase += dt.inMicroseconds / _step.inMicroseconds;
    if (_phase >= 1) {
      _phase -= _phase.floorToDouble();
      // A touch of jitter keeps a held vowel from drawing as a flat block.
      final v = widget.level.value;
      final jitter = v > 0.02
          ? (math.Random().nextDouble() - 0.5) * 0.18 * v
          : 0;
      _history = [..._history.skip(1), (v + jitter).clamp(0.0, 1.0)];
    }
    setState(() {});
  }

  @override
  void dispose() {
    widget.level.removeListener(_onLevel);
    _ticker.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final width = widget.bars * (widget.barWidth + widget.gap) - widget.gap;
    return RepaintBoundary(
      child: CustomPaint(
        size: Size(width, widget.height),
        painter: _WaveformPainter(
          history: _history,
          phase: _ticker.isActive ? _phase : 0,
          color: widget.color,
          barWidth: widget.barWidth,
          gap: widget.gap,
        ),
      ),
    );
  }
}

class _WaveformPainter extends CustomPainter {
  _WaveformPainter({
    required this.history,
    required this.phase,
    required this.color,
    required this.barWidth,
    required this.gap,
  });

  final List<double> history;
  final double phase;
  final Color color;
  final double barWidth;
  final double gap;

  @override
  void paint(Canvas canvas, Size size) {
    final pitch = barWidth + gap;
    final mid = size.height / 2;
    final rest = math.max(2.0, barWidth);
    final paint = Paint()..color = color;
    canvas.save();
    canvas.clipRect(Offset.zero & size);
    // Shift left by the fraction of a step that has elapsed, so the bars
    // glide rather than hop.
    final shift = -phase * pitch;
    for (var i = 0; i < history.length; i++) {
      final x = i * pitch + shift;
      // Older samples fade toward the left edge.
      final age = i / math.max(1, history.length - 1);
      final h = rest + (size.height - rest) * history[i];
      paint.color = color.withValues(alpha: 0.35 + 0.65 * age);
      canvas.drawRRect(
        RRect.fromRectAndRadius(
          Rect.fromCenter(
            center: Offset(x + barWidth / 2, mid),
            width: barWidth,
            height: h,
          ),
          Radius.circular(barWidth / 2),
        ),
        paint,
      );
    }
    canvas.restore();
  }

  @override
  bool shouldRepaint(_WaveformPainter old) => true;
}
