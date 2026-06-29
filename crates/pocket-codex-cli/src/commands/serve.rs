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

use std::{
    net::{SocketAddr, TcpStream},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use pocket_codex_broker_client::{run_register, Connector, RegisterConfig, TokenProvider};
use pocket_codex_codex::{spawn, stop, ListenSpec, SpawnOptions};
use pocket_codex_core::{
    config::Config,
    process::{find_codex_app_server, force_kill},
    service::{default_device_id, ServiceId, ServiceKind},
    state::PbRole,
};

use crate::{
    cli::ServeArgs,
    commands::{
        account, api_proxy,
        managed_pb::{self, EnsureOutcome, PbWorkerSpec},
        transport::{self, Transport},
        ui,
    },
};

/// Idle timeout applied to account-mode data bridges.
const ACCOUNT_DATA_IDLE: Duration = Duration::from_secs(1800);

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
    let transport = transport::resolve_transport(args.relay.relay.as_deref(), None, &config)?;

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
    let report = spawn(spawn_opts.clone())?;
    let local_addr = websocket_listen_addr(&report.info.listen).with_context(|| {
        format!("codex listen URL `{}` is not relayable TCP", report.info.listen)
    })?;

    match transport {
        Transport::SelfHost {
            relay,
        } => {
            let key = explicit_key
                .unwrap_or_else(|| ServiceId::new(&device, ServiceKind::App, &name).key());
            let outcome = managed_pb::ensure(PbWorkerSpec {
                role: PbRole::Register,
                key: key.clone(),
                local_addr,
                relay_addr: relay.clone(),
                codec,
            })?;
            print_serve_summary(
                &report.info,
                &outcome,
                &key,
                &relay,
                effective_proxy.as_deref(),
                proxy_requested,
                report.reused,
            );
            Ok(())
        },
        Transport::Account {
            backend,
        } => {
            ui::headline(ui::Tone::Ok, "codex app-server");
            ui::field("pid", &report.info.pid.to_string());
            ui::field("listen", &report.info.listen);
            ui::field("log", &report.info.log_file.display().to_string());
            api_proxy::print_proxy_status(
                effective_proxy.as_deref(),
                proxy_requested,
                report.reused,
                api_proxy::SpawnCommand::Serve,
            );
            if explicit_key.is_some() {
                ui::warn(
                    "--key is ignored in account mode; the service is namespaced to your account",
                );
            }
            serve_account(&backend, &device, &name, local_addr, spawn_opts).await
        },
    }
}

/// Account-mode host side: register the local app-server through the backend
/// broker. Runs in the foreground (holds the control tunnel) until interrupted.
async fn serve_account(
    backend: &str,
    device: &str,
    name: &str,
    local_addr: String,
    mut spawn_opts: SpawnOptions,
) -> Result<()> {
    let (host, port) = account::broker_endpoint(backend)?;
    let connector: Arc<dyn Connector> = Arc::new(account::BrokerTlsConnector::new(host, port)?);
    let tokens: Arc<dyn TokenProvider> =
        Arc::new(account::ConfigTokenProvider::new(backend.to_string()));
    let local: SocketAddr = local_addr
        .parse()
        .with_context(|| format!("codex listen addr `{local_addr}` is not a socket address"))?;

    // Pin the watchdog's respawn to the *resolved* listen address so a restart
    // always rebinds the same port the register tunnel forwards to (robust even
    // if the operator asked for `--port 0`). The watchdog lives for the lifetime
    // of this foreground `serve`; the process exit on Ctrl-C tears it down.
    spawn_opts.listen = ListenSpec::WebSocket {
        host: local.ip().to_string(),
        port: local.port(),
    };
    tokio::spawn(codex_health_watchdog(local_addr, spawn_opts));

    ui::headline(ui::Tone::Ok, "account register");
    ui::field("backend", backend);
    ui::field("service", &format!("{device}/app/{name}"));

    // Expose the host meta service alongside the app tunnel so a CLI-hosted
    // server's sessions are remote-viewable (#5) and its per-thread config
    // persists (#2) — parity with the in-app host. Best-effort: a meta failure
    // must not stop the app-server from serving.
    match spawn_meta_service(local).await {
        Ok(meta_local) => {
            let connector = connector.clone();
            let tokens = tokens.clone();
            let dev = device.to_string();
            let nm = name.to_string();
            tokio::spawn(async move {
                run_register(connector, tokens, RegisterConfig {
                    device: dev,
                    kind: ServiceKind::Meta,
                    name: nm,
                    client_instance_id: account::client_instance_id(),
                    local_addr: meta_local,
                    idle: ACCOUNT_DATA_IDLE,
                })
                .await;
            });
            ui::field("meta", &format!("{device}/meta/{name}"));
        },
        Err(e) => ui::warn(&format!("host meta service unavailable: {e:#}")),
    }

    ui::headline(ui::Tone::Action, "exposing — keep this running, Ctrl-C to stop");

    run_register(connector, tokens, RegisterConfig {
        device: device.to_string(),
        kind: ServiceKind::App,
        name: name.to_string(),
        client_instance_id: account::client_instance_id(),
        local_addr: local,
        idle: ACCOUNT_DATA_IDLE,
    })
    .await;
    Ok(())
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
    tokio::spawn(async move {
        if let Err(e) = pocket_codex_host_svc::serve(listener, app_ws, store).await {
            ui::warn(&format!("host meta service exited: {e:#}"));
        }
    });
    Ok(addr)
}

fn print_serve_summary(
    codex: &pocket_codex_core::state::CodexProcessInfo,
    pb: &EnsureOutcome,
    key: &str,
    relay: &str,
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
    ui::code(&format!("pocket-codex connect --key {key} --relay {relay}"));
}

fn websocket_listen_addr(listen: &str) -> Option<String> {
    listen
        .strip_prefix("ws://")
        .filter(|addr| !addr.is_empty())
        .map(ToOwned::to_owned)
}

/// Background task (account mode): probe the codex app-server's `/readyz` and
/// restart it when it stops responding, so turns recover without operator
/// intervention.
///
/// `/readyz` reflects the HTTP acceptor's liveness, so this recovers a codex
/// that has fully crashed or stopped accepting (process gone, connection
/// refused, hung acceptor) — the "registered on the relay but the remote is
/// dead" case. It does NOT catch a codex that still accepts connections but has
/// wedged deeper (a hung model turn keeps `/readyz` green); detecting that
/// would need a turn-level probe and is out of scope here.
async fn codex_health_watchdog(local_addr: String, spawn_opts: SpawnOptions) {
    let url = format!("http://{local_addr}/readyz");
    let client = match reqwest::Client::builder().timeout(HEALTH_TIMEOUT).build() {
        Ok(client) => client,
        // Building a loopback HTTP client should never fail; if it somehow does
        // there is nothing useful the watchdog can do, so bow out quietly.
        Err(_) => return,
    };
    let mut consecutive: u32 = 0;
    let mut restart_failures: u32 = 0;
    loop {
        tokio::time::sleep(HEALTH_INTERVAL).await;
        let healthy = matches!(
            client.get(&url).send().await,
            Ok(resp) if resp.status().is_success()
        );
        if healthy {
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
        let report = spawn(spawn_opts).context("respawning the codex app-server")?;
        // If spawn adopted the still-bound old process instead of starting a
        // fresh one, the restart did not take effect.
        anyhow::ensure!(
            !report.reused,
            "codex is still holding the listen port; restart did not take effect"
        );
        Ok(())
    })
    .await
    .context("codex restart task panicked")?
}

/// Block until nothing accepts TCP connections on `addr`, or `timeout` elapses.
/// Returns `true` if the port closed, `false` on timeout.
fn wait_for_port_closed(addr: &str, timeout: Duration) -> bool {
    let Ok(sock) = addr.parse::<SocketAddr>() else {
        return true;
    };
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&sock, Duration::from_millis(200)).is_err() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
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
