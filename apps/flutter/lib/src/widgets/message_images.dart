import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/attachment_refs.dart';
import 'package:pocket_codex/src/image_attachments.dart';

/// One message attachment resolved from its wire URL: either renderable
/// pixels (a `data:image/...` URL, decoded once here) or a host-side file we
/// can only name (a `localImage` path sent by a codex client on the host —
/// its pixels never crossed the wire, so an honest filename chip is all a
/// remote controller can show).
class ResolvedImage {
  ResolvedImage._({this.bytes, this.hostPath, this.broken = false});

  /// Decoded image bytes for a data URL; null for a host path or broken image.
  final Uint8List? bytes;

  /// Host filesystem path for a `localImage`; null for a data URL.
  final String? hostPath;

  /// True for a `data:image/...` URL whose payload failed to decode — kept
  /// (not dropped) so the strip can show an honest "couldn't load" placeholder
  /// instead of silently hiding an attachment the sender meant to include.
  final bool broken;

  /// Basename of [hostPath] for the chip label.
  String get hostName {
    final p = hostPath ?? '';
    final cut = p.lastIndexOf(RegExp(r'[/\\]'));
    return cut < 0 ? p : p.substring(cut + 1);
  }
}

/// Resolve wire URLs into renderable attachments, decoding each base64
/// payload exactly once (decoding per rebuild would jank the list).
/// An undecodable data URL becomes a `broken` placeholder (kept, not dropped);
/// non-data URLs become host-path chips.
List<ResolvedImage> resolveImageUrls(List<String> urls) {
  final out = <ResolvedImage>[];
  for (final url in urls) {
    if (url.startsWith('data:')) {
      final bytes = decodeImageDataUrl(url);
      out.add(
        bytes != null
            ? ResolvedImage._(bytes: bytes)
            : ResolvedImage._(broken: true),
      );
    } else {
      out.add(ResolvedImage._(hostPath: url));
    }
  }
  return out;
}

/// Whether the save-to-file dialog is available. Desktop-only: `file_selector`
/// has no mobile save implementation (mobile saving would need a gallery
/// plugin).
bool get canSaveImages =>
    !kIsWeb &&
    (defaultTargetPlatform == TargetPlatform.windows ||
        defaultTargetPlatform == TargetPlatform.macOS ||
        defaultTargetPlatform == TargetPlatform.linux);

/// Save [bytes] to a user-chosen file, reporting the outcome via a snackbar.
/// [suggestedName] seeds the dialog filename; a cancelled dialog is a no-op.
/// Guard call sites with [canSaveImages] (desktop-only).
Future<void> saveImageBytes(
  BuildContext context,
  Uint8List bytes, {
  required String suggestedName,
}) async {
  final l10n = AppLocalizations.of(context);
  final messenger = ScaffoldMessenger.of(context);
  final location = await getSaveLocation(suggestedName: suggestedName);
  if (location == null) return;
  try {
    await File(location.path).writeAsBytes(bytes);
    messenger.showSnackBar(
      SnackBar(content: Text(l10n.imageSaved(location.path))),
    );
  } catch (e) {
    messenger.showSnackBar(SnackBar(content: Text(l10n.imageSaveFailed('$e'))));
  }
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
            _ImageThumb(images: renderable, index: bytesIndex++, side: side)
          else if (image.broken)
            _brokenThumb(context, side)
          else
            _hostChip(context, image),
      ],
    );
  }

  Widget _brokenThumb(BuildContext context, double side) {
    final scheme = Theme.of(context).colorScheme;
    return Tooltip(
      message: AppLocalizations.of(context).imageLoadFailed,
      child: Container(
        width: side,
        height: side,
        decoration: BoxDecoration(
          color: scheme.surfaceContainerHighest,
          borderRadius: BorderRadius.circular(10),
          border: Border.all(color: scheme.outlineVariant),
        ),
        child: Icon(Icons.broken_image_outlined, color: scheme.outline),
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

/// Chips for a message's document attachments — host paths parsed back from
/// the text's attached-files block (see `attachment_refs.dart`). Their bytes
/// never crossed the wire; the chip names the file, the tooltip shows the
/// full HOST path the agent was given.
class FileRefChips extends StatelessWidget {
  /// Creates chips for the parsed host [paths].
  const FileRefChips({super.key, required this.paths});

  /// Absolute host paths, in message order.
  final List<String> paths;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Wrap(
      spacing: 6,
      runSpacing: 6,
      children: [
        for (final path in paths)
          Tooltip(
            message: path,
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
                    Icons.description_outlined,
                    size: 16,
                    color: scheme.onSurfaceVariant,
                  ),
                  const SizedBox(width: 6),
                  ConstrainedBox(
                    constraints: const BoxConstraints(maxWidth: 200),
                    child: Text(
                      hostPathBasename(path),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                  ),
                ],
              ),
            ),
          ),
      ],
    );
  }
}

/// One tappable thumbnail. Stateful so a desktop hover can reveal a quick
/// save button (`file_selector` is desktop-only) without a viewer round-trip.
class _ImageThumb extends StatefulWidget {
  const _ImageThumb({
    required this.images,
    required this.index,
    required this.side,
  });

  final List<Uint8List> images;
  final int index;
  final double side;

  @override
  State<_ImageThumb> createState() => _ImageThumbState();
}

class _ImageThumbState extends State<_ImageThumb> {
  bool _hovering = false;

  @override
  Widget build(BuildContext context) {
    final scale = MediaQuery.of(context).devicePixelRatio;
    final bytes = widget.images[widget.index];
    return MouseRegion(
      onEnter: (_) => setState(() => _hovering = true),
      onExit: (_) => setState(() => _hovering = false),
      child: ClipRRect(
        borderRadius: BorderRadius.circular(10),
        child: Stack(
          children: [
            Image.memory(
              bytes,
              key: Key('msg-image-${widget.index}'),
              width: widget.side,
              height: widget.side,
              fit: BoxFit.cover,
              // Decode near thumbnail resolution — full-res frames for every
              // thumb would hold megabytes of pixels per message. 2× the box so
              // a landscape image's SHORT edge still reaches the square cover
              // box (cacheWidth alone would decode it too short and blurry).
              cacheWidth: (widget.side * scale * 2).round(),
              gaplessPlayback: true,
              errorBuilder: (context, _, _) => SizedBox(
                width: widget.side,
                height: widget.side,
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
                  onTap: () => ImageViewerPage.show(
                    context,
                    widget.images,
                    widget.index,
                  ),
                ),
              ),
            ),
            if (canSaveImages && _hovering)
              Positioned(
                top: 4,
                right: 4,
                child: _SaveChip(
                  onPressed: () => saveImageBytes(
                    context,
                    bytes,
                    suggestedName:
                        'image-${widget.index + 1}.${sniffImageExtension(bytes)}',
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}

/// A small circular save button overlaid on a hovered thumbnail.
class _SaveChip extends StatelessWidget {
  const _SaveChip({required this.onPressed});

  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    return Material(
      color: Colors.black54,
      shape: const CircleBorder(),
      child: InkWell(
        customBorder: const CircleBorder(),
        onTap: onPressed,
        child: Padding(
          padding: const EdgeInsets.all(6),
          child: Tooltip(
            message: AppLocalizations.of(context).imageSave,
            child: const Icon(
              Icons.download_outlined,
              size: 18,
              color: Colors.white,
            ),
          ),
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

  Future<void> _save() {
    final bytes = widget.images[_index];
    // Sniff the real container (small GIF/WebP originals pass through
    // byte-identical) so the suggested extension is honest.
    return saveImageBytes(
      context,
      bytes,
      suggestedName: 'image-${_index + 1}.${sniffImageExtension(bytes)}',
    );
  }

  Widget _brokenViewer(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        const Icon(
          Icons.broken_image_outlined,
          color: Colors.white70,
          size: 48,
        ),
        const SizedBox(height: 8),
        Text(
          l10n.imageLoadFailed,
          style: const TextStyle(color: Colors.white70),
        ),
      ],
    );
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
          if (canSaveImages)
            IconButton(
              key: const Key('image-save-btn'),
              tooltip: l10n.imageSave,
              icon: const Icon(Icons.download_outlined),
              onPressed: _save,
            ),
        ],
      ),
      // A tap anywhere over a page pops the viewer, so the backdrop and margins
      // close it — not just the X button. Pinch and double-tap still zoom;
      // those are scale gestures, distinct from a no-move tap.
      body: PageView.builder(
        controller: _pager,
        itemCount: widget.images.length,
        onPageChanged: (i) => setState(() => _index = i),
        itemBuilder: (context, i) => GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: () => Navigator.of(context).maybePop(),
          child: InteractiveViewer(
            key: Key('image-viewer-$i'),
            minScale: 0.5,
            maxScale: 6,
            child: Center(
              child: Image.memory(
                widget.images[i],
                gaplessPlayback: true,
                errorBuilder: (context, _, _) => _brokenViewer(context),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
