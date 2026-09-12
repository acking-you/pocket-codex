import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/widgets/adaptive_sheet.dart';
import 'package:pocket_codex/src/widgets/turn_minimap.dart';

/// A searchable, virtualized index; opening or searching it never loads history.
Future<TurnMinimapItem?> showTurnOutline(
  BuildContext context, {
  required List<TurnMinimapItem> items,
  int current = 0,
}) => showAdaptivePanel<TurnMinimapItem>(
  context: context,
  scrollable: false,
  maxWidth: 560,
  builder: (_) => _TurnOutline(items: items, current: current),
);

class _TurnOutline extends StatefulWidget {
  const _TurnOutline({required this.items, required this.current});
  final List<TurnMinimapItem> items;
  final int current;
  @override
  State<_TurnOutline> createState() => _TurnOutlineState();
}

class _TurnOutlineState extends State<_TurnOutline> {
  late final _scroll = ScrollController(
    initialScrollOffset: widget.current * 76.0,
  );
  String _query = '';
  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final number = int.tryParse(_query);
    final matches = [
      for (var i = 0; i < widget.items.length; i++)
        if (_query.isEmpty ||
            (number != null
                ? number == i + 1
                : widget.items[i].userText.toLowerCase().contains(_query) ||
                      (widget.items[i].assistantText?.toLowerCase().contains(
                            _query,
                          ) ??
                          false)))
          i,
    ];
    final height = math.max(
      180.0,
      math.min(
        560.0,
        MediaQuery.sizeOf(context).height * .7 -
            MediaQuery.viewInsetsOf(context).bottom,
      ),
    );
    return SizedBox(
      height: height,
      child: Column(
        children: [
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    l10n.conversationOutline,
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                ),
                IconButton(
                  tooltip: MaterialLocalizations.of(context).closeButtonTooltip,
                  onPressed: () => Navigator.pop(context),
                  icon: const Icon(Icons.close),
                ),
              ],
            ),
          ),
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 4, 16, 10),
            child: TextField(
              key: const Key('turn-outline-search'),
              decoration: InputDecoration(
                prefixIcon: const Icon(Icons.search),
                hintText: l10n.searchTurns,
              ),
              onChanged: (value) {
                setState(() => _query = value.trim().toLowerCase());
                if (_scroll.hasClients) _scroll.jumpTo(0);
              },
            ),
          ),
          Expanded(
            child: matches.isEmpty
                ? Center(child: Text(l10n.noMatchingTurns))
                : ListView.builder(
                    key: const Key('turn-outline-list'),
                    controller: _scroll,
                    itemExtent: 76,
                    itemCount: matches.length,
                    itemBuilder: (_, at) {
                      final index = matches[at];
                      final item = widget.items[index];
                      return ListTile(
                        key: ValueKey('turn-outline-$index'),
                        selected: index == widget.current,
                        leading: SizedBox(
                          width: 44,
                          child: Text(
                            '${index + 1}',
                            textAlign: TextAlign.right,
                            style: TextStyle(color: scheme.onSurfaceVariant),
                          ),
                        ),
                        title: Text(
                          item.userText.isEmpty
                              ? l10n.turnPosition(
                                  index + 1,
                                  widget.items.length,
                                )
                              : item.userText,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                        ),
                        subtitle: Text(
                          item.assistantText ?? '',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                        ),
                        onTap: () => Navigator.pop(context, item),
                      );
                    },
                  ),
          ),
        ],
      ),
    );
  }
}
