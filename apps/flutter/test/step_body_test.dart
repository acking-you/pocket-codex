import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/screens/app_session/step_body.dart';
import 'package:pocket_codex/src/screens/app_session/transcript_model.dart';
import 'package:pocket_codex/src/widgets/code_block.dart';

Future<void> pumpStep(WidgetTester t, TranscriptItem item) async {
  await t.pumpWidget(
    MaterialApp(
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      locale: const Locale('en'),
      home: Scaffold(
        body: SingleChildScrollView(child: StepBody(item: item)),
      ),
    ),
  );
  await t.pump();
}

void main() {
  test('splitExitCode lifts the trailing marker only', () {
    expect(splitExitCode('a\nb\n[exit 2]'), (output: 'a\nb', exitCode: 2));
    expect(splitExitCode('[exit 0]'), (output: '', exitCode: 0));
    expect(splitExitCode('says [exit 1] mid-line'), (
      output: 'says [exit 1] mid-line',
      exitCode: null,
    ));
  });

  test('commandLanguage picks PowerShell only when the host ran it', () {
    expect(commandLanguage('ls -la'), 'bash');
    expect(commandLanguage('pwsh -Command Get-ChildItem'), 'powershell');
    expect(commandLanguage('"C:\\…\\powershell.exe" -NoProfile'), 'powershell');
  });

  test('toolResultText reads MCP and dynamic-tool content items', () {
    expect(
      toolResultText({
        'content': [
          {'type': 'text', 'text': 'one'},
          {'type': 'text', 'text': 'two'},
        ],
      }),
      'one\n\ntwo',
    );
    expect(
      toolResultText([
        {'type': 'inputText', 'text': 'x'},
      ]),
      'x',
    );
    // An image item has no text to lift, so the whole value stays JSON.
    expect(
      toolResultText({
        'content': [
          {'type': 'image', 'data': '…'},
        ],
      }),
      isNull,
    );
  });

  testWidgets('a command shows its line highlighted, output and exit chip', (
    t,
  ) async {
    await pumpStep(
      t,
      TranscriptItem(
        id: 'c1',
        type: 'commandExecution',
        title: 'cargo test',
        text: 'running 3 tests\nfailed\n[exit 101]',
      ),
    );
    final command = t.widget<CodeBlock>(find.byKey(const Key('step-command')));
    expect(command.code, 'cargo test');
    expect(command.language, 'bash');
    final output = t.widget<CodeBlock>(find.byKey(const Key('step-output')));
    // The marker becomes the chip, not a line of the output.
    expect(output.code, 'running 3 tests\nfailed');
    expect(find.text('exit 101'), findsOneWidget);
  });

  testWidgets(
    'a bounded block on desktop does not borrow the page scrollbar',
    (t) async {
      // Desktop gives every page a PrimaryScrollController; a block's bare
      // Scrollbar used to latch onto it and throw once it was detached.
      await pumpStep(
        t,
        TranscriptItem(
          id: 'c2',
          type: 'commandExecution',
          title: 'ls',
          text: List.generate(80, (i) => 'line $i').join('\n'),
        ),
      );
      // Scrolling the block is what makes its scrollbar resolve a controller.
      await t.drag(find.byKey(const Key('step-output')), const Offset(0, -120));
      await t.pump(const Duration(milliseconds: 400));
      expect(t.takeException(), isNull);
      await t.pumpWidget(const SizedBox());
      expect(t.takeException(), isNull);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
  );

  testWidgets('a tool call splits into its fields, JSON highlighted', (
    t,
  ) async {
    await pumpStep(
      t,
      TranscriptItem(
        id: 'm1',
        type: 'mcpToolCall',
        title: 'docs.search',
        text:
            '{"arguments":{"q":"flutter"},'
            '"result":{"content":[{"type":"text","text":"{\\"hits\\":2}"}]}}',
      ),
    );
    final blocks = t.widgetList<CodeBlock>(find.byType(CodeBlock)).toList();
    expect(blocks.map((b) => b.label), ['arguments', 'result']);
    expect(blocks[0].language, 'json');
    expect(blocks[0].code, contains('"q": "flutter"'));
    // The result's text is itself JSON, so it is lifted and pretty-printed.
    expect(blocks[1].language, 'json');
    expect(blocks[1].code, contains('"hits": 2'));
  });
}
