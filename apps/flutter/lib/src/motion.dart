import 'package:flutter/cupertino.dart' show CupertinoPageTransitionsBuilder;
import 'package:flutter/material.dart';

/// Shared motion tokens.
///
/// Three durations and two curves cover the app: a hover or press answers in
/// [Motion.fast], a panel opening or a row expanding takes [Motion.medium],
/// and a page or the theme changing takes [Motion.slow]. Entrances decelerate
/// ([Motion.enter]); things that move in place ease both ways
/// ([Motion.move]). One vocabulary keeps the app feeling like a single object
/// instead of a dozen widgets each timed by hand.
abstract final class Motion {
  /// Hover, press, small state swaps.
  static const fast = Duration(milliseconds: 120);

  /// Panels, expanding rows, the sidebar sliding.
  static const medium = Duration(milliseconds: 220);

  /// Page transitions and the whole-window theme change.
  static const slow = Duration(milliseconds: 320);

  /// Things arriving: decelerate into place.
  static const Curve enter = Curves.easeOutCubic;

  /// Things moving or resizing in place.
  static const Curve move = Curves.easeInOutCubic;

  /// [d], or zero when the platform asks for reduced motion — the state still
  /// changes, it just doesn't travel.
  static Duration of(BuildContext context, Duration d) =>
      MediaQuery.maybeDisableAnimationsOf(context) ?? false ? Duration.zero : d;

  /// Whether looping, purely decorative motion (pulses, typing dots) may run.
  static bool ambientAllowed(BuildContext context) =>
      !(MediaQuery.maybeDisableAnimationsOf(context) ?? false);
}

/// Desktop pages fade and rise a few pixels instead of Material's mobile zoom,
/// which reads as a phone animation inside a desktop window. Mobile keeps each
/// platform's native transition (Cupertino swipe-back on iOS).
const appPageTransitions = PageTransitionsTheme(
  builders: {
    TargetPlatform.android: FadeForwardsPageTransitionsBuilder(),
    TargetPlatform.iOS: CupertinoPageTransitionsBuilder(),
    TargetPlatform.macOS: _DesktopPageTransitionsBuilder(),
    TargetPlatform.windows: _DesktopPageTransitionsBuilder(),
    TargetPlatform.linux: _DesktopPageTransitionsBuilder(),
  },
);

class _DesktopPageTransitionsBuilder extends PageTransitionsBuilder {
  const _DesktopPageTransitionsBuilder();

  @override
  Duration get transitionDuration => Motion.medium;

  @override
  Widget buildTransitions<T>(
    PageRoute<T> route,
    BuildContext context,
    Animation<double> animation,
    Animation<double> secondaryAnimation,
    Widget child,
  ) {
    if (MediaQuery.maybeDisableAnimationsOf(context) ?? false) return child;
    final enter = CurvedAnimation(parent: animation, curve: Motion.enter);
    return FadeTransition(
      opacity: enter,
      child: SlideTransition(
        position: Tween(
          begin: const Offset(0, 0.012),
          end: Offset.zero,
        ).animate(enter),
        child: child,
      ),
    );
  }
}
