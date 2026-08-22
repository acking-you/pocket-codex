import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
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

  /// An image that exists only as a path on the host — a client-side file
  /// mention (see `ide_context.dart`) rather than a wire attachment.
  ResolvedImage.hostFile(String path) : this._(hostPath: path);

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

/// Fetches a host-side image's bytes, or null when it cannot be read (the
/// host is remote and the path sits outside the shared roots, the file is
/// gone, or it is too big to be worth pulling over a tunnel).
typedef HostImageLoader = Future<Uint8List?> Function(String hostPath);

/// Side of a composer attachment tile, and of a multi-image message thumbnail.
/// One constant because the two are meant to read as the same object: what you
/// staged in the composer is what appears in the sent message.
const double kAttachmentTileSide = 72;

/// Corner radius shared by every attachment tile and thumbnail.
const double kAttachmentTileRadius = 12;

/// The rounded-square frame every attachment tile and message thumbnail sits
/// in, optionally with a remove button in its top-right corner.
///
/// Exists so the composer strip and the transcript can't drift apart: they used
/// to hand-roll the same Container + Stack with different radii, borders and
/// button styling, which is exactly how a "staged image" and a "sent image"
/// ended up looking like two unrelated controls.
class AttachmentTile extends StatelessWidget {
  /// Creates a framed tile around [child].
  const AttachmentTile({
    super.key,
    required this.child,
    this.side = kAttachmentTileSide,
    this.onRemove,
    this.removeTooltip,
    this.removeKey,
    this.onTap,
    this.tapTooltip,
  });

  /// What the frame contains — usually an `Image.memory`.
  final Widget child;

  /// Width and height of the square frame.
  final double side;

  /// Removes this attachment; null hides the corner button (sent messages).
  final VoidCallback? onRemove;

  /// Tooltip for the remove button.
  final String? removeTooltip;

  /// Key for the remove button, so a test can target this specific tile.
  final Key? removeKey;

  /// Opens this attachment (preview); null leaves the tile inert.
  final VoidCallback? onTap;

  /// Tooltip for the tile itself.
  final String? tapTooltip;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final radius = BorderRadius.circular(kAttachmentTileRadius);
    Widget frame = Container(
      width: side,
      height: side,
      clipBehavior: Clip.antiAlias,
      decoration: BoxDecoration(
        borderRadius: radius,
        color: scheme.surfaceContainerHighest,
        border: Border.all(color: scheme.outlineVariant),
      ),
      child: child,
    );
    if (onTap != null) {
      // A transparent Material ON TOP of the (opaque) image, so the tap ripple
      // is visible — ink on an ancestor Material would paint underneath it.
      frame = Stack(
        children: [
          frame,
          Positioned.fill(
            child: Material(
              type: MaterialType.transparency,
              child: InkWell(
                mouseCursor: clickable,
                borderRadius: radius,
                onTap: onTap,
                child: tapTooltip == null
                    ? null
                    : Tooltip(message: tapTooltip!),
              ),
            ),
          ),
        ],
      );
    }
    if (onRemove == null) return frame;
    // The button overhangs the frame's corner rather than sitting inside it, so
    // it doesn't cover the picture it belongs to (matches the reference app).
    return SizedBox(
      width: side + 8,
      height: side + 8,
      child: Stack(
        children: [
          Positioned(left: 0, top: 8, child: frame),
          Positioned(
            right: 0,
            top: 0,
            child: _RemoveButton(
              buttonKey: removeKey,
              tooltip: removeTooltip,
              onPressed: onRemove!,
            ),
          ),
        ],
      ),
    );
  }
}

/// The circular ✕ that removes a staged attachment. A filled disc with a
/// hairline ring, so it stays legible over a light OR dark thumbnail without
/// needing to know what it's sitting on.
class _RemoveButton extends StatelessWidget {
  const _RemoveButton({required this.onPressed, this.tooltip, this.buttonKey});

  final VoidCallback onPressed;
  final String? tooltip;
  final Key? buttonKey;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final button = Material(
      color: scheme.inverseSurface,
      shape: CircleBorder(side: BorderSide(color: scheme.surface, width: 1.5)),
      child: InkWell(
        mouseCursor: clickable,
        key: buttonKey,
        customBorder: const CircleBorder(),
        onTap: onPressed,
        child: Padding(
          padding: const EdgeInsets.all(3),
          child: Icon(Icons.close, size: 12, color: scheme.onInverseSurface),
        ),
      ),
    );
    return tooltip == null ? button : Tooltip(message: tooltip!, child: button);
  }
}

/// Largest image worth loading inline: above this the decoded bitmap costs
/// more than the preview is worth, and the filename chip stays.
const int kMaxInlineImageBytes = 8 * 1024 * 1024;

/// [HostImageLoader] for a path on THIS machine. Null when the file is
/// missing, oversized or unreadable.
Future<Uint8List?> readLocalImage(String path) async {
  try {
    final file = File(path);
    if (!await file.exists()) return null;
    final length = await file.length();
    // An empty file decodes to nothing: the chip is a better answer than a
    // broken-image tile.
    if (length == 0 || length > kMaxInlineImageBytes) return null;
    return await file.readAsBytes();
  } catch (_) {
    return null;
  }
}

/// Thumbnail strip for a message's image attachments. Tapping a thumbnail
/// opens the fullscreen [ImageViewerPage] (swipe between the message's
/// images, pinch to zoom).
///
/// An image that only ever existed as a host path — a `localImage` or a file
/// the sending client mentioned — has no bytes on the wire. Given a
/// [hostImageLoader] the strip fetches them so the picture shows like any
/// other attachment; without one, or when the fetch fails, it falls back to
/// the honest filename chip.
class MessageImagesView extends StatefulWidget {
  /// Creates the strip for [images] (resolved once by the item model).
  const MessageImagesView({
    super.key,
    required this.images,
    this.hostImageLoader,
  });

  /// The message's resolved attachments, in send order.
  final List<ResolvedImage> images;

  /// How to read a host path's bytes; null leaves host images as chips.
  final HostImageLoader? hostImageLoader;

  @override
  State<MessageImagesView> createState() => _MessageImagesViewState();
}

// Host images already fetched, newest last. Bounded because the entries are
// whole decoded files and a long transcript would otherwise pin every picture
// it ever scrolled past; the list virtualises, so re-fetching an evicted one
// is cheap next to holding them all.
final Map<String, Uint8List?> _hostImageCache = {};
const _hostImageCacheMax = 24;

class _MessageImagesViewState extends State<MessageImagesView> {
  // Paths whose fetch is in flight, so a rebuild mid-load doesn't start it
  // again.
  final Set<String> _loading = {};

  @override
  void initState() {
    super.initState();
    _fetchHostImages();
  }

  @override
  void didUpdateWidget(MessageImagesView old) {
    super.didUpdateWidget(old);
    _fetchHostImages();
  }

  void _fetchHostImages() {
    final load = widget.hostImageLoader;
    if (load == null) return;
    for (final image in widget.images) {
      final path = image.hostPath;
      if (path == null) continue;
      if (_hostImageCache.containsKey(path) || !_loading.add(path)) continue;
      load(path)
          .then((bytes) {
            _hostImageCache[path] = bytes;
            if (_hostImageCache.length > _hostImageCacheMax) {
              _hostImageCache.remove(_hostImageCache.keys.first);
            }
          })
          .catchError((_) => _hostImageCache[path] = null)
          .whenComplete(() {
            _loading.remove(path);
            if (mounted) setState(() {});
          });
    }
  }

  /// Bytes to draw for [image]: its own (a data URL) or the host file's, once
  /// fetched.
  Uint8List? _bytesFor(ResolvedImage image) =>
      image.bytes ??
      (image.hostPath == null ? null : _hostImageCache[image.hostPath]);

  @override
  Widget build(BuildContext context) {
    // Also here, not just on init/update: the cache is bounded, so scrolling a
    // long transcript can evict an image that is still on screen. Without a
    // re-request it would silently drop back to a filename chip forever. The
    // call is idempotent — it starts work only for paths that are neither
    // cached (null counts) nor already in flight.
    _fetchHostImages();
    final images = widget.images;
    final renderable = [
      for (final i in images)
        if (_bytesFor(i) != null) _bytesFor(i)!,
    ];
    // A single image gets a larger preview; several tile as uniform squares.
    final side = renderable.length == 1 ? 180.0 : 96.0;
    var bytesIndex = 0;
    return Wrap(
      spacing: 6,
      runSpacing: 6,
      children: [
        for (final image in images)
          if (_bytesFor(image) != null)
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
      child: Stack(
        children: [
          AttachmentTile(
            side: widget.side,
            onTap: () =>
                ImageViewerPage.show(context, widget.images, widget.index),
            child: Image.memory(
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
              errorBuilder: (context, _, _) => Icon(
                Icons.broken_image_outlined,
                color: Theme.of(context).colorScheme.outline,
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
        mouseCursor: clickable,
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

  // One controller per page, so the −/+ buttons and the pinch gesture drive the
  // same transform (a button that fought the gesture would be worse than no
  // button) and so leaving a page resets its zoom.
  final Map<int, TransformationController> _zoom = {};

  /// Zoom steps the buttons walk through. Multiplicative rather than additive:
  /// +25 percentage points is a huge jump at 0.5× and a nudge at 6×.
  static const double _zoomStep = 1.25;
  static const double _minZoom = 0.5;
  static const double _maxZoom = 6;

  TransformationController _controllerFor(int i) =>
      _zoom.putIfAbsent(i, TransformationController.new);

  @override
  void dispose() {
    _pager.dispose();
    for (final c in _zoom.values) {
      c.dispose();
    }
    super.dispose();
  }

  /// Current scale of the visible page (the matrix is uniform-scale only —
  /// InteractiveViewer never shears — so the x-scale IS the zoom).
  double get _scale => _controllerFor(_index).value.getMaxScaleOnAxis();

  void _setZoom(double target) {
    final next = target.clamp(_minZoom, _maxZoom);
    // Scale about the viewport centre, which is what the buttons imply — the
    // default origin is the top-left, which would walk the image off-screen.
    final box = context.findRenderObject() as RenderBox?;
    final size = box?.size ?? Size.zero;
    _controllerFor(_index).value = Matrix4.identity()
      ..translateByDouble(size.width / 2, size.height / 2, 0, 1)
      ..scaleByDouble(next, next, next, 1)
      ..translateByDouble(-size.width / 2, -size.height / 2, 0, 1);
    setState(() {});
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
      body: Stack(
        children: [
          // A tap anywhere over a page pops the viewer, so the backdrop and
          // margins close it — not just the X button. Pinch and double-tap
          // still zoom; those are scale gestures, distinct from a no-move tap.
          PageView.builder(
            controller: _pager,
            itemCount: widget.images.length,
            onPageChanged: (i) => setState(() => _index = i),
            itemBuilder: (context, i) => GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTap: () => Navigator.of(context).maybePop(),
              child: InteractiveViewer(
                key: Key('image-viewer-$i'),
                transformationController: _controllerFor(i),
                minScale: _minZoom,
                maxScale: _maxZoom,
                // Keep the readout honest while the user pinches, not just
                // when a button is pressed.
                onInteractionEnd: (_) => setState(() {}),
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
          Positioned(
            left: 0,
            right: 0,
            bottom: 24,
            child: Center(child: _zoomBar()),
          ),
        ],
      ),
    );
  }

  /// The floating −/percentage/+ pill. Tapping the readout resets to 100%,
  /// which is the fastest way back after over-zooming.
  Widget _zoomBar() {
    final scale = _scale;
    return Material(
      key: const Key('image-zoom-bar'),
      color: Colors.black.withValues(alpha: 0.62),
      shape: const StadiumBorder(),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            _zoomButton(
              buttonKey: const Key('image-zoom-out'),
              icon: Icons.remove,
              // Disabled at the stop rather than silently doing nothing, so the
              // control says where the limit is.
              onPressed: scale <= _minZoom + 0.001
                  ? null
                  : () => _setZoom(scale / _zoomStep),
            ),
            InkWell(
              mouseCursor: clickable,
              key: const Key('image-zoom-reset'),
              customBorder: const StadiumBorder(),
              onTap: () => _setZoom(1),
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
                child: Text(
                  '${(scale * 100).round()}%',
                  style: const TextStyle(
                    color: Colors.white,
                    fontSize: 12.5,
                    // Tabular-ish: a fixed width stops the pill resizing as the
                    // number goes 98% → 100% → 125% mid-pinch.
                    fontFeatures: [FontFeature.tabularFigures()],
                  ),
                ),
              ),
            ),
            _zoomButton(
              buttonKey: const Key('image-zoom-in'),
              icon: Icons.add,
              onPressed: scale >= _maxZoom - 0.001
                  ? null
                  : () => _setZoom(scale * _zoomStep),
            ),
          ],
        ),
      ),
    );
  }

  Widget _zoomButton({
    required Key buttonKey,
    required IconData icon,
    required VoidCallback? onPressed,
  }) => IconButton(
    key: buttonKey,
    icon: Icon(icon, size: 18),
    color: Colors.white,
    disabledColor: Colors.white30,
    visualDensity: VisualDensity.compact,
    onPressed: onPressed,
  );
}
