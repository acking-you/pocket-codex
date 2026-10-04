import 'package:flutter/material.dart';

import '../desktop_theme.dart';

/// Highlights newly revealed history without replacing a row's stateful subtree.
class HistoryArrival extends StatefulWidget {
  const HistoryArrival({
    super.key,
    required this.revision,
    required this.child,
  });

  final int? revision;
  final Widget child;

  @override
  State<HistoryArrival> createState() => _HistoryArrivalState();
}

class _HistoryArrivalState extends State<HistoryArrival>
    with SingleTickerProviderStateMixin {
  late final _fade = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 1400),
  );
  bool _initialized = false;

  void _play() {
    if (widget.revision != null && !MediaQuery.disableAnimationsOf(context)) {
      _fade.reverse(from: 1);
    } else {
      _fade.reset();
    }
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (!_initialized || MediaQuery.disableAnimationsOf(context)) _play();
    _initialized = true;
  }

  @override
  void didUpdateWidget(HistoryArrival oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.revision != widget.revision) _play();
  }

  @override
  void dispose() {
    _fade.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: _fade,
    builder: (context, child) => DecoratedBox(
      decoration: BoxDecoration(
        color: Theme.of(
          context,
        ).colorScheme.primary.withValues(alpha: _fade.value * 0.12),
        borderRadius: BorderRadius.circular(kControlRadius),
      ),
      child: child,
    ),
    child: widget.child,
  );
}
