import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/attachment_refs.dart';

void main() {
  test('append + split round-trips text and paths', () {
    final sent = appendFileRefs('please review', [
      r'C:\Users\u\.codex\pocket-codex-uploads\1-report.pdf',
      '/home/u/.codex/pocket-codex-uploads/2-notes.md',
    ]);
    expect(sent, contains(kAttachedFilesHeader));
    final back = splitFileRefs(sent);
    expect(back.text, 'please review');
    expect(back.paths, [
      r'C:\Users\u\.codex\pocket-codex-uploads\1-report.pdf',
      '/home/u/.codex/pocket-codex-uploads/2-notes.md',
    ]);
  });

  test('paths with whitespace are quoted like a codex @file mention', () {
    final sent = appendFileRefs('x', ['/tmp/my report.pdf']);
    expect(sent, contains('- "/tmp/my report.pdf"'));
    expect(splitFileRefs(sent).paths, ['/tmp/my report.pdf']);
  });

  test('files-only message is just the block', () {
    final sent = appendFileRefs('', ['/tmp/a.csv']);
    expect(sent, startsWith(kAttachedFilesHeader));
    final back = splitFileRefs(sent);
    expect(back.text, isEmpty);
    expect(back.paths, ['/tmp/a.csv']);
  });

  test('no block → unchanged; nothing to attach → unchanged', () {
    expect(appendFileRefs('hi', const []), 'hi');
    final r = splitFileRefs('just some text');
    expect(r.text, 'just some text');
    expect(r.paths, isEmpty);
  });

  test('prose resembling the header is not mis-parsed into chips', () {
    // Header not at line start.
    final inline = 'see $kAttachedFilesHeader nothing';
    expect(splitFileRefs(inline).paths, isEmpty);
    // Header followed by non-list trailing text.
    final trailing = 'hi\n\n$kAttachedFilesHeader\n- /tmp/a\nand more prose';
    expect(splitFileRefs(trailing).paths, isEmpty);
    expect(splitFileRefs(trailing).text, trailing);
  });

  test('previews never surface the raw wire header', () {
    // File-only message → the preview IS the block → placeholder.
    final fileOnly = appendFileRefs('', ['/host/up/1/a.pdf']);
    expect(previewWithoutFileRefs(fileOnly, '[File]'), '[File]');
    // Server-truncated block still starts with the header → placeholder.
    expect(
      previewWithoutFileRefs(kAttachedFilesHeader.substring(0, 20), '[File]'),
      isNot('[File]'),
      reason: 'a PARTIAL header is ordinary text',
    );
    expect(
      previewWithoutFileRefs('$kAttachedFilesHeader\n- /x', '[File]'),
      '[File]',
    );
    // Text + block → just the text.
    final mixed = appendFileRefs('check this', ['/host/up/1/a.pdf']);
    expect(previewWithoutFileRefs(mixed, '[File]'), 'check this');
    // Plain text passes through.
    expect(previewWithoutFileRefs('hello', '[File]'), 'hello');
  });

  test('hostPathBasename handles both separators', () {
    expect(hostPathBasename(r'C:\a\b\report.pdf'), 'report.pdf');
    expect(hostPathBasename('/home/u/notes.md'), 'notes.md');
    expect(hostPathBasename('bare.txt'), 'bare.txt');
  });
}
