import 'dart:convert';

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

class _VoiceSetupDialogState extends State<VoiceSetupDialog> {
  final _model = TextEditingController();
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
                TextField(
                  controller: _model,
                  decoration: InputDecoration(
                    labelText: l.voiceModel,
                    helperText: l.voiceModelHint,
                  ),
                  maxLength: 128,
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
                  VoiceSettings(model: _model.text.trim(), voice: _voice),
                ),
          child: Text(l.voiceStart),
        ),
      ],
    );
  }
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
      final label = switch (c.phase) {
        VoicePhase.connecting => l.voiceConnecting,
        VoicePhase.active => c.muted ? l.voiceMuted : l.voiceListening,
        VoicePhase.stopping => l.voiceStopping,
        VoicePhase.failed => l.voiceUnavailable,
        VoicePhase.idle => l.voiceSessionEnded,
      };
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
                  if (c.phase == VoicePhase.active)
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
                      onPressed: () => c.stop(),
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
