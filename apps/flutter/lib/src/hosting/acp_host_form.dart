import 'dart:async';
import 'dart:math';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/hosting/argv_editor.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/ui_prefs.dart';

/// What the ACP form currently describes.
class AcpFormValue {
  /// Creates a form value.
  const AcpFormValue({
    required this.name,
    required this.spec,
    required this.save,
  });

  /// Instance name (defaults to the profile id when empty).
  final String name;

  /// The agent to host.
  final AcpAgentSpec spec;

  /// Whether to save a custom agent to "my agents".
  final bool save;

  /// Whether the value can be submitted.
  bool get valid =>
      spec.program.trim().isNotEmpty && spec.displayName.trim().isNotEmpty;
}

/// The profile id of a new custom agent named [name]. The readable part is
/// a lossy slug (names in other scripts all become `agent`), so [salt] —
/// random per new agent, see [newProfileSalt] — keeps two custom agents
/// from ever sharing a saved entry. The id only names the saved entry: the
/// host binds sessions to the program and arguments actually run.
String customProfileId(String name, String salt) {
  final slug = name
      .toLowerCase()
      .replaceAll(RegExp('[^a-z0-9]+'), '-')
      .replaceAll(RegExp(r'^-+|-+$'), '');
  final cut = slug.length > 40 ? slug.substring(0, 40) : slug;
  return 'custom-${cut.isEmpty ? 'agent' : cut}-$salt';
}

/// Eight random hex digits for [customProfileId].
String newProfileSalt([Random? random]) {
  final source = random ?? Random.secure();
  return [
    for (var i = 0; i < 8; i++) source.nextInt(16).toRadixString(16),
  ].join();
}

/// The new-ACP-host form: choose a preset, a saved agent, or a custom one,
/// then edit its name, program and argument vector.
class AcpHostForm extends ConsumerStatefulWidget {
  /// Creates the form; [onChanged] receives every edit.
  const AcpHostForm({super.key, required this.onChanged, this.enabled = true});

  /// Called with the current value.
  final ValueChanged<AcpFormValue> onChanged;

  /// Whether the fields can be edited.
  final bool enabled;

  @override
  ConsumerState<AcpHostForm> createState() => _AcpHostFormState();
}

class _AcpHostFormState extends ConsumerState<AcpHostForm> {
  static const _custom = '';
  // One per new custom agent this form describes.
  final String _salt = newProfileSalt();
  final _name = TextEditingController();
  final _displayName = TextEditingController();
  final _program = TextEditingController();
  List<String> _args = const [];
  List<String> _argsSeed = const [];
  String _choice = 'preset:opencode';
  String _profileId = 'opencode';
  bool _save = true;
  String? _located;
  bool _locating = false;
  Timer? _locateDebounce;
  int _locateSeq = 0;

  @override
  void initState() {
    super.initState();
    final presets = ref.read(bridgeApiProvider).acpPresets();
    if (presets.isNotEmpty) {
      _apply('preset:${presets.first.id}', presets.first.spec, emit: false);
    } else {
      _choice = _custom;
      _profileId = customProfileId('', _salt);
    }
    Future.microtask(() {
      if (!mounted) return;
      _emit();
      _locate();
    });
  }

  @override
  void dispose() {
    _locateDebounce?.cancel();
    _name.dispose();
    _displayName.dispose();
    _program.dispose();
    super.dispose();
  }

  bool get _isCustom => !_choice.startsWith('preset:');

  void _apply(String choice, AcpAgentSpec spec, {bool emit = true}) {
    _choice = choice;
    _profileId = spec.profileId;
    _displayName.text = spec.displayName;
    _program.text = spec.program;
    _args = List.of(spec.args);
    _argsSeed = _args;
    if (_name.text.trim().isEmpty || !_isCustom) _name.text = spec.profileId;
    if (emit) {
      _emit();
      _locate();
    }
  }

  AcpAgentSpec get _spec {
    final display = _displayName.text.trim();
    final id = _choice == _custom
        ? customProfileId(display, _salt)
        : _profileId;
    return AcpAgentSpec(
      profileId: id,
      displayName: display,
      // Paths may legitimately contain spaces; only surrounding whitespace,
      // which is never part of a typed path, is dropped.
      program: _program.text.trim(),
      args: List.unmodifiable(_args),
    );
  }

  void _emit() => widget.onChanged(
    AcpFormValue(
      name: _name.text.trim(),
      spec: _spec,
      save: _save && _isCustom,
    ),
  );

  void _locate() {
    _locateDebounce?.cancel();
    _locateDebounce = Timer(const Duration(milliseconds: 300), () async {
      final program = _program.text.trim();
      final seq = ++_locateSeq;
      if (program.isEmpty) {
        if (mounted) setState(() => _located = null);
        return;
      }
      setState(() => _locating = true);
      final found = await ref
          .read(bridgeApiProvider)
          .acpLocate(program, profileId: _profileId);
      if (!mounted || seq != _locateSeq) return;
      setState(() {
        _located = found;
        _locating = false;
      });
    });
  }

  Future<void> _browse() async {
    final file = await openFile();
    if (file == null || !mounted) return;
    setState(() => _program.text = file.path);
    _emit();
    _locate();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final small = Theme.of(context).textTheme.bodySmall;
    final api = ref.watch(bridgeApiProvider);
    final presets = api.acpPresets();
    final saved =
        ref.watch(uiPrefsProvider).valueOrNull?.acpAgents ??
        const <AcpAgentSpec>[];
    final enabled = widget.enabled;
    if (!api.acpHostingSupported()) {
      return Text(
        l10n.acpHostingUnsupported,
        key: const Key('acp-hosting-unsupported'),
        style: TextStyle(color: cautionColor(scheme)),
      );
    }
    final entries = <DropdownMenuItem<String>>[
      for (final preset in presets)
        DropdownMenuItem(
          value: 'preset:${preset.id}',
          child: Text(preset.displayName, overflow: TextOverflow.ellipsis),
        ),
      for (final agent in saved)
        DropdownMenuItem(
          value: 'saved:${agent.profileId}',
          child: Text(agent.displayName, overflow: TextOverflow.ellipsis),
        ),
      DropdownMenuItem(value: _custom, child: Text(l10n.acpAgentCustom)),
    ];
    final program = _program.text.trim();
    return Column(
      key: const Key('acp-host-form'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(l10n.acpHostHint),
        const SizedBox(height: 12),
        DropdownButtonFormField<String>(
          key: const Key('acp-agent-choice'),
          initialValue: entries.any((e) => e.value == _choice)
              ? _choice
              : _custom,
          isExpanded: true,
          decoration: InputDecoration(labelText: l10n.acpAgentLabel),
          items: entries,
          onChanged: !enabled
              ? null
              : (choice) {
                  if (choice == null) return;
                  setState(() {
                    if (choice.startsWith('preset:')) {
                      final id = choice.substring(7);
                      final preset = presets.firstWhere((p) => p.id == id);
                      _apply(choice, preset.spec);
                    } else if (choice.startsWith('saved:')) {
                      final id = choice.substring(6);
                      final agent = saved.firstWhere((a) => a.profileId == id);
                      _apply(choice, agent);
                    } else {
                      _apply(
                        _custom,
                        const AcpAgentSpec(
                          profileId: '',
                          displayName: '',
                          program: '',
                        ),
                      );
                    }
                  });
                },
        ),
        const SizedBox(height: 12),
        TextField(
          key: const Key('acp-display-name-field'),
          controller: _displayName,
          enabled: enabled && _isCustom,
          decoration: InputDecoration(labelText: l10n.acpAgentName),
          onChanged: (_) => _emit(),
        ),
        const SizedBox(height: 12),
        TextField(
          key: const Key('acp-instance-name-field'),
          controller: _name,
          enabled: enabled,
          decoration: InputDecoration(labelText: l10n.localHostName),
          onChanged: (_) => _emit(),
        ),
        const SizedBox(height: 12),
        Row(
          crossAxisAlignment: CrossAxisAlignment.end,
          children: [
            Expanded(
              child: TextField(
                key: const Key('acp-program-field'),
                controller: _program,
                enabled: enabled,
                autocorrect: false,
                enableSuggestions: false,
                decoration: InputDecoration(labelText: l10n.acpProgram),
                onChanged: (_) {
                  _emit();
                  _locate();
                },
              ),
            ),
            const SizedBox(width: 8),
            OutlinedButton(
              key: const Key('acp-program-browse'),
              onPressed: enabled ? _browse : null,
              child: Text(l10n.acpProgramBrowse),
            ),
          ],
        ),
        const SizedBox(height: 4),
        Text(l10n.acpProgramHint, style: small),
        if (program.isNotEmpty && !_locating)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: _located == null
                ? Text(
                    l10n.acpProgramNotFound,
                    key: const Key('acp-program-missing'),
                    style: small?.copyWith(color: scheme.error),
                  )
                : Text(
                    l10n.acpProgramFound(_located!),
                    key: const Key('acp-program-found'),
                    style: small?.copyWith(color: successColor(scheme)),
                    overflow: TextOverflow.ellipsis,
                    maxLines: 2,
                  ),
          ),
        const SizedBox(height: 12),
        ArgvEditor(
          key: const Key('acp-argv-editor'),
          initial: _argsSeed,
          enabled: enabled,
          onChanged: (args) {
            _args = args;
            _emit();
          },
        ),
        if (_isCustom) ...[
          CheckboxListTile(
            key: const Key('acp-save-agent'),
            contentPadding: EdgeInsets.zero,
            value: _save,
            title: Text(l10n.acpSaveAgent),
            onChanged: !enabled
                ? null
                : (value) {
                    setState(() => _save = value ?? false);
                    _emit();
                  },
          ),
          Text(l10n.acpSecretsNote, style: small),
        ],
      ],
    );
  }
}
