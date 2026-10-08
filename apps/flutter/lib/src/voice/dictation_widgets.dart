import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/motion.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/voice/dictation.dart';
import 'package:pocket_codex/src/voice/waveform.dart';

/// The composer's dictation microphone.
///
/// Idle, it is a quiet microphone whose corner dot says whether the line is
/// already open (a take then starts at once). During a take it is the done
/// button: filled, with a ring that breathes with the voice.
class DictationButton extends StatelessWidget {
  /// A microphone for [line].
  const DictationButton({
    super.key,
    required this.line,
    required this.onStart,
    required this.onFinish,
    this.enabled = true,
    this.size = 32,
    this.iconSize = 18,
  });

  /// The dictation line.
  final DictationLine line;

  /// Start a take.
  final VoidCallback onStart;

  /// Finish the take.
  final VoidCallback onFinish;

  /// False while another call holds the microphone.
  final bool enabled;

  /// Hit target side.
  final double size;

  /// Glyph size.
  final double iconSize;

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: line,
    builder: (context, _) {
      final l10n = AppLocalizations.of(context);
      final scheme = Theme.of(context).colorScheme;
      final take = line.take;
      final live =
          take == DictationTake.listening || take == DictationTake.finishing;
      final opening = take == DictationTake.opening;
      final tooltip = switch (take) {
        DictationTake.idle =>
          line.link == DictationLink.ready
              ? '${l10n.dictate} (${l10n.dictationReady})'
              : l10n.dictate,
        DictationTake.opening => l10n.dictationConnecting,
        DictationTake.listening => l10n.dictationStop,
        DictationTake.finishing => l10n.dictationFinishing,
      };
      final onPressed = switch (take) {
        DictationTake.idle => enabled ? onStart : null,
        DictationTake.opening => line.cancelTake,
        DictationTake.listening => onFinish,
        DictationTake.finishing => null,
      };
      final accent = dictationColor(scheme);
      final button = IconButton(
        key: const Key('dictate'),
        tooltip: tooltip,
        onPressed: onPressed,
        isSelected: live,
        icon: AnimatedSwitcher(
          duration: Motion.of(context, Motion.fast),
          child: opening
              ? SizedBox(
                  key: const ValueKey('opening'),
                  width: iconSize - 4,
                  height: iconSize - 4,
                  child: CircularProgressIndicator(
                    strokeWidth: 1.8,
                    color: accent,
                  ),
                )
              : Icon(
                  live ? Icons.check_rounded : Icons.mic_none_rounded,
                  key: ValueKey(live),
                  size: iconSize,
                ),
        ),
        style: IconButton.styleFrom(
          minimumSize: Size.square(size),
          fixedSize: Size.square(size),
          padding: EdgeInsets.zero,
          shape: const CircleBorder(),
          foregroundColor: live ? onDictationColor(scheme) : null,
          backgroundColor: live ? accent : null,
          disabledBackgroundColor: live ? accent.withValues(alpha: 0.6) : null,
          disabledForegroundColor: live ? onDictationColor(scheme) : null,
        ),
      );
      return SizedBox.square(
        dimension: size,
        child: Stack(
          clipBehavior: Clip.none,
          alignment: Alignment.center,
          children: [
            if (take == DictationTake.listening)
              Positioned.fill(
                child: IgnorePointer(
                  child: _VoiceRing(level: line.level, color: accent),
                ),
              ),
            button,
            // The line is open in the background: a take starts at once.
            Positioned(
              right: size * 0.16,
              top: size * 0.16,
              child: IgnorePointer(
                child: AnimatedScale(
                  scale:
                      take == DictationTake.idle &&
                          line.link == DictationLink.ready
                      ? 1
                      : 0,
                  duration: Motion.of(context, Motion.fast),
                  curve: Motion.enter,
                  child: Container(
                    key: const Key('dictation-ready-dot'),
                    width: 6,
                    height: 6,
                    decoration: BoxDecoration(
                      color: accent,
                      shape: BoxShape.circle,
                      border: Border.all(
                        color: surfacePanel(scheme),
                        width: 1.2,
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ],
        ),
      );
    },
  );
}

/// A soft ring around the done button that swells with the input level, so
/// the button itself shows that the voice is being heard.
class _VoiceRing extends StatelessWidget {
  const _VoiceRing({required this.level, required this.color});

  final ValueNotifier<double> level;
  final Color color;

  @override
  Widget build(BuildContext context) => ValueListenableBuilder<double>(
    valueListenable: level,
    builder: (context, v, _) {
      final spread = Motion.ambientAllowed(context) ? 1 + v * 0.55 : 1.0;
      return Transform.scale(
        scale: spread,
        child: DecoratedBox(
          decoration: BoxDecoration(
            shape: BoxShape.circle,
            color: color.withValues(alpha: 0.10 + v * 0.22),
          ),
        ),
      );
    },
  );
}

/// The strip a take adds to the composer, above its toolbar: what the line
/// is doing, the microphone's live waveform, the elapsed time, and the
/// discard and done controls with their keys.
class DictationBar extends StatelessWidget {
  /// The bar for [line].
  const DictationBar({
    super.key,
    required this.line,
    required this.onFinish,
    required this.onCancel,
    this.showKeys = true,
  });

  /// The dictation line.
  final DictationLine line;

  /// Finish the take.
  final VoidCallback onFinish;

  /// Discard the take.
  final VoidCallback onCancel;

  /// Whether to name the keyboard keys (desktop).
  final bool showKeys;

  String _clock(Duration d) {
    final m = d.inMinutes;
    final s = d.inSeconds % 60;
    return '$m:${s.toString().padLeft(2, '0')}';
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: line,
    builder: (context, _) {
      final l10n = AppLocalizations.of(context);
      final scheme = Theme.of(context).colorScheme;
      final accent = dictationColor(scheme);
      final muted = scheme.onSurfaceVariant;
      final take = line.take;
      final listening = take == DictationTake.listening;
      final status = switch (take) {
        DictationTake.opening => l10n.dictationConnecting,
        DictationTake.finishing => l10n.dictationFinishing,
        _ => l10n.dictationListening,
      };
      return Container(
        key: const Key('dictation-bar'),
        height: 36,
        padding: const EdgeInsets.only(left: 10, right: 4),
        decoration: BoxDecoration(
          color: accent.withValues(alpha: 0.08),
          borderRadius: BorderRadius.circular(kRowRadius + 4),
        ),
        child: Row(
          children: [
            _Pulse(active: listening, color: accent),
            const SizedBox(width: 8),
            Flexible(
              flex: 2,
              child: Text(
                status,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  fontSize: 12.5,
                  fontWeight: FontWeight.w600,
                  color: listening ? accent : muted,
                ),
              ),
            ),
            const SizedBox(width: 10),
            Expanded(
              flex: 3,
              child: LayoutBuilder(
                builder: (context, c) => Align(
                  alignment: Alignment.centerLeft,
                  child: Waveform(
                    key: const Key('dictation-waveform'),
                    level: line.level,
                    color: accent,
                    active: listening,
                    // As many bars as fit: the waveform spans the strip.
                    bars: ((c.maxWidth + 2) / 5).floor().clamp(6, 160),
                    height: 20,
                    barWidth: 3,
                    gap: 2,
                  ),
                ),
              ),
            ),
            const SizedBox(width: 8),
            // Ticks with the level notifier, which updates at audio rate.
            ValueListenableBuilder<double>(
              valueListenable: line.level,
              builder: (context, _, _) => Text(
                _clock(line.elapsed),
                style: TextStyle(
                  fontFamily: monoFontFamily,
                  fontSize: 12,
                  color: muted,
                  fontFeatures: const [FontFeature.tabularFigures()],
                ),
              ),
            ),
            if (showKeys) ...[
              const SizedBox(width: 10),
              // The first thing to give way on a narrow card: it may shrink
              // to nothing, never push the controls out.
              Flexible(
                child: Text(
                  l10n.dictationKeys,
                  maxLines: 1,
                  overflow: TextOverflow.fade,
                  softWrap: false,
                  style: TextStyle(fontSize: 11.5, color: muted),
                ),
              ),
            ],
            const SizedBox(width: 2),
            IconButton(
              key: const Key('dictation-cancel'),
              tooltip: l10n.dictationCancel,
              onPressed: onCancel,
              icon: const Icon(Icons.close_rounded, size: 17),
              style: IconButton.styleFrom(
                minimumSize: const Size.square(30),
                fixedSize: const Size.square(30),
                padding: EdgeInsets.zero,
                foregroundColor: muted,
              ),
            ),
          ],
        ),
      );
    },
  );
}

/// A dot that beats while the microphone is open.
class _Pulse extends StatefulWidget {
  const _Pulse({required this.active, required this.color});

  final bool active;
  final Color color;

  @override
  State<_Pulse> createState() => _PulseState();
}

class _PulseState extends State<_Pulse> with SingleTickerProviderStateMixin {
  late final AnimationController _c = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 1100),
  );

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _sync();
  }

  @override
  void didUpdateWidget(_Pulse old) {
    super.didUpdateWidget(old);
    _sync();
  }

  void _sync() {
    final run = widget.active && Motion.ambientAllowed(context);
    if (run && !_c.isAnimating) {
      _c.repeat(reverse: true);
    } else if (!run && _c.isAnimating) {
      _c
        ..stop()
        ..value = 0;
    }
  }

  @override
  void dispose() {
    _c.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: _c,
    builder: (context, _) {
      final t = Curves.easeInOut.transform(_c.value);
      final color = widget.active
          ? widget.color
          : Theme.of(context).colorScheme.onSurfaceVariant;
      return SizedBox.square(
        dimension: 14,
        child: Stack(
          alignment: Alignment.center,
          children: [
            if (widget.active)
              Container(
                width: 8 + 6 * t,
                height: 8 + 6 * t,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: color.withValues(alpha: 0.28 * (1 - t)),
                ),
              ),
            Container(
              width: 8,
              height: 8,
              decoration: BoxDecoration(shape: BoxShape.circle, color: color),
            ),
          ],
        ),
      );
    },
  );
}
