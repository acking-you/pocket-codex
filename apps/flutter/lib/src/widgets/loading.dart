import 'dart:async';

import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';

/// Drives the sweep for the skeleton shapes ([SkeletonBox]) beneath it, so a
/// screen waiting on data reads as "content is coming" rather than as a frozen
/// grey block.
///
/// The sweep is painted BY each box, as a gradient fill, rather than masked
/// over the subtree. A `ShaderMask` looks like the obvious tool and isn't:
/// `BlendMode.srcATop` keeps the *destination's* alpha, so the shimmer's own
/// alphas never reach the screen — the highlight can only tint a shape whose
/// opacity is already fixed, which is why the previous sweep was imperceptible.
class Shimmer extends StatefulWidget {
  /// Wraps [child]; every [SkeletonBox] inside it shares this sweep.
  const Shimmer({super.key, required this.child});

  /// The skeleton content to animate.
  final Widget child;

  @override
  State<Shimmer> createState() => _ShimmerState();
}

class _ShimmerState extends State<Shimmer> with SingleTickerProviderStateMixin {
  late final AnimationController _c = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 1250),
  );

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    // Reduced-motion users keep the skeleton and lose the movement: the shape
    // is what says "loading", the sweep is only polish.
    if (MediaQuery.disableAnimationsOf(context)) {
      _c.stop();
      _c.value = 0.5;
    } else if (!_c.isAnimating) {
      _c.repeat();
    }
  }

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: _c,
    builder: (context, child) => _ShimmerPhase(phase: _c.value, child: child!),
    child: widget.child,
  );
}

/// Carries the current sweep position down to the skeleton shapes.
class _ShimmerPhase extends InheritedWidget {
  const _ShimmerPhase({required this.phase, required super.child});

  /// 0→1 across one sweep.
  final double phase;

  static double? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<_ShimmerPhase>()?.phase;

  @override
  bool updateShouldNotify(_ShimmerPhase old) => old.phase != phase;
}

/// A rounded placeholder shape. Inside a [Shimmer] it carries the animated
/// sweep; on its own it falls back to a flat tint, so a stray use is dull
/// rather than broken.
class SkeletonBox extends StatelessWidget {
  /// Creates a placeholder box.
  const SkeletonBox({super.key, this.width, this.height = 14, this.radius = 7});

  /// Width (null = fill available).
  final double? width;

  /// Height in logical pixels.
  final double height;

  /// Corner radius.
  final double radius;

  @override
  Widget build(BuildContext context) {
    final onSurface = Theme.of(context).colorScheme.onSurface;
    final base = onSurface.withValues(alpha: 0.10);
    final highlight = onSurface.withValues(alpha: 0.28);
    final phase = _ShimmerPhase.maybeOf(context);
    return Container(
      width: width,
      height: height,
      decoration: BoxDecoration(
        color: phase == null ? base : null,
        // A band wide enough to register as light travelling across the shape;
        // the sweep runs from off one edge to off the other, so each box is
        // plain at the extremes and brightest as the band crosses it.
        gradient: phase == null
            ? null
            : LinearGradient(
                begin: Alignment.centerLeft,
                end: Alignment.centerRight,
                colors: [base, highlight, base],
                stops: const [0.15, 0.5, 0.85],
                transform: _SlideGradient(Curves.easeInOut.transform(phase)),
              ),
        borderRadius: BorderRadius.circular(radius),
      ),
    );
  }
}

/// Slides a gradient horizontally from off-shape left to off-shape right as
/// [t] goes 0→1.
class _SlideGradient extends GradientTransform {
  const _SlideGradient(this.t);
  final double t;
  @override
  Matrix4? transform(Rect bounds, {TextDirection? textDirection}) =>
      Matrix4.translationValues((t * 2 - 1) * bounds.width, 0, 0);
}

/// A quiet initial-history state. Fast cache hits never flash a loading shape.
class ChatLoadingSkeleton extends StatefulWidget {
  const ChatLoadingSkeleton({super.key});

  @override
  State<ChatLoadingSkeleton> createState() => _ChatLoadingSkeletonState();
}

class _ChatLoadingSkeletonState extends State<ChatLoadingSkeleton> {
  bool _visible = false;
  late final Timer _delay = Timer(const Duration(milliseconds: 250), () {
    if (mounted) setState(() => _visible = true);
  });

  @override
  void initState() {
    super.initState();
    _delay;
  }

  @override
  void dispose() {
    _delay.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Center(
    child: _visible
        ? Semantics(
            liveRegion: true,
            child: Text(
              AppLocalizations.of(context).historyLoading,
              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                color: Theme.of(context).colorScheme.onSurfaceVariant,
              ),
            ),
          )
        : const SizedBox.shrink(),
  );
}

/// A shimmer skeleton mimicking a list (services / sessions) while it loads.
class ListLoadingSkeleton extends StatelessWidget {
  /// Creates a list skeleton with [rows] placeholder rows.
  const ListLoadingSkeleton({super.key, this.rows = 6});

  /// Number of placeholder rows.
  final int rows;

  @override
  Widget build(BuildContext context) => Shimmer(
    child: ListView.builder(
      padding: const EdgeInsets.symmetric(vertical: 8),
      itemCount: rows,
      itemBuilder: (c, i) => Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 11),
        child: Row(
          children: [
            const SkeletonBox(width: 22, height: 22, radius: 11),
            const SizedBox(width: 14),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: const [
                  SkeletonBox(width: double.infinity, height: 12),
                  SizedBox(height: 7),
                  SkeletonBox(width: 120, height: 10),
                ],
              ),
            ),
          ],
        ),
      ),
    ),
  );
}
