import 'package:flutter_test/flutter_test.dart';
import 'package:markdown/markdown.dart' as md;
import 'package:pocket_codex/src/markdown_cjk.dart';

String render(String source) =>
    md.markdownToHtml(source, extensionSet: markdownExtensionSet).trim();

void main() {
  group('CJK strong emphasis', () {
    test('closes a run held open by full-width punctuation', () {
      expect(
        render('**重点是印度：**苹果页面同时列出 ₹700。'),
        '<p><strong>重点是印度：</strong>苹果页面同时列出 ₹700。</p>',
      );
    });

    test('opens a run held closed by a full-width bracket', () {
      expect(render('注意**「重点」**这里。'), '<p>注意<strong>「重点」</strong>这里。</p>');
    });

    test('handles a full-width comma before the closing run', () {
      expect(render('**先看这里，**再看那里。'), '<p><strong>先看这里，</strong>再看那里。</p>');
    });

    test('keeps nested inline syntax inside the emphasis', () {
      expect(
        render('**参见 [文档](https://x.dev)：**详见上文。'),
        '<p><strong>参见 <a href="https://x.dev">文档</a>：</strong>详见上文。</p>',
      );
      expect(
        render('**行内 `code`：**说明。'),
        '<p><strong>行内 <code>code</code>：</strong>说明。</p>',
      );
    });

    test('applies inside list items', () {
      expect(
        render('- **项目一：**说明'),
        '<ul>\n<li><strong>项目一：</strong>说明</li>\n</ul>',
      );
    });
  });

  group('leaves standard CommonMark alone', () {
    test('emphasis the stock rules already close', () {
      expect(render('这是**加粗**紧贴中文。'), '<p>这是<strong>加粗</strong>紧贴中文。</p>');
      expect(render('**开头加粗**后接中文。'), '<p><strong>开头加粗</strong>后接中文。</p>');
      expect(
        render('ASCII **bold** works.'),
        '<p>ASCII <strong>bold</strong> works.</p>',
      );
      expect(
        render('**a**b**c**'),
        '<p><strong>a</strong>b<strong>c</strong></p>',
      );
    });

    test('bold italic stays nested', () {
      expect(render('***粗斜体***。'), '<p><em><strong>粗斜体</strong></em>。</p>');
    });

    // Regression: an asterisk counted as punctuation once let this syntax
    // satisfy its own "delimiter next to punctuation" condition and claim the
    // triple run, yielding `<strong>*强调</strong>*`.
    test('bold italic glued to CJK letters stays nested', () {
      for (final source in ['文字***强调***文字', '前面***加粗斜体***后面', '这是***重点***。']) {
        expect(
          render(source),
          md
              .markdownToHtml(
                source,
                extensionSet: md.ExtensionSet.gitHubFlavored,
              )
              .trim(),
          reason: source,
        );
      }
    });

    test('escaped asterisks stay literal', () {
      expect(render(r'转义 \*\*不加粗：\*\*文字。'), '<p>转义 **不加粗：**文字。</p>');
    });

    test('spaced asterisks are not emphasis', () {
      expect(render('2 ** 3 ** 4 是乘方。'), '<p>2 ** 3 ** 4 是乘方。</p>');
    });

    test('code spans are never rewritten', () {
      expect(render('`**重点：**中文`'), '<p><code>**重点：**中文</code></p>');
      expect(
        render('```\n**重点：**中文\n```'),
        '<pre><code>**重点：**中文\n</code></pre>',
      );
    });

    test('GitHub extensions still apply', () {
      expect(
        render('| a | b |\n| --- | --- |\n| 1 | 2 |'),
        contains('<table>'),
      );
      expect(render('~~删除~~'), '<p><del>删除</del></p>');
    });
  });
}
