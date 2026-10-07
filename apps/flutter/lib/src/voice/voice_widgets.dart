import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/voice/voice_controller.dart';

/// Session-only speech settings; an empty model preserves the host default.
class VoiceSettings {
  const VoiceSettings({this.model, this.voice});
  final String? model;
  final String? voice;
}

/// Loads the host's actual catalog, keeping unsupported hosts retryable.
class VoiceSetupDialog extends StatefulWidget {
  const VoiceSetupDialog({
    super.key,
    required this.api,
    required this.serviceKey,
  });
  final BridgeApi api;
  final String serviceKey;
  @override
  State<VoiceSetupDialog> createState() => _VoiceSetupDialogState();
}

/// Realtime models offered in the setup dialog.
///
/// The app-server has no model catalogue for realtime (only voices), so this
/// is the list the protocol itself names for the version Pocket-Codex speaks
/// (V3, `gpt-live-1-codex` is its built-in default). "Host default" sends no
/// model and lets the host's `experimental_realtime_ws_model` decide; a custom
/// id stays available for a host configured with something else.
const voiceModels = <String>['gpt-live-1-codex'];

/// Sentinel for the "custom model id" entry in the model dropdown.
const _customModel = '__custom__';

class _VoiceSetupDialogState extends State<VoiceSetupDialog> {
  final _model = TextEditingController();
  // Null = host default; [_customModel] = typed id in [_model].
  String? _modelChoice;
  List<String>? _voices;
  String? _voice;
  String? _error;
  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _voices = null;
      _error = null;
    });
    try {
      final raw = await widget.api.appRealtimeRequest(
        widget.serviceKey,
        'thread/realtime/listVoices',
        '{}',
      );
      final voices = (jsonDecode(raw) as Map)['voices'] as Map;
      // Upstream V3 uses the V1 voice catalog; V2 names are not interchangeable.
      final names = (voices['v1'] as List).cast<String>();
      if (names.isEmpty) {
        throw StateError('The host returned no compatible voices');
      }
      if (mounted) {
        setState(() {
          _voices = names;
          _voice = names.contains(voices['defaultV1'])
              ? voices['defaultV1'] as String
              : names.first;
        });
      }
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
  }

  @override
  void dispose() {
    _model.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    return AlertDialog(
      title: Text(l.voiceLive),
      content: SizedBox(
        width: 400,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(l.voiceStartHint),
              const SizedBox(height: 16),
              if (_error != null) ...[
                Text(
                  l.voiceUnavailable,
                  style: TextStyle(color: Theme.of(context).colorScheme.error),
                ),
                Text(_error!, style: Theme.of(context).textTheme.bodySmall),
                TextButton(onPressed: _load, child: Text(l.retry)),
              ] else if (_voices == null)
                const LinearProgressIndicator()
              else ...[
                DropdownButtonFormField<String>(
                  initialValue: _voice,
                  isExpanded: true,
                  decoration: InputDecoration(labelText: l.voiceVoice),
                  items: [
                    for (final voice in _voices!)
                      DropdownMenuItem(value: voice, child: Text(voice)),
                  ],
                  onChanged: (value) => setState(() => _voice = value),
                ),
                const SizedBox(height: 12),
                DropdownButtonFormField<String?>(
                  key: const Key('voice-model'),
                  initialValue: _modelChoice,
                  isExpanded: true,
                  decoration: InputDecoration(labelText: l.voiceModel),
                  items: [
                    DropdownMenuItem(child: Text(l.voiceModelDefault)),
                    for (final model in voiceModels)
                      DropdownMenuItem(
                        value: model,
                        child: Text(
                          model == 'gpt-live-1-codex'
                              ? l.voiceModelFrameless
                              : model,
                        ),
                      ),
                    DropdownMenuItem(
                      value: _customModel,
                      child: Text(l.voiceModelCustom),
                    ),
                  ],
                  onChanged: (value) => setState(() => _modelChoice = value),
                ),
                if (_modelChoice == _customModel) ...[
                  const SizedBox(height: 8),
                  TextField(
                    key: const Key('voice-model-custom'),
                    controller: _model,
                    autofocus: true,
                    decoration: InputDecoration(
                      labelText: l.voiceModelCustomLabel,
                      helperText: l.voiceModelHint,
                    ),
                    maxLength: 128,
                  ),
                ],
                const SizedBox(height: 4),
                Text(
                  l.voiceTaskContinues,
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: Theme.of(context).colorScheme.onSurfaceVariant,
                  ),
                ),
              ],
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: Text(l.cancel),
        ),
        FilledButton(
          key: const Key('voice-start-confirm'),
          onPressed: _voices == null
              ? null
              : () => Navigator.pop(
                  context,
                  VoiceSettings(
                    model: switch (_modelChoice) {
                      _customModel => _model.text.trim(),
                      final choice => choice,
                    },
                    voice: _voice,
                  ),
                ),
          child: Text(l.voiceStart),
        ),
      ],
    );
  }
}

/// What the call is doing, in one line: connection state first, then who
/// holds the floor. Shared by the panel above the composer and the sidebar
/// status entry so the two never disagree.
String voiceStatusLabel(AppLocalizations l, VoiceController c) =>
    switch (c.phase) {
      VoicePhase.connecting => l.voiceConnecting,
      VoicePhase.reconnecting => l.voiceReconnecting,
      VoicePhase.active when c.muted => l.voiceMuted,
      VoicePhase.active => switch (c.turn) {
        VoiceTurn.user => l.voiceUserSpeaking,
        VoiceTurn.assistant => l.voiceSpeaking,
        VoiceTurn.listening => l.voiceListening,
      },
      VoicePhase.stopping => l.voiceStopping,
      VoicePhase.failed => l.voiceFailedShort,
      VoicePhase.idle => switch (c.endedReason) {
        final reason? => l.voiceEndedReason(reason),
        null => c.endedByUser ? l.voiceEndedByUser : l.voiceSessionEnded,
      },
    };

/// The call's persistent status entry: a pinned strip outside any scrolling
/// list, present for as long as a call is up (and briefly after it ends, so
/// the outcome is seen), with mute and hang-up one tap away.
///
/// [compact] is the icon-only form for the window strip when the sidebar is
/// collapsed. [title] names the conversation the call belongs to; [onOpen]
/// switches to it.
class VoiceStatusEntry extends StatefulWidget {
  const VoiceStatusEntry({
    super.key,
    required this.controller,
    this.title,
    this.onOpen,
    this.compact = false,
  });
  final VoiceController controller;
  final String? title;
  final VoidCallback? onOpen;
  final bool compact;

  @override
  State<VoiceStatusEntry> createState() => _VoiceStatusEntryState();
}

class _VoiceStatusEntryState extends State<VoiceStatusEntry> {
  // An ended call lingers this long, so its outcome registers.
  static const _linger = Duration(seconds: 6);
  Timer? _hide;
  bool _showEnded = false;
  VoicePhase? _last;

  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_phase);
  }

  @override
  void didUpdateWidget(VoiceStatusEntry old) {
    super.didUpdateWidget(old);
    if (!identical(old.controller, widget.controller)) {
      old.controller.removeListener(_phase);
      widget.controller.addListener(_phase);
    }
  }

  @override
  void dispose() {
    _hide?.cancel();
    widget.controller.removeListener(_phase);
    super.dispose();
  }

  void _phase() {
    final phase = widget.controller.phase;
    if (phase == _last) return;
    final wasLive = _last != null && _last != VoicePhase.idle;
    _last = phase;
    _hide?.cancel();
    final ended = phase == VoicePhase.idle || phase == VoicePhase.failed;
    if (ended && wasLive) {
      setState(() => _showEnded = true);
      // A failure stays until the user dismisses it or starts again: it is
      // the one outcome they must not miss.
      if (phase == VoicePhase.idle) {
        _hide = Timer(_linger, () {
          if (mounted) setState(() => _showEnded = false);
        });
      }
    } else if (!ended) {
      setState(() => _showEnded = false);
    }
  }

  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: widget.controller,
    builder: (context, _) {
      final c = widget.controller;
      final visible = c.busy || c.phase == VoicePhase.stopping || _showEnded;
      return AnimatedSize(
        duration: const Duration(milliseconds: 200),
        curve: Curves.easeOutCubic,
        alignment: Alignment.topCenter,
        child: visible
            ? _body(context, c)
            : widget.compact
            ? const SizedBox.shrink()
            : const SizedBox(width: double.infinity),
      );
    },
  );

  Widget _body(BuildContext context, VoiceController c) {
    final l = AppLocalizations.of(context);
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final failed = c.phase == VoicePhase.failed;
    final live = c.phase == VoicePhase.active;
    final tone = failed
        ? scheme.error
        : c.phase == VoicePhase.reconnecting
        ? (scheme.brightness == Brightness.light
              ? const Color(0xFF9A5B00)
              : const Color(0xFFE0A647))
        : c.busy
        ? scheme.tertiary
        : scheme.onSurfaceVariant;
    final label = voiceStatusLabel(l, c);
    final canMute = live || c.phase == VoicePhase.reconnecting;
    Widget action({
      required Key key,
      required String tooltip,
      required IconData icon,
      required VoidCallback onPressed,
      Color? color,
      Color? background,
    }) => IconButton(
      key: key,
      tooltip: tooltip,
      onPressed: onPressed,
      icon: Icon(icon, size: 16),
      style: IconButton.styleFrom(
        minimumSize: const Size(28, 28),
        fixedSize: const Size(28, 28),
        padding: EdgeInsets.zero,
        foregroundColor: color ?? scheme.onSurfaceVariant,
        backgroundColor: background,
      ),
    );
    final controls = [
      if (canMute)
        action(
          key: const Key('voice-status-mute'),
          tooltip: c.muted ? l.voiceUnmute : l.voiceMute,
          icon: c.muted ? Icons.mic_off_rounded : Icons.mic_none_rounded,
          onPressed: c.toggleMuted,
          color: c.muted ? scheme.error : null,
        ),
      if (c.busy)
        action(
          key: const Key('voice-status-hangup'),
          tooltip: l.voiceHangUp,
          icon: Icons.call_end_rounded,
          onPressed: c.hangUp,
          color: Colors.white,
          background: const Color(0xFFD93A3A),
        )
      else
        action(
          key: const Key('voice-status-dismiss'),
          tooltip: MaterialLocalizations.of(context).closeButtonLabel,
          icon: Icons.close_rounded,
          onPressed: () => setState(() => _showEnded = false),
        ),
    ];
    final indicator = _VoiceLevel(
      color: tone,
      active: live && !c.muted,
      speaking: c.turn != VoiceTurn.listening,
    );
    if (widget.compact) {
      return Tooltip(
        message: [label, ?widget.title].join('\n'),
        child: Container(
          key: const Key('voice-status-compact'),
          height: 30,
          padding: const EdgeInsets.only(left: 8, right: 2),
          decoration: BoxDecoration(
            color: tone.withValues(alpha: 0.10),
            borderRadius: BorderRadius.circular(15),
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [indicator, const SizedBox(width: 4), ...controls],
          ),
        ),
      );
    }
    return Padding(
      padding: const EdgeInsets.fromLTRB(8, 4, 8, 4),
      child: Material(
        key: const Key('voice-status'),
        color: tone.withValues(alpha: 0.09),
        borderRadius: BorderRadius.circular(10),
        child: InkWell(
          borderRadius: BorderRadius.circular(10),
          onTap: widget.onOpen,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(10, 6, 4, 6),
            child: Row(
              children: [
                indicator,
                const SizedBox(width: 8),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      AnimatedSwitcher(
                        duration: const Duration(milliseconds: 160),
                        child: Text(
                          label,
                          key: ValueKey(label),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: theme.textTheme.bodySmall?.copyWith(
                            fontWeight: FontWeight.w600,
                            color: failed ? scheme.error : scheme.onSurface,
                          ),
                        ),
                      ),
                      if (widget.title case final title?)
                        Text(
                          title,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: theme.textTheme.bodySmall?.copyWith(
                            fontSize: 11,
                            color: scheme.onSurfaceVariant,
                          ),
                        ),
                    ],
                  ),
                ),
                ...controls,
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// Three bars that breathe while the call is listening and move faster while
/// someone is speaking; still when muted, connecting or ended.
class _VoiceLevel extends StatefulWidget {
  const _VoiceLevel({
    required this.color,
    required this.active,
    required this.speaking,
  });
  final Color color;
  final bool active;
  final bool speaking;

  @override
  State<_VoiceLevel> createState() => _VoiceLevelState();
}

class _VoiceLevelState extends State<_VoiceLevel>
    with SingleTickerProviderStateMixin {
  late final AnimationController _t = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 1400),
  );

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _sync();
  }

  @override
  void didUpdateWidget(_VoiceLevel old) {
    super.didUpdateWidget(old);
    _sync();
  }

  void _sync() {
    final animate = widget.active && !MediaQuery.disableAnimationsOf(context);
    _t.duration = Duration(milliseconds: widget.speaking ? 700 : 1400);
    if (animate && !_t.isAnimating) {
      _t.repeat();
    } else if (!animate && _t.isAnimating) {
      _t.stop();
      _t.value = 0;
    }
  }

  @override
  void dispose() {
    _t.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => SizedBox(
    width: 16,
    height: 16,
    child: AnimatedBuilder(
      animation: _t,
      builder: (context, _) => Row(
        mainAxisAlignment: MainAxisAlignment.spaceEvenly,
        children: [
          for (var i = 0; i < 3; i++)
            Container(
              width: 3,
              height: widget.active
                  ? 5 +
                        9 *
                            (0.5 +
                                    0.5 *
                                        math.sin(
                                          (_t.value + i / 3) * 2 * math.pi,
                                        ))
                                .abs()
                  : (i == 1 ? 9 : 6),
              decoration: BoxDecoration(
                color: widget.color,
                borderRadius: BorderRadius.circular(2),
              ),
            ),
        ],
      ),
    ),
  );
}

/// Compact controls leave the transcript and approval cards accessible.
class VoicePanel extends StatelessWidget {
  const VoicePanel({
    super.key,
    required this.controller,
    required this.onHistory,
    required this.onRetry,
  });
  final VoiceController controller;
  final VoidCallback onHistory;
  final VoidCallback? onRetry;
  @override
  Widget build(BuildContext context) => AnimatedBuilder(
    animation: controller,
    builder: (context, _) {
      final l = AppLocalizations.of(context);
      final c = controller;
      final label = voiceStatusLabel(l, c);
      final last = c.partial.entries.lastOrNull;
      final text = last?.value ?? c.transcript.lastOrNull?.text;
      return Card(
        margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  const Icon(Icons.graphic_eq, size: 20),
                  const SizedBox(width: 8),
                  Expanded(child: Text(label, maxLines: 2)),
                  IconButton(
                    tooltip: l.voiceHistory,
                    onPressed: onHistory,
                    icon: const Icon(Icons.history),
                  ),
                  if (c.phase == VoicePhase.active ||
                      c.phase == VoicePhase.reconnecting)
                    IconButton(
                      key: const Key('voice-mute'),
                      tooltip: c.muted ? l.voiceUnmute : l.voiceMute,
                      onPressed: c.toggleMuted,
                      icon: Icon(c.muted ? Icons.mic_off : Icons.mic),
                    ),
                  if (c.busy)
                    IconButton(
                      key: const Key('voice-stop'),
                      tooltip: l.voiceStop,
                      onPressed: c.hangUp,
                      icon: const Icon(Icons.call_end),
                    )
                  else if (c.phase != VoicePhase.stopping)
                    IconButton(
                      tooltip: l.voiceStart,
                      onPressed: onRetry,
                      icon: const Icon(Icons.play_arrow),
                    ),
                ],
              ),
              if (text != null && text.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(bottom: 8),
                  child: Text(
                    text,
                    maxLines: 3,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
              if (c.error != null)
                ExpansionTile(
                  title: Text(l.voiceErrorDetails),
                  tilePadding: EdgeInsets.zero,
                  children: [SelectableText(c.error!)],
                ),
            ],
          ),
        ),
      );
    },
  );
}

/// Canonical persisted speech history, loaded in bounded upstream pages.
class VoiceHistoryDialog extends StatefulWidget {
  const VoiceHistoryDialog({
    super.key,
    required this.api,
    required this.serviceKey,
    required this.threadId,
  });
  final BridgeApi api;
  final String serviceKey;
  final String threadId;
  @override
  State<VoiceHistoryDialog> createState() => _VoiceHistoryDialogState();
}

class _VoiceHistoryDialogState extends State<VoiceHistoryDialog> {
  final _entries = <Map<String, dynamic>>[];
  final _entryIds = <String>{};
  String? _cursor;
  bool _loading = false;
  bool _more = true;
  String? _error;
  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    if (_loading || !_more) return;
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final raw = await widget.api.appRealtimeRequest(
        widget.serviceKey,
        'thread/timeline/list',
        jsonEncode({
          'threadId': widget.threadId,
          'limit': 100,
          if (_cursor != null) 'cursor': _cursor,
        }),
      );
      final page = jsonDecode(raw) as Map;
      if (!mounted) return;
      setState(() {
        for (final entry in page['data'] as List) {
          if (entry['type'] != 'realtime') continue;
          final item = Map<String, dynamic>.from(entry['item'] as Map);
          if (item['type'] == 'transcriptSegment' &&
              item['id'] is String &&
              _entryIds.add(item['id'] as String)) {
            _entries.add(item);
          }
        }
        _cursor = page['nextCursor'] as String?;
        _more = _cursor != null;
      });
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    return AlertDialog(
      title: Text(l.voiceHistory),
      content: SizedBox(
        width: 560,
        height: MediaQuery.sizeOf(context).height * .55,
        child: ListView.builder(
          itemCount: _entries.length + 1,
          itemBuilder: (context, index) {
            if (index < _entries.length) {
              final item = _entries[index];
              return ListTile(
                title: Text(item['role'] == 'user' ? l.voiceYou : l.voiceAgent),
                subtitle: SelectableText(item['text'] as String? ?? ''),
              );
            }
            return Column(
              children: [
                if (_entries.isEmpty && !_loading && _error == null)
                  Text(l.voiceHistoryEmpty),
                if (_error != null) Text(_error!),
                if (_loading)
                  const LinearProgressIndicator()
                else if (_more)
                  TextButton(
                    onPressed: _load,
                    child: Text(_error == null ? l.voiceHistoryMore : l.retry),
                  ),
              ],
            );
          },
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: Text(MaterialLocalizations.of(context).closeButtonLabel),
        ),
      ],
    );
  }
}
