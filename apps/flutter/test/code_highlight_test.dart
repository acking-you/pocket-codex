import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/code_highlight.dart';

void main() {
  const base = TextStyle(fontFamily: 'monospace', fontSize: 12.5);

  group('resolveLanguage', () {
    test('accepts a grammar by its own name', () {
      expect(resolveLanguage('rust'), 'rust');
      expect(resolveLanguage('dart'), 'dart');
    });

    test('maps the aliases models actually write', () {
      expect(resolveLanguage('sh'), 'bash');
      expect(resolveLanguage('ps1'), 'powershell');
      expect(resolveLanguage('ts'), 'typescript');
      expect(resolveLanguage('yml'), 'yaml');
      expect(resolveLanguage('toml'), 'ini');
      expect(resolveLanguage('patch'), 'diff');
    });

    test('is case- and whitespace-insensitive, and ignores fence extras', () {
      expect(resolveLanguage('  Rust  '), 'rust');
      expect(resolveLanguage('dart title="main.dart"'), 'dart');
    });

    test('declines the cases that must stay plain', () {
      expect(resolveLanguage(''), isNull); // no info string
      expect(resolveLanguage('text'), isNull);
      expect(resolveLanguage('plaintext'), isNull);
      expect(resolveLanguage('brainfuck'), isNull); // not registered
    });
  });

  group('highlightCode', () {
    test('colours a known language into more than one span', () {
      final span = highlightCode(
        code: 'fn main() {\n    let x = 1; // hi\n}',
        language: 'rust',
        base: base,
        brightness: Brightness.light,
      );
      expect(span.children, isNotNull);
      expect(span.children!.length, greaterThan(1));
      // Whatever the grammar decides, the visible text must survive intact.
      expect(span.toPlainText(), 'fn main() {\n    let x = 1; // hi\n}');
    });

    test('dark and light produce different colours for the same source', () {
      // Walk the tree: the ROOT span's toString() says nothing about its
      // children, so comparing that would pass no matter what the theme did.
      List<Color> colours(Brightness b) {
        final out = <Color>[];
        void walk(InlineSpan s) {
          if (s is TextSpan) {
            final c = s.style?.color;
            if (c != null) out.add(c);
            s.children?.forEach(walk);
          }
        }

        walk(
          highlightCode(
            code: 'fn main() { let x = 1; }',
            language: 'rust',
            base: base,
            brightness: b,
          ),
        );
        return out;
      }

      final light = colours(Brightness.light);
      expect(light, isNotEmpty);
      expect(light, isNot(colours(Brightness.dark)));
    });

    test('allowItalic:false forces every span upright', () {
      // A comment is what the theme italicises; with allowItalic:false no span
      // in the tree may keep a slant.
      final span = highlightCode(
        code: 'fn main() {} // a comment here',
        language: 'rust',
        base: base,
        brightness: Brightness.light,
        allowItalic: false,
      );
      var sawItalic = false;
      void walk(InlineSpan s) {
        if (s is TextSpan) {
          if (s.style?.fontStyle == FontStyle.italic) sawItalic = true;
          s.children?.forEach(walk);
        }
      }

      walk(span);
      expect(sawItalic, isFalse);
      expect(span.toPlainText(), 'fn main() {} // a comment here');
    });

    test('an unknown language falls back to one plain span', () {
      final span = highlightCode(
        code: 'some free text',
        language: '',
        base: base,
        brightness: Brightness.light,
      );
      expect(span.text, 'some free text');
      expect(span.style, base);
      expect(span.children, isNull);
    });

    test('a huge block is left plain rather than swept', () {
      final big = 'let x = 1;\n' * 5000; // > 20k chars
      final span = highlightCode(
        code: big,
        language: 'rust',
        base: base,
        brightness: Brightness.light,
      );
      expect(span.text, big);
      expect(span.children, isNull);
    });

    test('an empty block does not render nothing', () {
      final span = highlightCode(
        code: '',
        language: 'dart',
        base: base,
        brightness: Brightness.light,
      );
      expect(span.toPlainText(), '');
    });

    test('text is preserved verbatim for every registered language', () {
      const samples = {
        'bash': r'set -e\necho "$HOME" # note',
        'python': 'def f(x):\n    return x * 2  # note',
        'json': '{"a": [1, 2], "b": null}',
        'yaml': 'key: value\nlist:\n  - one',
        'diff': '--- a\n+++ b\n-old\n+new',
        'ini': '[section]\nkey = "value"',
      };
      for (final entry in samples.entries) {
        final span = highlightCode(
          code: entry.value,
          language: entry.key,
          base: base,
          brightness: Brightness.dark,
        );
        expect(
          span.toPlainText(),
          entry.value,
          reason: '${entry.key} lost or reordered text',
        );
      }
    });
  });
}
