import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/widgets/app_toast.dart';

import 'approval_review.dart';

/// Historical model-review presentation, with the original payload preserved.
class ApprovalReviewCard extends StatelessWidget {
  const ApprovalReviewCard({
    super.key,
    required this.raw,
    this.request,
    this.result,
  });
  final String raw;
  final ApprovalReviewRequest? request;
  final ApprovalReviewResult? result;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final decision = result;
    final action = request;
    final allowed = decision?.outcome == 'allow';
    final color = decision == null
        ? scheme.primary
        : allowed
        ? successColor(scheme)
        : scheme.error;
    final title = decision == null ? l10n.reviewRequest : l10n.reviewResult;
    String level(String value) => switch (value) {
      'low' => l10n.reviewLow,
      'medium' => l10n.reviewMedium,
      'high' => l10n.reviewHigh,
      'critical' => l10n.reviewCritical,
      _ => l10n.reviewUnknown,
    };
    Widget badge(String text) => Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      decoration: BoxDecoration(
        color: scheme.surfaceContainerLow,
        borderRadius: BorderRadius.circular(6),
      ),
      child: Text(text, style: Theme.of(context).textTheme.labelSmall),
    );
    return Container(
      key: Key(
        decision == null ? 'approval-review-request' : 'approval-review-result',
      ),
      margin: const EdgeInsets.symmetric(vertical: 6),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: surfacePanel(scheme),
        border: Border.all(color: scheme.outlineVariant),
        borderRadius: BorderRadius.circular(kPanelRadius),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(
                decision == null
                    ? Icons.policy_outlined
                    : allowed
                    ? Icons.check_circle_outline
                    : Icons.block,
                size: 18,
                color: color,
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  title,
                  style: Theme.of(context).textTheme.titleSmall,
                ),
              ),
              if (decision != null)
                Text(
                  allowed ? l10n.reviewAllowed : l10n.reviewDenied,
                  style: TextStyle(color: color, fontWeight: FontWeight.w600),
                ),
            ],
          ),
          const SizedBox(height: 10),
          if (decision != null) ...[
            Wrap(
              spacing: 6,
              runSpacing: 6,
              children: [
                badge(l10n.reviewRisk(level(decision.risk))),
                badge(l10n.reviewAuthorization(level(decision.authorization))),
              ],
            ),
            const SizedBox(height: 10),
            Text(
              decision.rationale,
              style: Theme.of(context).textTheme.bodyMedium,
            ),
          ],
          if (action != null) ...[
            badge(action.tool),
            const SizedBox(height: 8),
            Text(
              action.summary,
              maxLines: 4,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(context).textTheme.bodyMedium,
            ),
            if (action.cwd case final cwd?) ...[
              const SizedBox(height: 6),
              Text(
                cwd,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(fontSize: 11, color: scheme.onSurfaceVariant),
              ),
            ],
          ],
          const SizedBox(height: 6),
          _RawReview(raw: raw),
        ],
      ),
    );
  }
}

class _RawReview extends StatefulWidget {
  const _RawReview({required this.raw});
  final String raw;
  @override
  State<_RawReview> createState() => _RawReviewState();
}

class _RawReviewState extends State<_RawReview> {
  bool _expanded = false;

  int _boundary(int offset) {
    if (offset > 0 && offset < widget.raw.length) {
      final unit = widget.raw.codeUnitAt(offset);
      if (unit >= 0xDC00 && unit <= 0xDFFF) return offset - 1;
    }
    return offset;
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Expanded(
              child: Align(
                alignment: Alignment.centerLeft,
                child: TextButton.icon(
                  key: const Key('review-raw-toggle'),
                  onPressed: () => setState(() => _expanded = !_expanded),
                  icon: Icon(
                    _expanded ? Icons.expand_less : Icons.expand_more,
                    size: 16,
                  ),
                  label: Text(l10n.reviewRaw),
                ),
              ),
            ),
            IconButton(
              tooltip: l10n.copy,
              icon: const Icon(Icons.content_copy_outlined, size: 16),
              onPressed: () {
                Clipboard.setData(ClipboardData(text: widget.raw));
                showToastOk(context, l10n.copied);
              },
            ),
          ],
        ),
        if (_expanded)
          SizedBox(
            height: 240,
            // Bound layout even for a multi-megabyte, single-line request. Copy
            // retains the exact original; blocks are created only inside the viewport.
            child: ListView.builder(
              key: const Key('review-raw-content'),
              primary: false,
              itemCount: (widget.raw.length / 800).ceil(),
              itemBuilder: (_, i) => SelectableText(
                widget.raw.substring(
                  _boundary(i * 800),
                  _boundary(math.min((i + 1) * 800, widget.raw.length)),
                ),
                style: TextStyle(
                  fontFamily: monoFontFamily,
                  fontFamilyFallback: monoCjkFallback,
                  fontSize: 12,
                  color: scheme.onSurfaceVariant,
                ),
              ),
            ),
          ),
      ],
    );
  }
}
