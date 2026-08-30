/// Cards and gauges the session screen puts *around* the transcript: the
/// composer's prompts, and the status readouts in its bar.
///
/// Belongs here: a widget the screen composes into its chrome, which does not
/// render a transcript row. Transcript rows live in `transcript_view.dart`, tool
/// activity in `activity_cards.dart`, diffs in `diff_view.dart`.
library;

import 'dart:convert';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/app_modes.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/context_status.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/git_diff.dart';
import 'package:pocket_codex/src/screens/app_session/diff_view.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/widgets/links.dart';

/// A blocking choice box for a server approval request (run a command, edit
/// files, grant permission, …). Decisions use the v2 wire values
/// (`accept`/`decline`/`acceptForSession`). The request stays pending on the
/// host until answered — even across app restarts (replayed on resume) — so
/// the user can always come back and decide.
/// One question parsed from a `request_user_input` elicitation.
class _UiQuestion {
  const _UiQuestion({
    required this.id,
    required this.header,
    required this.question,
    required this.isOther,
    required this.isSecret,
    required this.options,
  });
  final String id;
  final String header;
  final String question;
  final bool isOther;
  final bool isSecret;
  final List<({String label, String? description})> options;
}

/// Interactive card for an `item/tool/requestUserInput` elicitation: the model
/// is asking the user structured questions (NOT requesting permission to run a
/// command, so "完全放行" does not — and should not — suppress it). Each
/// question's options render as selectable chips; `isOther` adds a free-text
/// field; `isSecret` obscures it. Submitting sends one answer per question id;
/// cancel sends an empty answer set so the turn proceeds without input.
class UserInputCard extends StatefulWidget {
  const UserInputCard({
    super.key,
    required this.prompt,
    required this.onAnswer,
  });
  final AppEvent prompt;
  final Future<void> Function(AppEvent, Map<String, List<String>>) onAnswer;

  @override
  State<UserInputCard> createState() => _UserInputCardState();
}

class _UserInputCardState extends State<UserInputCard> {
  // Sentinel "choice" meaning the free-text 其他 field for a question.
  static const _other = '\u0000other';
  late final List<_UiQuestion> _questions = _parse(widget.prompt.raw);
  final Map<String, String> _choice = {}; // qid -> option label or _other
  final Map<String, TextEditingController> _otherCtrls = {};
  bool _submitting = false;

  @override
  void dispose() {
    for (final c in _otherCtrls.values) {
      c.dispose();
    }
    super.dispose();
  }

  static List<_UiQuestion> _parse(String raw) {
    try {
      final m = jsonDecode(raw) as Map<String, dynamic>;
      final qs = (m['questions'] as List?) ?? const [];
      final out = <_UiQuestion>[];
      for (final q in qs) {
        if (q is! Map<String, dynamic>) continue;
        final id = q['id'] as String?;
        if (id == null || id.isEmpty) continue;
        final opts = <({String label, String? description})>[];
        for (final o in (q['options'] as List?) ?? const []) {
          if (o is! Map<String, dynamic>) continue;
          final label = o['label'] as String?;
          if (label == null || label.isEmpty) continue;
          opts.add((label: label, description: o['description'] as String?));
        }
        out.add(
          _UiQuestion(
            id: id,
            header: (q['header'] as String?) ?? '',
            question: (q['question'] as String?) ?? '',
            isOther: (q['isOther'] as bool?) ?? false,
            isSecret: (q['isSecret'] as bool?) ?? false,
            options: opts,
          ),
        );
      }
      return out;
    } catch (_) {
      return const [];
    }
  }

  TextEditingController _ctrl(String qid) =>
      _otherCtrls.putIfAbsent(qid, TextEditingController.new);

  String? _answer(_UiQuestion q) {
    // No options → pure free-text; an explicit "其他" pick → free-text too.
    if (q.options.isEmpty) {
      final t = _ctrl(q.id).text.trim();
      return t.isEmpty ? null : t;
    }
    final c = _choice[q.id];
    if (c == null) return null;
    if (c == _other) {
      final t = _ctrl(q.id).text.trim();
      return t.isEmpty ? null : t;
    }
    return c;
  }

  bool get _complete =>
      _questions.isNotEmpty && _questions.every((q) => _answer(q) != null);

  Future<void> _submit() async {
    final answers = <String, List<String>>{};
    for (final q in _questions) {
      final a = _answer(q);
      if (a != null) answers[q.id] = [a];
    }
    setState(() => _submitting = true);
    await widget.onAnswer(widget.prompt, answers);
  }

  Future<void> _cancel() async {
    setState(() => _submitting = true);
    await widget.onAnswer(widget.prompt, const {});
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    return Container(
      key: const Key('user-input-card'),
      margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      decoration: BoxDecoration(
        color: scheme.surfaceContainerHigh,
        border: Border.all(color: scheme.outlineVariant, width: 0.5),
        borderRadius: BorderRadius.circular(kPanelRadius),
      ),
      child: Padding(
        padding: const EdgeInsets.all(14),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(Icons.help_outline, size: 18, color: scheme.primary),
                const SizedBox(width: 9),
                Expanded(
                  child: Text(
                    l10n.userInputTitle,
                    style: TextStyle(
                      color: scheme.onSurface,
                      fontWeight: FontWeight.w500,
                      fontSize: 14,
                    ),
                  ),
                ),
              ],
            ),
            if (_questions.isEmpty)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: SelectableText(
                  widget.prompt.raw,
                  style: const TextStyle(fontSize: 12),
                ),
              )
            else
              for (final q in _questions) _questionBlock(context, q, scheme),
            const SizedBox(height: 6),
            Wrap(
              alignment: WrapAlignment.end,
              spacing: 8,
              children: [
                TextButton(
                  onPressed: _submitting ? null : _cancel,
                  child: Text(l10n.cancel),
                ),
                FilledButton(
                  key: const Key('user-input-submit'),
                  onPressed: (_complete && !_submitting) ? _submit : null,
                  child: Text(l10n.userInputSubmit),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _questionBlock(
    BuildContext context,
    _UiQuestion q,
    ColorScheme scheme,
  ) {
    final l10n = AppLocalizations.of(context);
    return Padding(
      padding: const EdgeInsets.only(top: 10),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (q.header.isNotEmpty)
            Text(
              q.header,
              style: TextStyle(
                fontSize: 12,
                fontWeight: FontWeight.w600,
                color: scheme.onSurfaceVariant,
              ),
            ),
          if (q.question.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: 2),
              child: Text(q.question, style: const TextStyle(fontSize: 13.5)),
            ),
          const SizedBox(height: 6),
          // A question with no options is a pure free-text prompt; otherwise show
          // the option chips (+ an "其他" chip when free text is also allowed).
          if (q.options.isNotEmpty)
            Wrap(
              spacing: 8,
              runSpacing: 6,
              children: [
                for (final o in q.options)
                  ChoiceChip(
                    label: Text(o.label),
                    tooltip:
                        (o.description != null && o.description!.isNotEmpty)
                        ? o.description
                        : null,
                    selected: _choice[q.id] == o.label,
                    onSelected: _submitting
                        ? null
                        : (_) => setState(() => _choice[q.id] = o.label),
                  ),
                if (q.isOther)
                  ChoiceChip(
                    label: Text(l10n.userInputOther),
                    selected: _choice[q.id] == _other,
                    onSelected: _submitting
                        ? null
                        : (_) => setState(() => _choice[q.id] = _other),
                  ),
              ],
            ),
          if (q.options.isEmpty || (q.isOther && _choice[q.id] == _other))
            Padding(
              padding: const EdgeInsets.only(top: 6),
              child: TextField(
                controller: _ctrl(q.id),
                obscureText: q.isSecret,
                onChanged: (_) => setState(() {}),
                style: const TextStyle(fontSize: 13),
                decoration: const InputDecoration(
                  isDense: true,
                  border: OutlineInputBorder(),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

class ApprovalCard extends StatelessWidget {
  const ApprovalCard({super.key, required this.prompt, required this.onDecide});
  final AppEvent prompt;
  final Future<void> Function(AppEvent, String) onDecide;

  ({IconData icon, String title}) _meta(AppLocalizations l10n) {
    final k = prompt.kind;
    if (k.contains('fileChange')) {
      return (icon: Icons.edit_document, title: l10n.approvalFilePrompt);
    }
    if (k.contains('permissions')) {
      return (
        icon: Icons.shield_outlined,
        title: l10n.approvalPermissionPrompt,
      );
    }
    return (icon: Icons.terminal, title: l10n.approvalPrompt);
  }

  /// Best-effort detail from the request params (command / cwd / reason / files).
  String _detail() {
    try {
      final p = jsonDecode(prompt.raw) as Map<String, dynamic>;
      final parts = <String>[];
      if (p['command'] is String) parts.add(p['command'] as String);
      if (p['cwd'] is String) parts.add('cwd: ${p['cwd']}');
      if (p['reason'] is String) parts.add(p['reason'] as String);
      if (p['changes'] is List) {
        for (final c in (p['changes'] as List)) {
          if (c is Map && c['path'] is String) parts.add(c['path'] as String);
        }
      }
      if (parts.isNotEmpty) return parts.join('\n');
    } catch (_) {}
    return prompt.raw;
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final meta = _meta(l10n);
    return Container(
      key: const Key('approval-card'),
      margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
      decoration: BoxDecoration(
        color: scheme.surfaceContainerHigh,
        border: Border.all(color: scheme.outlineVariant, width: 0.5),
        borderRadius: BorderRadius.circular(kPanelRadius),
      ),
      child: Padding(
        padding: const EdgeInsets.all(14),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(meta.icon, size: 18, color: scheme.primary),
                const SizedBox(width: 9),
                Expanded(
                  child: Text(
                    meta.title,
                    style: TextStyle(
                      color: scheme.onSurface,
                      fontWeight: FontWeight.w500,
                      fontSize: 14,
                    ),
                  ),
                ),
              ],
            ),
            const SizedBox(height: 10),
            Container(
              width: double.infinity,
              constraints: const BoxConstraints(maxHeight: 160),
              padding: const EdgeInsets.all(10),
              decoration: BoxDecoration(
                color: scheme.surface.withValues(alpha: 0.6),
                borderRadius: BorderRadius.circular(8),
              ),
              child: SingleChildScrollView(
                child: linkifyText(
                  context,
                  _detail(),
                  selectable: true,
                  style: const TextStyle(
                    fontFamily: monoFontFamily,
                    fontFamilyFallback: monoCjkFallback,
                    fontSize: 12,
                  ),
                ),
              ),
            ),
            const SizedBox(height: 8),
            Wrap(
              alignment: WrapAlignment.end,
              spacing: 8,
              children: [
                TextButton(
                  onPressed: () => onDecide(prompt, 'decline'),
                  child: Text(l10n.deny),
                ),
                TextButton(
                  onPressed: () => onDecide(prompt, 'acceptForSession'),
                  child: Text(l10n.approveForSession),
                ),
                FilledButton(
                  key: const Key('approve-btn'),
                  onPressed: () => onDecide(prompt, 'accept'),
                  child: Text(l10n.approve),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

/// Compact token count: `840`, `12.3k`, `1.2M`.
String formatTokenCount(int n) {
  if (n < 1000) return '$n';
  if (n < 1000000) return '${(n / 1000).toStringAsFixed(1)}k';
  return '${(n / 1000000).toStringAsFixed(1)}M';
}

/// A stepped effort selector: a track with a stop per level and a thumb that
/// GLIDES between them when the level changes (tap or drag), rather than a
/// Material slider that snaps. Fixed layout — nothing here reflows the panel as
/// the level or its label changes.
///
/// Deliberately no `LayoutBuilder`: this lives inside a `MenuAnchor` panel,
/// which measures intrinsic width, and `LayoutBuilder` can't answer that. All
/// geometry is fractional (`Align` / `FractionallySizedBox`), which is
/// intrinsic-safe; a `GlobalKey` reads the track width to map a tap to a level.
class EffortSteps extends StatefulWidget {
  const EffortSteps({
    super.key,
    required this.levels,
    required this.current,
    required this.onChanged,
  });

  final List<ReasoningEffort> levels;
  final ReasoningEffort? current;
  final ValueChanged<ReasoningEffort> onChanged;

  @override
  State<EffortSteps> createState() => _EffortStepsState();
}

class _EffortStepsState extends State<EffortSteps> {
  static const double _thumbW = 14;
  static const double _height = 24;
  final GlobalKey _trackKey = GlobalKey();

  void _selectAt(double localDx) {
    final n = widget.levels.length;
    if (n <= 1) return;
    final box = _trackKey.currentContext?.findRenderObject() as RenderBox?;
    final w = box?.size.width ?? 0;
    final usable = w - _thumbW;
    if (usable <= 0) return;
    // The stops sit `_thumbW/2` in from each edge (so the thumb never
    // overflows); map the tap into that usable span.
    final f = ((localDx - _thumbW / 2) / usable).clamp(0.0, 1.0);
    final ni = (f * (n - 1)).round();
    if (widget.levels[ni] != widget.current) {
      widget.onChanged(widget.levels[ni]);
    }
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final l10n = AppLocalizations.of(context);
    final n = widget.levels.length;
    // An effort the model doesn't list (an unknown server default) parks the
    // thumb mid-scale rather than lying about the level.
    final found = widget.levels.indexWhere((e) => e == widget.current);
    final idx = found < 0 ? (n - 1) ~/ 2 : found;
    final target = n <= 1 ? 0.0 : idx / (n - 1);
    final atMax = idx == n - 1;

    // A bare GestureDetector reports no cursor, so this track would hover as
    // plain content despite being tappable AND draggable.
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTapDown: (d) => _selectAt(d.localPosition.dx),
        onHorizontalDragUpdate: (d) => _selectAt(d.localPosition.dx),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            SizedBox(
              key: _trackKey,
              height: _height,
              // One painter, exact geometry: the thumb sits ON each stop at every
              // level (Align mapped different-sized children to the bounds, which
              // drifted the thumb off the dots — worst at 极高). Animate `frac`.
              child: TweenAnimationBuilder<double>(
                tween: Tween<double>(end: target),
                duration: const Duration(milliseconds: 240),
                curve: Curves.easeOutCubic,
                builder: (context, frac, _) => CustomPaint(
                  size: const Size(double.infinity, _height),
                  painter: _EffortPainter(
                    frac: frac,
                    levels: n,
                    activeIdx: idx,
                    thumbW: _thumbW,
                    atMax: atMax,
                    track: scheme.surfaceContainerHighest,
                    thumbFill: scheme.surface,
                    thumbBorder: scheme.outlineVariant,
                    shadow: scheme.shadow,
                    dotOff: scheme.outline,
                    brightness: Theme.of(context).brightness,
                    fill: scheme.primary,
                    onFill: scheme.onPrimary,
                  ),
                ),
              ),
            ),
            const SizedBox(height: 4),
            // End labels — the reference's Faster ↔ Smarter poles.
            Row(
              children: [
                Text(
                  l10n.effortFaster,
                  style: TextStyle(
                    fontSize: 10.5,
                    color: scheme.onSurfaceVariant,
                  ),
                ),
                const Spacer(),
                Text(
                  l10n.effortSmarter,
                  style: TextStyle(
                    fontSize: 10.5,
                    color: scheme.onSurfaceVariant,
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

/// Paints the effort track: a rounded track, a flat violet fill up to the
/// thumb, a stop per level, and a rounded-square thumb centred exactly on
/// its stop. At the top level the fill gets a scattered "dither" shimmer —
/// the reference's Ultracode flourish.
class _EffortPainter extends CustomPainter {
  _EffortPainter({
    required this.frac,
    required this.levels,
    required this.activeIdx,
    required this.thumbW,
    required this.atMax,
    required this.track,
    required this.thumbFill,
    required this.thumbBorder,
    required this.shadow,
    required this.dotOff,
    required this.brightness,
    required this.fill,
    required this.onFill,
  });

  final double frac;
  final int levels;
  final int activeIdx;
  final double thumbW;
  final bool atMax;
  final Color track;
  final Color thumbFill;
  final Color thumbBorder;
  final Color shadow;
  final Color dotOff;
  final Brightness brightness;

  /// The flat fill up to the thumb — the theme's accent, so the slider belongs
  /// to the palette instead of being the one violet thing on screen. Flat, not
  /// a gradient, per the design's rules.
  final Color fill;

  /// Ink drawn ON the fill (the passed stops and the max-level shimmer). Taken
  /// from the theme rather than assumed white: the accent is light enough that
  /// white-on-accent barely shows.
  final Color onFill;

  // A fixed scatter of unit-square offsets for the max-level shimmer, generated
  // once so it doesn't flicker as `frac` animates.
  static final List<Offset> _dither = () {
    final r = math.Random(7);
    return [
      for (var i = 0; i < 90; i++) Offset(r.nextDouble(), r.nextDouble()),
    ];
  }();

  @override
  void paint(Canvas canvas, Size size) {
    final h = size.height;
    final cy = h / 2;
    const trackH = 6.0;
    const thumbH = 20.0;
    final usable = size.width - thumbW;
    double xAt(double f) => thumbW / 2 + usable * f;
    final cx = xAt(frac);

    // Track.
    final trackRect = RRect.fromLTRBR(
      0,
      cy - trackH / 2,
      size.width,
      cy + trackH / 2,
      const Radius.circular(trackH / 2),
    );
    canvas.drawRRect(trackRect, Paint()..color = track);

    // Flat fill up to the thumb.
    final fillRight = cx.clamp(trackH, size.width);
    final fillRect = RRect.fromLTRBR(
      0,
      cy - trackH / 2,
      fillRight,
      cy + trackH / 2,
      const Radius.circular(trackH / 2),
    );
    canvas.drawRRect(fillRect, Paint()..color = fill);

    // Max-level shimmer: scattered translucent squares over the fill.
    if (atMax) {
      canvas.save();
      canvas.clipRRect(fillRect);
      final dot = Paint()..color = onFill.withValues(alpha: 0.28);
      for (final o in _dither) {
        final x = o.dx * fillRight;
        final y = cy - trackH / 2 + o.dy * trackH;
        canvas.drawRect(Rect.fromLTWH(x, y, 1.4, 1.4), dot);
      }
      canvas.restore();
    }

    // A stop per level, on the track centre-line.
    for (var i = 0; i < levels; i++) {
      final x = xAt(levels <= 1 ? 0 : i / (levels - 1));
      final on = i <= activeIdx;
      canvas.drawCircle(
        Offset(x, cy),
        2,
        Paint()..color = on ? onFill.withValues(alpha: 0.9) : dotOff,
      );
    }

    // The thumb — a rounded square, centred exactly on the current stop.
    final thumbRect = RRect.fromRectAndRadius(
      Rect.fromCenter(center: Offset(cx, cy), width: thumbW, height: thumbH),
      const Radius.circular(5),
    );
    canvas.drawRRect(
      thumbRect.shift(const Offset(0, 1)),
      Paint()
        ..color = shadow.withValues(alpha: 0.22)
        ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 2),
    );
    canvas.drawRRect(thumbRect, Paint()..color = thumbFill);
    canvas.drawRRect(
      thumbRect,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1
        ..color = thumbBorder,
    );
  }

  @override
  bool shouldRepaint(_EffortPainter old) =>
      old.frac != frac ||
      old.activeIdx != activeIdx ||
      old.levels != levels ||
      old.atMax != atMax ||
      old.brightness != brightness ||
      // Follows the theme, so it changes on a light/dark switch.
      old.fill != fill ||
      old.onFill != onFill;
}

/// A small circular context-window gauge for the app bar: a ring filled to the
/// usage fraction with the percent in the middle. Hover shows [tooltip]
/// (desktop); tap opens the detail sheet via [onTap].
class ContextGauge extends StatelessWidget {
  const ContextGauge({
    super.key,
    required this.status,
    required this.onTap,
    required this.tooltip,
  });
  final ContextStatus status;
  final VoidCallback onTap;
  final String tooltip;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    // Warn (amber) past 75%, alert (error) past 90%.
    final f = status.fraction;
    final color = f >= 0.9
        ? scheme.error
        : f >= 0.75
        ? cautionColor(scheme)
        : scheme.primary;
    return Tooltip(
      message: tooltip,
      child: InkResponse(
        mouseCursor: clickable,
        onTap: onTap,
        radius: 22,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 6),
          child: SizedBox(
            width: 30,
            height: 30,
            child: CustomPaint(
              painter: _GaugePainter(
                fraction: f,
                color: color,
                track: scheme.surfaceContainerHighest,
              ),
              child: Center(
                child: Text(
                  '${status.percent}',
                  style: TextStyle(
                    fontSize: 9.5,
                    fontWeight: FontWeight.w600,
                    color: color,
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _GaugePainter extends CustomPainter {
  _GaugePainter({
    required this.fraction,
    required this.color,
    required this.track,
  });
  final double fraction;
  final Color color;
  final Color track;

  @override
  void paint(Canvas canvas, Size size) {
    const stroke = 3.0;
    final rect = Offset.zero & size;
    final center = rect.center;
    final radius = (size.shortestSide - stroke) / 2;
    final base = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = stroke
      ..color = track;
    canvas.drawCircle(center, radius, base);
    final arc = Paint()
      ..style = PaintingStyle.stroke
      ..strokeCap = StrokeCap.round
      ..strokeWidth = stroke
      ..color = color;
    canvas.drawArc(
      Rect.fromCircle(center: center, radius: radius),
      -1.5708, // start at 12 o'clock
      6.28318 * fraction.clamp(0.0, 1.0),
      false,
      arc,
    );
  }

  @override
  bool shouldRepaint(_GaugePainter old) =>
      old.fraction != fraction || old.color != color || old.track != track;
}

/// The environment side panel: where this conversation is actually running —
/// the checkout it acts on, the branch, and everything it has changed there,
/// with each changed file expandable to its colour-coded hunks.
///
/// Every row is state the app genuinely knows; deliberately no commit/push or
/// pull-request controls, because nothing on the host side implements them and
/// a button that does nothing is worse than no button.
class EnvPanel extends StatelessWidget {
  const EnvPanel({super.key, required this.diff, this.branch, this.cwd});
  final DiffModel? diff;
  final String? branch;

  /// Project root this conversation runs in (the checkout being changed).
  final String? cwd;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final d = diff;
    final hasChanges = d != null && !d.isEmpty;
    // The whole panel is content to read and copy — branch, path, and above
    // all the diff. Wrapping it in a SelectionArea makes every bit of that
    // drag-selectable (desktop) / long-press-selectable (touch), the same as
    // the transcript.
    return SelectionArea(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 8, 8, 8),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    l10n.envTitle,
                    style: Theme.of(context).textTheme.titleSmall,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
              ],
            ),
          ),
          const Divider(height: 1),
          Expanded(child: _body(context, scheme, l10n, hasChanges, d)),
        ],
      ),
    );
  }

  /// The scrolling body. ONE scrollable, and a lazy one: the file rows are
  /// built by index so an expanded 1,400-line diff isn't laid out on every
  /// frame of a scroll through the other 54 files.
  Widget _body(
    BuildContext context,
    ColorScheme scheme,
    AppLocalizations l10n,
    bool hasChanges,
    DiffModel? d,
  ) {
    final leading = <Widget>[
      // Where the work lands: branch + working-tree totals, as one card.
      Padding(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 4),
        child: _card(context, scheme, l10n, hasChanges, d),
      ),
      if (cwd != null && cwd!.trim().isNotEmpty) ...[
        _heading(context, l10n.envProject),
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 0, 16, 4),
          child: Row(
            children: [
              Icon(
                Icons.folder_outlined,
                size: 14,
                color: scheme.onSurfaceVariant,
              ),
              const SizedBox(width: 7),
              Expanded(
                child: Tooltip(
                  message: cwd!,
                  child: Text(
                    cwd!,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontFamily: monoFontFamily,
                      fontFamilyFallback: monoCjkFallback,
                      fontSize: 11.5,
                      color: scheme.onSurfaceVariant,
                    ),
                  ),
                ),
              ),
            ],
          ),
        ),
      ],
      _heading(context, l10n.envSource),
      if (!hasChanges)
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 4, 16, 8),
          child: Text(
            l10n.noChanges,
            style: TextStyle(fontSize: 12.5, color: scheme.outline),
          ),
        ),
    ];
    final files = hasChanges ? d!.files : const <DiffFile>[];
    return ListView.builder(
      padding: const EdgeInsets.only(bottom: 16),
      itemCount: leading.length + files.length,
      itemBuilder: (c, i) {
        if (i < leading.length) return leading[i];
        final f = files[i - leading.length];
        return DiffFileTile(
          // Lazy building recycles elements, so the expanded/collapsed state
          // has to live somewhere that survives scrolling off-screen.
          key: PageStorageKey<String>('env-diff-${f.path}'),
          file: f,
          initiallyExpanded: false,
        );
      },
    );
  }

  /// The branch card: which checkout state the agent is writing into.
  Widget _card(
    BuildContext context,
    ColorScheme scheme,
    AppLocalizations l10n,
    bool hasChanges,
    DiffModel? d,
  ) => Container(
    padding: const EdgeInsets.fromLTRB(12, 10, 12, 11),
    decoration: BoxDecoration(
      color: scheme.surfaceContainerLow,
      border: Border.all(color: scheme.outlineVariant, width: 0.5),
      borderRadius: BorderRadius.circular(12),
    ),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Icon(Icons.computer, size: 14, color: scheme.primary),
            const SizedBox(width: 7),
            Text(
              l10n.envLocal,
              style: TextStyle(
                fontSize: 12,
                fontWeight: FontWeight.w600,
                color: scheme.onSurface,
              ),
            ),
          ],
        ),
        const SizedBox(height: 8),
        Row(
          children: [
            Icon(
              Icons.account_tree_outlined,
              size: 14,
              color: scheme.onSurfaceVariant,
            ),
            const SizedBox(width: 7),
            Expanded(
              child: Text(
                branch ?? '—',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  fontFamily: monoFontFamily,
                  fontFamilyFallback: monoCjkFallback,
                  fontSize: 12.5,
                  color: scheme.onSurface,
                ),
              ),
            ),
          ],
        ),
        if (hasChanges) ...[
          const SizedBox(height: 8),
          Row(
            children: [
              Text(
                '+${d!.added}',
                style: TextStyle(
                  fontSize: 12.5,
                  fontWeight: FontWeight.w600,
                  color: additionColor(scheme),
                  fontFeatures: const [FontFeature.tabularFigures()],
                ),
              ),
              const SizedBox(width: 8),
              Text(
                '−${d.removed}',
                style: TextStyle(
                  fontSize: 12.5,
                  fontWeight: FontWeight.w600,
                  color: scheme.error,
                  fontFeatures: const [FontFeature.tabularFigures()],
                ),
              ),
              const SizedBox(width: 10),
              Flexible(
                child: Text(
                  l10n.envFilesChanged(d.files.length),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 12,
                    color: scheme.onSurfaceVariant,
                  ),
                ),
              ),
            ],
          ),
        ],
      ],
    ),
  );

  /// A small all-caps section heading, the panel's only structural chrome.
  Widget _heading(BuildContext context, String text) => Padding(
    padding: const EdgeInsets.fromLTRB(16, 14, 16, 6),
    child: Text(
      text.toUpperCase(),
      style: TextStyle(
        fontSize: 10.5,
        fontWeight: FontWeight.w700,
        letterSpacing: 0.7,
        color: Theme.of(context).colorScheme.onSurfaceVariant,
      ),
    ),
  );
}
