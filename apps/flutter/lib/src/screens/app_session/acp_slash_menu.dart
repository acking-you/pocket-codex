/// Slash commands an ACP agent offers (TRD §4.7.5), listed above the composer
/// while its text starts with `/`.
library;

import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';

/// Watches [controller]; while the text is `/` plus a command prefix it shows
/// the matching commands, and choosing one replaces the text with
/// `/{name} `. Commands are re-read when [revision] changes.
class AcpSlashMenu extends ConsumerStatefulWidget {
  /// Creates the menu.
  const AcpSlashMenu({
    super.key,
    required this.serviceKey,
    required this.threadId,
    required this.controller,
    this.revision,
  });

  /// ACP service.
  final String serviceKey;

  /// Session whose commands are listed.
  final String threadId;

  /// The composer's text.
  final TextEditingController controller;

  /// Bumped on `acp/config/updated` (commands changed).
  final ValueListenable<int>? revision;

  @override
  ConsumerState<AcpSlashMenu> createState() => _AcpSlashMenuState();
}

class _AcpSlashMenuState extends ConsumerState<AcpSlashMenu> {
  List<AcpCommand>? _commands;
  bool _loading = false;

  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_onText);
    widget.revision?.addListener(_invalidate);
  }

  @override
  void didUpdateWidget(AcpSlashMenu old) {
    super.didUpdateWidget(old);
    if (old.controller != widget.controller) {
      old.controller.removeListener(_onText);
      widget.controller.addListener(_onText);
    }
    if (old.revision != widget.revision) {
      old.revision?.removeListener(_invalidate);
      widget.revision?.addListener(_invalidate);
    }
    if (old.threadId != widget.threadId) _commands = null;
  }

  @override
  void dispose() {
    widget.controller.removeListener(_onText);
    widget.revision?.removeListener(_invalidate);
    super.dispose();
  }

  void _invalidate() => _commands = null;

  /// The command prefix being typed, or null when the menu should hide.
  String? get _query {
    final text = widget.controller.text;
    if (!text.startsWith('/') || text.contains(RegExp(r'\s'))) return null;
    return text.substring(1).toLowerCase();
  }

  void _onText() {
    if (_query == null) {
      if (mounted) setState(() {});
      return;
    }
    if (_commands == null && !_loading) {
      _loading = true;
      ref
          .read(bridgeApiProvider)
          .appSlashCommands(widget.serviceKey, widget.threadId)
          .then((c) => _commands = c, onError: (_) => _commands = const [])
          .whenComplete(() {
            _loading = false;
            if (mounted) setState(() {});
          });
    }
    if (mounted) setState(() {});
  }

  void _choose(AcpCommand command) {
    final text = '/${command.name} ';
    widget.controller.value = TextEditingValue(
      text: text,
      selection: TextSelection.collapsed(offset: text.length),
    );
  }

  @override
  Widget build(BuildContext context) {
    final query = _query;
    final commands = _commands;
    if (query == null || commands == null) return const SizedBox.shrink();
    final matches = [
      for (final c in commands)
        if (c.name.toLowerCase().startsWith(query)) c,
    ];
    if (matches.isEmpty) return const SizedBox.shrink();
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      key: const Key('acp-slash-menu'),
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
      child: Material(
        color: scheme.surfaceContainerHigh,
        shape: RoundedRectangleBorder(
          side: BorderSide(color: scheme.outlineVariant, width: 0.5),
          borderRadius: BorderRadius.circular(10),
        ),
        clipBehavior: Clip.antiAlias,
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 220),
          child: ListView(
            shrinkWrap: true,
            padding: const EdgeInsets.symmetric(vertical: 4),
            children: [
              for (final c in matches)
                ListTile(
                  key: Key('acp-slash-${c.name}'),
                  dense: true,
                  title: Text('/${c.name}'),
                  subtitle: Text(
                    [c.description, if (c.hint != null) c.hint!].join(' · '),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                  onTap: () => _choose(c),
                ),
            ],
          ),
        ),
      ),
    );
  }
}
