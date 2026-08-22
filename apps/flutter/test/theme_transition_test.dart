// The light/dark switch is animated, not a hard cut. The whole window changes
// colour at once, so a jump is the most jarring thing the app can do.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/theme.dart';

void main() {
  testWidgets('switching brightness cross-fades instead of jumping', (t) async {
    Widget app(ThemeMode mode) => MaterialApp(
      theme: lightTheme(),
      darkTheme: darkTheme(),
      themeMode: mode,
      // Mirrors main.dart: the eased, slightly longer curve is the point.
      themeAnimationDuration: const Duration(milliseconds: 350),
      themeAnimationCurve: Curves.easeInOutCubic,
      home: Builder(
        builder: (c) => Scaffold(
          backgroundColor: Theme.of(c).colorScheme.surface,
          body: const SizedBox.expand(),
        ),
      ),
    );
    Color surface(WidgetTester t) =>
        t.widget<Scaffold>(find.byType(Scaffold)).backgroundColor!;

    await t.pumpWidget(app(ThemeMode.light));
    await t.pumpAndSettle();
    final light = surface(t);

    await t.pumpWidget(app(ThemeMode.dark));
    // Part-way through, the colour must be neither endpoint — that is what
    // proves it interpolated rather than swapped.
    await t.pump(const Duration(milliseconds: 175));
    final mid = surface(t);
    expect(mid, isNot(light));

    await t.pumpAndSettle();
    final dark = surface(t);
    expect(mid, isNot(dark));
    // And it genuinely lands on the dark surface rather than stalling mid-way.
    expect(dark.computeLuminance(), lessThan(light.computeLuminance()));
  });
}
