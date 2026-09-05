//! `pocket-codex serve` high-level host-side orchestration.
//!
//! ```text
//!                       pocket-codex serve …
//!                                │
//!                                ▼
//!                ServiceId::new(device, App, name).key()
//!                          (or args.key)
//!                                │
//!                                ▼
//!              pocket_codex_codex::spawn(SpawnOptions)
//!                                │
//!                                ▼
//!              websocket_listen_addr("ws://host:port")
//!                                │
//!                                ▼
//!              managed_pb::ensure(PbWorkerSpec {
//!                role:  PbRole::Register,
//!                key,
//!                local_addr,
//!                relay_addr,
//!                codec,
//!              })
//!                                │
//!                                ▼
//!              print_serve_summary + "client setup: pocket-codex
//!              connect --key <key> --relay <relay>"
//! ```
//!
//! `serve` is the host side of an app-server pairing: it owns the
//! `codex app-server` child and the pb-mapper register worker that
//! exposes its WebSocket. Non-WebSocket listen URLs (for example unix
//! sockets) are rejected because pb-mapper needs a relayable TCP
//! endpoint.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use pocket_codex_codex::{
    spawn_ready, stop, ListenSpec, SpawnOptions, SpawnReadyError, READY_TIMEOUT,
};
use pocket_codex_core::{
    config::Config,
    process::{find_codex_app_server, force_kill, send_sigterm, wait_for_port_closed},
    service::{default_device_id, ServiceId, ServiceKind},
    state::PbRole,
};

use crate::{
    cli::ServeArgs,
    commands::{
        api_proxy, codex,
        managed_pb::{self, EnsureOutcome, PbWorkerSpec},
        transport::{self, Transport},
        ui,
    },
};

/// How often the watchdog probes the codex app-server's health endpoint.
const HEALTH_INTERVAL: Duration = Duration::from_secs(15);
/// Per-probe timeout — a wedged app-server typically hangs rather than
/// refusing.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(4);
/// Consecutive failed probes before the app-server is treated as wedged.
const HEALTH_FAILURES: u32 = 3;
/// Pause after a restart before probing resumes, so a freshly spawned
/// app-server isn't re-flagged while it is still booting.
const HEALTH_RESTART_GRACE: Duration = Duration::from_secs(12);
/// Upper bound on the backoff between repeated failed restarts, so a
/// hopelessly-broken codex is retried calmly rather than hammered every cycle.
const MAX_RESTART_BACKOFF: Duration = Duration::from_secs(300);
/// After this many consecutive failed restarts, stop logging every attempt.
const MAX_RESTART_WARNINGS: u32 = 3;

/// Run the host-side one-shot setup flow.
pub async fn run(args: ServeArgs) -> Result<()> {
    // Resolve the effective upstream proxy once (explicit flag or env). The
    // spawned app-server reads proxy settings only from its environment, never
    // from codex's config.toml, so we inject it there via SpawnOptions. Only an
    // explicit `--proxy` is validated eagerly (see resolve_app_server_proxy).
    let proxy_requested = args.proxy.is_some();
    let effective_proxy = api_proxy::resolve_app_server_proxy(args.proxy.as_deref())?;

    let config = Config::load()?;
    let transport =
        transport::resolve_transport(args.relay.relay.as_deref(), None, &config).await?;

    let device = args.device.clone().unwrap_or_else(default_device_id);
    let name = args.name.clone();
    let codec = args.codec;
    let explicit_key = args.key.clone();

    let requested_listen = ListenSpec::WebSocket {
        host: args.host,
        port: args.port,
    };
    let spawn_opts = SpawnOptions {
        binary: args.codex_binary,
        listen: requested_listen,
        extra_args: args.extra,
        log_file: None,
        proxy: effective_proxy.clone(),
    };
    // Spawn + readiness in one step: don't publish (or print success for) a
    // child that died on boot — classically a bind failure on an
    // already-taken port, which used to surface only minutes later as
    // `codex stale` in `status`. Fail now, with the child's own error output.
    let report =
        spawn_ready(spawn_opts.clone(), READY_TIMEOUT).map_err(codex::spawn_ready_error)?;
    let local_addr = websocket_listen_addr(&report.info.listen).with_context(|| {
        format!("codex listen URL `{}` is not relayable TCP", report.info.listen)
    })?;

    if explicit_key.is_some() && transport.is_account() {
        ui::warn("--key is ignored in account mode; the service is namespaced to your account");
    }
    let app = ServiceId::new(&device, ServiceKind::App, &name);
    // An explicit `--key` is honoured only in self-host mode: the relay confines
    // an account credential to its own namespace, so a key outside it is refused.
    let key = match explicit_key.filter(|_| !transport.is_account()) {
        Some(key) => key,
        None => transport.key(&app),
    };
    let outcome = managed_pb::ensure(PbWorkerSpec {
        role: PbRole::Register,
        key: key.clone(),
        local_addr: local_addr.clone(),
        session: transport.session.clone(),
        codec,
    })
    .await?;
    print_serve_summary(
        &report.info,
        &outcome,
        &key,
        &transport,
        effective_proxy.as_deref(),
        proxy_requested,
        report.reused,
    );

    // Self-host mode is done: the register worker is a detached child, so the
    // operator gets their shell back. Account mode stays in the foreground
    // because the host meta service below is an IN-PROCESS listener — it cannot
    // outlive this command, so exiting here would publish a meta key with
    // nothing behind it.
    if !transport.is_account() {
        return Ok(());
    }
    serve_account_foreground(&transport, &device, &name, &key, local_addr, spawn_opts).await
}

/// Account-mode tail of `serve`: publish the host meta service beside the app
/// service, supervise codex's health, and hold both until Ctrl-C — then tear
/// the whole host down, `app_key`'s detached register worker and codex
/// included.
async fn serve_account_foreground(
    transport: &Transport,
    device: &str,
    name: &str,
    app_key: &str,
    local_addr: String,
    mut spawn_opts: SpawnOptions,
) -> Result<()> {
    let local: SocketAddr = local_addr
        .parse()
        .with_context(|| format!("codex listen addr `{local_addr}` is not a socket address"))?;

    // Expose the host meta service alongside the app service so a CLI-hosted
    // server's sessions are remote-viewable (#5) and its per-thread config
    // persists (#2) — parity with the in-app host. Best-effort: a meta failure
    // must not stop the app-server from serving. Its own registration rather
    // than a share of the app's, because the relay maps one local address per key
    // and this is a second listener.
    let meta = match spawn_meta_service(local).await {
        Ok(meta_local) => {
            pocket_codex_pb::register(&transport.session, pocket_codex_pb::RegisterOptions {
                key: transport.key(&ServiceId::new(device, ServiceKind::Meta, name)),
                local_addr: meta_local.to_string(),
                codec: false,
            })
            .await
            .map(Some)
        },
        Err(e) => Err(e),
    };
    match &meta {
        Ok(_) => ui::field("meta", &format!("{device}/meta/{name}")),
        Err(e) => ui::warn(&format!("host meta service unavailable: {e:#}")),
    }

    // Pin the watchdog's respawn to the *resolved* listen address so a restart
    // always rebinds the same port the register worker forwards to (robust even
    // if the operator asked for `--port 0`).
    spawn_opts.listen = ListenSpec::WebSocket {
        host: local.ip().to_string(),
        port: local.port(),
    };
    tokio::spawn(codex_health_watchdog(local_addr.clone(), spawn_opts));

    ui::headline(ui::Tone::Action, "exposing — keep this running, Ctrl-C to stop");
    tokio::signal::ctrl_c()
        .await
        .context("waiting for Ctrl-C")?;

    // Ctrl-C was advertised as stopping the host, so it has to stop ALL of it. The
    // meta service is in-process and dies with us, but the app register worker is
    // a detached child and codex is its own process — leaving those behind would
    // keep the app service reachable on the relay while its meta service vanished,
    // which is a worse state than either fully up or fully down.
    ui::headline(ui::Tone::Change, "stopping");
    // Awaited rather than dropped, so the relay frees the meta key now instead of
    // at its next lease sweep.
    if let Ok(Some(registration)) = meta {
        let _ = registration.stop().await;
    }
    let stopped = managed_pb::stop_matching(managed_pb::StopFilter {
        role: Some(PbRole::Register),
        key: Some(app_key.to_string()),
    })?;
    for outcome in &stopped {
        if let managed_pb::StopOutcome::Stopped(session) = outcome {
            ui::field("stopped pb register", &session.pid.to_string());
        }
    }
    match stop_codex_at(&local_addr) {
        Some(pid) => ui::field("stopped codex", &pid.to_string()),
        None => ui::muted("    codex was already gone"),
    }
    Ok(())
}

/// Stop the codex app-server listening on `listen_addr`, returning its pid.
///
/// Port-targeted rather than `pocket_codex_codex::stop`, which knows only about
/// a single recorded codex: this command may be one of several hosts on the
/// machine, and it must stop the one IT started and no other.
fn stop_codex_at(listen_addr: &str) -> Option<u32> {
    let pid = find_codex_app_server(&format!("ws://{listen_addr}"))?;
    send_sigterm(pid);
    // Escalate only if it ignores the graceful stop, so a codex mid-write gets a
    // chance to finish.
    if !wait_for_port_closed(listen_addr, Duration::from_secs(6)) {
        force_kill(pid);
    }
    Some(pid)
}

/// Bind a loopback listener for the host meta service, start it (resuming into
/// the colocated app-server at `app_ws` and persisting per-thread config under
/// `CODEX_HOME`), and return its bound address so the caller can register a
/// `meta:` tunnel for it. The task lives for the lifetime of this foreground
/// `serve`; process exit on Ctrl-C tears it down.
async fn spawn_meta_service(app_ws: SocketAddr) -> Result<SocketAddr> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("binding the host meta service listener")?;
    let addr = listener
        .local_addr()
        .context("reading the meta service listener address")?;
    let db_path = pocket_codex_host_svc::store::default_db_path()
        .context("resolving the meta config store path")?;
    let store = Arc::new(
        pocket_codex_host_svc::store::ConfigStore::open(db_path)
            .await
            .context("opening the meta config store")?,
    );
    let host_config_path = pocket_codex_host_svc::store::default_host_config_path()
        .context("resolving the host config store path")?;
    let host = Arc::new(
        pocket_codex_host_svc::store::HostStore::open(host_config_path)
            .await
            .context("opening the host config store")?,
    );
    tokio::spawn(async move {
        if let Err(e) = pocket_codex_host_svc::serve(listener, app_ws, store, host).await {
            ui::warn(&format!("host meta service exited: {e:#}"));
        }
    });
    Ok(addr)
}

fn print_serve_summary(
    codex: &pocket_codex_core::state::CodexProcessInfo,
    pb: &EnsureOutcome,
    key: &str,
    transport: &Transport,
    effective_proxy: Option<&str>,
    proxy_requested: bool,
    reused: bool,
) {
    ui::headline(ui::Tone::Ok, "codex app-server");
    ui::field("pid", &codex.pid.to_string());
    ui::field("listen", &codex.listen);
    ui::field("log", &codex.log_file.display().to_string());
    api_proxy::print_proxy_status(
        effective_proxy,
        proxy_requested,
        reused,
        api_proxy::SpawnCommand::Serve,
    );
    pb.render("pb register");
    ui::headline(ui::Tone::Action, "client setup");
    ui::code(&crate::commands::api::client_setup_command("connect", key, transport));
}

fn websocket_listen_addr(listen: &str) -> Option<String> {
    listen
        .strip_prefix("ws://")
        .filter(|addr| !addr.is_empty())
        .map(ToOwned::to_owned)
}

/// Background task (account mode): probe HTTP readiness and initialized
/// thread RPCs, restarting the app-server after consecutive failures.
///
/// `thread/list` catches a stalled request dispatcher even when `/readyz`
/// and WebSocket pongs remain healthy. It does not run a model turn, so a
/// single hung generation on an otherwise responsive server is not restarted.
async fn codex_health_watchdog(local_addr: String, spawn_opts: SpawnOptions) {
    let url = format!("http://{local_addr}/readyz");
    let client = match reqwest::Client::builder()
        .timeout(HEALTH_TIMEOUT)
        .no_proxy()
        .build()
    {
        Ok(client) => client,
        // Building a loopback HTTP client should never fail; if it somehow does
        // there is nothing useful the watchdog can do, so bow out quietly.
        Err(_) => return,
    };
    let mut consecutive: u32 = 0;
    let mut restart_failures: u32 = 0;
    loop {
        tokio::time::sleep(HEALTH_INTERVAL).await;
        let http_ready = matches!(
            client.get(&url).send().await,
            Ok(resp) if resp.status().is_success()
        );
        let rpc_ready = if http_ready {
            let result = pocket_codex_codex::readiness::probe_rpc(
                &format!("ws://{local_addr}"),
                READY_TIMEOUT,
            )
            .await;
            if let Err(error) = &result {
                tracing::warn!(%local_addr, error = %format!("{error:#}"), "app-server functional health probe failed");
            }
            result.is_ok()
        } else {
            false
        };
        if rpc_ready {
            consecutive = 0;
            restart_failures = 0;
            continue;
        }
        consecutive += 1;
        if consecutive < HEALTH_FAILURES {
            continue;
        }
        ui::warn(&format!(
            "codex app-server failed {HEALTH_FAILURES} health checks — restarting it"
        ));
        match restart_codex(spawn_opts.clone()).await {
            Ok(()) => {
                ui::headline(ui::Tone::Ok, "codex app-server restarted");
                restart_failures = 0;
            },
            Err(e) => {
                restart_failures += 1;
                if restart_failures <= MAX_RESTART_WARNINGS {
                    ui::warn(&format!(
                        "could not restart codex app-server (attempt {restart_failures}): {e:#}"
                    ));
                } else if restart_failures == MAX_RESTART_WARNINGS + 1 {
                    ui::warn(
                        "codex app-server is not recovering — will keep retrying quietly; check \
                         the codex install on this host",
                    );
                }
            },
        }
        consecutive = 0;
        // After repeated failures back off (capped) so a hopeless loop neither
        // spams nor hammers; a transient wedge that restarts cleanly resets the
        // counter and returns to the short grace.
        let backoff = HEALTH_RESTART_GRACE
            .saturating_mul(1u32 << restart_failures.min(5))
            .min(MAX_RESTART_BACKOFF);
        tokio::time::sleep(backoff).await;
    }
}

/// Stop the wedged codex app-server and spawn a fresh one on the same port. The
/// register tunnel forwards to a fixed local address, so a same-port respawn
/// keeps routing intact. Escalates to a hard kill if the process ignores the
/// graceful stop, and reports failure (rather than false success) when codex is
/// still holding the port afterwards. Runs the blocking process calls off the
/// async runtime.
async fn restart_codex(spawn_opts: SpawnOptions) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        let listen_url = spawn_opts.listen.to_listen_url();
        stop().context("stopping the wedged codex app-server")?;
        // Wait for the listen port to free; otherwise spawn() would adopt the
        // dying process instead of starting a clean one.
        if let Some(addr) = spawn_opts.listen.as_socket_addr() {
            if !wait_for_port_closed(&addr, Duration::from_secs(8)) {
                // The graceful SIGTERM didn't free the port — a wedged codex may
                // be ignoring it; hard-kill whatever codex still owns the port.
                if let Some(pid) = find_codex_app_server(&listen_url) {
                    force_kill(pid);
                    wait_for_port_closed(&addr, Duration::from_secs(5));
                }
            }
        }
        let report = spawn_ready(spawn_opts, READY_TIMEOUT).map_err(|err| match err {
            SpawnReadyError::Spawn(e) => {
                anyhow::Error::new(e).context("respawning the codex app-server")
            },
            // An adopted (reused) listener means spawn found the OLD process
            // still bound — the restart did not take effect; report that
            // precisely rather than as a readiness failure. Otherwise a
            // respawn that dies on boot must count as a FAILED restart — so
            // the watchdog's backoff engages — not as a success that resets it.
            SpawnReadyError::NotReady {
                report,
                failure,
            } => {
                if report.reused {
                    anyhow::anyhow!(
                        "codex is still holding the listen port; restart did not take effect"
                    )
                } else {
                    anyhow::anyhow!("{failure}")
                }
            },
        })?;
        // Adopted-and-ready reads the same way: the old process survived, so
        // the restart did not take effect.
        anyhow::ensure!(
            !report.reused,
            "codex is still holding the listen port; restart did not take effect"
        );
        Ok(())
    })
    .await
    .context("codex restart task panicked")?
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_listen_addr_extracts_tcp_addr() {
        assert_eq!(
            websocket_listen_addr("ws://127.0.0.1:18080").as_deref(),
            Some("127.0.0.1:18080")
        );
        assert_eq!(websocket_listen_addr("unix:///tmp/codex.sock"), None);
    }
}
