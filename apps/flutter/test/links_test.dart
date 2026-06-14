import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/widgets/links.dart';

void main() {
  group('autolinkifyMarkdown', () {
    test('inserts a space between a full-width colon and a bare URL', () {
      expect(
        autolinkifyMarkdown('官网：https://flutter.dev'),
        '官网： https://flutter.dev',
      );
    });

    test('inserts a space between a CJK char and a bare URL', () {
      expect(
        autolinkifyMarkdown('仓库https://github.com/x'),
        '仓库 https://github.com/x',
      );
    });

    test('leaves an explicit markdown link untouched', () {
      const md = '见 [Flutter](https://flutter.dev) 文档';
      expect(autolinkifyMarkdown(md), md);
    });

    test('leaves a URL already preceded by a space untouched', () {
      const md = 'see https://example.com now';
      expect(autolinkifyMarkdown(md), md);
    });

    test('leaves an angle-bracket autolink untouched', () {
      const md = '<https://example.com>';
      expect(autolinkifyMarkdown(md), md);
    });
  });
}
