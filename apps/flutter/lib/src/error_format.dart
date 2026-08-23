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
/// LAUNCH — an embedded (自带) host that can't spawn its bundled
/// `codex-windows-sandbox-setup` / `codex-command-runner` exes for a sandboxed
/// turn. The UI rewrites this into actionable guidance (switch to no-sandbox
/// Full mode, or use an external host) instead of the raw
/// "windows sandbox: spawn setup refresh".
///
/// Deliberately narrow: it matches only the *cannot-launch-the-exe* signal, not
/// any message mentioning "setup". A properly-installed (usually external) host
/// can fail a sandboxed turn with a genuinely different, still-actionable
/// remedy — e.g. "Windows sandbox setup is missing or out of date; rerun the
/// sandbox setup with elevation" or a setup-marker version mismatch — and those
/// must reach the user verbatim rather than be replaced with "switch to Full".
bool isSandboxHelperFailure(String message) {
  final lower = message.toLowerCase();
  final mentionsWindowsSandbox =
      lower.contains('windows sandbox') || lower.contains('windows-sandbox');
  // The exe couldn't be launched at all — the embedded "no bundled helpers"
  // case. `spawn setup` is the exact `.context()` on the failing spawn.
  final cannotLaunchHelper =
      lower.contains('program not found') ||
      lower.contains('failed to spawn') ||
      lower.contains('spawn setup');
  return mentionsWindowsSandbox && cannotLaunchHelper;
}

/// True when starting/re-registering a hosting failed because another LIVE
/// instance already owns the service name — the broker refused the
/// registration (first-wins) or the self-hosted relay reported a healthy
/// publisher for the key. The UI rewrites this into localized guidance
/// (stop the other instance or pick another name) instead of the raw reason.
/// Matches the exact phrasings produced by `engine/serve.rs`'s pre-flight,
/// the broker's conflict nack, and the CLI relay probe.
bool isHostNameConflict(String message) {
  final lower = message.toLowerCase();
  return lower.contains('name is already in use') ||
      lower.contains('already owns') ||
      lower.contains('already registered and online');
}

/// True when a reachability probe failed because the far end ANSWERED and
/// refused the handshake over the relay — the tunnel carried bytes, but the
/// relay rejected us (a missing or stale authentication code). This is not a
/// dead backend, and the fix is different: the key/token has to be re-supplied,
/// not the host restarted.
///
/// The relay answers the WebSocket upgrade with `403 missing or invalid
/// authentication code`, which reaches us through the transport's error chain.
bool isRelayAuthRejection(String message) {
  final lower = message.toLowerCase();
  return lower.contains('authentication code') ||
      (lower.contains('403') && lower.contains('forbidden'));
}

/// True when the probe never got an answer at all — the far end is genuinely
/// silent (not listening, wedged, or gone). Distinct from
/// [isRelayAuthRejection], where it answered and said no.
bool isProbeTimeout(String message) {
  final lower = message.toLowerCase();
  return lower.contains('timed out') || lower.contains('timeout');
}
