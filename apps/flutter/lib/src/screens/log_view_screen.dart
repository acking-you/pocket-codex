import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/log_manager.dart';

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
      WidgetsBinding.instance.addPostFrameCallback(
        (_) => _scrollToBottom(jump: true),
      );
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
    ScaffoldMessenger.of(
      context,
    ).showSnackBar(SnackBar(content: Text(l10n.logsCopied(_filtered.length))));
  }

  static String _fmtTime(int ms) {
    final dt = DateTime.fromMillisecondsSinceEpoch(ms);
    String two(int n) => n.toString().padLeft(2, '0');
    return '${two(dt.hour)}:${two(dt.minute)}:${two(dt.second)}.'
        '${dt.millisecond.toString().padLeft(3, '0')}';
  }

  Color _levelColor(String level, bool dark) {
    switch (LogManager.normalizeLevel(level)) {
      case 'ERROR':
        return dark ? Colors.red.shade300 : Colors.red.shade700;
      case 'WARN':
        return dark ? Colors.orange.shade300 : Colors.orange.shade800;
      case 'INFO':
        return dark ? Colors.blue.shade300 : Colors.blue.shade700;
      case 'DEBUG':
        return dark ? Colors.green.shade300 : Colors.green.shade700;
      case 'TRACE':
        return dark ? Colors.grey.shade400 : Colors.grey.shade600;
      default:
        return dark ? Colors.white70 : Colors.black87;
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    return Scaffold(
      appBar: AppBar(
        title: Text(l10n.logsTitle),
        actions: [
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
      ),
      body: Column(
        children: [
          _toolbar(l10n),
          Expanded(child: _list(scheme)),
          _bottomBar(l10n, scheme),
        ],
      ),
      floatingActionButton: _followTail
          ? null
          : FloatingActionButton.small(
              tooltip: l10n.logsScrollBottom,
              onPressed: () {
                setState(() => _followTail = true);
                _scrollToBottom();
              },
              child: const Icon(Icons.arrow_downward),
            ),
    );
  }

  Widget _toolbar(AppLocalizations l10n) => Padding(
    padding: const EdgeInsets.fromLTRB(12, 12, 12, 4),
    child: Row(
      children: [
        SizedBox(
          width: 150,
          child: DropdownButtonFormField<String?>(
            initialValue: _levelFilter,
            isExpanded: true,
            decoration: InputDecoration(
              labelText: l10n.logsLevel,
              border: const OutlineInputBorder(),
              isDense: true,
            ),
            items: [
              DropdownMenuItem<String?>(
                value: null,
                child: Text(l10n.logsLevelAll),
              ),
              ...LogManager.levels.map(
                (lvl) => DropdownMenuItem<String?>(
                  value: lvl,
                  child: Text(LogManager.thresholdLabel(lvl)),
                ),
              ),
            ],
            onChanged: (value) {
              _levelFilter = value;
              _applyFilters();
            },
          ),
        ),
        const SizedBox(width: 12),
        Expanded(
          child: TextField(
            controller: _keywordCtrl,
            onChanged: _onKeywordChanged,
            decoration: InputDecoration(
              labelText: l10n.logsKeyword,
              hintText: l10n.logsKeywordHint,
              prefixIcon: const Icon(Icons.search),
              isDense: true,
              suffixIcon: _keyword.isEmpty
                  ? null
                  : IconButton(
                      icon: const Icon(Icons.clear),
                      onPressed: () {
                        _keywordCtrl.clear();
                        _keyword = '';
                        _applyFilters();
                      },
                    ),
              border: const OutlineInputBorder(),
            ),
          ),
        ),
      ],
    ),
  );

  Widget _list(ColorScheme scheme) {
    final dark = Theme.of(context).brightness == Brightness.dark;
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
                  style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
                  children: [
                    TextSpan(
                      text: '${log.level.padRight(5)} ',
                      style: TextStyle(
                        fontWeight: FontWeight.bold,
                        color: _levelColor(log.level, dark),
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
