import 'dart:convert';
import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/file_exports.dart';
import 'package:pocket_codex/src/file_links.dart';
import 'package:pocket_codex/src/widgets/app_toast.dart';
import 'package:pocket_codex/src/widgets/links.dart';

const _previewLimit = 8 * 1024 * 1024;
const _textLimit = 1024 * 1024;

/// Captures the host and session for each explicit transcript link action.
class SessionFileLinks extends StatelessWidget {
  const SessionFileLinks({
    super.key,
    required this.api,
    required this.serviceKey,
    required this.threadId,
    required this.cwd,
    required this.child,
  });
  final BridgeApi api;
  final String serviceKey;
  final String? threadId;
  final String? cwd;
  final Widget child;

  @override
  Widget build(BuildContext context) => SessionLinkScope(
    open: (context, href) async {
      final uri = Uri.tryParse(href.trim());
      if (uri != null && isHostLocalWebUrl(uri)) {
        await showDialog<void>(
          context: context,
          builder: (_) =>
              _HostWebLink(api: api, serviceKey: serviceKey, url: href),
        );
        return true;
      }
      final link = FileLink.parse(href, cwd: cwd);
      if (link == null) return false;
      await showDialog<void>(
        context: context,
        builder: (_) => _FileLinkDialog(
          api: api,
          serviceKey: serviceKey,
          threadId: threadId,
          link: link,
        ),
      );
      return true;
    },
    child: child,
  );
}

class _HostWebLink extends StatefulWidget {
  const _HostWebLink({
    required this.api,
    required this.serviceKey,
    required this.url,
  });
  final BridgeApi api;
  final String serviceKey;
  final String url;
  @override
  State<_HostWebLink> createState() => _HostWebLinkState();
}

class _HostWebLinkState extends State<_HostWebLink> {
  bool _checking = true;
  @override
  void initState() {
    super.initState();
    _check();
  }

  Future<void> _check() async {
    var local = false;
    try {
      local = await widget.api.metaHostIsLocal(widget.serviceKey);
    } catch (_) {
      /* Unverified hosts stay remote. */
    }
    if (!mounted) return;
    if (local) {
      await openWebUrl(context, widget.url);
      if (mounted) Navigator.of(context).pop();
    } else {
      setState(() => _checking = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return AlertDialog(
      title: Text(l10n.fileLinkRemoteWebTitle),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SelectableText(widget.url),
            const SizedBox(height: 16),
            if (_checking) ...[
              const LinearProgressIndicator(),
              const SizedBox(height: 8),
              Text(l10n.fileLinkCheckingHost),
            ] else
              Text(l10n.fileLinkRemoteWeb),
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Clipboard.setData(ClipboardData(text: widget.url)),
          child: Text(l10n.copy),
        ),
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(l10n.fileLinkClose),
        ),
      ],
    );
  }
}

class _FileLinkDialog extends StatefulWidget {
  const _FileLinkDialog({
    required this.api,
    required this.serviceKey,
    required this.threadId,
    required this.link,
  });
  final BridgeApi api;
  final String serviceKey;
  final String? threadId;
  final FileLink link;
  @override
  State<_FileLinkDialog> createState() => _FileLinkDialogState();
}

class _FileLinkDialogState extends State<_FileLinkDialog> {
  bool _busy = false;
  String? _error;
  FilePreviewData? _preview;
  Future<bool>? _local;
  bool _retryDownload = false;
  Future<bool> _isLocal() => _local ??= widget.api
      .metaHostIsLocal(widget.serviceKey)
      .catchError((_) => false);

  Future<void> _run(bool download) async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
      _retryDownload = download;
    });
    final l10n = AppLocalizations.of(context);
    final messenger = ToastMessenger.of(context);
    Directory? staging;
    try {
      final local = await _isLocal();
      if (!mounted) return;
      if (download) {
        String source;
        if (local) {
          source = widget.link.path;
        } else {
          staging = await Directory.systemTemp.createTemp('pcx-download-');
          source = '${staging.path}/content';
          await widget.api.metaFileDownload(
            widget.serviceKey,
            widget.threadId,
            widget.link.href,
            source,
          );
        }
        if (!mounted) return;
        final saved = await exportFile(source, widget.link.name);
        if (saved != null) messenger.ok(l10n.fileDownloaded(saved));
      } else {
        FilePreviewData preview;
        if (local) {
          final file = await File(widget.link.path).open();
          try {
            final size = await file.length();
            preview = FilePreviewData(
              bytes: await file.read(math.min(size, _previewLimit)),
              totalSize: size,
            );
          } finally {
            await file.close();
          }
        } else {
          preview = await widget.api.metaFilePreview(
            widget.serviceKey,
            widget.threadId,
            widget.link.href,
          );
        }
        if (mounted) setState(() => _preview = preview);
      }
    } catch (error) {
      if (mounted) setState(() => _error = '${l10n.fileLinkFailed}\n$error');
    } finally {
      try {
        await staging?.delete(recursive: true);
      } catch (_) {
        /* The OS may have removed temporary files. */
      }
      if (mounted) setState(() => _busy = false);
    }
  }

  Widget _content(FilePreviewData preview) {
    final l10n = AppLocalizations.of(context);
    final ext = widget.link.name.split('.').last.toLowerCase();
    final image = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp'].contains(ext);
    if (image && !preview.truncated) {
      return InteractiveViewer(
        child: Image.memory(
          preview.bytes,
          fit: BoxFit.contain,
          errorBuilder: (_, _, _) => Text(l10n.fileLinkUnsupported),
        ),
      );
    }
    if (image ||
        preview.bytes.take(8192).contains(0) ||
        ['pdf', 'doc', 'docx', 'xlsx', 'pptx', 'zip'].contains(ext)) {
      return Text(l10n.fileLinkUnsupported);
    }
    String text;
    try {
      text = utf8.decode(
        preview.bytes.take(_textLimit).toList(),
        allowMalformed: preview.truncated || preview.bytes.length > _textLimit,
      );
    } on FormatException {
      {
        return Text(l10n.fileLinkUnsupported);
      }
    }
    final lines = text.split('\n');
    final truncated = preview.truncated || preview.bytes.length > _textLimit;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (truncated)
          Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: Text(l10n.fileLinkTruncated),
          ),
        Expanded(
          child: SelectionArea(
            child: ListView.builder(
              itemCount: lines.length,
              itemBuilder: (context, index) => Text(
                lines[index],
                style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
              ),
            ),
          ),
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return AlertDialog(
      title: Text(
        widget.link.name,
        maxLines: 2,
        overflow: TextOverflow.ellipsis,
      ),
      content: SizedBox(
        width: 720,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              ConstrainedBox(
                constraints: const BoxConstraints(maxHeight: 96),
                child: SingleChildScrollView(
                  child: SelectableText(widget.link.path),
                ),
              ),
              const SizedBox(height: 12),
              if (_busy) ...[
                const LinearProgressIndicator(),
                Text(l10n.fileLinkLoading),
              ],
              if (_error != null) ...[
                Text(_error!),
                TextButton(
                  onPressed: _busy ? null : () => _run(_retryDownload),
                  child: Text(l10n.retry),
                ),
              ],
              if (_preview case final preview?)
                SizedBox(
                  height: math.min(
                    480,
                    MediaQuery.sizeOf(context).height * 0.45,
                  ),
                  child: _content(preview),
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(l10n.fileLinkClose),
        ),
        TextButton(
          onPressed: _busy ? null : () => _run(false),
          child: Text(l10n.fileLinkPreview),
        ),
        FilledButton.icon(
          onPressed: _busy ? null : () => _run(true),
          icon: const Icon(Icons.download),
          label: Text(l10n.fileLinkDownload),
        ),
      ],
    );
  }
}
