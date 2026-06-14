import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/git_diff.dart';

void main() {
  group('DiffModel.parse', () {
    test('parses a multi-file unified diff with counts and stripped paths', () {
      const raw = '''
diff --git a/lib/main.dart b/lib/main.dart
index 111..222 100644
--- a/lib/main.dart
+++ b/lib/main.dart
@@ -1,3 +1,4 @@
 import 'x';
-final old = 1;
+final neu = 2;
+final extra = 3;
diff --git a/README.md b/README.md
index 333..444 100644
--- a/README.md
+++ b/README.md
@@ -10,2 +10,2 @@
-old line
+new line
''';
      final d = DiffModel.parse(raw);
      expect(d.files.length, 2);
      expect(d.files[0].path, 'lib/main.dart');
      expect(d.files[0].added, 2);
      expect(d.files[0].removed, 1);
      expect(d.files[1].path, 'README.md');
      expect(d.files[1].added, 1);
      expect(d.files[1].removed, 1);
      expect(d.added, 3);
      expect(d.removed, 2);
      expect(d.isEmpty, isFalse);
      // The `index`/`---` metadata lines are not rendered as diff lines.
      expect(d.files[0].lines.any((l) => l.text.startsWith('index')), isFalse);
      // Hunk header preserved.
      expect(d.files[0].lines.first.kind, DiffLineKind.hunk);
    });

    test('empty input yields an empty model', () {
      expect(DiffModel.parse('').isEmpty, isTrue);
      expect(DiffModel.parse('   \n  ').isEmpty, isTrue);
    });

    test('keeps full paths that contain spaces (not truncated)', () {
      const raw = '''
diff --git a/lib/my widget.dart b/lib/my widget.dart
index 1..2 100644
--- a/lib/my widget.dart
+++ b/lib/my widget.dart
@@ -1 +1 @@
-a
+b
''';
      final d = DiffModel.parse(raw);
      expect(d.files.single.path, 'lib/my widget.dart');
    });

    test('uses the old path for deletions (+++ /dev/null)', () {
      const raw = '''
diff --git a/old.txt b/old.txt
deleted file mode 100644
--- a/old.txt
+++ /dev/null
@@ -1,2 +0,0 @@
-gone one
-gone two
''';
      final d = DiffModel.parse(raw);
      expect(d.files.single.path, 'old.txt');
      expect(d.files.single.removed, 2);
      expect(d.added, 0);
    });

    test('uses the new path for additions (--- /dev/null)', () {
      const raw = '''
diff --git a/new.txt b/new.txt
new file mode 100644
--- /dev/null
+++ b/new.txt
@@ -0,0 +1 @@
+hello
''';
      final d = DiffModel.parse(raw);
      expect(d.files.single.path, 'new.txt');
      expect(d.files.single.added, 1);
    });

    test('does not count +++/--- file markers as added/removed lines', () {
      const raw = '''
diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1 +1 @@
-a
+b
''';
      final d = DiffModel.parse(raw);
      expect(d.added, 1);
      expect(d.removed, 1);
    });
  });
}
