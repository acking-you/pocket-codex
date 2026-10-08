import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';

import 'package:pocket_codex/src/motion.dart';

/// A whole-window cross-fade for a light/dark switch.
///
/// `MaterialApp` already lerps [ThemeData], but every colour chosen by
/// `brightness` (diff greens, caution ambers, shadows, the many
/// `light ? a : b` decisions) flips at the halfway point, so the switch reads
/// as a jump with some colours sliding around it. Instead this freezes the
/// last frame as an image, applies the new theme underneath, and fades the
/// image out: every pixel moves from its old colour to its new one at once.
///
/// Install [ThemeTransitionHost] once, around the app's content, and switch
/// through [ThemeTransition.run]. A system-driven change, which has no call
/// site to wrap, keeps the stock lerp.
abstract final class ThemeTransition {
  static final _boundary = GlobalKey();
  static final _snapshot = ValueNotifier<ui.Image?>(null);

  /// Capture the current frame, then run [apply] (which changes the theme).
  /// Falls back to a plain [apply] when there is nothing to capture or motion
  /// is reduced.
  static void run(BuildContext context, VoidCallback apply) {
    final boundary =
        _boundary.currentContext?.findRenderObject() as RenderRepaintBoundary?;
    final animate = Motion.of(context, Motion.slow) > Duration.zero;
    if (!animate || boundary == null || boundary.debugNeedsPaint) {
      apply();
      return;
    }
    try {
      final ratio = MediaQuery.devicePixelRatioOf(context);
      final image = boundary.toImageSync(pixelRatio: ratio);
      _snapshot.value?.dispose();
      _snapshot.value = image;
    } catch (_) {
      // A platform without image capture still switches, just without the fade.
    }
    apply();
  }
}

/// Hosts the frame [ThemeTransition] captures and the fading snapshot.
class ThemeTransitionHost extends StatelessWidget {
  /// Wraps [child], the app's routed content.
  const ThemeTransitionHost({super.key, required this.child});

  /// The app content.
  final Widget child;

  @override
  Widget build(BuildContext context) => Stack(
    textDirection: TextDirection.ltr,
    fit: StackFit.passthrough,
    children: [
      RepaintBoundary(key: ThemeTransition._boundary, child: child),
      ValueListenableBuilder<ui.Image?>(
        valueListenable: ThemeTransition._snapshot,
        builder: (context, image, _) => image == null
            ? const SizedBox.shrink()
            : _FadingSnapshot(key: ObjectKey(image), image: image),
      ),
    ],
  );
}

class _FadingSnapshot extends StatefulWidget {
  const _FadingSnapshot({super.key, required this.image});
  final ui.Image image;

  @override
  State<_FadingSnapshot> createState() => _FadingSnapshotState();
}

class _FadingSnapshotState extends State<_FadingSnapshot>
    with SingleTickerProviderStateMixin {
  late final AnimationController _fade = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 380),
  );

  @override
  void initState() {
    super.initState();
    _fade.forward().whenComplete(() {
      if (!mounted) return;
      if (identical(ThemeTransition._snapshot.value, widget.image)) {
        ThemeTransition._snapshot.value = null;
        widget.image.dispose();
      }
    });
  }

  @override
  void dispose() {
    _fade.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Positioned.fill(
    // Never intercept input: the new theme underneath is already live.
    child: IgnorePointer(
      child: FadeTransition(
        opacity: ReverseAnimation(
          CurvedAnimation(parent: _fade, curve: Curves.easeInOutCubic),
        ),
        child: RawImage(
          image: widget.image,
          fit: BoxFit.fill,
          filterQuality: FilterQuality.none,
        ),
      ),
    ),
  );
}
