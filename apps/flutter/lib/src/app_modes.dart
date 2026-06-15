import 'package:pocket_codex/l10n/gen/app_localizations.dart';

/// A permission preset bundling a codex approval policy + sandbox mode into a
/// single user-facing choice (mirrors the "permission mode" idea in the
/// reference UI). Wire values match the app-server protocol.
enum PermissionMode {
  /// Ask before running; read-only sandbox.
  readOnly(approval: 'on-request', sandbox: 'read-only'),

  /// Write within the workspace; ask only on failure. The default.
  auto(approval: 'on-failure', sandbox: 'workspace-write'),

  /// No sandbox, never ask. The "bypass permissions" preset.
  full(approval: 'never', sandbox: 'danger-full-access');

  const PermissionMode({required this.approval, required this.sandbox});

  /// codex `approvalPolicy` wire value.
  final String approval;

  /// codex `sandbox` wire value.
  final String sandbox;

  /// Localized short label.
  String label(AppLocalizations l) => switch (this) {
    PermissionMode.readOnly => l.modeReadOnly,
    PermissionMode.auto => l.modeAuto,
    PermissionMode.full => l.modeFull,
  };

  /// Localized one-line description.
  String describe(AppLocalizations l) => switch (this) {
    PermissionMode.readOnly => l.modeReadOnlyDesc,
    PermissionMode.auto => l.modeAutoDesc,
    PermissionMode.full => l.modeFullDesc,
  };
}

/// The model's reasoning effort ("thinking level"). Wire values are the lowercase
/// names the codex app-server's `ReasoningEffort` enum accepts. Which levels are
/// available is model-dependent — the picker shows only the ones a model lists in
/// its `supportedReasoningEfforts` (e.g. some models support `xhigh`/`minimal` but
/// not `low`/`high`). Declared low→high so the picker can order them by intensity.
enum ReasoningEffort {
  /// Least thinking, fastest (gpt-5-class models).
  minimal(wire: 'minimal'),

  /// Low.
  low(wire: 'low'),

  /// Balanced (the usual default).
  medium(wire: 'medium'),

  /// Thorough.
  high(wire: 'high'),

  /// Most thorough, slowest (extra-high).
  xhigh(wire: 'xhigh');

  const ReasoningEffort({required this.wire});

  /// codex `effort` / `reasoning_effort` wire value.
  final String wire;

  /// Parse a server/wire value into an effort, or null for unknown/absent.
  /// Tolerant of values we don't surface (e.g. `none`) by mapping them to null.
  static ReasoningEffort? fromWire(String? value) => switch (value) {
    'minimal' => ReasoningEffort.minimal,
    'low' => ReasoningEffort.low,
    'medium' => ReasoningEffort.medium,
    'high' => ReasoningEffort.high,
    'xhigh' => ReasoningEffort.xhigh,
    _ => null,
  };

  /// Localized short label.
  String label(AppLocalizations l) => switch (this) {
    ReasoningEffort.minimal => l.effortMinimal,
    ReasoningEffort.low => l.effortLow,
    ReasoningEffort.medium => l.effortMedium,
    ReasoningEffort.high => l.effortHigh,
    ReasoningEffort.xhigh => l.effortXhigh,
  };

  /// Localized one-line description.
  String describe(AppLocalizations l) => switch (this) {
    ReasoningEffort.minimal => l.effortMinimalDesc,
    ReasoningEffort.low => l.effortLowDesc,
    ReasoningEffort.medium => l.effortMediumDesc,
    ReasoningEffort.high => l.effortHighDesc,
    ReasoningEffort.xhigh => l.effortXhighDesc,
  };
}
