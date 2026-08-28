import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/fonts.dart';
import 'package:pocket_codex/src/log_manager.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/widgets/app_toast.dart';
import 'package:pocket_codex/src/widgets/search_field.dart';
import 'package:pocket_codex/src/widgets/status_dots.dart';
import 'package:pocket_codex/src/widgets/utility_page.dart';

/// Real-time viewer for the app's captured runtime logs (`tracing` events from
/// the Rust bridge — hosting, tunnels, sessions, embedded codex, …). Reads the
/// shared [LogManager] buffer, with a minimum-level threshold + keyword filter,
/// and auto-follows the tail while pinned to the bottom.
class LogViewScreen extends StatefulWidget {
  /// Creates the log viewer.
  const LogViewScreen({super.key});

  @override
  State<LogViewScreen> createState() => _LogViewScreenState();
}

class _LogViewScreenState extends State<LogViewScreen> {
  final LogManager _logs = LogManager.instance;
  final ScrollController _scroll = ScrollController();
  final TextEditingController _keywordCtrl = TextEditingController();

  StreamSubscription<List<LogLine>>? _sub;
  Timer? _keywordDebounce;
  String? _levelFilter;
  String _keyword = '';
  bool _followTail = true;
  List<LogLine> _filtered = [];

  @override
  void initState() {
    super.initState();
    _filtered = _logs.filter();
    _sub = _logs.stream.listen((_) => _applyFilters(scrollToTail: true));
  }

  @override
  void dispose() {
    _sub?.cancel();
    _keywordDebounce?.cancel();
    _scroll.dispose();
    _keywordCtrl.dispose();
    super.dispose();
  }

  void _applyFilters({bool scrollToTail = false}) {
    final next = _logs.filter(level: _levelFilter, keyword: _keyword);
    if (!mounted) return;
    setState(() => _filtered = next);
    if (scrollToTail && _followTail) {
      // Guard `mounted`: a post-frame callback isn't cancelled on dispose, and
      // _scrollToBottom would touch the disposed ScrollController.
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) _scrollToBottom(jump: true);
      });
    }
  }

  void _onKeywordChanged(String value) {
    _keywordDebounce?.cancel();
    _keywordDebounce = Timer(const Duration(milliseconds: 150), () {
      _keyword = value.trim();
      _applyFilters();
    });
  }

  void _scrollToBottom({bool jump = false}) {
    if (!_scroll.hasClients || !_scroll.position.hasContentDimensions) return;
    final target = _scroll.position.maxScrollExtent;
    if (jump) {
      _scroll.jumpTo(target);
    } else {
      _scroll.animateTo(
        target,
        duration: const Duration(milliseconds: 200),
        curve: Curves.easeOut,
      );
    }
  }

  Future<void> _copy() async {
    final text = _filtered
        .map(
          (l) =>
              '[${l.level}] ${_fmtTime(l.timestampMs)} ${l.target}: ${l.message}',
        )
        .join('\n');
    await Clipboard.setData(ClipboardData(text: text));
    if (!mounted) return;
    final l10n = AppLocalizations.of(context);
    showToastOk(context, l10n.logsCopied(_filtered.length));
  }

  static String _fmtTime(int ms) {
    final dt = DateTime.fromMillisecondsSinceEpoch(ms);
    String two(int n) => n.toString().padLeft(2, '0');
    return '${two(dt.hour)}:${two(dt.minute)}:${two(dt.second)}.'
        '${dt.millisecond.toString().padLeft(3, '0')}';
  }

  /// Level colours come from the app's own palette rather than Material's stock
  /// hues: error and caution are already named, and info/debug borrow the syntax
  /// palette's blue and green, which are tuned to sit with the warm neutrals.
  Color _levelColor(String level, ColorScheme scheme) =>
      switch (LogManager.normalizeLevel(level)) {
        'ERROR' => scheme.error,
        'WARN' => cautionColor(scheme),
        'INFO' => infoColor(scheme),
        'DEBUG' => successColor(scheme),
        'TRACE' => onSurfaceMuted(scheme),
        _ => scheme.onSurfaceVariant,
      };

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final desktop = isDesktop && MediaQuery.sizeOf(context).width >= 840;
    final content = Column(
      children: [
        _toolbar(l10n),
        Expanded(
          child: desktop
              ? Container(
                  margin: const EdgeInsets.fromLTRB(12, 4, 12, 0),
                  decoration: BoxDecoration(
                    color: scheme.surfaceBright,
                    borderRadius: BorderRadius.circular(12),
                    border: Border.all(color: scheme.outlineVariant),
                  ),
                  clipBehavior: Clip.antiAlias,
                  child: _list(scheme),
                )
              : _list(scheme),
        ),
        _bottomBar(l10n, scheme),
      ],
    );
    return UtilityPage(
      route: '/logs',
      title: l10n.logsTitle,
      actions: [
        if (desktop)
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 6),
            child: StatusChip(
              color: successColor(scheme),
              label: l10n.logsLive,
              filled: true,
            ),
          ),
        IconButton(
          tooltip: l10n.logsCopy,
          icon: const Icon(Icons.copy_all_outlined),
          onPressed: _filtered.isEmpty ? null : _copy,
        ),
        IconButton(
          tooltip: l10n.logsClear,
          icon: const Icon(Icons.clear_all),
          onPressed: () {
            _logs.clear();
            _applyFilters();
          },
        ),
      ],
      body: desktop
          ? Center(
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 1120),
                child: SizedBox.expand(child: content),
              ),
            )
          : content,
      // A quiet raised pill rather than a Material FAB: the conversation's own
      // jump-to-latest reads this way, and a tinted circle would be the loudest
      // thing on a page of monospace text.
      floatingActionButton: _followTail
          ? null
          : DecoratedBox(
              decoration: BoxDecoration(
                borderRadius: BorderRadius.circular(kPanelRadius),
                boxShadow: panelShadow(scheme),
              ),
              child: Material(
                elevation: 0,
                color: scheme.surfaceBright,
                shape: RoundedRectangleBorder(
                  borderRadius: BorderRadius.circular(kPanelRadius),
                  side: BorderSide(color: scheme.outline),
                ),
                child: InkWell(
                  mouseCursor: clickable,
                  borderRadius: BorderRadius.circular(kPanelRadius),
                  onTap: () {
                    setState(() => _followTail = true);
                    _scrollToBottom();
                  },
                  child: Padding(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 12,
                      vertical: 9,
                    ),
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        const Icon(Icons.arrow_downward, size: 16),
                        const SizedBox(width: 7),
                        Text(
                          l10n.logsScrollBottom,
                          style: const TextStyle(
                            fontSize: 12.5,
                            fontWeight: FontWeight.w500,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
            ),
    );
  }

  Widget _toolbar(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    // Level reads as a set of choices rather than a form field, so it's a chip
    // row like the sessions filters — one tap instead of open-scroll-pick.
    Widget levelChip(String? value, String label) => Padding(
      padding: const EdgeInsets.only(right: 6),
      child: ChoiceChip(
        key: Key('log-level-${value ?? 'all'}'),
        label: Text(label),
        selected: _levelFilter == value,
        showCheckmark: false,
        side: BorderSide(color: scheme.outlineVariant),
        onSelected: (_) {
          _levelFilter = value;
          _applyFilters();
        },
      ),
    );
    final chips = [
      levelChip(null, l10n.logsLevelAll),
      for (final lvl in LogManager.levels)
        levelChip(lvl, LogManager.thresholdLabel(lvl)),
    ];
    // Six thresholds plus the keyword box exceed a phone's width, so the
    // thresholds drop to their own scrolling row instead of being squeezed.
    final stacked = MediaQuery.sizeOf(context).width < 720;
    final search = SearchField(
      controller: _keywordCtrl,
      hintText: l10n.logsKeywordHint,
      onChanged: _onKeywordChanged,
      // Track the live controller text, not the debounced `_keyword`, so
      // the clear button appears/disappears immediately as you type.
      suffix: ValueListenableBuilder<TextEditingValue>(
        valueListenable: _keywordCtrl,
        builder: (_, value, _) => value.text.isEmpty
            ? const SizedBox.shrink()
            : IconButton(
                icon: const Icon(Icons.clear, size: 17),
                tooltip: l10n.cancel,
                onPressed: () {
                  _keywordCtrl.clear();
                  _keyword = '';
                  _applyFilters();
                },
              ),
      ),
    );
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 12, 12, 4),
      child: stacked
          ? Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                search,
                const SizedBox(height: 8),
                SingleChildScrollView(
                  scrollDirection: Axis.horizontal,
                  child: Row(children: chips),
                ),
              ],
            )
          : Row(
              children: [
                Expanded(child: search),
                const SizedBox(width: 10),
                ...chips,
              ],
            ),
    );
  }

  Widget _list(ColorScheme scheme) {
    if (_filtered.isEmpty) {
      return Center(
        child: Text(
          AppLocalizations.of(context).logsEmpty,
          style: TextStyle(color: scheme.onSurfaceVariant),
        ),
      );
    }
    return NotificationListener<ScrollNotification>(
      onNotification: (_) {
        if (!_scroll.hasClients || !_scroll.position.hasContentDimensions) {
          return false;
        }
        final toBottom =
            _scroll.position.maxScrollExtent - _scroll.position.pixels;
        if (toBottom > 48 && _followTail) {
          setState(() => _followTail = false);
        } else if (toBottom <= 8 && !_followTail) {
          setState(() => _followTail = true);
        }
        return false;
      },
      child: Scrollbar(
        controller: _scroll,
        child: ListView.builder(
          controller: _scroll,
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
          itemCount: _filtered.length,
          itemBuilder: (context, i) {
            final log = _filtered[i];
            return Padding(
              padding: const EdgeInsets.only(bottom: 3),
              child: SelectableText.rich(
                TextSpan(
                  style: const TextStyle(
                    fontFamily: monoFontFamily,
                    fontSize: 12,
                  ),
                  children: [
                    TextSpan(
                      text: '${log.level.padRight(5)} ',
                      style: TextStyle(
                        fontWeight: FontWeight.w600,
                        color: _levelColor(log.level, scheme),
                      ),
                    ),
                    TextSpan(
                      text: '${_fmtTime(log.timestampMs)} ',
                      style: TextStyle(color: scheme.onSurfaceVariant),
                    ),
                    TextSpan(
                      text: '${log.target}: ',
                      style: TextStyle(
                        color: scheme.onSurfaceVariant,
                        fontStyle: FontStyle.italic,
                      ),
                    ),
                    TextSpan(
                      text: log.message,
                      style: TextStyle(color: scheme.onSurface),
                    ),
                  ],
                ),
              ),
            );
          },
        ),
      ),
    );
  }

  Widget _bottomBar(AppLocalizations l10n, ColorScheme scheme) => Container(
    padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
    decoration: BoxDecoration(
      border: Border(top: BorderSide(color: scheme.outlineVariant)),
    ),
    child: Row(
      children: [
        Icon(
          _followTail ? Icons.vertical_align_bottom : Icons.pause,
          size: 16,
          color: scheme.onSurfaceVariant,
        ),
        const SizedBox(width: 8),
        Text(
          l10n.logsVisible(_filtered.length, _logs.count),
          style: TextStyle(color: scheme.onSurfaceVariant, fontSize: 12),
        ),
      ],
    ),
  );
}
