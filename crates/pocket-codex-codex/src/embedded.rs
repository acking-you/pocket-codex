//! Run codex's app-server **in-process** over a localhost WebSocket.
//!
//! Compiled only under the `embedded-codex` feature (the desktop self-contained
//! mode). Instead of spawning an external `codex` binary, we run codex's own
//! app-server as a task in our process, listening on `ws://127.0.0.1:<port>`, so
//! the rest of the bridge — which connects to a `ws://` app-server and tunnels
//! it — works unchanged, with no codex install required. The `external` (spawn)
//! path stays available and is the default.

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
    run_main_with_transport_options(
        Arg0DispatchPaths::default(),
        CliConfigOverrides::default(),
        LoaderOverrides::default(),
        /* strict_config */ false,
        /* default_analytics_enabled */ false,
        transport,
        SessionSource::default(),
        AppServerWebsocketAuthSettings::default(),
        AppServerRuntimeOptions::default(),
    )
    .await
    .map_err(|e| anyhow::anyhow!("embedded codex app-server exited: {e}"))?;
    Ok(())
}
