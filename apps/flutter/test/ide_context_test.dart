import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/ide_context.dart';

// The real payload from a desktop-app message that pasted a clipboard image.
const _desktopAppMessage = '''
# Files mentioned by the user:

## codex-clipboard-14b4d4d8.png: C:/Users/23946/AppData/Local/Temp/codex-clipboard-14b4d4d8.png

## My request for Codex:
为什么仍然黑屏''';

// What the vendored codex TUI renders (ide_context/prompt.rs).
const _tuiMessage = '''
# Context from my IDE setup:

## Active file: /home/u/proj/src/lib.rs

## My request for Codex:
Ask about this file''';

void main() {
  group('splitIdeContext', () {
    test('keeps a message without the marker exactly as sent', () {
      const plain = 'why is it still a black screen?\n\nsecond paragraph';
      final split = splitIdeContext(plain);
      expect(split.text, plain);
      expect(split.files, isEmpty);
    });

    test('strips the desktop app wrapper back to the request', () {
      final split = splitIdeContext(_desktopAppMessage);
      expect(split.text, '为什么仍然黑屏');
      expect(split.files.single.name, 'codex-clipboard-14b4d4d8.png');
      expect(
        split.files.single.path,
        'C:/Users/23946/AppData/Local/Temp/codex-clipboard-14b4d4d8.png',
      );
    });

    test('strips the TUI wrapper and reads its active file', () {
      final split = splitIdeContext(_tuiMessage);
      expect(split.text, 'Ask about this file');
      expect(split.files.single.path, '/home/u/proj/src/lib.rs');
    });

    test('splits on the LAST marker, as upstream does', () {
      // A user who quotes the marker in their own text must not truncate the
      // message: rsplit semantics keep everything after the final occurrence.
      const quoted =
          '## My request for Codex:\n'
          'here is what the marker looks like: ## My request for Codex:\n'
          'the real request';
      expect(splitIdeContext(quoted).text, 'the real request');
    });

    test('a files-only message leaves empty text', () {
      final split = splitIdeContext(
        _desktopAppMessage.replaceAll('为什么仍然黑屏', ''),
      );
      expect(split.text, isEmpty);
      expect(split.files, hasLength(1));
    });

    test('ignores context lines that are not paths', () {
      const noPaths = '''
# Context from my IDE setup:

## Active selection range: line 3, column 1 to line 9, column 2

## My request for Codex:
go''';
      final split = splitIdeContext(noPaths);
      expect(split.text, 'go');
      expect(split.files, isEmpty);
    });

    test('reads several mentions', () {
      const two = '''
# Files mentioned by the user:

## a.png: /tmp/a.png

## notes.md: /home/u/notes.md

## My request for Codex:
look''';
      expect(splitIdeContext(two).files.map((f) => f.path), [
        '/tmp/a.png',
        '/home/u/notes.md',
      ]);
    });

    test('never reads mentions from the request itself', () {
      const afterMarker = '''
## My request for Codex:
## fake.png: /tmp/fake.png''';
      expect(splitIdeContext(afterMarker).files, isEmpty);
    });
  });

  group('injected context fragments', () {
    test('a whole-message fragment is machinery, not a title', () {
      expect(
        isContextFragment(
          '<recommended_plugins>\n- Google Drive (gd@openai)\n</recommended_plugins>',
        ),
        isTrue,
      );
      // Upstream matches case-insensitively and tolerates surrounding space.
      expect(
        isContextFragment(
          '\n <ENVIRONMENT_CONTEXT>cwd=/x</environment_context> ',
        ),
        isTrue,
      );
    });

    test('a server-truncated fragment is still wire, not a title', () {
      // Previews are cut to a length budget, so a long plugin list arrives
      // without its close marker — and used to sail through as a title.
      expect(
        isContextFragment('<recommended_plugins>\n- Google Drive (gd@ope'),
        isTrue,
      );
      // The extreme of that: cut to just the opening tag, which is exactly
      // what the sessions list was showing as a title.
      expect(isContextFragment('<recommended_plugins>'), isTrue);
    });

    test('a person quoting a marker still owns their message', () {
      expect(
        isContextFragment('why does <turn_aborted> show up in my logs?'),
        isFalse,
      );
      expect(isContextFragment('plain question'), isFalse);
      // A closed tag with a body that isn't one of the known fragments.
      expect(isContextFragment('<mytag>hello</mytag>'), isFalse);
    });

    test('the AGENTS.md payload counts, marker case and all', () {
      // Upstream marks this one with prose, not a tag, and not in lowercase.
      expect(
        isContextFragment(
          '# AGENTS.md instructions for D:\\p\n\n<INSTRUCTIONS>\nbe terse\n</INSTRUCTIONS>',
        ),
        isTrue,
      );
    });

    test('a voice handoff is unwrapped, never hidden', () {
      const wrapped =
          '<realtime_delegation>\n  <input>run ls</input>\n'
          '  <transcript_delta>user: run ls</transcript_delta>\n'
          '</realtime_delegation>';
      expect(isContextFragment(wrapped), isFalse);
      expect(realtimeDelegationInput(wrapped), 'run ls');
      expect(realtimeDelegationInput('plain text'), isNull);
    });
  });

  group('looksLikeImagePath', () {
    test('accepts the decodable extensions, case-insensitively', () {
      for (final p in ['/a/b.png', 'C:/x.JPG', 'x.jpeg', 'y.webp', 'z.BMP']) {
        expect(looksLikeImagePath(p), isTrue, reason: p);
      }
    });

    test('rejects everything else', () {
      for (final p in ['/a/notes.md', 'x.', 'noextension', '/a/b.pdf']) {
        expect(looksLikeImagePath(p), isFalse, reason: p);
      }
    });
  });
}
