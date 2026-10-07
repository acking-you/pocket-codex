import 'package:flutter/material.dart';
import 'package:pocket_codex/src/motion.dart';

/// A one-line label that cross-fades, with a short rise, when its text
/// changes. A picker chip saying "High" and then "Medium" shows that the
/// setting moved instead of just flickering to a new word.
///
/// [resize] also eases the width between the old and the new text, for a
/// label whose box hugs it (a chip); leave it off when the label sits in a
/// fixed slot.
class AnimatedLabel extends StatelessWidget {
  /// Shows [text] in [style].
  const AnimatedLabel(
    this.text, {
    super.key,
    this.style,
    this.alignment = AlignmentDirectional.centerStart,
    this.textAlign,
    this.resize = true,
  });

  /// The label.
  final String text;

  /// Its style.
  final TextStyle? style;

  /// Where the outgoing and incoming text line up while they overlap.
  final AlignmentGeometry alignment;

  /// Alignment inside the label's own box.
  final TextAlign? textAlign;

  /// Animate the width as the text changes.
  final bool resize;

  @override
  Widget build(BuildContext context) {
    final duration = Motion.of(context, Motion.medium);
    final switcher = AnimatedSwitcher(
      duration: duration,
      switchInCurve: Motion.enter,
      switchOutCurve: Curves.easeIn,
      layoutBuilder: (current, previous) =>
          Stack(alignment: alignment, children: [...previous, ?current]),
      transitionBuilder: (child, animation) => FadeTransition(
        opacity: animation,
        child: SlideTransition(
          position: Tween(
            begin: const Offset(0, 0.25),
            end: Offset.zero,
          ).animate(animation),
          child: child,
        ),
      ),
      child: Text(
        text,
        key: ValueKey(text),
        style: style,
        textAlign: textAlign,
        textWidthBasis: TextWidthBasis.longestLine,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        softWrap: false,
      ),
    );
    if (!resize) return switcher;
    // AnimatedSize reports its animated width even when the slot it sits in
    // is narrower, which overflows a squeezed chip. Clip to the incoming
    // constraints so the label can still ellipsize while it eases.
    return ClipRect(
      child: AnimatedSize(
        duration: duration,
        curve: Motion.move,
        alignment: alignment,
        child: switcher,
      ),
    );
  }
}
