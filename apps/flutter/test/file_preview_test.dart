import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/widgets/file_preview.dart';
import 'package:pocket_codex/src/widgets/markdown_view.dart';

Future<void> pumpPreview(
  WidgetTester t,
  String name,
  String text, {
  bool truncated = false,
}) async {
  await t.pumpWidget(
    MaterialApp(
      locale: const Locale('en'),
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      home: Scaffold(
        body: SizedBox(
          width: 600,
          height: 400,
          child: FilePreview(
            name: name,
            bytes: Uint8List.fromList(utf8.encode(text)),
            text: text,
            truncated: truncated,
          ),
        ),
      ),
    ),
  );
  await t.pump();
}

String painted(WidgetTester t) => t
    .widgetList<RichText>(find.byType(RichText))
    .map((w) => w.text.toPlainText(includePlaceholders: false))
    .join('\n');

void main() {
  test('previewKindFor picks a renderer by type and completeness', () {
    PreviewKind kind(
      String name, {
      bool truncated = false,
      String body = 'x',
    }) => previewKindFor(
      name,
      Uint8List.fromList(utf8.encode(body)),
      truncated: truncated,
    );
    expect(kind('README.md'), PreviewKind.markdown);
    expect(kind('a.json'), PreviewKind.json);
    // Half a JSON document does not parse; show it as source.
    expect(kind('a.json', truncated: true), PreviewKind.source);
    expect(kind('a.csv'), PreviewKind.table);
    expect(kind('logo.svg'), PreviewKind.svg);
    expect(kind('logo.svg', truncated: true), PreviewKind.source);
    expect(kind('main.rs'), PreviewKind.source);
    expect(kind('a.pdf'), PreviewKind.unsupported);
    expect(kind('blob.txt', body: 'a\u0000b'), PreviewKind.unsupported);
  });

  test('parseDelimited follows CSV quoting', () {
    expect(parseDelimited('a,b\n"x, y","say ""hi"""\r\n1,'), [
      ['a', 'b'],
      ['x, y', 'say "hi"'],
      ['1', ''],
    ]);
    expect(parseDelimited('a\tb', separator: '\t'), [
      ['a', 'b'],
    ]);
    expect(parseDelimited('1\n2\n3\n', limit: 2), hasLength(2));
  });

  testWidgets('markdown renders by default and switches to source', (t) async {
    await pumpPreview(t, 'README.md', '# Title\n\nSome **bold** text.');
    expect(find.byType(MarkdownView), findsOneWidget);
    expect(painted(t), isNot(contains('**')));
    await t.tap(find.text('Source'));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('file-preview-source')), findsOneWidget);
    expect(painted(t), contains('**bold**'));
  });

  testWidgets('json is pretty-printed', (t) async {
    await pumpPreview(t, 'data.json', '{"a":1,"b":[true]}');
    expect(find.byKey(const Key('file-preview-json')), findsOneWidget);
    expect(painted(t), contains('"a": 1'));
  });

  testWidgets('csv renders as a table with a header row', (t) async {
    await pumpPreview(t, 'rows.csv', 'name,score\nada,3\nlin,5');
    expect(find.byKey(const Key('file-preview-table')), findsOneWidget);
    expect(find.text('score'), findsOneWidget);
    expect(find.text('lin'), findsOneWidget);
  });

  test('a complete picture past the text limit is still a picture', () {
    final bytes = Uint8List.fromList(utf8.encode('<svg/>'));
    expect(
      previewKindFor('shot.png', bytes, truncated: false, textTruncated: true),
      PreviewKind.image,
    );
    expect(
      previewKindFor('logo.svg', bytes, truncated: false, textTruncated: true),
      PreviewKind.svg,
    );
    // Documents parse from the decoded text, which stops short.
    expect(
      previewKindFor('a.json', bytes, truncated: false, textTruncated: true),
      PreviewKind.source,
    );
    expect(
      previewKindFor('shot.png', bytes, truncated: true),
      PreviewKind.unsupported,
    );
  });

  testWidgets('a very wide csv shows as source, not a grid of cells', (
    t,
  ) async {
    // One row of a hundred thousand cells, then a few more rows.
    final wide = '${List.filled(100000, 'x').join(',')}\n1,2\n3,4';
    await pumpPreview(t, 'wide.csv', wide);
    expect(find.byKey(const Key('file-preview-table')), findsNothing);
    expect(find.byKey(const Key('file-preview-mode')), findsNothing);
    expect(find.byKey(const Key('file-preview-source')), findsOneWidget);
    expect(t.widgetList(find.byType(Text)).length, lessThan(200));
    expect(t.takeException(), isNull);
  });

  test('tableFits bounds columns and total cells', () {
    expect(tableFits([List.filled(64, 'a')]), isTrue);
    expect(tableFits([List.filled(65, 'a')]), isFalse);
    expect(tableFits(List.filled(300, List.filled(20, 'a'))), isTrue);
    expect(tableFits(List.filled(300, List.filled(21, 'a'))), isFalse);
  });

  testWidgets('svg is drawn', (t) async {
    await pumpPreview(
      t,
      'logo.svg',
      '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">'
          '<rect width="10" height="10" fill="red"/></svg>',
    );
    expect(find.byType(SvgPicture), findsOneWidget);
  });

  testWidgets('plain source has no render toggle', (t) async {
    await pumpPreview(t, 'main.rs', 'fn main() {}', truncated: true);
    expect(find.byKey(const Key('file-preview-mode')), findsNothing);
    expect(find.byKey(const Key('file-preview-source')), findsOneWidget);
    expect(find.textContaining('limited preview'), findsOneWidget);
  });
}
