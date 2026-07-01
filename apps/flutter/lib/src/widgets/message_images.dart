import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/image_attachments.dart';

/// One message attachment resolved from its wire URL: either renderable
/// pixels (a `data:image/...` URL, decoded once here) or a host-side file we
/// can only name (a `localImage` path sent by a codex client on the host —
/// its pixels never crossed the wire, so an honest filename chip is all a
/// remote controller can show).
class ResolvedImage {
  ResolvedImage._({this.bytes, this.hostPath});

  /// Decoded image bytes for a data URL; null for a host path.
  final Uint8List? bytes;

  /// Host filesystem path for a `localImage`; null for a data URL.
  final String? hostPath;

  /// Basename of [hostPath] for the chip label.
  String get hostName {
    final p = hostPath ?? '';
    final cut = p.lastIndexOf(RegExp(r'[/\\]'));
    return cut < 0 ? p : p.substring(cut + 1);
  }
}

/// Resolve wire URLs into renderable attachments, decoding each base64
/// payload exactly once (decoding per rebuild would jank the list).
/// Undecodable data URLs are dropped; non-data URLs become host-path chips.
List<ResolvedImage> resolveImageUrls(List<String> urls) {
  final out = <ResolvedImage>[];
  for (final url in urls) {
    if (url.startsWith('data:')) {
      final bytes = decodeImageDataUrl(url);
      if (bytes != null) out.add(ResolvedImage._(bytes: bytes));
    } else {
      out.add(ResolvedImage._(hostPath: url));
    }
  }
  return out;
}

/// Thumbnail strip for a message's image attachments. Tapping a thumbnail
/// opens the fullscreen [ImageViewerPage] (swipe between the message's
/// images, pinch to zoom). Host-only images render as filename chips.
class MessageImagesView extends StatelessWidget {
  /// Creates the strip for [images] (resolved once by the item model).
  const MessageImagesView({super.key, required this.images});

  /// The message's resolved attachments, in send order.
  final List<ResolvedImage> images;

  @override
  Widget build(BuildContext context) {
    final renderable = [
      for (final i in images)
        if (i.bytes != null) i.bytes!,
    ];
    // A single image gets a larger preview; several tile as uniform squares.
    final side = renderable.length == 1 ? 180.0 : 96.0;
    var bytesIndex = 0;
    return Wrap(
      spacing: 6,
      runSpacing: 6,
      children: [
        for (final image in images)
          if (image.bytes != null)
            _thumb(context, renderable, bytesIndex++, side)
          else
            _hostChip(context, image),
      ],
    );
  }

  Widget _thumb(
    BuildContext context,
    List<Uint8List> renderable,
    int index,
    double side,
  ) {
    final scale = MediaQuery.of(context).devicePixelRatio;
    return ClipRRect(
      borderRadius: BorderRadius.circular(10),
      child: Stack(
        children: [
          Image.memory(
          renderable[index],
          key: Key('msg-image-$index'),
          width: side,
          height: side,
          fit: BoxFit.cover,
          // Decode near thumbnail resolution — full-res frames for every
          // thumb would hold megabytes of pixels per message. 2× the box so a
          // landscape image's SHORT edge still reaches the square cover box
          // (cacheWidth alone would decode it too short and upscale blurry).
          cacheWidth: (side * scale * 2).round(),
          gaplessPlayback: true,
          errorBuilder: (context, _, _) => SizedBox(
            width: side,
            height: side,
            child: Icon(
              Icons.broken_image_outlined,
              color: Theme.of(context).colorScheme.outline,
            ),
          ),
          ),
          // A local transparent Material ON TOP of the opaque image so the
          // tap ripple is actually visible (ink on the distant Scaffold
          // Material would paint underneath the bubble and image).
          Positioned.fill(
            child: Material(
              type: MaterialType.transparency,
              child: InkWell(
                onTap: () => ImageViewerPage.show(context, renderable, index),
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _hostChip(BuildContext context, ResolvedImage image) {
    final scheme = Theme.of(context).colorScheme;
    return Tooltip(
      message: AppLocalizations.of(context).imageOnHost(image.hostPath ?? ''),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
        decoration: BoxDecoration(
          color: scheme.surfaceContainerHighest,
          borderRadius: BorderRadius.circular(10),
          border: Border.all(color: scheme.outlineVariant),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(
              Icons.image_outlined,
              size: 16,
              color: scheme.onSurfaceVariant,
            ),
            const SizedBox(width: 6),
            ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 180),
              child: Text(
                image.hostName,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.bodySmall,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Fullscreen image viewer: swipe (PageView) between a message's images,
/// pinch/scroll to zoom (InteractiveViewer), save-to-file on desktop.
class ImageViewerPage extends StatefulWidget {
  /// Creates the viewer over [images] starting at [initialIndex].
  const ImageViewerPage({
    super.key,
    required this.images,
    required this.initialIndex,
  });

  /// Decoded image bytes, in message order.
  final List<Uint8List> images;

  /// Which image to show first.
  final int initialIndex;

  /// Push the viewer as a fullscreen dialog route.
  static Future<void> show(
    BuildContext context,
    List<Uint8List> images,
    int initialIndex,
  ) => Navigator.of(context).push(
    MaterialPageRoute<void>(
      fullscreenDialog: true,
      builder: (_) =>
          ImageViewerPage(images: images, initialIndex: initialIndex),
    ),
  );

  @override
  State<ImageViewerPage> createState() => _ImageViewerPageState();
}

class _ImageViewerPageState extends State<ImageViewerPage> {
  late final PageController _pager = PageController(
    initialPage: widget.initialIndex,
  );
  late int _index = widget.initialIndex;

  @override
  void dispose() {
    _pager.dispose();
    super.dispose();
  }

  /// Save is desktop-only: file_selector's save dialog has no mobile
  /// implementation (mobile saving would need a gallery plugin).
  bool get _canSave =>
      !kIsWeb &&
      (defaultTargetPlatform == TargetPlatform.windows ||
          defaultTargetPlatform == TargetPlatform.macOS ||
          defaultTargetPlatform == TargetPlatform.linux);

  Future<void> _save() async {
    final l10n = AppLocalizations.of(context);
    final messenger = ScaffoldMessenger.of(context);
    final bytes = widget.images[_index];
    // Sniff the real container (small GIF/WebP originals pass through
    // byte-identical) so the suggested extension is honest.
    final location = await getSaveLocation(
      suggestedName: 'image-${_index + 1}.${sniffImageExtension(bytes)}',
    );
    if (location == null) return;
    try {
      await File(location.path).writeAsBytes(bytes);
      messenger.showSnackBar(
        SnackBar(content: Text(l10n.imageSaved(location.path))),
      );
    } catch (e) {
      messenger.showSnackBar(
        SnackBar(content: Text(l10n.imageSaveFailed('$e'))),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Scaffold(
      backgroundColor: Colors.black,
      appBar: AppBar(
        backgroundColor: Colors.black,
        foregroundColor: Colors.white,
        title: widget.images.length > 1
            ? Text('${_index + 1}/${widget.images.length}')
            : null,
        actions: [
          if (_canSave)
            IconButton(
              key: const Key('image-save-btn'),
              tooltip: l10n.imageSave,
              icon: const Icon(Icons.download_outlined),
              onPressed: _save,
            ),
        ],
      ),
      body: PageView.builder(
        controller: _pager,
        itemCount: widget.images.length,
        onPageChanged: (i) => setState(() => _index = i),
        itemBuilder: (context, i) => InteractiveViewer(
          key: Key('image-viewer-$i'),
          minScale: 0.5,
          maxScale: 6,
          child: Center(
            child: Image.memory(widget.images[i], gaplessPlayback: true),
          ),
        ),
      ),
    );
  }
}
