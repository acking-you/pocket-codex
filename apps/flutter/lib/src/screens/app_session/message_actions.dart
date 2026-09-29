import 'dart:math' as math;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/widgets/app_toast.dart';
import 'package:share_plus/share_plus.dart';

enum _MessageAction { copy, select, edit, share }

/// Mobile message actions, with text selection isolated from the transcript.
class MessageActions extends StatefulWidget {
  /// Wraps a completed message without changing its tap or scroll behavior.
  const MessageActions({
    super.key,
    required this.text,
    required this.isUser,
    required this.child,
    this.completedAtLabel,
    this.onEdit,
  });

  /// A snapshot of the message source to copy, select, or share.
  final String text;

  /// Uses the prompt-sharing label for messages authored by the user.
  final bool isUser;

  /// The existing message, including its links and attachment gestures.
  final Widget child;

  /// Explicitly labels the known turn completion time, not a message timestamp.
  final String? completedAtLabel;

  /// Offers text reuse only when the caller has an editable conversation.
  final VoidCallback? onEdit;

  @override
  State<MessageActions> createState() => _MessageActionsState();
}

class _MessageActionsState extends State<MessageActions> {
  bool _menuOpen = false;

  Future<void> _open(Offset globalPosition) async {
    if (_menuOpen) return;
    _menuOpen = true;
    final text = widget.text;
    final onEdit = widget.onEdit;
    final l10n = AppLocalizations.of(context);
    final overlay =
        Navigator.of(context).overlay!.context.findRenderObject() as RenderBox;
    final message = context.findRenderObject() as RenderBox;
    final edge = overlay.globalToLocal(
      message.localToGlobal(
        Offset(widget.isUser ? message.size.width : 0, message.size.height),
      ),
    );
    final position = edge.dy < overlay.size.height
        ? edge + const Offset(0, 8)
        : overlay.globalToLocal(globalPosition);
    final width = math.min(280.0, overlay.size.width - 32);
    FocusManager.instance.primaryFocus?.unfocus();
    HapticFeedback.selectionClick();
    final action = await showMenu<_MessageAction>(
      context: context,
      position: RelativeRect.fromRect(
        Rect.fromLTWH(position.dx, position.dy, 1, 1),
        Offset.zero & overlay.size,
      ),
      constraints: BoxConstraints(minWidth: width, maxWidth: width),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(kPanelRadius),
      ),
      items: [
        if (widget.completedAtLabel case final label?)
          PopupMenuItem<_MessageAction>(
            enabled: false,
            child: Text(label, style: Theme.of(context).textTheme.bodySmall),
          ),
        _entry(_MessageAction.copy, Icons.content_copy_outlined, l10n.copy),
        _entry(
          _MessageAction.select,
          Icons.text_snippet_outlined,
          l10n.selectText,
        ),
        if (onEdit != null)
          _entry(_MessageAction.edit, Icons.edit_outlined, l10n.editMessage),
        _entry(
          _MessageAction.share,
          Icons.ios_share_outlined,
          widget.isUser ? l10n.sharePrompt : l10n.shareMessage,
        ),
      ],
    );
    _menuOpen = false;
    if (!mounted) return;
    switch (action) {
      case _MessageAction.copy:
        await _copyMessage(context, text);
      case _MessageAction.select:
        await showDialog<void>(
          context: context,
          builder: (_) => _MessageSelection(text: text),
        );
      case _MessageAction.edit:
        // A monitoring refresh can revoke editing while the menu is open.
        if (widget.onEdit != null) onEdit?.call();
      case _MessageAction.share:
        try {
          final size = MediaQuery.sizeOf(context);
          // Anchor to the pressed point, which is visible even for a message
          // taller than the viewport. Clamp again in case the device rotated
          // while the menu was open; iPad requires a nonempty, on-screen rect.
          await SharePlus.instance.share(
            ShareParams(
              text: text,
              sharePositionOrigin: Rect.fromLTWH(
                globalPosition.dx.clamp(0, size.width - 1),
                globalPosition.dy.clamp(0, size.height - 1),
                1,
                1,
              ),
            ),
          );
        } catch (_) {
          if (mounted) showToastError(context, l10n.messageShareFailed);
        }
      case null:
        break;
    }
  }

  PopupMenuItem<_MessageAction> _entry(
    _MessageAction action,
    IconData icon,
    String label,
  ) => PopupMenuItem(
    key: Key('message-action-${action.name}'),
    value: action,
    child: Row(
      children: [
        Icon(icon, size: 22),
        const SizedBox(width: 16),
        Expanded(child: Text(label)),
      ],
    ),
  );

  @override
  Widget build(BuildContext context) {
    final platform = Theme.of(context).platform;
    if (widget.text.trim().isEmpty ||
        (platform != TargetPlatform.android &&
            platform != TargetPlatform.iOS)) {
      return widget.child;
    }
    // The child long-press wins over the enclosing SelectionArea. Keep its
    // selection registration so attached mice can still drag-select text.
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      supportedDevices: const {
        PointerDeviceKind.touch,
        PointerDeviceKind.stylus,
        PointerDeviceKind.invertedStylus,
      },
      onLongPressStart: (details) => _open(details.globalPosition),
      child: widget.child,
    );
  }
}

Future<void> _copyMessage(BuildContext context, String text) async {
  final l10n = AppLocalizations.of(context);
  try {
    await Clipboard.setData(ClipboardData(text: text));
    if (context.mounted) showToastOk(context, l10n.copied);
  } catch (_) {
    if (context.mounted) showToastError(context, l10n.messageCopyFailed);
  }
}

class _MessageSelection extends StatelessWidget {
  const _MessageSelection({required this.text});

  final String text;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Dialog.fullscreen(
      child: Scaffold(
        appBar: AppBar(
          leading: CloseButton(onPressed: () => Navigator.of(context).pop()),
          title: Text(l10n.selectText),
          actions: [
            IconButton(
              tooltip: l10n.copy,
              onPressed: () => _copyMessage(context, text),
              icon: const Icon(Icons.content_copy_outlined),
            ),
          ],
        ),
        body: SafeArea(
          top: false,
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(20),
            child: SelectableText(
              text,
              key: const Key('message-selectable-text'),
              style: Theme.of(
                context,
              ).textTheme.bodyLarge?.copyWith(height: 1.6),
            ),
          ),
        ),
      ),
    );
  }
}
