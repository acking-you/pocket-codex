//! Host a local codex app-server **and** a local Responses API proxy from the
//! app, each published through the account broker — the in-app equivalent of
//! running `pocket-codex serve` + `pocket-codex api serve` under one name.
//! Desktop only: it spawns the user's `codex` binary as a child process and
//! reuses its login (`~/.codex/auth.json`) for the API proxy.
//!
//! One `serve_start` publishes **three** relay tunnels under the same name:
//! `app:<name>` (codex app-server, remote control), `api:<name>` (the
//! in-process Responses API proxy), and `meta:<name>` (the in-process host meta
//! service — remote session inventory + per-thread config). The register
//! tunnels are independent: [`serve_deregister`] takes one off the relay (an
//! *unpublish*) without stopping codex or the in-process servers, and
//! [`serve_reregister`] re-publishes it instantly. [`serve_stop`] is the full
//! teardown (all tunnels + codex + proxy + meta service); [`serve_stop_all`]
//! (app quit) stops every host so a real quit leaves no orphan — closing to the
//! tray keeps hosting alive.
//!
//! The codex spawn/watchdog mirrors
//! `crates/pocket-codex-cli/src/commands/serve.rs`; the API proxy is the shared
//! [`pocket_codex_api_proxy`] crate run in-process (the CLI runs it as a
//! detached `__worker api-proxy` subprocess instead).

use std::{
    collections::HashMap,
    net::{SocketAddr, TcpStream},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};
use once_cell::sync::OnceCell;
use pocket_codex_broker_client::{run_register, Connector, RegisterConfig, TokenProvider};
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

/// One active local host: a codex app-server + an in-process Responses API
/// proxy, each published through its own broker register tunnel. Tracked
/// process-globally; several can run at once, keyed by service name. The two
/// register tunnels can be dropped/re-added independently of the processes.
struct LocalServe {
    device: String,
    name: String,
    // codex app-server (remote control).
    app_key: String,
    app_local: SocketAddr,
    pid: u32,
    /// `Some` while the app tunnel is published; `None` once deregistered.
    app_register: Option<JoinHandle<()>>,
    watchdog: JoinHandle<()>,
    // in-process Responses API proxy.
    api_key: String,
    api_local: SocketAddr,
    api_proxy: JoinHandle<()>,
    /// `Some` while the api tunnel is published; `None` once deregistered.
    api_register: Option<JoinHandle<()>>,
    // host-side meta service: makes this host's local sessions remote-viewable
    // and persists per-thread config, published as a third `meta:<name>` tunnel.
    meta_key: String,
    meta_local: SocketAddr,
    meta_svc: JoinHandle<()>,
    /// `Some` while the meta tunnel is published; `None` once deregistered.
    meta_register: Option<JoinHandle<()>>,
}

/// Result of [`serve_start`], surfaced to the UI.
#[derive(Debug, Clone)]
pub struct ServeReport {
    /// Device id both services were registered under.
    pub device: String,
    /// Service instance name (shared by the app + api tunnels).
    pub name: String,
    /// `pcx:<device>:app:<name>` key — what discovery + `app_connect` use.
    pub app_service_key: String,
    /// Loopback `host:port` codex is listening on.
    pub app_listen_addr: String,
    /// `pcx:<device>:api:<name>` key — what an `api connect` resolves.
    pub api_service_key: String,
    /// Loopback `host:port` the in-app Responses API proxy is listening on.
    pub api_listen_addr: String,
    /// `pcx:<device>:meta:<name>` key — the host meta service tunnel.
    pub meta_service_key: String,
    /// Loopback `host:port` the in-app meta service is listening on.
    pub meta_listen_addr: String,
    /// The codex process id.
    pub pid: u32,
    /// Whether an already-running host was reused rather than freshly spawned.
    pub reused: bool,
}

/// Status of one local host, surfaced to the UI.
#[derive(Debug, Clone, Default)]
pub struct ServeStatus {
    /// Service instance name.
    pub name: String,
    /// Device id.
    pub device: String,
    /// codex process id.
    pub pid: Option<u32>,
    /// codex is actually accepting on its listen port.
    pub alive: bool,
    /// Loopback `host:port` codex listens on.
    pub app_listen_addr: String,
    /// `pcx:<device>:app:<name>` key.
    pub app_service_key: String,
    /// The app tunnel is published (register task live).
    pub app_registered: bool,
    /// Loopback `host:port` the API proxy listens on.
    pub api_listen_addr: String,
    /// `pcx:<device>:api:<name>` key.
    pub api_service_key: String,
    /// The api tunnel is published (register task live).
    pub api_registered: bool,
    /// Loopback `host:port` the meta service listens on.
    pub meta_listen_addr: String,
    /// `pcx:<device>:meta:<name>` key.
    pub meta_service_key: String,
    /// The meta tunnel is published (register task live).
    pub meta_registered: bool,
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

/// The process-global per-thread config store, shared by every local meta
/// service: all hosts on this machine share one `CODEX_HOME` and therefore one
/// config map, so they must write through one serialized store. Opened once,
/// lazily, and the success cached; a store-open failure (an unresolvable /
/// unwritable `CODEX_HOME`) is returned as an `Err` for the caller to surface
/// as a hosting error rather than panicking the process (the bridge builds with
/// `panic = "abort"`, so an `expect` here would take the whole app down).
fn config_store() -> Result<Arc<pocket_codex_host_svc::store::ConfigStore>> {
    static STORE: OnceCell<Arc<pocket_codex_host_svc::store::ConfigStore>> = OnceCell::new();
    STORE
        .get_or_try_init(|| -> Result<Arc<pocket_codex_host_svc::store::ConfigStore>> {
            // Co-locate the config store with the sessions it annotates (under
            // CODEX_HOME) so every host on this machine shares one map.
            let path = pocket_codex_host_svc::store::default_db_path()?;
            let store = runtime::runtime()
                .block_on(pocket_codex_host_svc::store::ConfigStore::open(path))
                .context("opening the host meta config store")?;
            Ok(Arc::new(store))
        })
        .map(Arc::clone)
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

/// Spawn one broker register tunnel for `kind`, forwarding to `local`.
fn spawn_register(
    connector: Arc<dyn Connector>,
    tokens: Arc<dyn TokenProvider>,
    device: &str,
    kind: ServiceKind,
    name: &str,
    local: SocketAddr,
) -> JoinHandle<()> {
    runtime::runtime().spawn(run_register(connector, tokens, RegisterConfig {
        device: device.to_string(),
        kind,
        name: name.to_string(),
        client_instance_id: client_instance_id(),
        local_addr: local,
        idle: account::ACCOUNT_DATA_IDLE,
    }))
}

/// `true` if a register handle is missing or finished (i.e. not publishing).
fn tunnel_down(handle: &Option<JoinHandle<()>>) -> bool {
    handle.as_ref().is_none_or(|h| h.is_finished())
}

/// Start hosting a local codex app-server **and** a Responses API proxy under
/// the signed-in account, publishing both `app:<name>` and `api:<name>`.
/// Re-hosting a name whose codex is still alive just re-registers any dropped
/// tunnels (no restart). `proxy` is the upstream proxy both codex and the API
/// proxy use to reach chatgpt.com (`None` = inherit the app's environment).
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
    let app_key = ServiceId::new(&device, ServiceKind::App, &name).key();
    let api_key = ServiceId::new(&device, ServiceKind::Api, &name).key();
    let meta_key = ServiceId::new(&device, ServiceKind::Meta, &name).key();

    // Resolve the upstream proxy once (validated when explicit).
    let proxy = proxy
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if let Some(p) = proxy.as_deref() {
        pocket_codex_api_proxy::validate_proxy(p)?;
    }

    // Re-host / collision handling, under the hosts lock:
    // - same name + codex alive  → re-register any dropped tunnels, return.
    // - same name + codex dead    → drop the stale entry, then spawn fresh.
    // - requested port taken      → reject.
    {
        let mut guard = hosts().lock().expect("serve hosts poisoned");
        if let Some(ls) = guard.get_mut(&name) {
            if listen_addr_open(&ls.app_local.to_string()) {
                let (connector, tokens) = account::broker_transport(&support)?;
                let dev = ls.device.clone();
                let nm = ls.name.clone();
                let app_local = ls.app_local;
                let api_local = ls.api_local;
                let meta_local = ls.meta_local;
                if tunnel_down(&ls.app_register) {
                    ls.app_register = Some(spawn_register(
                        connector.clone(),
                        tokens.clone(),
                        &dev,
                        ServiceKind::App,
                        &nm,
                        app_local,
                    ));
                }
                if tunnel_down(&ls.api_register) {
                    ls.api_register = Some(spawn_register(
                        connector.clone(),
                        tokens.clone(),
                        &dev,
                        ServiceKind::Api,
                        &nm,
                        api_local,
                    ));
                }
                if tunnel_down(&ls.meta_register) {
                    ls.meta_register = Some(spawn_register(
                        connector,
                        tokens,
                        &dev,
                        ServiceKind::Meta,
                        &nm,
                        meta_local,
                    ));
                }
                return Ok(ServeReport {
                    device: dev,
                    name: nm,
                    app_service_key: ls.app_key.clone(),
                    app_listen_addr: app_local.to_string(),
                    api_service_key: ls.api_key.clone(),
                    api_listen_addr: api_local.to_string(),
                    meta_service_key: ls.meta_key.clone(),
                    meta_listen_addr: meta_local.to_string(),
                    pid: ls.pid,
                    reused: true,
                });
            }
            // codex dead → retire the stale entry and fall through to spawn.
            if let Some(stale) = guard.remove(&name) {
                stop_host_tasks(stale);
            }
        }
        if port != 0 && guard.values().any(|ls| ls.app_local.port() == port) {
            bail!("port {port} is already used by another local host");
        }
    }

    // The API proxy reuses the host's codex login (`CODEX_ACCESS_TOKEN` or
    // `~/.codex/auth.json`). Fail fast here rather than registering an api tunnel
    // to a proxy that would immediately exit because the login is missing.
    runtime::runtime()
        .block_on(pocket_codex_api_proxy::check_auth())
        .context("hosting needs a codex login; run `codex login` first")?;

    // Open the (process-global) meta config store before spawning anything, so an
    // unwritable CODEX_HOME surfaces as a hosting error here instead of after a
    // codex child is already running (or via a panic in the supervisor).
    let config_store = config_store()?;

    // Spawn codex. Runs on the flutter_rust_bridge worker thread (not the UI
    // thread), so the port-resolve poll inside `spawn` is fine.
    let spawn_opts = SpawnOptions {
        binary: Some(binary),
        listen: ListenSpec::WebSocket {
            host: "127.0.0.1".to_string(),
            port,
        },
        extra_args: Vec::new(),
        log_file: None,
        proxy: proxy.clone(),
    };
    let report = spawn(spawn_opts.clone()).context("spawning codex app-server")?;
    let listen_addr = report
        .info
        .listen
        .strip_prefix("ws://")
        .map(str::to_string)
        .ok_or_else(|| anyhow!("codex listen `{}` is not a ws:// address", report.info.listen))?;
    let app_local: SocketAddr = listen_addr
        .parse()
        .with_context(|| format!("codex listen `{listen_addr}` is not a socket address"))?;

    // Reserve an ephemeral loopback port for the in-process API proxy and learn
    // it (so we can register that port). A supervisor task keeps the proxy alive
    // on that port — re-binding + restarting it with backoff if it ever exits —
    // so the registered `api:<name>` tunnel keeps forwarding to a live proxy
    // rather than a dead socket.
    let api_std = std::net::TcpListener::bind("127.0.0.1:0")
        .context("binding the in-app API proxy listener")?;
    api_std
        .set_nonblocking(true)
        .context("setting the API proxy listener non-blocking")?;
    let api_local: SocketAddr = api_std
        .local_addr()
        .context("reading the API proxy listener address")?;
    let api_proxy =
        runtime::runtime().spawn(api_proxy_supervisor(api_local, api_std, proxy.clone()));

    // Reserve an ephemeral loopback port for the in-process meta service the
    // same way, and keep it alive under a supervisor so the registered
    // `meta:<name>` tunnel always forwards to a live server. It resumes into the
    // codex we just spawned (`app_local`) and shares the host-global config
    // store.
    let meta_std = std::net::TcpListener::bind("127.0.0.1:0")
        .context("binding the in-app meta service listener")?;
    meta_std
        .set_nonblocking(true)
        .context("setting the meta service listener non-blocking")?;
    let meta_local: SocketAddr = meta_std
        .local_addr()
        .context("reading the meta service listener address")?;
    let meta_svc = runtime::runtime().spawn(meta_svc_supervisor(
        meta_local,
        meta_std,
        app_local,
        config_store,
    ));

    // Pin the watchdog's respawn to the resolved codex port.
    let mut watchdog_opts = spawn_opts;
    watchdog_opts.listen = ListenSpec::WebSocket {
        host: app_local.ip().to_string(),
        port: app_local.port(),
    };

    let (connector, tokens) = account::broker_transport(&support)?;
    let app_register = Some(spawn_register(
        connector.clone(),
        tokens.clone(),
        &device,
        ServiceKind::App,
        &name,
        app_local,
    ));
    let api_register = Some(spawn_register(
        connector.clone(),
        tokens.clone(),
        &device,
        ServiceKind::Api,
        &name,
        api_local,
    ));
    let meta_register =
        Some(spawn_register(connector, tokens, &device, ServiceKind::Meta, &name, meta_local));
    let watchdog = runtime::runtime().spawn(health_watchdog(app_local.to_string(), watchdog_opts));

    hosts()
        .lock()
        .expect("serve hosts poisoned")
        .insert(name.clone(), LocalServe {
            device: device.clone(),
            name: name.clone(),
            app_key: app_key.clone(),
            app_local,
            pid: report.info.pid,
            app_register,
            watchdog,
            api_key: api_key.clone(),
            api_local,
            api_proxy,
            api_register,
            meta_key: meta_key.clone(),
            meta_local,
            meta_svc,
            meta_register,
        });

    Ok(ServeReport {
        device,
        name,
        app_service_key: app_key,
        app_listen_addr: app_local.to_string(),
        api_service_key: api_key,
        api_listen_addr: api_local.to_string(),
        meta_service_key: meta_key,
        meta_listen_addr: meta_local.to_string(),
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
            name: ls.name.clone(),
            device: ls.device.clone(),
            pid: Some(ls.pid),
            alive: listen_addr_open(&ls.app_local.to_string()),
            app_listen_addr: ls.app_local.to_string(),
            app_service_key: ls.app_key.clone(),
            app_registered: !tunnel_down(&ls.app_register),
            api_listen_addr: ls.api_local.to_string(),
            api_service_key: ls.api_key.clone(),
            api_registered: !tunnel_down(&ls.api_register),
            meta_listen_addr: ls.meta_local.to_string(),
            meta_service_key: ls.meta_key.clone(),
            meta_registered: !tunnel_down(&ls.meta_register),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Take one tunnel (`kind` = `"app"`/`"api"`/`"meta"`) off the relay without
/// stopping the host: abort its register task (so it won't reconnect) and force
/// the backend to drop the relay key now (aborting alone waits out the lease).
/// Reversible via [`serve_reregister`].
pub fn serve_deregister(name: &str, kind: &str) -> Result<()> {
    let kind: ServiceKind = kind
        .parse()
        .map_err(|_| anyhow!("invalid service kind `{kind}`"))?;
    let support = runtime::support_dir()?;
    let (device, svc_name) = {
        let mut guard = hosts().lock().expect("serve hosts poisoned");
        let ls = guard
            .get_mut(name)
            .ok_or_else(|| anyhow!("`{name}` is not hosting locally"))?;
        match kind {
            ServiceKind::App => {
                if let Some(h) = ls.app_register.take() {
                    h.abort();
                }
            },
            ServiceKind::Api => {
                if let Some(h) = ls.api_register.take() {
                    h.abort();
                }
            },
            ServiceKind::Meta => {
                if let Some(h) = ls.meta_register.take() {
                    h.abort();
                }
            },
            // `kind` came from FromStr, which never yields Unknown; arm exists
            // only to keep the match exhaustive.
            ServiceKind::Unknown => {},
        }
        (ls.device.clone(), ls.name.clone())
    };
    // Force the relay to drop the key now (the aborted register tunnel alone
    // would otherwise linger until its lease expires). Best-effort: the local
    // forward is already stopped, so a failure only delays the relay drop.
    if let Err(e) = runtime::runtime().block_on(account::deregister_service(
        &support,
        &device,
        kind.as_key_segment(),
        &svc_name,
    )) {
        tracing::warn!(
            error = %format!("{e:#}"),
            service = %svc_name,
            kind = %kind.as_key_segment(),
            "force-dropping the relay key on deregister failed; it lingers until lease expiry"
        );
    }
    Ok(())
}

/// Re-publish a previously [`serve_deregister`]'d tunnel: spawn its register
/// task again, forwarding to the still-running process. No-op if already live.
pub fn serve_reregister(name: &str, kind: &str) -> Result<()> {
    let kind: ServiceKind = kind
        .parse()
        .map_err(|_| anyhow!("invalid service kind `{kind}`"))?;
    let support = runtime::support_dir()?;
    let (connector, tokens) = account::broker_transport(&support)?;
    let mut guard = hosts().lock().expect("serve hosts poisoned");
    let ls = guard
        .get_mut(name)
        .ok_or_else(|| anyhow!("`{name}` is not hosting locally"))?;
    let device = ls.device.clone();
    let svc_name = ls.name.clone();
    match kind {
        ServiceKind::App => {
            if tunnel_down(&ls.app_register) {
                let local = ls.app_local;
                ls.app_register = Some(spawn_register(
                    connector,
                    tokens,
                    &device,
                    ServiceKind::App,
                    &svc_name,
                    local,
                ));
            }
        },
        ServiceKind::Api => {
            if tunnel_down(&ls.api_register) {
                let local = ls.api_local;
                ls.api_register = Some(spawn_register(
                    connector,
                    tokens,
                    &device,
                    ServiceKind::Api,
                    &svc_name,
                    local,
                ));
            }
        },
        ServiceKind::Meta => {
            if tunnel_down(&ls.meta_register) {
                let local = ls.meta_local;
                ls.meta_register = Some(spawn_register(
                    connector,
                    tokens,
                    &device,
                    ServiceKind::Meta,
                    &svc_name,
                    local,
                ));
            }
        },
        // `kind` came from FromStr, which never yields Unknown.
        ServiceKind::Unknown => {},
    }
    Ok(())
}

/// Fully stop one host by name: abort all register tunnels + watchdog + the
/// API proxy + meta service tasks, stop its codex, and force the relay to drop
/// all keys now. Best-effort + idempotent (no-op when that name isn't hosting).
pub fn serve_stop(name: &str) -> Result<()> {
    let removed = hosts().lock().expect("serve hosts poisoned").remove(name);
    if let Some(ls) = removed {
        let device = ls.device.clone();
        let svc_name = ls.name.clone();
        stop_host_tasks(ls);
        if let Ok(support) = runtime::support_dir() {
            runtime::runtime().block_on(async {
                let _ = account::deregister_service(&support, &device, "app", &svc_name).await;
                let _ = account::deregister_service(&support, &device, "api", &svc_name).await;
                // Log the meta drop specifically: a backend not yet rebuilt with
                // the `meta` kind rejects it, which is worth surfacing (the key
                // then lingers until its lease expires).
                if let Err(e) =
                    account::deregister_service(&support, &device, "meta", &svc_name).await
                {
                    tracing::warn!(
                        error = %format!("{e:#}"),
                        service = %svc_name,
                        "force-dropping the meta relay key on stop failed (older backend?); it \
                         lingers until lease expiry"
                    );
                }
            });
        }
    }
    Ok(())
}

/// Stop every host (called on app quit so a real quit leaves no orphan codex).
/// Process exit closes the broker tunnels, so no explicit relay drop is needed.
pub fn serve_stop_all() {
    let all: Vec<LocalServe> = hosts()
        .lock()
        .expect("serve hosts poisoned")
        .drain()
        .map(|(_, ls)| ls)
        .collect();
    for ls in all {
        stop_host_tasks(ls);
    }
}

/// Abort a host's background tasks (all register tunnels, watchdog, API proxy,
/// meta service) and stop its codex. Does not touch the relay (callers that
/// need an immediate relay drop force-deregister separately).
fn stop_host_tasks(ls: LocalServe) {
    if let Some(h) = ls.app_register {
        h.abort();
    }
    if let Some(h) = ls.api_register {
        h.abort();
    }
    if let Some(h) = ls.meta_register {
        h.abort();
    }
    ls.watchdog.abort();
    ls.api_proxy.abort();
    ls.meta_svc.abort();
    stop_codex_at(&ls.app_local.to_string());
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

/// Keep an in-process service alive on `addr`. Runs `serve` and, if it ever
/// exits (a transient auth/IO error, or an `axum::serve` accept error),
/// re-binds the SAME loopback port and restarts it with backoff — so the
/// registered tunnel keeps forwarding to a live server instead of a dead
/// socket. `first` is the listener already bound in [`serve_start`] (so the
/// port is reserved); later restarts re-bind it. `label` names the service in
/// logs. Aborting this task (on `serve_stop`) drops the listener.
async fn supervise<F, Fut>(label: &str, addr: SocketAddr, first: std::net::TcpListener, serve: F)
where
    F: Fn(tokio::net::TcpListener) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let mut reserved = Some(first);
    let mut failures: u32 = 0;
    loop {
        let std_listener = match reserved.take() {
            Some(listener) => listener,
            None => match std::net::TcpListener::bind(addr) {
                Ok(listener) => {
                    let _ = listener.set_nonblocking(true);
                    listener
                },
                Err(e) => {
                    tracing::warn!(error = %e, %addr, "re-binding {label} failed");
                    failures = failures.saturating_add(1);
                    tokio::time::sleep(proxy_restart_backoff(failures)).await;
                    continue;
                },
            },
        };
        let listener = match tokio::net::TcpListener::from_std(std_listener) {
            Ok(listener) => listener,
            Err(e) => {
                tracing::warn!(error = %e, "adopting {label} listener failed");
                failures = failures.saturating_add(1);
                tokio::time::sleep(proxy_restart_backoff(failures)).await;
                continue;
            },
        };
        let started = Instant::now();
        match serve(listener).await {
            Ok(()) => tracing::warn!("{label} returned; restarting"),
            Err(e) => tracing::warn!(error = %format!("{e:#}"), "{label} exited; restarting"),
        }
        // A server that ran a while then died hit a transient fault — reset the
        // backoff; one that fails fast (e.g. a persistently missing login) backs
        // off progressively.
        failures = if started.elapsed() > Duration::from_secs(60) {
            1
        } else {
            failures.saturating_add(1)
        };
        tokio::time::sleep(proxy_restart_backoff(failures)).await;
    }
}

/// Supervise the in-process Responses API proxy (forwards to chatgpt.com).
async fn api_proxy_supervisor(
    addr: SocketAddr,
    first: std::net::TcpListener,
    proxy: Option<String>,
) {
    supervise("the in-app API proxy", addr, first, move |listener| {
        let proxy = proxy.clone();
        async move { pocket_codex_api_proxy::serve(listener, proxy).await }
    })
    .await
}

/// Supervise the in-process host meta service. It resumes into the colocated
/// codex at `app_ws_addr` and shares the host-global `store`.
async fn meta_svc_supervisor(
    addr: SocketAddr,
    first: std::net::TcpListener,
    app_ws_addr: SocketAddr,
    store: Arc<pocket_codex_host_svc::store::ConfigStore>,
) {
    supervise("the in-app meta service", addr, first, move |listener| {
        let store = store.clone();
        async move { pocket_codex_host_svc::serve(listener, app_ws_addr, store).await }
    })
    .await
}

/// Backoff before restarting a supervised service: 2s, doubling, capped at
/// [`MAX_RESTART_BACKOFF`].
fn proxy_restart_backoff(failures: u32) -> Duration {
    Duration::from_secs(2)
        .saturating_mul(1u32 << failures.saturating_sub(1).min(7))
        .min(MAX_RESTART_BACKOFF)
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
