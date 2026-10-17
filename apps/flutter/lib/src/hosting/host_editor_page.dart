import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:pocket_codex/src/hosting/acp_host_form.dart';
import 'package:pocket_codex/src/hosting/agent_choice_grid.dart';
import 'package:pocket_codex/src/hosting/claude_preparation_progress.dart';
import 'package:pocket_codex/src/hosting/codex_runtime_section.dart';
import 'package:pocket_codex/src/hosting/editor_block.dart';
import 'package:pocket_codex/src/hosting/host_editor_actions.dart';
import 'package:pocket_codex/src/hosting/host_editor_drafts.dart';
import 'package:pocket_codex/src/hosting/host_status_section.dart';
import 'package:pocket_codex/src/hosting/model_service_section.dart';
import 'package:pocket_codex/src/hosting/official_login_panel.dart';
import 'package:pocket_codex/src/hosting/opencode_runtime_section.dart';
import 'package:pocket_codex/src/motion.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/widgets/busy_label.dart';
import 'package:pocket_codex/src/widgets/glass.dart';
import 'package:pocket_codex/src/widgets/group_card.dart';
import 'package:pocket_codex/src/widgets/utility_page.dart';
import 'package:pocket_codex/src/app_icons.dart';

/// Open the hosting editor: a new host, or the running [existing] one.
/// [initialChoice] preselects a new host's agent ([AgentChoice]).
Future<void> showHostingEditor(
  BuildContext context, {
  AppServeStatus? existing,
  AgentChoice? initialChoice,
}) => Navigator.of(context).push(
  MaterialPageRoute<void>(
    builder: (_) =>
        HostingEditor(existing: existing, initialChoice: initialChoice),
  ),
);

/// The hosting editor, one page for creating and editing a local host.
///
/// A new host: pick the agent ([AgentChoiceGrid]), fill its runtime (Codex:
/// [CodexRuntimeSection]; OpenCode: [OpenCodeRuntimeSection]; ACP:
/// [AcpHostForm]) and its model service
/// ([ModelServiceSection]), then Start: saved and started as one operation
/// ([BridgeApi.appServeStartConfigured]), so a failed start leaves no new
/// configuration behind.
///
/// A running host: its status ([HostStatusSection]), its runtime (Codex:
/// executable, port and proxy; ACP: program and arguments) and its model
/// service, all read from the bridge first. Save stores them and changes
/// nothing that runs; an explicit, confirmed "restart to apply" restarts only
/// that host ([BridgeApi.appServeApply]) with exactly the revision shown, and
/// a failed restart brings the previous configuration back. Leaving with
/// unsaved edits asks first, for back, the page's own navigation and its
/// shortcuts alike.
class HostingEditor extends ConsumerStatefulWidget {
  /// Creates the editor; [existing] edits a running host.
  const HostingEditor({super.key, this.existing, this.initialChoice});

  /// The running host edited, or null for a new host.
  final AppServeStatus? existing;

  /// A new host's preselected agent; Codex when null.
  final AgentChoice? initialChoice;

  @override
  ConsumerState<HostingEditor> createState() => _HostingEditorState();
}

class _HostingEditorState extends ConsumerState<HostingEditor>
    with HostEditorDrafts<HostingEditor>, HostEditorActions<HostingEditor> {
  @override
  AppServeStatus? get existing => widget.existing;

  @override
  void initState() {
    super.initState();
    if (widget.initialChoice case final initial? when existing == null) {
      choice = initial;
    }
    initEditor();
  }

  @override
  void dispose() {
    disposeEditor();
    super.dispose();
  }

  Widget _agentGrid() {
    final metadata = api.localDeviceMetadata();
    final saved =
        ref.watch(uiPrefsProvider).valueOrNull?.acpAgents ??
        const <AcpAgentSpec>[];
    return GroupCard(
      key: const Key('host-agent-section'),
      title: AppLocalizations.of(context).hostSectionAgent,
      hint:
          '${AppLocalizations.of(context).servicesLocalDevice} · ${metadata.hostname} · ${metadata.platform}',
      children: [
        EditorBlock(
          child: AgentChoiceGrid(
            value: choice,
            presets: api.acpPresets(),
            saved: saved,
            onChanged: busy ? null : selectChoice,
          ),
        ),
      ],
    );
  }

  /// What the agent's existing configuration provides, in words.
  Widget _inherited(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    if (isOpenCodeHost) {
      return EditorNote(
        l10n.hostInheritedOpenCode,
        key: const Key('host-inherited-opencode'),
      );
    }
    if (isAcpHost) return EditorNote(l10n.hostInheritedAcp);
    final s = setup;
    if (s == null) return const LinearProgressIndicator();
    final source = s.effectiveSource.isNotEmpty
        ? s.effectiveSource
        : s.hasCustomProvider
        ? 'custom'
        : s.hasAuth
        ? 'account'
        : 'none';
    final (text, color) = switch (source) {
      'custom' => (
        l10n.hostInheritedCodexCustom(s.selectedProvider ?? '—'),
        successColor(scheme),
      ),
      'account' => (l10n.hostInheritedCodexAccount, successColor(scheme)),
      'invalid' => (l10n.hostInheritedCodexInvalid, scheme.error),
      _ => (l10n.hostInheritedCodexNone, cautionColor(scheme)),
    };
    return Column(
      key: const Key('host-inherited-status'),
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        EditorNote(text, color: color),
        const SizedBox(height: 4),
        EditorNote(l10n.hostInheritedCodexHome(s.codexHome)),
      ],
    );
  }

  /// Save and restart controls for a running host's configuration.
  Widget _configActions(AppLocalizations l10n, AppServeStatus? live) {
    final scheme = Theme.of(context).colorScheme;
    final dirty = editorDirty;
    final ready = load == EditorLoad.ready;
    final gone = hostGone(live);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        if (configPending && !dirty) ...[
          EditorNote(
            l10n.hostConfigPending,
            key: const Key('host-config-pending'),
            color: cautionColor(scheme),
          ),
          const SizedBox(height: 8),
        ],
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            OutlinedButton(
              key: const Key('host-config-save'),
              style: running == HostAction.save
                  ? OutlinedButton.styleFrom(
                      disabledForegroundColor: scheme.onSurface,
                    )
                  : null,
              onPressed: busy || !ready || !dirty ? null : saveConfig,
              child: BusyLabel(
                busy: running == HostAction.save,
                label: l10n.hostSaveConfig,
                busyLabel: l10n.hostSaving,
              ),
            ),
            Tooltip(
              message: l10n.hostApplyNote,
              child: FilledButton(
                key: const Key('host-config-apply'),
                style: _working(
                  HostAction.apply,
                  scheme.primary,
                  scheme.onPrimary,
                ),
                onPressed: busy || !ready || gone || !(configPending || dirty)
                    ? null
                    : applyConfig,
                child: BusyLabel(
                  busy: running == HostAction.apply,
                  icon: AppIcons.restart_alt,
                  label: dirty
                      ? l10n.hostSaveAndRestart
                      : l10n.hostApplyRestart,
                  busyLabel: l10n.hostRestarting,
                ),
              ),
            ),
          ],
        ),
      ],
    );
  }

  ModelServiceSection _model(AppLocalizations l10n, AppServeStatus? live) =>
      ModelServiceSection(
        draft: draft,
        enabled: !busy,
        // Codex hosts also publish `api:<name>`, which keeps the official
        // sign-in whatever is chosen here.
        info: isCodexHost
            ? '${l10n.hostSectionModelHint}\n\n${l10n.hostApiProxyNote}'
            : null,
        inherited: _inherited(l10n),
        official: !isCodexHost
            ? null
            : OfficialLoginPanel(
                status: setup,
                enabled: !busy,
                binary: officialBinary,
                proxy: officialProxy,
                onChanged: loadSetup,
              ),
        footer: existing == null ? null : _configActions(l10n, live),
      );

  /// The running host's state notes: gone, replaced, a newer saved
  /// configuration, or the load gate.
  List<Widget> _hostNotes(AppLocalizations l10n, AppServeStatus? live) {
    final scheme = Theme.of(context).colorScheme;
    return [
      if (hostGone(live))
        EditorNote(
          l10n.hostGone,
          key: const Key('host-gone'),
          color: scheme.error,
        )
      else if (hostReplaced(live))
        EditorNote(
          l10n.hostReplaced,
          key: const Key('host-replaced'),
          color: scheme.error,
          action: TextButton(
            onPressed: busy ? null : loadConfig,
            child: Text(l10n.hostConfigReload),
          ),
        ),
      if (newerConfig case final newer?)
        EditorNote(
          l10n.hostConfigNewer,
          key: const Key('host-config-newer'),
          color: cautionColor(scheme),
          action: TextButton(
            onPressed: busy ? null : () => adopt(newer),
            child: Text(l10n.hostConfigReload),
          ),
        ),
    ];
  }

  Widget _loadGate(AppLocalizations l10n) {
    if (load == EditorLoad.loading) {
      return const Padding(
        key: Key('host-config-loading'),
        padding: EdgeInsets.symmetric(vertical: 12),
        child: LinearProgressIndicator(),
      );
    }
    return GroupCard(
      key: const Key('host-config-load-failed'),
      title: l10n.hostConfigLoadFailedTitle,
      children: [
        EditorBlock(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              EditorNote(
                l10n.hostConfigLoadFailed(loadError ?? ''),
                color: Theme.of(context).colorScheme.error,
              ),
              const SizedBox(height: 8),
              OutlinedButton.icon(
                key: const Key('host-config-retry'),
                onPressed: loadConfig,
                icon: const Icon(AppIcons.refresh, size: 18),
                label: Text(l10n.retry),
              ),
            ],
          ),
        ),
      ],
    );
  }

  Widget _runningRuntime(AppLocalizations l10n, AppServeStatus host) {
    if (host.isOpenCode) {
      return OpenCodeRuntimeSection(
        form: openCode,
        enabled: !busy,
        onBrowse: browseOpenCode,
        fixedName: true,
        info: l10n.hostRuntimeEditNote,
        sharedHistoryHost: sharedHistoryHost,
      );
    }
    if (host.isCodex) {
      return CodexRuntimeSection(
        form: runtime,
        enabled: !busy,
        onBrowse: browseCodex,
        fixedName: true,
        info: l10n.hostRuntimeEditNote,
      );
    }
    final spec = editedSpecBase;
    return GroupCard(
      key: const Key('host-agent-edit'),
      title: l10n.hostSectionAgent,
      info: l10n.hostAgentEditNote,
      children: [
        EditorBlock(
          child: spec == null
              ? EditorNote(l10n.hostAgentUnknown)
              : Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    AcpHostForm(
                      key: ValueKey('host-agent-form-$agentFormSeq'),
                      choice: choiceFor(spec),
                      initial: spec,
                      fixedName: host.name,
                      enabled: !busy,
                      onChanged: agentChanged,
                      onPrepare: prepareClaudeEnvironment,
                    ),
                    if (claudeSetup case final setup? when setup.running)
                      _claudeProgress(setup),
                  ],
                ),
        ),
      ],
    );
  }

  List<Widget> _sections(AppLocalizations l10n) {
    // Names and the shared OpenCode history follow the hosts that run.
    ref.watch(localServeListProvider);
    final host = existing;
    final live = liveHost;
    if (host != null) {
      return [
        HostStatusSection(host: live ?? host),
        ..._hostNotes(l10n, live),
        if (load != EditorLoad.ready)
          _loadGate(l10n)
        else ...[
          _runningRuntime(l10n, host),
          _model(l10n, live),
        ],
      ];
    }
    return [
      _agentGrid(),
      if (choice == openCodeChoice)
        OpenCodeRuntimeSection(
          form: openCode,
          enabled: !busy,
          onBrowse: browseOpenCode,
          takenNames: takenNames,
          sharedHistoryHost: sharedHistoryHost,
        )
      else if (isAcpChoice(choice))
        GroupCard(
          key: const Key('acp-runtime-section'),
          title: l10n.hostRuntimeInfo,
          info: l10n.acpHostHint,
          children: [
            EditorBlock(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                mainAxisSize: MainAxisSize.min,
                children: [
                  AcpHostForm(
                    // One form per choice, restored from its own draft.
                    key: ValueKey('host-agent-new-$choice'),
                    choice: choice,
                    restored: acpDraftFor(choice),
                    enabled: !busy,
                    takenNames: takenNames,
                    onChanged: agentChanged,
                    onPrepare: prepareClaudeEnvironment,
                  ),
                  if (claudeSetup case final setup? when setup.running)
                    _claudeProgress(setup),
                ],
              ),
            ),
          ],
        )
      else
        CodexRuntimeSection(
          form: runtime,
          enabled: !busy,
          onBrowse: browseCodex,
          takenNames: takenNames,
        ),
      _model(l10n, null),
    ];
  }

  /// The running preparation's card; Cancel or Stop waiting sits in the
  /// action bar, by who owns the run.
  Widget _claudeProgress(AcpClaudeSetup setup) => Padding(
    padding: const EdgeInsets.only(top: 14),
    child: ClaudePreparationProgress(
      key: const Key('claude-prepare-progress'),
      setup: setup,
      joined: !ownsClaudePrepare,
    ),
  );

  /// A filled button's style while [action] runs: disabled like every
  /// control, but in its working colours, so it reads as busy, not off.
  ButtonStyle? _working(
    HostAction action,
    Color background,
    Color foreground,
  ) => running == action
      ? FilledButton.styleFrom(
          disabledBackgroundColor: background,
          disabledForegroundColor: foreground,
        )
      : null;

  /// The rule above the actions, with a thin running line along it while an
  /// operation is under way.
  Widget _rule(ColorScheme scheme) => Stack(
    clipBehavior: Clip.none,
    children: [
      Divider(height: 1, color: scheme.outlineVariant),
      Positioned(
        left: 0,
        right: 0,
        // Over the hairline and the pixel above it, clear of the bar below.
        top: -1,
        child: IgnorePointer(
          child: AnimatedSwitcher(
            duration: Motion.of(context, Motion.fast),
            child: busy
                ? LinearProgressIndicator(
                    key: const Key('host-editor-progress'),
                    minHeight: 2,
                    color: signalColor(scheme),
                    backgroundColor: Colors.transparent,
                  )
                : const SizedBox(height: 2),
          ),
        ),
      ),
    ],
  );

  /// The last outcome, pinned above the actions so it is seen wherever the
  /// form is scrolled: it rises in as it arrives, and the space it took
  /// closes when it goes.
  Widget _outcome(List<(Key, String, IconData, Color)> messages) =>
      Motion.resize(
        context,
        alignment: Alignment.bottomCenter,
        child: messages.isEmpty
            ? const SizedBox(width: double.infinity)
            : Padding(
                padding: const EdgeInsets.fromLTRB(20, 0, 20, 10),
                child: Align(
                  alignment: Alignment.topCenter,
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(maxWidth: 680),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        for (final (key, message, icon, color) in messages)
                          EntranceMotion(
                            key: ValueKey((key, message)),
                            offset: 6,
                            child: _OutcomeCard(
                              textKey: key,
                              message: message,
                              icon: icon,
                              color: color,
                            ),
                          ),
                      ],
                    ),
                  ),
                ),
              ),
      );

  Widget _actions(AppLocalizations l10n) {
    final scheme = Theme.of(context).colorScheme;
    final host = existing;
    return EditorActionBar(
      children: [
        // While Claude's adapter is being prepared by this page, Cancel
        // stops that preparation (only the npm process it started); one
        // joined from another caller is not this page's to stop, so it
        // offers Stop waiting, which ends only this page's operation.
        // Close stays beside either, unavailable until it is over.
        if ((claudeSetup?.running ?? false) && ownsClaudePrepare)
          TextButton(
            key: const Key('claude-prepare-cancel'),
            onPressed: cancelClaudePrepare,
            child: Text(l10n.claudePrepareCancel),
          )
        else if (claudeSetup?.running ?? false)
          TextButton(
            key: const Key('claude-prepare-stop-waiting'),
            onPressed: stopWaitingForClaude,
            child: Text(l10n.claudePrepareStopWaiting),
          ),
        TextButton(
          key: const Key('host-editor-close'),
          // Through the page's PopScope, so unsaved edits are asked
          // about first.
          onPressed: busy ? null : () => Navigator.of(context).maybePop(),
          child: Text(
            host == null
                ? l10n.cancel
                : MaterialLocalizations.of(context).closeButtonLabel,
          ),
        ),
        if (host != null)
          FilledButton(
            key: const Key('stop-hosting-btn'),
            style: FilledButton.styleFrom(
              backgroundColor: scheme.error,
              foregroundColor: scheme.onError,
              // Its working colours while it stops (see [_working]).
              disabledBackgroundColor: running == HostAction.stop
                  ? scheme.error
                  : null,
              disabledForegroundColor: running == HostAction.stop
                  ? scheme.onError
                  : null,
            ),
            onPressed: busy || hostGone(liveHost) ? null : stop,
            child: BusyLabel(
              busy: running == HostAction.stop,
              label: l10n.stopHosting,
              busyLabel: l10n.hostStopping,
            ),
          )
        else
          FilledButton(
            key: const Key('start-hosting-btn'),
            style: _working(HostAction.start, scheme.primary, scheme.onPrimary),
            onPressed: busy ? null : start,
            child: BusyLabel(
              busy: running == HostAction.start,
              label: l10n.startHosting,
              busyLabel: l10n.localHostStarting,
            ),
          ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final host = existing;
    final messages = [
      if (error != null)
        (
          const Key('host-editor-error'),
          error!,
          AppIcons.error_outline,
          scheme.error,
        ),
      if (notice != null)
        (
          const Key('host-editor-notice'),
          notice!,
          AppIcons.check_circle_outline,
          successColor(scheme),
        ),
    ];
    return PopScope(
      canPop: canLeave,
      onPopInvokedWithResult: (didPop, _) async {
        // Gone from its route now, though still mounted while it animates
        // out: nothing it began goes on.
        if (didPop) {
          retireEditor();
          return;
        }
        final navigator = Navigator.of(context);
        if (await confirmLeave() && mounted) navigator.pop();
      },
      child: UtilityLeaveGuard(
        confirm: confirmLeave,
        child: UtilityPage(
          route: '/manage',
          title: host == null
              ? l10n.localHostDialogTitle
              : l10n.hostEditorTitle(host.name),
          parent: UtilityParent(title: l10n.manageServices, route: '/manage'),
          body: Column(
            children: [
              Expanded(
                child: SingleChildScrollView(
                  key: const Key('host-editor-scroll'),
                  padding: const EdgeInsets.fromLTRB(20, 18, 20, 24),
                  child: Align(
                    alignment: Alignment.topCenter,
                    child: ConstrainedBox(
                      // The settings column width, so editor and settings read
                      // as one family of pages.
                      constraints: const BoxConstraints(maxWidth: 680),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          for (final section in _sections(l10n)) ...[
                            section,
                            const SizedBox(height: 12),
                          ],
                        ],
                      ),
                    ),
                  ),
                ),
              ),
              _outcome(messages),
              _rule(scheme),
              _actions(l10n),
            ],
          ),
        ),
      ),
    );
  }
}

/// One outcome of the editor's last operation, on a raised card with its
/// mark.
class _OutcomeCard extends StatelessWidget {
  const _OutcomeCard({
    required this.textKey,
    required this.message,
    required this.icon,
    required this.color,
  });

  final Key textKey;
  final String message;
  final IconData icon;
  final Color color;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final radius = BorderRadius.circular(kPanelRadius);
    return Padding(
      padding: const EdgeInsets.only(top: 6),
      child: DecoratedBox(
        decoration: BoxDecoration(
          borderRadius: radius,
          boxShadow: raisedCardShadows(theme.colorScheme),
        ),
        child: Card(
          margin: EdgeInsets.zero,
          shape: RoundedRectangleBorder(borderRadius: radius),
          // A long failure scrolls within its card rather than squeezing the
          // form away.
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxHeight: 168),
            child: SingleChildScrollView(
              padding: const EdgeInsets.fromLTRB(14, 11, 16, 11),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Padding(
                    padding: const EdgeInsets.only(top: 1),
                    child: Icon(icon, size: 17, color: color),
                  ),
                  const SizedBox(width: 10),
                  Expanded(
                    child: Text(
                      message,
                      key: textKey,
                      style: theme.textTheme.bodyMedium?.copyWith(color: color),
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
}
