import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
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
      // Code blocks reach for AppLocalizations (the copy button's tooltip), so
      // the harness has to supply the delegates a real app would.
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
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

  testWidgets('a fenced block is syntax-highlighted, with its text intact', (
    tester,
  ) async {
    await pumpMarkdown(
      tester,
      '```rust\nfn main() {\n    let x = 1; // note\n}\n```',
    );

    // Find the block's own RichText (the header carries the language name).
    final span = tester
        .widgetList<RichText>(find.byType(RichText))
        .map((w) => w.text)
        .firstWhere((s) => s.toPlainText().contains('fn main()'));
    final colours = <Color>{};
    void walk(InlineSpan s) {
      if (s is TextSpan) {
        final c = s.style?.color;
        if (c != null) colours.add(c);
        s.children?.forEach(walk);
      }
    }

    walk(span);
    // More than one colour is the whole point; one would mean the highlighter
    // never ran and the fallback plain span was rendered.
    expect(colours.length, greaterThan(1));
    // And highlighting must not rewrite what the agent wrote.
    expect(span.toPlainText(), contains('fn main() {\n    let x = 1; // note'));
  });

  testWidgets('an unfenced block stays plain rather than guessing', (
    tester,
  ) async {
    await pumpMarkdown(tester, '```\nsome free-form output\n```');

    final span = tester
        .widgetList<RichText>(find.byType(RichText))
        .map((w) => w.text)
        .firstWhere((s) => s.toPlainText().contains('free-form'));
    expect(span.toPlainText(), contains('some free-form output'));
    // No info string → no grammar → one uniform colour, not a mis-guess.
    final colours = <Color>{};
    void walk(InlineSpan s) {
      if (s is TextSpan) {
        final c = s.style?.color;
        if (c != null) colours.add(c);
        s.children?.forEach(walk);
      }
    }

    walk(span);
    expect(colours.length, lessThanOrEqualTo(1));
  });
}
