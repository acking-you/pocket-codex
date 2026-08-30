/// Rows for the agent *doing* something: tool calls, plans, and the turn
/// progress they roll up into.
///
/// Belongs here: a widget for a non-message `TranscriptItem`, and the plan
/// parsing those rows share. Messages live in `transcript_view.dart`.
///
/// Note the direction of the dependency: `transcript_view.dart` imports this,
/// because a message row can embed an activity card, and not the reverse.
library;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/git_diff.dart';
import 'package:pocket_codex/src/screens/app_session/diff_view.dart';
import 'package:pocket_codex/src/screens/app_session/transcript_model.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/widgets/app_toast.dart';
import 'package:pocket_codex/src/widgets/links.dart';
import 'package:pocket_codex/src/widgets/markdown_view.dart';

/// One parsed plan step.
typedef PlanStep = ({String status, String text});

/// The explanatory lead-in and checklist encoded in one plan item.
typedef ParsedPlan = ({String explanation, List<PlanStep> steps});

final _planStepPattern = RegExp(r'^\s*-\s*\[(.)\]\s?(.*)$');

/// In plan mode the model wraps its proposal in `<proposed_plan>…</proposed_plan>`
/// and codex doesn't always strip the tags, so they leak into the rendered
/// message. Detect them and return the text without the wrapper tags plus an
/// `isPlan` flag the UI uses to badge the message as a plan. Streaming-safe:
/// strips whichever tag has arrived so far (the open tag leads the content).
({bool isPlan, String text}) readProposedPlan(String raw) {
  final open = RegExp(r'<\s*proposed_plan\s*>', caseSensitive: false);
  if (!open.hasMatch(raw)) return (isPlan: false, text: raw);
  final close = RegExp(r'<\s*/\s*proposed_plan\s*>', caseSensitive: false);
  return (
    isPlan: true,
    text: raw.replaceAll(open, '').replaceAll(close, '').trim(),
  );
}

/// Split a plan item's text into its lead-in prose and its checklist steps.
ParsedPlan parsePlan(String text) {
  final explanation = <String>[];
  final steps = <PlanStep>[];
  for (final line in text.split('\n')) {
    final match = _planStepPattern.firstMatch(line);
    if (match != null) {
      final mark = match.group(1)!;
      final status = mark == 'x'
          ? 'completed'
          : mark == '~'
          ? 'in_progress'
          : 'pending';
      steps.add((status: status, text: match.group(2)!.trim()));
    } else if (line.trim().isNotEmpty) {
      explanation.add(line);
    }
  }
  return (explanation: explanation.join('\n'), steps: steps);
}

/// Compact live checklist pinned above the composer while a turn is active.
class TurnProgressTracker extends StatefulWidget {
  const TurnProgressTracker({
    super.key,
    required this.steps,
    required this.changedFiles,
    required this.added,
    required this.removed,
    required this.compact,
    this.onViewDiff,
  });

  final List<PlanStep> steps;
  final int changedFiles;
  final int added;
  final int removed;
  final bool compact;
  final VoidCallback? onViewDiff;

  @override
  State<TurnProgressTracker> createState() => _TurnProgressTrackerState();
}

class _TurnProgressTrackerState extends State<TurnProgressTracker> {
  bool _expanded = true;

  int get _currentStep {
    final active = widget.steps.indexWhere((s) => s.status == 'in_progress');
    if (active >= 0) return active + 1;
    final done = widget.steps.where((s) => s.status == 'completed').length;
    return (done + 1).clamp(1, widget.steps.length);
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final muted = scheme.onSurfaceVariant;
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        AnimatedSize(
          duration: const Duration(milliseconds: 180),
          curve: Curves.easeOutCubic,
          child: !_expanded
              ? const SizedBox.shrink()
              : Container(
                  key: const Key('turn-progress-panel'),
                  constraints: const BoxConstraints(maxHeight: 210),
                  decoration: BoxDecoration(
                    color: scheme.surfaceBright,
                    borderRadius: BorderRadius.circular(kPanelRadius),
                    border: Border.all(color: scheme.outlineVariant),
                    boxShadow: panelShadow(scheme),
                  ),
                  child: SingleChildScrollView(
                    padding: const EdgeInsets.fromLTRB(12, 9, 12, 9),
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [for (final step in widget.steps) _step(step)],
                    ),
                  ),
                ),
        ),
        const SizedBox(height: 4),
        Material(
          key: const Key('turn-progress-summary'),
          color: scheme.surfaceBright,
          shape: StadiumBorder(side: BorderSide(color: scheme.outlineVariant)),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              InkWell(
                mouseCursor: clickable,
                customBorder: const StadiumBorder(),
                onTap: () => setState(() => _expanded = !_expanded),
                child: Padding(
                  padding: const EdgeInsets.fromLTRB(10, 6, 8, 6),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      SizedBox(
                        width: 13,
                        height: 13,
                        child: CircularProgressIndicator(
                          strokeWidth: 1.8,
                          color: scheme.primary,
                        ),
                      ),
                      const SizedBox(width: 7),
                      Text(
                        l10n.turnProgressStep(
                          _currentStep,
                          widget.steps.length,
                        ),
                        style: TextStyle(fontSize: 12, color: muted),
                      ),
                      if (widget.changedFiles > 0) ...[
                        Text('  ·  ', style: TextStyle(color: muted)),
                        if (!widget.compact) ...[
                          Text(
                            l10n.envFilesChanged(widget.changedFiles),
                            style: TextStyle(fontSize: 12, color: muted),
                          ),
                          const SizedBox(width: 5),
                        ],
                        Text(
                          '+${widget.added}',
                          style: TextStyle(
                            fontSize: 11.5,
                            color: additionColor(scheme),
                            fontFeatures: const [FontFeature.tabularFigures()],
                          ),
                        ),
                        const SizedBox(width: 3),
                        Text(
                          '−${widget.removed}',
                          style: TextStyle(
                            fontSize: 11.5,
                            color: scheme.error,
                            fontFeatures: const [FontFeature.tabularFigures()],
                          ),
                        ),
                      ],
                      const SizedBox(width: 3),
                      Icon(
                        _expanded ? Icons.expand_less : Icons.expand_more,
                        size: 16,
                        color: muted,
                      ),
                    ],
                  ),
                ),
              ),
              if (widget.onViewDiff != null) ...[
                SizedBox(
                  height: 22,
                  child: VerticalDivider(
                    width: 1,
                    thickness: 1,
                    color: scheme.outlineVariant,
                  ),
                ),
                IconButton(
                  key: const Key('turn-progress-diff'),
                  tooltip: l10n.viewDiff,
                  visualDensity: VisualDensity.compact,
                  constraints: const BoxConstraints.tightFor(
                    width: 34,
                    height: 30,
                  ),
                  onPressed: widget.onViewDiff,
                  icon: const Icon(Icons.difference_outlined, size: 16),
                ),
              ],
            ],
          ),
        ),
      ],
    );
  }

  Widget _step(PlanStep step) {
    final scheme = Theme.of(context).colorScheme;
    final muted = scheme.onSurfaceVariant;
    final icon = switch (step.status) {
      'completed' => Icon(Icons.check_circle_outline, size: 15, color: muted),
      'in_progress' => SizedBox(
        width: 15,
        height: 15,
        child: CircularProgressIndicator(
          strokeWidth: 1.8,
          color: scheme.primary,
        ),
      ),
      _ => Icon(Icons.circle_outlined, size: 15, color: muted),
    };
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(padding: const EdgeInsets.only(top: 1), child: icon),
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              step.text,
              style: TextStyle(
                fontSize: 12.5,
                height: 1.3,
                color: step.status == 'completed' ? muted : scheme.onSurface,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// A status-iconed checklist for a `plan` item (codex `update_plan`). The
/// summarizer encodes the plan as an optional explanation plus `- [x|~| ] step`
/// lines; this renders each step with a completed / in-progress / pending icon.
class PlanCard extends StatefulWidget {
  const PlanCard({super.key, required this.item});
  final TranscriptItem item;

  @override
  State<PlanCard> createState() => _PlanCardState();
}

class _PlanCardState extends State<PlanCard> {
  bool _expanded = true;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final (:explanation, :steps) = parsePlan(widget.item.text);
    final done = steps.where((s) => s.status == 'completed').length;
    final muted = scheme.onSurfaceVariant;

    return Container(
      margin: const EdgeInsets.symmetric(vertical: 4),
      decoration: BoxDecoration(
        color: scheme.surfaceContainerLow,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: scheme.outlineVariant),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // Header: icon + "Plan" + progress, tap to collapse.
          InkWell(
            mouseCursor: clickable,
            borderRadius: BorderRadius.circular(12),
            onTap: () => setState(() => _expanded = !_expanded),
            child: Padding(
              padding: const EdgeInsets.fromLTRB(12, 10, 10, 10),
              child: Row(
                children: [
                  Icon(
                    Icons.checklist_rounded,
                    size: 17,
                    color: scheme.primary,
                  ),
                  const SizedBox(width: 8),
                  Text(
                    l10n.toolPlan,
                    style: Theme.of(context).textTheme.titleSmall,
                  ),
                  if (steps.isNotEmpty) ...[
                    const SizedBox(width: 8),
                    Text(
                      '$done/${steps.length}',
                      style: TextStyle(fontSize: 12, color: muted),
                    ),
                  ],
                  const Spacer(),
                  Icon(
                    _expanded ? Icons.expand_less : Icons.expand_more,
                    size: 18,
                    color: muted,
                  ),
                ],
              ),
            ),
          ),
          if (_expanded) ...[
            if (explanation.isNotEmpty)
              Padding(
                padding: const EdgeInsets.fromLTRB(14, 0, 14, 8),
                child: MarkdownView(data: explanation),
              ),
            if (steps.isNotEmpty)
              Padding(
                padding: const EdgeInsets.fromLTRB(12, 0, 12, 10),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [for (final s in steps) _stepRow(s)],
                ),
              ),
          ],
        ],
      ),
    );
  }

  Widget _stepRow(PlanStep s) {
    final scheme = Theme.of(context).colorScheme;
    final (icon, color) = switch (s.status) {
      'completed' => (Icons.check_circle_rounded, additionColor(scheme)),
      'in_progress' => (Icons.timelapse_rounded, scheme.primary),
      _ => (Icons.radio_button_unchecked, scheme.onSurfaceVariant),
    };
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.only(top: 1, right: 8),
            child: Icon(icon, size: 16, color: color),
          ),
          Expanded(
            child: linkifyText(
              context,
              s.text,
              style: TextStyle(
                fontSize: 13.5,
                height: 1.35,
                color: s.status == 'completed'
                    ? scheme.onSurfaceVariant
                    : scheme.onSurface,
                fontWeight: s.status == 'in_progress'
                    ? FontWeight.w600
                    : FontWeight.normal,
                decoration: s.status == 'completed'
                    ? TextDecoration.lineThrough
                    : null,
                decorationColor: scheme.onSurfaceVariant,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// A run of ≥2 consecutive same-type activity items, collapsed into one row.
class ActivityGroup {
  ActivityGroup(this.type, this.items);
  final String type;
  final List<TranscriptItem> items;
}

/// Visual identity for every app-server v2 activity item PocketCodex exposes.
/// Keeping this in one place prevents grouped and individual rows from drifting
/// as upstream adds richer thread-item variants.
({IconData icon, String label}) activityMeta(
  String type,
  AppLocalizations l10n,
) {
  return switch (type) {
    'webSearch' => (icon: Icons.travel_explore, label: l10n.toolSearched),
    'commandExecution' => (icon: Icons.terminal, label: l10n.toolRan),
    'fileChange' => (icon: Icons.edit_document, label: l10n.toolEdited),
    'mcpToolCall' ||
    'dynamicToolCall' => (icon: Icons.extension, label: l10n.toolCalled),
    'collabAgentToolCall' => (
      icon: Icons.groups_outlined,
      label: l10n.toolCollaborated,
    ),
    'subAgentActivity' => (
      icon: Icons.account_tree_outlined,
      label: l10n.toolSubAgent,
    ),
    'imageView' => (
      icon: Icons.image_search_outlined,
      label: l10n.toolViewedImage,
    ),
    'imageGeneration' => (
      icon: Icons.auto_awesome_outlined,
      label: l10n.toolGeneratedImage,
    ),
    'sleep' => (icon: Icons.hourglass_empty, label: l10n.toolWaited),
    'hookPrompt' => (icon: Icons.webhook_outlined, label: l10n.toolHook),
    'enteredReviewMode' => (
      icon: Icons.fact_check_outlined,
      label: l10n.toolEnteredReview,
    ),
    'exitedReviewMode' => (
      icon: Icons.fact_check_outlined,
      label: l10n.toolExitedReview,
    ),
    'reasoning' => (icon: Icons.lightbulb_outline, label: l10n.toolThinking),
    'plan' => (icon: Icons.checklist, label: l10n.toolPlan),
    _ => (icon: Icons.bolt, label: l10n.toolActivity),
  };
}

/// One glyph in a message's hover action row.
///
/// A washed square on hover rather than a bare icon: the reference app's row of
/// actions reads as a set of small controls, and an icon that only changes ink
/// gives no target to aim at under a pointer.
class MessageAction extends StatefulWidget {
  const MessageAction({
    super.key,
    required this.icon,
    required this.tooltip,
    required this.onPressed,
  });

  final IconData icon;
  final String tooltip;
  final VoidCallback onPressed;

  @override
  State<MessageAction> createState() => _MessageActionState();
}

class _MessageActionState extends State<MessageAction> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Tooltip(
      message: widget.tooltip,
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: GestureDetector(
          onTap: widget.onPressed,
          child: Container(
            padding: const EdgeInsets.all(5),
            decoration: BoxDecoration(
              // Off-state is the wash at zero alpha, not transparent: Material
              // lerps through transparent BLACK, which flashes a dark box.
              color: _hovered
                  ? scheme.surfaceContainerHigh
                  : scheme.surfaceContainerHigh.withValues(alpha: 0),
              borderRadius: BorderRadius.circular(6),
            ),
            child: Icon(
              widget.icon,
              size: 15,
              color: _hovered ? scheme.onSurface : onSurfaceMuted(scheme),
            ),
          ),
        ),
      ),
    );
  }
}

/// A run of ≥2 consecutive `agentMessage` items — one turn's reply, split
/// across several server items — rendered as a single block.
class AgentTurn {
  AgentTurn(this.items);
  final List<TranscriptItem> items;

  /// Still producing text: the block keeps its actions hidden until the whole
  /// reply has settled, not just its first part.
  bool get streaming => items.any((i) => i.streaming);

  /// When the turn finished, per the server.
  int? get completedAt => items.first.turnCompletedAt;

  /// The reply as one document. Blank parts are dropped so a placeholder item
  /// can't open the block with an empty paragraph.
  String get text =>
      items.map((i) => i.text.trim()).where((t) => t.isNotEmpty).join('\n\n');
}

/// Collapses a run of same-type tool calls (e.g. several shell commands) into a
/// single low-chrome row ("Ran command ×3") that expands to the individual
/// [ActivityCard]s — so long tool sequences don't flood the transcript.
class GroupedActivityCard extends StatefulWidget {
  const GroupedActivityCard({super.key, required this.group});
  final ActivityGroup group;

  @override
  State<GroupedActivityCard> createState() => _GroupedActivityCardState();
}

class _GroupedActivityCardState extends State<GroupedActivityCard> {
  bool _expanded = false;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final muted = scheme.onSurfaceVariant;
    final meta = activityMeta(widget.group.type, l10n);
    final n = widget.group.items.length;
    final anyStreaming = widget.group.items.any((i) => i.streaming);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Container(
          margin: const EdgeInsets.symmetric(vertical: 2),
          decoration: BoxDecoration(
            border: Border.all(color: scheme.outlineVariant, width: 0.5),
            borderRadius: BorderRadius.circular(12),
          ),
          child: InkWell(
            mouseCursor: clickable,
            borderRadius: BorderRadius.circular(12),
            onTap: () => setState(() => _expanded = !_expanded),
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
              child: Row(
                children: [
                  Icon(meta.icon, size: 17, color: scheme.primary),
                  const SizedBox(width: 10),
                  Text(
                    '${meta.label} ×$n',
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: scheme.onSurface,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                  const Spacer(),
                  if (anyStreaming)
                    SizedBox(
                      width: 12,
                      height: 12,
                      child: CircularProgressIndicator(
                        strokeWidth: 1.8,
                        color: muted,
                      ),
                    )
                  else
                    Icon(
                      _expanded ? Icons.expand_less : Icons.expand_more,
                      size: 18,
                      color: muted.withValues(alpha: 0.7),
                    ),
                ],
              ),
            ),
          ),
        ),
        if (_expanded)
          Padding(
            padding: const EdgeInsets.only(left: 16, top: 2),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                for (final it in widget.group.items)
                  it.type == 'fileChange'
                      ? FileChangeCard(item: it)
                      : ActivityCard(item: it),
              ],
            ),
          ),
      ],
    );
  }
}

/// A collapsible card for a tool / activity item (web search, command, file
/// edit, MCP/skill call, reasoning, …) so the user can see — and expand — what
/// the agent is doing, like Codex / Gemini.
class ActivityCard extends StatefulWidget {
  const ActivityCard({super.key, required this.item});
  final TranscriptItem item;

  @override
  State<ActivityCard> createState() => _ActivityCardState();
}

class _ActivityCardState extends State<ActivityCard> {
  bool _expanded = false;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final item = widget.item;
    final meta = activityMeta(item.type, l10n);
    final muted = scheme.onSurfaceVariant;
    final title = item.title.trim();
    final detail = item.text.trim();
    // Reasoning is prose the model wrote in Markdown (its summaries open with
    // a `**bold**` header); everything else here is literal output — a command
    // line, a tool payload, a diff — where a `*` means an asterisk and the
    // monospace column matters. Only the prose gets rendered as Markdown.
    final prose = item.type == 'reasoning';
    // One-line value: the title (command/query/tool) or a peek of the detail.
    final rawValue = title.isNotEmpty
        ? title
        : detail
              .split('\n')
              .firstWhere((s) => s.trim().isNotEmpty, orElse: () => '');
    final value = prose ? markdownPlainPreview(rawValue) : rawValue;
    // Expandable when there's detail or the value is long enough to truncate.
    final expandable = detail.isNotEmpty || value.length > 56;
    final body = [
      if (title.isNotEmpty) title,
      if (detail.isNotEmpty) detail,
    ].join('\n\n');

    // Two idioms. A phone gets a soft bordered card — a comfortable tap target
    // in a list of them. A desktop transcript gets a timeline: the steps of a
    // turn are rows hanging off one continuous rail, so a dozen tool calls read
    // as a sequence instead of a dozen boxes.
    final doc = MediaQuery.sizeOf(context).width >= docLayoutWidth;
    return Container(
      margin: EdgeInsets.symmetric(vertical: doc ? 0 : 2),
      padding: doc ? const EdgeInsets.only(left: 12) : EdgeInsets.zero,
      decoration: BoxDecoration(
        border: doc
            ? Border(left: BorderSide(color: scheme.outlineVariant, width: 1.5))
            : Border.all(color: scheme.outlineVariant, width: 0.5),
        borderRadius: doc ? null : BorderRadius.circular(12),
      ),
      clipBehavior: doc ? Clip.none : Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          InkWell(
            mouseCursor: clickable,
            onTap: expandable
                ? () => setState(() => _expanded = !_expanded)
                : null,
            child: Padding(
              padding: EdgeInsets.symmetric(
                horizontal: doc ? 0 : 12,
                vertical: doc ? 4 : 10,
              ),
              child: Row(
                children: [
                  Icon(
                    meta.icon,
                    size: doc ? 15 : 17,
                    color: doc ? muted : scheme.primary,
                  ),
                  SizedBox(width: doc ? 8 : 10),
                  Text(
                    meta.label,
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: doc ? muted : scheme.onSurface,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                  if (value.isNotEmpty) ...[
                    const SizedBox(width: 8),
                    Expanded(
                      child: Text(
                        value,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: prose
                            ? TextStyle(fontSize: 12.5, color: muted)
                            : TextStyle(
                                fontFamily: monoFontFamily,
                                fontFamilyFallback: monoCjkFallback,
                                fontSize: 12,
                                color: muted,
                              ),
                      ),
                    ),
                  ] else
                    const Spacer(),
                  if (item.streaming)
                    SizedBox(
                      width: 12,
                      height: 12,
                      child: CircularProgressIndicator(
                        strokeWidth: 1.8,
                        color: muted,
                      ),
                    )
                  else if (expandable)
                    Icon(
                      _expanded ? Icons.expand_less : Icons.expand_more,
                      size: 18,
                      color: muted.withValues(alpha: 0.7),
                    ),
                ],
              ),
            ),
          ),
          if (_expanded && body.isNotEmpty)
            Container(
              width: double.infinity,
              margin: EdgeInsets.only(bottom: doc ? 6 : 0),
              padding: const EdgeInsets.all(11),
              constraints: const BoxConstraints(maxHeight: 320),
              decoration: BoxDecoration(
                border: doc
                    ? null
                    : Border(
                        top: BorderSide(
                          color: scheme.outlineVariant,
                          width: 0.5,
                        ),
                      ),
                borderRadius: doc ? BorderRadius.circular(8) : null,
                color: scheme.surfaceContainerLowest,
              ),
              child: SingleChildScrollView(
                child: prose
                    ? MarkdownView(data: body, muted: true)
                    : linkifyText(
                        context,
                        body,
                        selectable: true,
                        style: const TextStyle(
                          fontFamily: monoFontFamily,
                          fontFamilyFallback: monoCjkFallback,
                          fontSize: 12,
                        ),
                      ),
              ),
            ),
        ],
      ),
    );
  }
}

/// A three-dot "typing" indicator shown while the model is starting a reply.
class TypingIndicator extends StatefulWidget {
  const TypingIndicator({super.key, required this.elapsed});

  /// Live elapsed-time label (the same value as the status-bar timer);
  /// empty leaves just the pulsing dots.
  final String elapsed;

  @override
  State<TypingIndicator> createState() => _TypingIndicatorState();
}

class _TypingIndicatorState extends State<TypingIndicator>
    with SingleTickerProviderStateMixin {
  late final AnimationController _c = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 1100),
  )..repeat();

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final color = Theme.of(context).colorScheme.onSurfaceVariant;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 14),
      child: Row(
        children: [
          ...List.generate(3, (i) {
            return AnimatedBuilder(
              animation: _c,
              builder: (context, _) {
                // Stagger each dot's pulse.
                final t = (_c.value + i * 0.2) % 1.0;
                final o = 0.3 + 0.7 * (1 - (t - 0.5).abs() * 2).clamp(0.0, 1.0);
                return Padding(
                  padding: const EdgeInsets.only(right: 5),
                  child: Opacity(
                    opacity: o,
                    child: Container(
                      width: 7,
                      height: 7,
                      decoration: BoxDecoration(
                        color: color,
                        shape: BoxShape.circle,
                      ),
                    ),
                  ),
                );
              },
            );
          }),
          // The same live elapsed clock as the status bar, trailing the dots.
          if (widget.elapsed.isNotEmpty) ...[
            const SizedBox(width: 2),
            Text(
              widget.elapsed,
              style: TextStyle(
                fontSize: 12,
                color: color,
                fontFeatures: const [FontFeature.tabularFigures()],
              ),
            ),
          ],
        ],
      ),
    );
  }
}

/// A reviewable file-change card: a low-chrome header (edited file(s) + ±counts)
/// that expands to the colored per-file diff (reusing [DiffFileTile]), so the
/// agent's edits can be reviewed inline. Falls back to copyable path rows when
/// no diff text is present.
class FileChangeCard extends StatefulWidget {
  const FileChangeCard({super.key, required this.item});
  final TranscriptItem item;

  @override
  State<FileChangeCard> createState() => _FileChangeCardState();
}

class _FileChangeCardState extends State<FileChangeCard> {
  bool _expanded = false;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final muted = scheme.onSurfaceVariant;
    final diff = DiffModel.parse(widget.item.text);
    final hasDiff = !diff.isEmpty;
    final title = widget.item.title.trim();
    final expandable = hasDiff || widget.item.text.trim().isNotEmpty;
    return Container(
      margin: const EdgeInsets.symmetric(vertical: 2),
      decoration: BoxDecoration(
        border: Border.all(color: scheme.outlineVariant, width: 0.5),
        borderRadius: BorderRadius.circular(12),
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          InkWell(
            mouseCursor: clickable,
            onTap: expandable
                ? () => setState(() => _expanded = !_expanded)
                : null,
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
              child: Row(
                children: [
                  Icon(Icons.edit_document, size: 17, color: scheme.primary),
                  const SizedBox(width: 10),
                  Text(
                    l10n.toolEdited,
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: scheme.onSurface,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontFamily: monoFontFamily,
                        fontFamilyFallback: monoCjkFallback,
                        fontSize: 12,
                        color: muted,
                      ),
                    ),
                  ),
                  if (hasDiff) ...[
                    Text(
                      '+${diff.added}',
                      style: TextStyle(
                        fontSize: 11.5,
                        color: additionColor(scheme),
                      ),
                    ),
                    const SizedBox(width: 3),
                    Text(
                      '−${diff.removed}',
                      style: TextStyle(fontSize: 11.5, color: scheme.error),
                    ),
                    const SizedBox(width: 4),
                  ],
                  if (expandable)
                    Icon(
                      _expanded ? Icons.expand_less : Icons.expand_more,
                      size: 18,
                      color: muted.withValues(alpha: 0.7),
                    ),
                ],
              ),
            ),
          ),
          if (_expanded)
            DecoratedBox(
              decoration: BoxDecoration(
                border: Border(
                  top: BorderSide(color: scheme.outlineVariant, width: 0.5),
                ),
              ),
              // Color goes on a Material (not the box) so the diff's
              // ListTile-based tiles paint their ink/background correctly —
              // which also means it has to be opaque, blended over the page.
              child: Material(
                color: Color.alphaBlend(
                  scheme.surfaceContainerLow,
                  scheme.surface,
                ),
                child: hasDiff
                    ? Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          for (final f in diff.files) DiffFileTile(file: f),
                        ],
                      )
                    : Padding(
                        padding: const EdgeInsets.all(10),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            for (final p
                                in widget.item.text
                                    .split('\n')
                                    .map((s) => s.trim())
                                    .where((s) => s.isNotEmpty))
                              _CopyablePath(path: p),
                          ],
                        ),
                      ),
              ),
            ),
        ],
      ),
    );
  }
}

/// A monospace file path with a copy button.
class _CopyablePath extends StatelessWidget {
  const _CopyablePath({required this.path});
  final String path;

  @override
  Widget build(BuildContext context) {
    final muted = Theme.of(context).colorScheme.onSurfaceVariant;
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Expanded(
          child: SelectableText(
            path,
            style: const TextStyle(
              fontFamily: monoFontFamily,
              fontFamilyFallback: monoCjkFallback,
              fontSize: 12,
            ),
          ),
        ),
        Tooltip(
          message: AppLocalizations.of(context).copy,
          child: InkResponse(
            mouseCursor: clickable,
            radius: 16,
            onTap: () {
              Clipboard.setData(ClipboardData(text: path));
              showToastOk(context, AppLocalizations.of(context).copied);
            },
            child: Padding(
              padding: const EdgeInsets.all(2),
              child: Icon(Icons.copy_outlined, size: 14, color: muted),
            ),
          ),
        ),
      ],
    );
  }
}
