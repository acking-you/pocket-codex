import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';

import '../fonts.dart';

/// Edits an argument vector one argument per row.
///
/// Arguments are never joined, split or interpreted: what a row holds is
/// exactly one argument as the agent will receive it, including spaces,
/// quotes, `$VARS` and empty strings.
class ArgvEditor extends StatefulWidget {
  /// Creates the editor seeded with [initial].
  const ArgvEditor({
    super.key,
    required this.initial,
    required this.onChanged,
    this.enabled = true,
  });

  /// Arguments to start with.
  final List<String> initial;

  /// Called with the full vector after every edit.
  final ValueChanged<List<String>> onChanged;

  /// Whether rows can be edited.
  final bool enabled;

  @override
  State<ArgvEditor> createState() => _ArgvEditorState();
}

class _ArgvEditorState extends State<ArgvEditor> {
  final List<TextEditingController> _rows = [];

  @override
  void initState() {
    super.initState();
    _reset(widget.initial);
  }

  @override
  void didUpdateWidget(ArgvEditor old) {
    super.didUpdateWidget(old);
    // A different seed (another preset picked) replaces the rows.
    if (!identical(old.initial, widget.initial)) _reset(widget.initial);
  }

  void _reset(List<String> args) {
    for (final row in _rows) {
      row.dispose();
    }
    _rows
      ..clear()
      ..addAll(args.map((arg) => TextEditingController(text: arg)));
  }

  @override
  void dispose() {
    for (final row in _rows) {
      row.dispose();
    }
    super.dispose();
  }

  void _emit() => widget.onChanged([for (final row in _rows) row.text]);

  void _add() {
    setState(() => _rows.add(TextEditingController()));
    _emit();
  }

  void _remove(int index) {
    setState(() => _rows.removeAt(index).dispose());
    _emit();
  }

  void _move(int from, int to) {
    setState(() => _rows.insert(to, _rows.removeAt(from)));
    _emit();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final small = Theme.of(context).textTheme.bodySmall;
    final enabled = widget.enabled;
    return LayoutBuilder(
      builder: (context, constraints) =>
          _rowsColumn(l10n, small, enabled, constraints.maxWidth >= 340),
    );
  }

  /// [reorder] adds move buttons; narrow phones keep only remove, so the
  /// field stays wide enough to read.
  Widget _rowsColumn(
    AppLocalizations l10n,
    TextStyle? small,
    bool enabled,
    bool reorder,
  ) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(l10n.acpArguments, style: Theme.of(context).textTheme.labelLarge),
        const SizedBox(height: 2),
        Text(l10n.acpArgumentsHint, style: small),
        const SizedBox(height: 6),
        for (var i = 0; i < _rows.length; i++)
          Padding(
            padding: const EdgeInsets.only(bottom: 6),
            child: Row(
              children: [
                Expanded(
                  child: TextField(
                    key: Key('acp-arg-field-$i'),
                    controller: _rows[i],
                    enabled: enabled,
                    autocorrect: false,
                    enableSuggestions: false,
                    style: const TextStyle(
                      fontFamily: monoFontFamily,
                      fontFamilyFallback: monoCjkFallback,
                    ),
                    decoration: InputDecoration(
                      isDense: true,
                      labelText: l10n.acpArgumentLabel(i + 1),
                    ),
                    onChanged: (_) => _emit(),
                  ),
                ),
                if (reorder) ...[
                  IconButton(
                    key: Key('acp-arg-up-$i'),
                    tooltip: l10n.acpArgumentMoveUp,
                    icon: const Icon(Icons.arrow_upward),
                    onPressed: enabled && i > 0 ? () => _move(i, i - 1) : null,
                  ),
                  IconButton(
                    key: Key('acp-arg-down-$i'),
                    tooltip: l10n.acpArgumentMoveDown,
                    icon: const Icon(Icons.arrow_downward),
                    onPressed: enabled && i < _rows.length - 1
                        ? () => _move(i, i + 1)
                        : null,
                  ),
                ],
                IconButton(
                  key: Key('acp-arg-remove-$i'),
                  tooltip: l10n.acpArgumentRemove,
                  icon: const Icon(Icons.close),
                  onPressed: enabled ? () => _remove(i) : null,
                ),
              ],
            ),
          ),
        Align(
          alignment: AlignmentDirectional.centerStart,
          child: TextButton.icon(
            key: const Key('acp-arg-add'),
            onPressed: enabled ? _add : null,
            icon: const Icon(Icons.add),
            label: Text(l10n.acpArgumentAdd),
          ),
        ),
      ],
    );
  }
}
