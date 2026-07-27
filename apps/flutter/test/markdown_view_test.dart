import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/widgets/markdown_view.dart';

/// Everything [MarkdownView] actually painted, concatenated. `includePlaceholders`
/// keeps `WidgetSpan`s (the favicons) out of the string.
String renderedText(WidgetTester tester) => tester
    .widgetList<RichText>(find.byType(RichText))
    .map((w) => w.text.toPlainText(includePlaceholders: false))
    .join('\n');

Future<void> pumpMarkdown(WidgetTester tester, String data) async {
  await tester.pumpWidget(
    MaterialApp(
      home: Scaffold(
        body: SelectionArea(
          child: SingleChildScrollView(child: MarkdownView(data: data)),
        ),
      ),
    ),
  );
  await tester.pump();
}

void main() {
  testWidgets('bolds CJK text that CommonMark leaves literal', (tester) async {
    await pumpMarkdown(tester, '**重点是印度：**苹果页面同时列出 ₹700。');

    final text = renderedText(tester);
    expect(text, contains('重点是印度：苹果页面'));
    expect(text, isNot(contains('*')));
  });

  testWidgets('a wide table scrolls horizontally instead of clipping', (
    tester,
  ) async {
    await tester.binding.setSurfaceSize(const Size(360, 640));
    addTearDown(() => tester.binding.setSurfaceSize(null));

    await pumpMarkdown(
      tester,
      '| 地区 | App Store 标价 | 约合人民币 | 结论 |\n'
      '| --- | --- | --- | --- |\n'
      '| 印度 | ₹700 或 ₹2,900 | 约 ¥49 / ¥204 | ₹700 可能只对旧订阅开放 |\n',
    );

    final scrollables = tester
        .widgetList<SingleChildScrollView>(find.byType(SingleChildScrollView))
        .where((s) => s.scrollDirection == Axis.horizontal);
    expect(scrollables, isNotEmpty);
    // The last column survived the narrow viewport.
    expect(renderedText(tester), contains('结论'));
    // The table scrolls *inside* the message; it must not widen it, or every
    // paragraph in the transcript would wrap past the right edge of the window.
    expect(tester.getSize(find.byType(MarkdownView)).width, 360);
    expect(tester.takeException(), isNull);
  });

  testWidgets('links render with a favicon slot and no raw markup', (
    tester,
  ) async {
    await pumpMarkdown(tester, '价格见 [苹果官网](https://www.apple.com/cn/) 页面。');

    // The network favicon can't load in a test binding, so the generic globe
    // stands in — that is the same fallback a blocked or 404ing host gets.
    expect(find.byIcon(Icons.public), findsOneWidget);
    final text = renderedText(tester);
    expect(text, contains('苹果官网'));
    expect(text, isNot(contains('https://')));
  });

  testWidgets('keeps formatting inside a link label', (tester) async {
    await pumpMarkdown(tester, '见 [**苹果**官网](https://www.apple.com/cn/)。');

    final span = tester
        .widgetList<RichText>(find.byType(RichText))
        .map((w) => w.text)
        .firstWhere((s) => s.toPlainText().contains('苹果'));
    final weights = <FontWeight?>[];
    span.visitChildren((child) {
      if (child is TextSpan && child.text == '苹果') {
        weights.add(child.style?.fontWeight);
      }
      return true;
    });
    expect(weights, [FontWeight.w600]);
  });

  testWidgets('a non-web link gets no favicon', (tester) async {
    await pumpMarkdown(tester, '见 [第二节](#section-2) 说明。');

    expect(find.byIcon(Icons.public), findsNothing);
    expect(renderedText(tester), contains('第二节'));
  });
}
