//! Host a local `codex app-server` from the app and register it through the
//! account broker — the in-app equivalent of `pocket-codex serve` (account
//! mode). Desktop only: it spawns the user's `codex` binary as a child process.
//!
//! Several hosts can run at once, keyed by service name. `serve_start` spawns
//! (or reuses) codex on a loopback WebSocket, then runs two background tasks on
//! the engine runtime — the broker register tunnel (`run_register`,
//! reconnecting forever) and a health watchdog that restarts codex if it stops
//! responding. `serve_stop(name)` aborts both and stops that host's codex
//! (port-targeted, so hosts are independent); `serve_stop_all` (called on app
//! quit) stops every host, so a real quit leaves no orphan — closing to the
//! tray keeps the app, and thus hosting, alive.
//!
//! This mirrors `crates/pocket-codex-cli/src/commands/serve.rs::serve_account`;
//! the watchdog/restart logic is duplicated here (logging via `tracing` rather
//! than the CLI's `ui::`) to avoid pulling an HTTP probe dependency into the
//! process-manager crate.

use std::{
    collections::HashMap,
    net::{SocketAddr, TcpStream},
    sync::Mutex,
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};
use once_cell::sync::OnceCell;
use pocket_codex_broker_client::{run_register, RegisterConfig};
use pocket_codex_codex::{locate_binary, spawn, ListenSpec, SpawnOptions};
use pocket_codex_core::{
    process::{find_codex_app_server, force_kill, send_sigterm, tcp_port_open},
    service::{default_device_id, ServiceId, ServiceKind},
};
use tokio::task::JoinHandle;

use crate::engine::{
    account,
    config::{load_config, save_config},
    runtime,
};

/// How often the watchdog probes codex's `/readyz`.
const HEALTH_INTERVAL: Duration = Duration::from_secs(15);
/// Per-probe timeout — a wedged app-server hangs rather than refusing.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(4);
/// Consecutive failed probes before codex is treated as wedged.
const HEALTH_FAILURES: u32 = 3;
/// Pause after a restart before probing resumes (codex is still booting).
const HEALTH_RESTART_GRACE: Duration = Duration::from_secs(12);
/// Upper bound on the backoff between repeated failed restarts.
const MAX_RESTART_BACKOFF: Duration = Duration::from_secs(300);

/// One active local host (codex app-server + its register tunnel + watchdog),
/// tracked process-globally. Several can run at once, keyed by service name.
struct LocalServe {
    device: String,
    name: String,
    service_key: String,
    listen_addr: String,
    pid: u32,
    register: JoinHandle<()>,
    watchdog: JoinHandle<()>,
}

/// Result of [`serve_start`], surfaced to the UI.
#[derive(Debug, Clone)]
pub struct ServeReport {
    /// Device id the service was registered under.
    pub device: String,
    /// Service instance name.
    pub name: String,
    /// `pcx:<device>:app:<name>` key — what discovery + `app_connect` use.
    pub service_key: String,
    /// Loopback `host:port` codex is listening on.
    pub listen_addr: String,
    /// The codex process id.
    pub pid: u32,
    /// Whether an already-running codex was reused rather than freshly spawned.
    pub reused: bool,
}

/// Status of local hosting, surfaced to the UI.
#[derive(Debug, Clone, Default)]
pub struct ServeStatus {
    /// The register tunnel task is live (we are hosting / trying to).
    pub running: bool,
    /// codex is actually accepting on its listen port.
    pub alive: bool,
    /// codex process id, when hosting.
    pub pid: Option<u32>,
    /// Loopback `host:port`, when hosting.
    pub listen_addr: Option<String>,
    /// Device id, when hosting.
    pub device: Option<String>,
    /// Service instance name, when hosting.
    pub name: Option<String>,
    /// `pcx:<device>:app:<name>` key, when hosting.
    pub service_key: Option<String>,
}

fn hosts() -> &'static Mutex<HashMap<String, LocalServe>> {
    static HOSTS: OnceCell<Mutex<HashMap<String, LocalServe>>> = OnceCell::new();
    HOSTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Stable-per-process client instance id for the broker register handshake (the
/// broker treats a new instance with the same key as a takeover).
fn client_instance_id() -> String {
    format!("app-{}", std::process::id())
}

/// The resolved codex binary path (explicit override → persisted config →
/// PATH), or `None` when none resolve so the UI can prompt for one.
pub fn codex_locate() -> Option<String> {
    let configured = runtime::support_dir()
        .ok()
        .and_then(|dir| load_config(&dir).ok())
        .and_then(|c| c.codex.binary.clone());
    locate_binary(configured.as_deref()).map(|p| p.display().to_string())
}

/// Validate an upstream proxy URL: codex's WebSocket tunnel writes a plaintext
/// CONNECT, so only `http://` and `socks5(h)://` work — `https://` would break
/// it. Mirrors the CLI's `api_proxy::validate_proxy`.
fn validate_proxy(raw: &str) -> Result<()> {
    let url = reqwest::Url::parse(raw).with_context(|| format!("parsing proxy URL `{raw}`"))?;
    match url.scheme() {
        "http" | "socks5" | "socks5h" => {},
        "https" => bail!(
            "https:// proxies are not supported (the WebSocket tunnel needs a plaintext \
             CONNECT); use an http:// or socks5:// proxy"
        ),
        other => bail!("unsupported proxy scheme `{other}`; use http or socks5"),
    }
    if url.host_str().is_none() {
        bail!("proxy URL `{raw}` is missing a host");
    }
    Ok(())
}

/// Start hosting a local codex app-server under the signed-in account. Spawns
/// (or reuses) codex on `127.0.0.1:port`, persists an explicit
/// `binary_override` for next time, and runs the register tunnel + watchdog in
/// the background. `proxy` is the upstream proxy codex uses to reach
/// chatgpt.com (`None` = no forced proxy / inherit the app's environment).
pub fn serve_start(
    port: u16,
    binary_override: Option<String>,
    name: Option<String>,
    proxy: Option<String>,
) -> Result<ServeReport> {
    let support = runtime::support_dir()?;
    let mut config = load_config(&support)?;
    if config.account_token().is_none() {
        bail!("sign in with GitHub before hosting a local app-server");
    }

    // Resolve the binary: explicit override → persisted config → `$PATH`.
    let override_trimmed = binary_override
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let candidate = override_trimmed
        .map(str::to_string)
        .or_else(|| config.codex.binary.clone());
    let binary = locate_binary(candidate.as_deref()).ok_or_else(|| {
        anyhow!(
            "could not find the codex binary{}; install codex or set its path",
            candidate
                .as_deref()
                .map(|c| format!(" at `{c}`"))
                .unwrap_or_default()
        )
    })?;
    // Remember an explicit override so the user only points at it once.
    if let Some(ov) = override_trimmed {
        if config.codex.binary.as_deref() != Some(ov) {
            config.codex.binary = Some(ov.to_string());
            save_config(&support, &config)?;
        }
    }

    let device = default_device_id();
    let name = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "default".to_string());
    let service_key = ServiceId::new(&device, ServiceKind::App, &name).key();

    // Several hosts can coexist, but each needs a distinct name (service key) and
    // a distinct port. Reject a collision rather than silently double-registering.
    {
        let guard = hosts().lock().expect("serve hosts poisoned");
        if let Some(ls) = guard.get(&name) {
            if !ls.register.is_finished() {
                bail!("already hosting `{name}` on {}", ls.listen_addr);
            }
        }
        if port != 0
            && guard
                .values()
                .any(|ls| ls.listen_addr == format!("127.0.0.1:{port}"))
        {
            bail!("port {port} is already used by another local host");
        }
    }

    // Resolve the upstream proxy: a non-empty value is validated + injected into
    // codex's environment; an empty/absent value means no forced proxy.
    let proxy = proxy
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if let Some(p) = proxy.as_deref() {
        validate_proxy(p)?;
    }

    // Spawn codex. This runs on the flutter_rust_bridge worker thread (not the
    // UI thread), so the up-to-15s port-resolve poll inside `spawn` is fine.
    let spawn_opts = SpawnOptions {
        binary: Some(binary),
        listen: ListenSpec::WebSocket {
            host: "127.0.0.1".to_string(),
            port,
        },
        extra_args: Vec::new(),
        log_file: None,
        proxy,
    };
    let report = spawn(spawn_opts.clone()).context("spawning codex app-server")?;
    let listen_addr = report
        .info
        .listen
        .strip_prefix("ws://")
        .map(str::to_string)
        .ok_or_else(|| anyhow!("codex listen `{}` is not a ws:// address", report.info.listen))?;
    let local: SocketAddr = listen_addr
        .parse()
        .with_context(|| format!("codex listen `{listen_addr}` is not a socket address"))?;

    // Pin the watchdog's respawn to the resolved port (robust even if port 0 was
    // requested) so a restart rebinds the port the register tunnel forwards to.
    let mut watchdog_opts = spawn_opts;
    watchdog_opts.listen = ListenSpec::WebSocket {
        host: local.ip().to_string(),
        port: local.port(),
    };

    let (connector, tokens) = account::broker_transport(&support)?;
    let register = runtime::runtime().spawn(run_register(connector, tokens, RegisterConfig {
        device: device.clone(),
        kind: ServiceKind::App,
        name: name.clone(),
        client_instance_id: client_instance_id(),
        local_addr: local,
        idle: account::ACCOUNT_DATA_IDLE,
    }));
    let watchdog = runtime::runtime().spawn(health_watchdog(listen_addr.clone(), watchdog_opts));

    hosts()
        .lock()
        .expect("serve hosts poisoned")
        .insert(name.clone(), LocalServe {
            device: device.clone(),
            name: name.clone(),
            service_key: service_key.clone(),
            listen_addr: listen_addr.clone(),
            pid: report.info.pid,
            register,
            watchdog,
        });

    Ok(ServeReport {
        device,
        name,
        service_key,
        listen_addr,
        pid: report.info.pid,
        reused: report.reused,
    })
}

/// Snapshot of every local host, sorted by name for a stable UI order.
pub fn serve_status() -> Vec<ServeStatus> {
    let guard = hosts().lock().expect("serve hosts poisoned");
    let mut out: Vec<ServeStatus> = guard
        .values()
        .map(|ls| ServeStatus {
            running: !ls.register.is_finished(),
            alive: listen_addr_open(&ls.listen_addr),
            pid: Some(ls.pid),
            listen_addr: Some(ls.listen_addr.clone()),
            device: Some(ls.device.clone()),
            name: Some(ls.name.clone()),
            service_key: Some(ls.service_key.clone()),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Stop one host by name: abort its register + watchdog tasks and stop its
/// codex. Best-effort + idempotent (no-op when that name isn't hosting).
pub fn serve_stop(name: &str) -> Result<()> {
    let removed = hosts().lock().expect("serve hosts poisoned").remove(name);
    if let Some(ls) = removed {
        ls.register.abort();
        ls.watchdog.abort();
        stop_codex_at(&ls.listen_addr);
    }
    Ok(())
}

/// Stop every host (called on app quit so a real quit leaves no orphan codex).
pub fn serve_stop_all() {
    let all: Vec<LocalServe> = hosts()
        .lock()
        .expect("serve hosts poisoned")
        .drain()
        .map(|(_, ls)| ls)
        .collect();
    for ls in all {
        ls.register.abort();
        ls.watchdog.abort();
        stop_codex_at(&ls.listen_addr);
    }
}

/// Stop the codex app-server listening on `listen_addr` — graceful SIGTERM,
/// then a force-kill if it keeps the port. Port-targeted (via
/// [`find_codex_app_server`] on the listen URL) so hosts on different ports
/// stop independently, unlike the single-codex `pocket_codex_codex::stop`.
fn stop_codex_at(listen_addr: &str) {
    let listen_url = format!("ws://{listen_addr}");
    let Some(pid) = find_codex_app_server(&listen_url) else {
        return;
    };
    send_sigterm(pid);
    if !wait_for_port_closed(listen_addr, Duration::from_secs(6)) {
        if let Some(pid) = find_codex_app_server(&listen_url) {
            force_kill(pid);
        }
    }
}

/// `true` if something is accepting TCP on a `host:port` listen address.
fn listen_addr_open(listen_addr: &str) -> bool {
    match listen_addr.rsplit_once(':') {
        Some((host, port)) => port
            .parse::<u16>()
            .map(|p| tcp_port_open(host, p))
            .unwrap_or(false),
        None => false,
    }
}

/// Probe codex's `/readyz` and restart it when it stops responding, so turns
/// recover without the user intervening. Mirrors the CLI watchdog; logs via
/// `tracing`.
async fn health_watchdog(local_addr: String, spawn_opts: SpawnOptions) {
    let url = format!("http://{local_addr}/readyz");
    let client = match reqwest::Client::builder().timeout(HEALTH_TIMEOUT).build() {
        Ok(client) => client,
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
        tracing::warn!("codex app-server failed {HEALTH_FAILURES} health checks; restarting it");
        match restart_codex(spawn_opts.clone()).await {
            Ok(()) => {
                tracing::info!("codex app-server restarted");
                restart_failures = 0;
            },
            Err(e) => {
                restart_failures += 1;
                tracing::warn!(error = %format!("{e:#}"), attempt = restart_failures, "codex restart failed");
            },
        }
        consecutive = 0;
        let backoff = HEALTH_RESTART_GRACE
            .saturating_mul(1u32 << restart_failures.min(5))
            .min(MAX_RESTART_BACKOFF);
        tokio::time::sleep(backoff).await;
    }
}

/// Stop the wedged codex and spawn a fresh one on the same port (escalating to
/// a hard kill if it ignores the graceful stop). Blocking work runs off the
/// async runtime.
async fn restart_codex(spawn_opts: SpawnOptions) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        // Stop only the codex on THIS host's port (not the global single-codex
        // stop), so restarting one host never tears down another.
        if let Some(addr) = spawn_opts.listen.as_socket_addr() {
            stop_codex_at(&addr);
        }
        let report = spawn(spawn_opts).context("respawning the codex app-server")?;
        anyhow::ensure!(
            !report.reused,
            "codex is still holding the listen port; restart did not take effect"
        );
        Ok(())
    })
    .await
    .context("codex restart task panicked")?
}

/// Block until nothing accepts on `addr`, or `timeout` elapses (`true` on
/// close).
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
