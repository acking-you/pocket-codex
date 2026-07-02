//! Run codex's app-server **in-process** over a localhost WebSocket.
//!
//! Compiled only under the `embedded-codex` feature (the desktop self-contained
//! mode). Instead of spawning an external `codex` binary, we run codex's own
//! app-server as a task in our process, listening on `ws://127.0.0.1:<port>`,
//! so the rest of the bridge — which connects to a `ws://` app-server and
//! tunnels it — works unchanged, with no codex install required. The `external`
//! (spawn) path stays available and is the default.

use anyhow::{Context, Result};
use codex_app_server::{
    run_main_with_transport_options, AppServerRuntimeOptions, AppServerTransport,
    AppServerWebsocketAuthSettings,
};
use codex_arg0::Arg0DispatchPaths;
use codex_config::LoaderOverrides;
use codex_protocol::protocol::SessionSource;
use codex_utils_cli::CliConfigOverrides;

/// Run codex's app-server in-process, serving `listen_url` (a `ws://IP:PORT`).
///
/// Resolves when the server stops, so callers spawn it on the async runtime to
/// host codex for the lifetime of a local serve. The transport, auth, and
/// runtime options mirror what the standalone `codex-app-server --listen <url>`
/// binary assembles, minus the process-level arg0 dispatch (we are not a
/// multi-call binary): no websocket auth, default config/loader overrides, and
/// the `AppServer` session source.
pub async fn run(listen_url: &str) -> Result<()> {
    let transport: AppServerTransport = listen_url
        .parse()
        .with_context(|| format!("parsing embedded app-server listen URL `{listen_url}`"))?;
    // codex re-execs this path for its sandbox/exec helper modes and refuses to
    // start without it. In-process we are the host app (pocket_codex), not a
    // codex multi-call binary, so we hand it our own exe to satisfy startup and
    // bind the listener. Running *sandboxed commands* additionally needs arg0
    // dispatch so codex's helper invocation re-enters codex instead of a second
    // app window — tracked as a follow-up; model turns work without it.
    let arg0_paths = Arg0DispatchPaths {
        codex_self_exe: std::env::current_exe().ok(),
        ..Default::default()
    };
    run_main_with_transport_options(
        arg0_paths,
        embedded_config_overrides(),
        LoaderOverrides::default(),
        // strict_config
        false,
        // default_analytics_enabled
        false,
        transport,
        SessionSource::default(),
        AppServerWebsocketAuthSettings::default(),
        AppServerRuntimeOptions::default(),
    )
    .await
    .map_err(|e| anyhow::anyhow!("embedded codex app-server exited: {e}"))?;
    Ok(())
}

/// Config overrides applied to the in-process app-server, equivalent to
/// `codex -c key=value`. Sourced from `POCKET_CODEX_CODEX_CONFIG` — one
/// `key=value` pair per LINE (values are parsed as TOML, falling back to a
/// literal string). The host app uses this to steer the embedded codex without
/// a `config.toml`; e.g. selecting the Windows sandbox level
/// (`windows.sandbox=unelevated`) for a self-contained build that bundles the
/// restricted-token helper exes but cannot request elevation. Empty/unset →
/// codex's own defaults.
///
/// Splitting is newline-only on purpose: `,` and `;` occur inside legitimate
/// TOML values (arrays like `["a", "b"]`, inline tables, PATH-like strings), so
/// using them as pair separators would shred a single override into fragments —
/// erroring out embedded startup, or worse, silently applying a mangled value.
fn embedded_config_overrides() -> CliConfigOverrides {
    let raw_overrides = std::env::var("POCKET_CODEX_CODEX_CONFIG")
        .ok()
        .map(|raw| parse_override_lines(&raw))
        .unwrap_or_default();
    CliConfigOverrides {
        raw_overrides,
    }
}

/// One `key=value` override per non-blank line, trimmed. Newline-only split so
/// commas/semicolons inside TOML values survive (see
/// [`embedded_config_overrides`]).
fn parse_override_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse_override_lines;

    #[test]
    fn parses_one_override_per_line_and_preserves_commas() {
        assert_eq!(parse_override_lines("windows.sandbox=unelevated"), vec![
            "windows.sandbox=unelevated"
        ]);
        // Blank lines and surrounding whitespace are dropped/trimmed.
        assert_eq!(
            parse_override_lines("  windows.sandbox=unelevated \n\n  model=\"gpt-5.5\" "),
            vec!["windows.sandbox=unelevated", "model=\"gpt-5.5\""]
        );
        // A comma-bearing TOML value stays a SINGLE override (the bug this guards).
        assert_eq!(
            parse_override_lines(
                "sandbox_permissions=[\"disk-full-read-access\", \"disk-write-cwd\"]"
            ),
            vec!["sandbox_permissions=[\"disk-full-read-access\", \"disk-write-cwd\"]"]
        );
        assert!(parse_override_lines("").is_empty());
        assert!(parse_override_lines("   \n  \n").is_empty());
    }
}
