import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/motion.dart';
import 'package:pocket_codex/src/voice/dictation.dart';
import 'package:pocket_codex/src/voice/waveform.dart';

/// The composer's dictation microphone: starts a take, and while one is
/// recording or transcribing shows that state instead.
class DictationButton extends StatelessWidget {
  /// A microphone for [controller]; [onFinish] stops and inserts the take.
  const DictationButton({
    super.key,
    required this.controller,
    required this.onFinish,
    this.enabled = true,
    this.size = 32,
    this.iconSize = 18,
  });

  /// The dictation.
  final DictationController controller;

  /// Stop recording and insert what was said.
  final VoidCallback onFinish;

  /// False while the composer cannot take input.
  final bool enabled;

  /// Hit target side.
  final double size;

  /// Glyph size.
  final double iconSize;

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: controller,
    builder: (context, _) {
      final l10n = AppLocalizations.of(context);
      final scheme = Theme.of(context).colorScheme;
      final phase = controller.phase;
      final (icon, tooltip, onPressed) = switch (phase) {
        DictationPhase.idle => (
          Icons.mic_none_rounded,
          l10n.dictate,
          enabled ? controller.start : null,
        ),
        DictationPhase.starting => (
          Icons.mic_none_rounded,
          l10n.dictationStarting,
          controller.cancel,
        ),
        DictationPhase.recording => (
          Icons.stop_rounded,
          l10n.dictationStop,
          onFinish,
        ),
        DictationPhase.transcribing => (
          Icons.mic_none_rounded,
          l10n.dictationTranscribing,
          null,
        ),
      };
      final busy =
          phase == DictationPhase.starting ||
          phase == DictationPhase.transcribing;
      return IconButton(
        key: const Key('dictate'),
        tooltip: tooltip,
        onPressed: onPressed,
        isSelected: phase == DictationPhase.recording,
        icon: AnimatedSwitcher(
          duration: Motion.of(context, Motion.fast),
          child: busy
              ? SizedBox(
                  key: const ValueKey('busy'),
                  width: iconSize - 4,
                  height: iconSize - 4,
                  child: CircularProgressIndicator(
                    strokeWidth: 1.8,
                    color: scheme.onSurfaceVariant,
                  ),
                )
              : Icon(icon, key: ValueKey(icon), size: iconSize),
        ),
        style: IconButton.styleFrom(
          minimumSize: Size(size, size),
          fixedSize: Size(size, size),
          padding: EdgeInsets.zero,
          foregroundColor: phase == DictationPhase.recording
              ? scheme.onTertiary
              : null,
          backgroundColor: phase == DictationPhase.recording
              ? scheme.tertiary
              : null,
        ),
      );
    },
  );
}

/// Shown in place of the composer's text field while a take is recording or
/// being transcribed: a live waveform of the microphone, the elapsed time,
/// and discard / finish controls.
class DictationStrip extends StatelessWidget {
  /// The strip for [controller].
  const DictationStrip({
    super.key,
    required this.controller,
    required this.onFinish,
  });

  /// The dictation.
  final DictationController controller;

  /// Stop and insert the text.
  final VoidCallback onFinish;

  String _clock(Duration d) {
    final m = d.inMinutes;
    final s = d.inSeconds % 60;
    return '$m:${s.toString().padLeft(2, '0')}';
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: controller,
    builder: (context, _) {
      final l10n = AppLocalizations.of(context);
      final scheme = Theme.of(context).colorScheme;
      final recording = controller.phase == DictationPhase.recording;
      final muted = scheme.onSurfaceVariant;
      return Container(
        key: const Key('dictation-strip'),
        height: 40,
        padding: const EdgeInsets.only(left: 4),
        child: Row(
          children: [
            IconButton(
              key: const Key('dictation-cancel'),
              tooltip: l10n.dictationCancel,
              onPressed: controller.cancel,
              icon: const Icon(Icons.close_rounded, size: 18),
              style: IconButton.styleFrom(
                minimumSize: const Size(30, 30),
                fixedSize: const Size(30, 30),
                padding: EdgeInsets.zero,
                foregroundColor: muted,
              ),
            ),
            const SizedBox(width: 6),
            Expanded(
              child: LayoutBuilder(
                builder: (context, c) => Align(
                  alignment: Alignment.centerLeft,
                  child: recording
                      ? Waveform(
                          key: const Key('dictation-waveform'),
                          level: controller.level,
                          color: scheme.tertiary,
                          // As many bars as fit: the waveform spans the field.
                          bars: ((c.maxWidth + 2) / 5).floor().clamp(8, 160),
                          height: 24,
                          barWidth: 3,
                          gap: 2,
                        )
                      : Text(
                          controller.phase == DictationPhase.starting
                              ? l10n.dictationStarting
                              : l10n.dictationTranscribing,
                          style: TextStyle(fontSize: 13, color: muted),
                        ),
                ),
              ),
            ),
            if (recording) ...[
              const SizedBox(width: 8),
              // Ticks with the waveform's rebuilds of the level notifier.
              ValueListenableBuilder<double>(
                valueListenable: controller.level,
                builder: (context, _, _) => Text(
                  _clock(controller.elapsed),
                  style: TextStyle(
                    fontFamily: monoFontFamily,
                    fontSize: 12,
                    color: muted,
                    fontFeatures: const [FontFeature.tabularFigures()],
                  ),
                ),
              ),
            ],
          ],
        ),
      );
    },
  );
}
