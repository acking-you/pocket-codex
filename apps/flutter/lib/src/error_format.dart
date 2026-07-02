import 'package:flutter_rust_bridge/flutter_rust_bridge.dart'
    show AnyhowException;

/// Turn any error thrown across the bridge into a human-readable message.
///
/// Rust `anyhow` errors surface as [AnyhowException] whose `message` carries
/// the error chain *and* a `Stack backtrace:` dump full of `<unknown>`
/// frames. Showing that verbatim floods the UI, so we unwrap the message
/// and drop everything from the backtrace marker onward, keeping just the
/// error (and its `caused by` chain).
String friendlyError(Object error) {
  var text = error is AnyhowException ? error.message : error.toString();

  // anyhow appends the backtrace after a blank line; cut it off. Match a few
  // spellings defensively (`Stack backtrace:` / `Backtrace:`).
  for (final marker in const [
    '\nStack backtrace',
    '\nBacktrace',
    'Stack backtrace:',
  ]) {
    final idx = text.indexOf(marker);
    if (idx != -1) {
      text = text.substring(0, idx);
      break;
    }
  }

  return text.trim();
}

/// True when a turn-failure message is the Windows sandbox helper failing to
/// start — an embedded (自带) host that can't spawn `codex-windows-sandbox-setup`
/// / `codex-command-runner` for a sandboxed turn. The UI rewrites this into
/// actionable guidance (switch to no-sandbox Full mode, or use an external
/// host) instead of showing the raw "windows sandbox: spawn setup refresh".
bool isSandboxHelperFailure(String message) {
  final lower = message.toLowerCase();
  final mentionsWindowsSandbox =
      lower.contains('windows sandbox') || lower.contains('windows-sandbox');
  final looksLikeSpawnFailure =
      lower.contains('setup') ||
      lower.contains('spawn') ||
      lower.contains('command-runner') ||
      lower.contains('program not found');
  return mentionsWindowsSandbox && looksLikeSpawnFailure;
}
