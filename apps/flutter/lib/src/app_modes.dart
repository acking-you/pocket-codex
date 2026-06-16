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

/// The model's reasoning effort ("thinking level"). The wire value is whatever
/// lowercase token the codex app-server advertises — as of the v2 protocol this
/// is an OPEN string (`{type:string, minLength:1}`), not a fixed enum, so a model
/// may expose `none`, `minimal`, `xhigh`, or future/custom levels beyond the
/// classic `low`/`medium`/`high`. We therefore wrap an opaque wire string rather
/// than enumerate: the picker offers exactly the model's `supportedReasoningEfforts`,
/// and any token we don't have a localized label for is shown title-cased.
///
/// The five named constants below are conveniences for the common levels; they
/// are NOT an exhaustive set. Equality is by [wire], so values round-trip through
/// maps/sets and selection checks regardless of whether they're "known".
class ReasoningEffort {
  /// Wraps a codex `effort` / `reasoning_effort` wire token (assumed non-empty).
  const ReasoningEffort(this.wire);

  /// Least thinking, fastest (gpt-5-class models).
  static const minimal = ReasoningEffort('minimal');

  /// Low.
  static const low = ReasoningEffort('low');

  /// Balanced (the usual default).
  static const medium = ReasoningEffort('medium');

  /// Thorough.
  static const high = ReasoningEffort('high');

  /// Most thorough, slowest (extra-high).
  static const xhigh = ReasoningEffort('xhigh');

  /// Convenience set of the common levels, low→high, used as a fallback when a
  /// model advertises no `supportedReasoningEfforts`.
  static const known = [minimal, low, medium, high, xhigh];

  /// codex `effort` / `reasoning_effort` wire value.
  final String wire;

  /// Parse a server/wire value into an effort. `null`/empty → null (absent);
  /// ANY other non-empty token is accepted verbatim (open string, forward-compat).
  static ReasoningEffort? fromWire(String? value) =>
      (value == null || value.isEmpty) ? null : ReasoningEffort(value);

  /// Localized short label; unknown tokens fall back to a title-cased wire value.
  String label(AppLocalizations l) => switch (wire) {
    'minimal' => l.effortMinimal,
    'low' => l.effortLow,
    'medium' => l.effortMedium,
    'high' => l.effortHigh,
    'xhigh' => l.effortXhigh,
    _ => _humanize(wire),
  };

  /// Localized one-line description; empty for tokens we don't recognize.
  String describe(AppLocalizations l) => switch (wire) {
    'minimal' => l.effortMinimalDesc,
    'low' => l.effortLowDesc,
    'medium' => l.effortMediumDesc,
    'high' => l.effortHighDesc,
    'xhigh' => l.effortXhighDesc,
    _ => '',
  };

  static String _humanize(String w) =>
      w.isEmpty ? w : '${w[0].toUpperCase()}${w.substring(1)}';

  @override
  bool operator ==(Object other) =>
      other is ReasoningEffort && other.wire == wire;

  @override
  int get hashCode => wire.hashCode;

  @override
  String toString() => 'ReasoningEffort($wire)';
}
