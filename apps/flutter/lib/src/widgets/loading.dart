import 'package:flutter/material.dart';

/// A shimmering sweep used for skeleton loaders, so screens fade in with a
/// smooth "loading" feel instead of a bare spinner. Wrap skeleton shapes
/// ([SkeletonBox]) in this to animate them.
class Shimmer extends StatefulWidget {
  /// Wraps [child] with an animated highlight sweep.
  const Shimmer({super.key, required this.child});

  /// The skeleton content to shimmer over.
  final Widget child;

  @override
  State<Shimmer> createState() => _ShimmerState();
}

class _ShimmerState extends State<Shimmer> with SingleTickerProviderStateMixin {
  late final AnimationController _c = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 1400),
  )..repeat();

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final onSurface = Theme.of(context).colorScheme.onSurface;
    final base = onSurface.withValues(alpha: 0.06);
    final highlight = onSurface.withValues(alpha: 0.16);
    return AnimatedBuilder(
      animation: _c,
      builder: (context, child) => ShaderMask(
        blendMode: BlendMode.srcATop,
        shaderCallback: (bounds) => LinearGradient(
          begin: Alignment.centerLeft,
          end: Alignment.centerRight,
          colors: [base, highlight, base],
          stops: const [0.30, 0.5, 0.70],
          transform: _SlideGradient(_c.value),
        ).createShader(bounds),
        child: child,
      ),
      child: widget.child,
    );
  }
}

/// Slides a gradient horizontally from off-screen left to off-screen right as
/// [t] goes 0→1.
class _SlideGradient extends GradientTransform {
  const _SlideGradient(this.t);
  final double t;
  @override
  Matrix4? transform(Rect bounds, {TextDirection? textDirection}) =>
      Matrix4.translationValues((t * 2 - 1) * bounds.width, 0, 0);
}

/// A solid rounded placeholder shape; group several inside a [Shimmer].
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
  Widget build(BuildContext context) => Container(
    width: width,
    height: height,
    decoration: BoxDecoration(
      color: Theme.of(context).colorScheme.onSurface.withValues(alpha: 0.12),
      borderRadius: BorderRadius.circular(radius),
    ),
  );
}

/// A shimmer skeleton mimicking a conversation while a thread loads.
class ChatLoadingSkeleton extends StatelessWidget {
  /// Creates the chat skeleton.
  const ChatLoadingSkeleton({super.key});

  @override
  Widget build(BuildContext context) {
    Widget bubble({required bool me, required double w, int lines = 1}) => Align(
      alignment: me ? Alignment.centerRight : Alignment.centerLeft,
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 9),
        child: Column(
          crossAxisAlignment: me
              ? CrossAxisAlignment.end
              : CrossAxisAlignment.start,
          children: [
            for (var i = 0; i < lines; i++) ...[
              SkeletonBox(width: w * (i == lines - 1 ? 0.55 : 1), height: 12),
              if (i < lines - 1) const SizedBox(height: 7),
            ],
          ],
        ),
      ),
    );

    return Shimmer(
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 820),
          child: ListView(
            padding: const EdgeInsets.fromLTRB(16, 18, 16, 12),
            children: [
              bubble(me: true, w: 220),
              bubble(me: false, w: 340, lines: 3),
              bubble(me: true, w: 150),
              bubble(me: false, w: 300, lines: 2),
            ],
          ),
        ),
      ),
    );
  }
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
