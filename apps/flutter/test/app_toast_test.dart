import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/widgets/app_toast.dart';

/// Mounts a button that raises a toast, so each test drives the real
/// `ScaffoldMessenger` path rather than the builder in isolation.
Widget _host(void Function(BuildContext) raise) => MaterialApp(
  theme: lightTheme(),
  home: Scaffold(
    body: Builder(
      builder: (context) =>
          TextButton(onPressed: () => raise(context), child: const Text('go')),
    ),
  ),
);

Future<void> _tap(WidgetTester t) async {
  await t.tap(find.text('go'));
  await t.pump(); // let the snack bar animate in
  await t.pump(const Duration(milliseconds: 400));
}

void main() {
  testWidgets('a confirmation reads as success, not as a warning', (t) async {
    await t.pumpWidget(_host((c) => showToastOk(c, 'saved')));
    await _tap(t);

    expect(find.text('saved'), findsOneWidget);
    final icon = t.widget<Icon>(find.byIcon(Icons.check_circle_outline));
    // The success hue, so the tone is legible before the text is read.
    expect(icon.color, successColor(lightTheme().colorScheme));
  });

  testWidgets('a failure is marked with the error colour', (t) async {
    await t.pumpWidget(_host((c) => showToastError(c, 'upload failed')));
    await _tap(t);

    expect(find.text('upload failed'), findsOneWidget);
    final icon = t.widget<Icon>(find.byIcon(Icons.error_outline));
    expect(icon.color, lightTheme().colorScheme.error);
  });

  testWidgets('a neutral notice is neither', (t) async {
    await t.pumpWidget(_host((c) => showToast(c, 'resumed')));
    await _tap(t);

    expect(find.byIcon(Icons.info_outline), findsOneWidget);
    expect(find.byIcon(Icons.check_circle_outline), findsNothing);
    expect(find.byIcon(Icons.error_outline), findsNothing);
  });

  testWidgets('it draws its own surface instead of Material\'s slab', (
    t,
  ) async {
    await t.pumpWidget(_host((c) => showToastOk(c, 'copied')));
    await _tap(t);

    // Transparent + flat: every visible pixel belongs to the card inside, so the
    // toast matches the app's raised cards rather than the default dark block.
    final bar = t.widget<SnackBar>(find.byType(SnackBar));
    expect(bar.backgroundColor, Colors.transparent);
    expect(bar.elevation, 0);
  });

  testWidgets('a failure stays up longer than a confirmation', (t) async {
    await t.pumpWidget(_host((c) => showToastOk(c, 'copied')));
    await _tap(t);
    final ok = t.widget<SnackBar>(find.byType(SnackBar)).duration;

    await t.pumpWidget(_host((c) => showToastError(c, 'nope')));
    await _tap(t);
    final failed = t.widget<SnackBar>(find.byType(SnackBar)).duration;

    // An error carries a reason worth reading; "Copied" does not.
    expect(failed, greaterThan(ok));
  });

  testWidgets('tapping copy twice replaces the toast rather than queueing', (
    t,
  ) async {
    await t.pumpWidget(_host((c) => showToastOk(c, 'copied')));
    await _tap(t);
    await _tap(t);
    await t.pump(const Duration(milliseconds: 400));

    // Without hideCurrentSnackBar the second would wait out the first.
    expect(find.byType(SnackBar), findsOneWidget);
  });

  testWidgets('a captured messenger still reports after its context is gone', (
    t,
  ) async {
    // The download/upload paths hold a messenger across an await because their
    // own context may be disposed by the time the work finishes.
    late ToastMessenger messenger;
    await t.pumpWidget(
      MaterialApp(
        theme: lightTheme(),
        home: Scaffold(
          body: Builder(
            builder: (context) {
              messenger = ToastMessenger.of(context);
              return const Text('mounted');
            },
          ),
        ),
      ),
    );

    messenger.ok('downloaded');
    await t.pump();
    await t.pump(const Duration(milliseconds: 400));
    expect(find.text('downloaded'), findsOneWidget);
  });
}
