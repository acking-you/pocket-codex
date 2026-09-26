import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/file_links.dart';

void main() {
  test('resolves host paths without using the controller path convention', () {
    expect(
      FileLink.parse('src/a%20b.rs:12:3', cwd: '/project')?.path,
      '/project/src/a b.rs',
    );
    expect(FileLink.parse('src/a.rs:12:3', cwd: '/project')?.line, 12);
    expect(FileLink.parse('file:///tmp/a%20b.md#L15')?.path, '/tmp/a b.md');
    expect(FileLink.parse('file:///tmp/a%20b.md#L15')?.line, 15);
    expect(FileLink.parse(r'C:\work\a.rs:4')?.path, r'C:\work\a.rs');
    expect(
      FileLink.parse('src/a.rs', cwd: r'C:\work')?.path,
      r'C:\work\src\a.rs',
    );
    expect(FileLink.parse('file:///C:/work/a%20b.md')?.path, r'C:\work\a b.md');
    expect(FileLink.parse('file://localhost/tmp/a')?.path, '/tmp/a');
  });

  test('does not turn anchors, web URLs or unsafe schemes into files', () {
    for (final href in [
      '',
      '#title',
      '//example.com/a',
      'https://example.com/a',
      'javascript:alert(1)',
      'mailto:me@example.com',
      'file://server/share/a',
      '%00secret',
      '%zz',
    ]) {
      expect(FileLink.parse(href, cwd: '/project'), isNull, reason: href);
    }
    expect(FileLink.parse('relative.txt'), isNull);
  });

  test('recognizes loopback and wildcard hosts, including IPv6', () {
    for (final host in [
      'localhost',
      'localhost.',
      'app.localhost',
      '127.0.0.1',
      '127.2.3.4',
      '0.0.0.0',
      '[::1]',
      '[::]',
      '[0:0:0:0:0:0:0:1]',
      '[::ffff:127.0.0.1]',
    ]) {
      expect(
        isHostLocalWebUrl(Uri.parse('http://$host:3000/a?q=x')),
        isTrue,
        reason: host,
      );
    }
    for (final url in [
      'https://example.com',
      'http://localhost.example.com',
      'http://192.168.1.2',
      'file:///tmp/a',
    ]) {
      expect(isHostLocalWebUrl(Uri.parse(url)), isFalse, reason: url);
    }
  });
}
