import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/widgets/loading.dart';

Widget _host(Widget child, {bool reduceMotion = false}) => MaterialApp(
  home: MediaQuery(
    data: MediaQueryData(disableAnimations: reduceMotion),
    child: Scaffold(body: child),
  ),
);

/// Where the sweep's band currently sits, as the gradient's own transform
/// resolves it. The transform object itself has no value equality, so the
/// resolved matrix is what tells two frames apart.
String _bandPosition(LinearGradient g) =>
    '${g.transform!.transform(const Rect.fromLTWH(0, 0, 100, 12))}';

BoxDecoration _firstSkeleton(WidgetTester t) =>
    t
            .widget<Container>(
              find
                  .descendant(
                    of: find.byType(SkeletonBox),
                    matching: find.byType(Container),
                  )
                  .first,
            )
            .decoration!
        as BoxDecoration;

void main() {
  testWidgets('a skeleton inside a shimmer paints a moving gradient', (
    t,
  ) async {
    // Regression, twice over. The sweep was first painted with a ShaderMask in
    // `srcATop`, which keeps the DESTINATION's alpha — so the shimmer's own
    // alphas never reached the screen and the skeleton looked frozen. Forcing
    // the shape opaque to "fix" that made srcATop yield solid black bars.
    // The sweep is a gradient fill now, with no blend mode in the way.
    await t.pumpWidget(_host(const ChatLoadingSkeleton()));
    final first = _firstSkeleton(t).gradient! as LinearGradient;
    expect(first.colors.first.a, lessThan(0.2), reason: 'base stays subtle');
    expect(first.colors[1].a, greaterThan(first.colors.first.a * 2));

    await t.pump(const Duration(milliseconds: 400));
    final moved = _firstSkeleton(t).gradient! as LinearGradient;
    expect(
      _bandPosition(moved),
      isNot(_bandPosition(first)),
      reason: 'the band travels across the shape',
    );
    await t.pumpWidget(_host(const SizedBox()));
  });

  testWidgets('reduced motion keeps the skeleton and holds the sweep still', (
    t,
  ) async {
    await t.pumpWidget(
      _host(const ListLoadingSkeleton(rows: 2), reduceMotion: true),
    );
    final first = _firstSkeleton(t).gradient! as LinearGradient;
    await t.pump(const Duration(milliseconds: 400));
    final later = _firstSkeleton(t).gradient! as LinearGradient;
    expect(_bandPosition(later), _bandPosition(first));
    expect(find.byType(SkeletonBox), findsWidgets);
    await t.pumpAndSettle();
  });

  testWidgets('a stray skeleton outside a shimmer is flat, not black', (
    t,
  ) async {
    await t.pumpWidget(_host(const SkeletonBox(width: 40)));
    final decoration = _firstSkeleton(t);
    expect(decoration.gradient, isNull);
    expect(decoration.color!.a, lessThan(0.2));
  });
}
