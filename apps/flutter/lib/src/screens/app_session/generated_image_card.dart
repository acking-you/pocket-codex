import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/widgets/message_images.dart';

import 'transcript_model.dart';

Map<String, dynamic> _details(String text) {
  try {
    final value = jsonDecode(text);
    return value is Map<String, dynamic> ? value : const {};
  } on FormatException {
    return const {};
  }
}

/// Native image-generation statuses use snake_case, unlike other v2 items.
bool imageGenerationInProgress(String text) => const {
  'in_progress',
  'inProgress',
  'generating',
  'queued',
}.contains(_details(text)['status']);

/// A generated artifact stays visible outside the collapsed tool activity.
class GeneratedImageCard extends StatelessWidget {
  const GeneratedImageCard({
    super.key,
    required this.item,
    this.hostImageLoader,
    this.imageCacheScope,
  });

  final TranscriptItem item;
  final HostImageLoader? hostImageLoader;
  final Object? imageCacheScope;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final details = _details(item.text);
    final status = details['status'];
    final failed = status == 'failed' || details['failure'] != null;
    final generating = item.streaming && !failed && status != 'completed';
    final failure = details['failure'];
    final limited = failure is Map && failure['type'] == 'usageLimitExceeded';
    final label = failed
        ? (limited ? l10n.imageGenerationLimit : l10n.imageGenerationFailed)
        : generating
        ? l10n.imageGenerating
        : item.images.isNotEmpty
        ? l10n.toolGeneratedImage
        : imageGenerationInProgress(item.text) || status == 'cancelled'
        ? l10n.imageGenerationIncomplete
        : l10n.imageGenerationUnavailable;
    return Padding(
      key: ValueKey('generated-image-${item.id}'),
      padding: const EdgeInsets.symmetric(vertical: 12),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(
                failed ? Icons.error_outline : Icons.auto_awesome_outlined,
                size: 18,
                color: failed ? scheme.error : scheme.primary,
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  l10n.toolGeneratedImage,
                  style: Theme.of(context).textTheme.labelLarge,
                ),
              ),
            ],
          ),
          if (item.title.isNotEmpty && item.title != status) ...[
            const SizedBox(height: 6),
            Text(
              item.title,
              maxLines: 3,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(context).textTheme.bodySmall,
            ),
          ],
          const SizedBox(height: 10),
          if (failed || (!generating && item.images.isEmpty))
            Text(
              label,
              style: TextStyle(
                color: failed ? scheme.error : scheme.onSurfaceVariant,
              ),
            ),
          if (item.images.isNotEmpty)
            MessageImagesView(
              images: item.images,
              hostImageLoader: hostImageLoader,
              cacheScope: imageCacheScope,
              previewSide: 280,
            )
          else if (generating)
            LayoutBuilder(
              builder: (context, constraints) => ImageLoadingPlaceholder(
                label: l10n.imageGenerating,
                side: 280.0.clamp(0.0, constraints.maxWidth),
              ),
            ),
        ],
      ),
    );
  }
}
