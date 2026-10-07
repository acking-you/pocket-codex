/// The expanded body of a tool step: what a command ran and printed, what a
/// tool was called with and returned, laid out by what the step is rather than
/// as one undifferentiated dump.
library;

import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/screens/app_session/transcript_model.dart';
import 'package:pocket_codex/src/widgets/code_block.dart';
import 'package:pocket_codex/src/widgets/links.dart';

/// Step types whose detail is a JSON object of the item's fields (the bridge
/// serialises a chosen subset for them).
const _jsonSteps = {
  'mcpToolCall',
  'dynamicToolCall',
  'collabAgentToolCall',
  'webSearch',
  'subAgentActivity',
  'imageGeneration',
};

/// The highlighter grammar for a command line: PowerShell when the host ran
/// one, a POSIX shell otherwise.
String commandLanguage(String command) =>
    RegExp(r'\b(powershell|pwsh)(\.exe)?\b', caseSensitive: false)
        .hasMatch(command)
    ? 'powershell'
    : 'bash';

/// Splits the bridge's trailing `[exit N]` marker off a command's output.
({String output, int? exitCode}) splitExitCode(String detail) {
  final m = RegExp(r'(?:^|\n)\[exit (-?\d+)\]\s*$').firstMatch(detail);
  if (m == null) return (output: detail, exitCode: null);
  return (
    output: detail.substring(0, m.start).trimRight(),
    exitCode: int.tryParse(m.group(1)!),
  );
}

/// [raw] decoded when it is a JSON object or array, else null.
Object? _tryJson(String raw) {
  final t = raw.trim();
  if (!(t.startsWith('{') || t.startsWith('['))) return null;
  try {
    return jsonDecode(t);
  } catch (_) {
    return null;
  }
}

String _pretty(Object? value) =>
    const JsonEncoder.withIndent('  ').convert(value);

/// The text a tool result carries, when it is the common "content items"
/// shape (`{content: [{type: text, text}]}` from MCP, or a bare list of
/// `{text}` items from a dynamic tool). Null when there is no text to lift.
String? toolResultText(Object? value) {
  final items = switch (value) {
    {'content': final List<Object?> c} => c,
    final List<Object?> l => l,
    _ => null,
  };
  if (items == null || items.isEmpty) return null;
  final texts = <String>[];
  for (final item in items) {
    if (item is! Map || item['text'] is! String) return null;
    texts.add(item['text'] as String);
  }
  return texts.join('\n\n');
}

/// The expanded body for one step, picked by its type.
class StepBody extends StatelessWidget {
  /// Lays out [item]'s title and detail.
  const StepBody({super.key, required this.item});

  /// The step.
  final TranscriptItem item;

  @override
  Widget build(BuildContext context) {
    final title = item.title.trim();
    final detail = item.text.trim();
    final children = switch (item.type) {
      'commandExecution' => _command(context, title, detail),
      final t when _jsonSteps.contains(t) => _tool(context, title, detail),
      _ => [_plain(context, [title, detail].where((s) => s.isNotEmpty))],
    };
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: children,
    );
  }

  List<Widget> _command(BuildContext context, String command, String detail) {
    final l10n = AppLocalizations.of(context);
    final split = splitExitCode(detail);
    final output = split.output;
    final language = commandLanguage(command);
    return [
      if (command.isNotEmpty)
        CodeBlock(
          key: const Key('step-command'),
          code: command,
          language: language,
          label: language == 'powershell' ? 'powershell' : 'shell',
          maxHeight: 160,
          margin: const EdgeInsets.only(top: 2, bottom: 6),
        ),
      if (output.isNotEmpty || split.exitCode != null)
        CodeBlock(
          key: const Key('step-output'),
          code: output,
          // Output is literal; only a whole JSON document is worth colouring.
          language: _tryJson(output) == null ? '' : 'json',
          label: l10n.stepOutput,
          maxHeight: 280,
          margin: const EdgeInsets.only(bottom: 6),
          trailing: [
            if (split.exitCode case final code?) _ExitChip(code: code),
          ],
        ),
    ];
  }

  List<Widget> _tool(BuildContext context, String title, String detail) {
    final decoded = _tryJson(detail);
    if (decoded is! Map<String, Object?>) {
      return [
        if (detail.isNotEmpty)
          CodeBlock(
            code: detail,
            language: decoded == null ? '' : 'json',
            maxHeight: 320,
            margin: const EdgeInsets.only(top: 2, bottom: 6),
          ),
      ];
    }
    // One section per field the bridge kept (arguments, result, error…), so
    // the call's input and its answer are separate things to read and copy.
    return [
      for (final MapEntry(:key, :value) in decoded.entries)
        _field(context, key, value),
    ];
  }

  Widget _field(BuildContext context, String key, Object? value) {
    const margin = EdgeInsets.only(top: 2, bottom: 6);
    final text = value is String ? value : toolResultText(value);
    if (text != null) {
      final nested = _tryJson(text);
      return CodeBlock(
        code: nested == null ? text : _pretty(nested),
        language: nested == null ? '' : 'json',
        label: key,
        maxHeight: 280,
        margin: margin,
      );
    }
    return CodeBlock(
      code: _pretty(value),
      language: 'json',
      label: key,
      maxHeight: 280,
      margin: margin,
    );
  }

  Widget _plain(BuildContext context, Iterable<String> parts) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      padding: const EdgeInsets.all(11),
      margin: const EdgeInsets.only(top: 2, bottom: 6),
      constraints: const BoxConstraints(maxHeight: 320),
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(8),
        color: scheme.surfaceContainerLow,
        border: Border.all(color: scheme.outlineVariant),
      ),
      child: SingleChildScrollView(
        primary: false,
        child: linkifyText(
          context,
          parts.join('\n\n'),
          selectable: true,
          style: const TextStyle(
            fontFamily: monoFontFamily,
            fontFamilyFallback: monoCjkFallback,
            fontSize: 12,
            height: 1.45,
          ),
        ),
      ),
    );
  }
}

/// A command's exit status: quiet when it succeeded, the error colour when
/// it did not.
class _ExitChip extends StatelessWidget {
  const _ExitChip({required this.code});
  final int code;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final ok = code == 0;
    final fg = ok ? scheme.onSurfaceVariant : scheme.error;
    return Container(
      key: const Key('step-exit'),
      margin: const EdgeInsets.only(right: 2),
      padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 1),
      decoration: BoxDecoration(
        color: ok
            ? scheme.surfaceContainerHighest
            : scheme.errorContainer.withValues(alpha: 0.6),
        borderRadius: BorderRadius.circular(99),
      ),
      child: Text(
        'exit $code',
        style: TextStyle(
          fontFamily: monoFontFamily,
          fontSize: 10.5,
          fontWeight: FontWeight.w600,
          color: fg,
        ),
      ),
    );
  }
}
