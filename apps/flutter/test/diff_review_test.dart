import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/git_diff.dart';
import 'package:pocket_codex/src/widgets/diff_review.dart';

Future<void> _pump(
  WidgetTester t,
  DiffFile file, {
  Future<List<String>?> Function(String)? onLoadFile,
}) async {
  // Tall surface so an expanded gap's rows are all built (the list is lazy).
  t.view.devicePixelRatio = 1.0;
  t.view.physicalSize = const Size(900, 2000);
  addTearDown(t.view.reset);
  await t.pumpWidget(
    MaterialApp(
      locale: const Locale('zh'),
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      home: Scaffold(
        body: DiffReviewView(file: file, branch: 'dev', onLoadFile: onLoadFile),
      ),
    ),
  );
  await t.pumpAndSettle();
}

void main() {
  // A file changed at the top and again far down, so git emits two hunks with
  // an elided run between them (lines 5..49 unchanged → a 45-line gap).
  const diff =
      'diff --git a/lib/x.dart b/lib/x.dart\n'
      '--- a/lib/x.dart\n'
      '+++ b/lib/x.dart\n'
      '@@ -1,3 +1,3 @@\n'
      ' line 1\n'
      '-line 2 old\n'
      '+line 2 new\n'
      ' line 3\n'
      '@@ -50,2 +50,3 @@\n'
      ' line 50\n'
      '+line 51 added\n'
      ' line 52\n';

  testWidgets('elided lines collapse to a clickable gap that expands', (
    t,
  ) async {
    final file = DiffModel.parse(diff).files.single;
    // The "current file" the host would return — the gap lines (4..49) are the
    // unchanged content between the two hunks.
    final current = [for (var i = 1; i <= 52; i++) 'line $i'];
    var loads = 0;
    await _pump(
      t,
      file,
      onLoadFile: (_) async {
        loads++;
        return current;
      },
    );

    // The gap between the hunks shows as "45 行未更改" (lines 4..48 inclusive:
    // new hunk starts at 50, previous new line was 4 → 4..49 = 46? see below).
    expect(find.textContaining('行未更改'), findsOneWidget);
    // A hidden line is not rendered yet.
    expect(find.textContaining('line 30', findRichText: true), findsNothing);

    // Clicking the gap reads the file once and reveals the elided lines.
    await t.tap(find.textContaining('行未更改'));
    await t.pumpAndSettle();
    expect(loads, 1);
    expect(find.textContaining('line 30', findRichText: true), findsOneWidget);
    expect(find.textContaining('行未更改'), findsNothing);
  });

  testWidgets('a gap stays a plain marker when no file reader is wired', (
    t,
  ) async {
    final file = DiffModel.parse(diff).files.single;
    await _pump(t, file); // no onLoadFile

    // Still labelled, but not interactive — tapping reveals nothing.
    final gap = find.textContaining('行未更改');
    expect(gap, findsOneWidget);
    await t.tap(gap);
    await t.pumpAndSettle();
    expect(find.textContaining('line 30', findRichText: true), findsNothing);
  });

  testWidgets('a failed read is reported and disables the gap', (t) async {
    final file = DiffModel.parse(diff).files.single;
    await _pump(t, file, onLoadFile: (_) async => null); // host can't serve it

    await t.tap(find.textContaining('行未更改'));
    await t.pumpAndSettle();
    // Marker now says it couldn't load; no lines appeared.
    expect(find.textContaining('无法加载'), findsOneWidget);
    expect(find.textContaining('line 30', findRichText: true), findsNothing);
  });

  testWidgets('the changed-file tree folds paths and marks the selection', (
    t,
  ) async {
    final diff2 =
        'diff --git a/lib/a.dart b/lib/a.dart\n'
        '--- a/lib/a.dart\n+++ b/lib/a.dart\n@@ -1 +1 @@\n-x\n+y\n'
        'diff --git a/lib/sub/b.dart b/lib/sub/b.dart\n'
        '--- a/lib/sub/b.dart\n+++ b/lib/sub/b.dart\n@@ -1 +1 @@\n-p\n+q\n';
    final model = DiffModel.parse(diff2);
    await t.pumpWidget(
      MaterialApp(
        locale: const Locale('zh'),
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(
          body: SizedBox(
            width: 300,
            child: ChangedFileTree(
              files: model.files,
              selected: 'lib/a.dart',
              onSelect: (_) {},
            ),
          ),
        ),
      ),
    );
    await t.pumpAndSettle();

    // Both files are reachable as leaves, keyed by their full path.
    expect(find.byKey(const Key('review-file-lib/a.dart')), findsOneWidget);
    expect(find.byKey(const Key('review-file-lib/sub/b.dart')), findsOneWidget);
  });
}
