import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';

/// A full-screen editing surface sharing the compact composer's draft.
class ExpandedComposer extends StatelessWidget {
  /// Opens an editor using the caller's controller and focus node.
  const ExpandedComposer({
    super.key,
    required this.controller,
    required this.focusNode,
  });

  /// The shared draft, including its current selection.
  final TextEditingController controller;

  /// Focus owned and disposed by the session screen.
  final FocusNode focusNode;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Dialog.fullscreen(
      child: SafeArea(
        child: Column(
          children: [
            AppBar(
              automaticallyImplyLeading: false,
              title: Text(l10n.editMessage),
              actions: [
                TextButton(
                  key: const Key('composer-editor-done'),
                  onPressed: () => Navigator.of(context).pop(),
                  child: Text(l10n.done),
                ),
                const SizedBox(width: 8),
              ],
            ),
            Expanded(
              child: Padding(
                padding: const EdgeInsets.all(20),
                child: TextField(
                  key: const Key('composer-expanded-input'),
                  controller: controller,
                  focusNode: focusNode,
                  autofocus: true,
                  expands: true,
                  minLines: null,
                  maxLines: null,
                  textAlignVertical: TextAlignVertical.top,
                  keyboardType: TextInputType.multiline,
                  textInputAction: TextInputAction.newline,
                  decoration: InputDecoration(
                    hintText: l10n.messageHint,
                    border: InputBorder.none,
                    filled: false,
                    contentPadding: EdgeInsets.zero,
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
