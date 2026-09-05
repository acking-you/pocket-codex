//! Host a local codex app-server **and** a local Responses API proxy from the
//! app, each published directly on the relay — the in-app equivalent of running
//! `pocket-codex serve` + `pocket-codex api serve` under one name. Desktop
//! only: it spawns the user's `codex` binary as a child process and reuses its
//! login (`~/.codex/auth.json`) for the API proxy.
//!
//! One `serve_start` publishes **three** services under the same name:
//! `app:<name>` (codex app-server, remote control), `api:<name>` (the
//! in-process Responses API proxy), and `meta:<name>` (the in-process host meta
//! service — remote session inventory + per-thread config). Each is a
//! [`Published`] handle whose lifetime IS the publication: [`serve_deregister`]
//! drops one (an *unpublish*) without stopping codex or the in-process servers,
//! and [`serve_reregister`] publishes it again. [`serve_stop`] is the full
//! teardown (all three + codex + proxy + meta service); [`serve_stop_all`]
//! (app quit) stops every host so a real quit leaves no orphan — closing to the
//! tray keeps hosting alive.
//!
//! Publishing is the same in account and self-host mode; the only difference is
//! the credential and key namespace [`Transport`] carries. Reconnecting a
//! dropped registration is the pb-mapper SDK's job, so the only failure this
//! module acts on is a name CONFLICT — see [`register_service`].
//!
//! The codex spawn/watchdog mirrors
//! `crates/pocket-codex-cli/src/commands/serve.rs`; the API proxy is the shared
//! [`pocket_codex_api_proxy`] crate run in-process (the CLI runs it as a
//! detached `__worker api-proxy` subprocess instead).

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};
use once_cell::sync::OnceCell;
use pocket_codex_codex::{
    locate_binary, spawn_ready, ListenSpec, SpawnOptions, SpawnReadyError, StartupFailure,
    READY_TIMEOUT,
};
use pocket_codex_core::{
    process::{
        find_codex_app_server, force_kill, pid_running, send_sigterm, tcp_port_open,
        wait_for_port_closed,
    },
    service::{default_device_id, ServiceId, ServiceKind},
};
use pocket_codex_pb::{publish_pending, PublishError, Published, RegisterOptions};
use tokio::task::JoinHandle;

use crate::engine::{
    config::{load_config, save_config},
    logging, runtime,
    transport::{self, Transport},
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
/// proxy, each published on the relay under its own key. Tracked
/// process-globally; several can run at once, keyed by service name. The three
/// registrations can be dropped/re-added independently of the processes.
struct LocalServe {
    device: String,
    name: String,
    // codex app-server (remote control).
    app_key: String,
    app_local: SocketAddr,
    pid: u32,
    /// `Some` while the app service is published; `None` once deregistered.
    /// Holding it IS the publication — dropping it unpublishes.
    app_register: Option<Published>,
    watchdog: JoinHandle<()>,
    /// The in-process codex app-server task (embedded mode). `None` for an
    /// external (spawned-binary) host. Aborted on stop. Swapped for a fresh
    /// task by [`embedded_health_watchdog`] when the server wedges, so this
    /// always holds the LIVE supervisor.
    embedded: Option<JoinHandle<()>>,
    /// Tails an external codex's log file into the in-app log viewer. `Some`
    /// for an external host, `None` for embedded (whose logs already stream
    /// through the in-process tracing layer). Aborted on stop.
    log_tail: Option<JoinHandle<()>>,
    // in-process Responses API proxy.
    api_key: String,
    api_local: SocketAddr,
    api_proxy: JoinHandle<()>,
    /// `Some` while the api service is published; `None` once deregistered.
    api_register: Option<Published>,
    // host-side meta service: makes this host's local sessions remote-viewable
    // and persists per-thread config, published as a third `meta:<name>` tunnel.
    meta_key: String,
    meta_local: SocketAddr,
    meta_svc: JoinHandle<()>,
    /// `Some` while the meta service is published; `None` once deregistered.
    meta_register: Option<Published>,
    /// The resolved external codex binary path, or `None` for an embedded host
    /// (which runs codex in-process). Surfaced in the host details for
    /// debugging.
    codex_binary: Option<String>,
    /// The upstream proxy codex + the API proxy were started with, or `None`
    /// when they inherit the app's own environment. Surfaced for debugging.
    proxy: Option<String>,
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
    /// This host runs codex IN-PROCESS (the compiled-in `embedded-codex`)
    /// rather than spawning an external binary.
    pub embedded: bool,
    /// The resolved external codex binary path, or `None` for an embedded host.
    pub codex_binary: Option<String>,
    /// Upstream proxy codex + the API proxy were started with, or `None` when
    /// they inherit the app's environment.
    pub proxy: Option<String>,
}

fn hosts() -> &'static Mutex<HashMap<String, LocalServe>> {
    static HOSTS: OnceCell<Mutex<HashMap<String, LocalServe>>> = OnceCell::new();
    HOSTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Lock the process-global host map, RECOVERING a poisoned lock instead of
/// panicking. The guarded value is a plain registry of `JoinHandle`s +
/// metadata with no cross-field invariant a panic could half-update, so a
/// previous panic-while-holding leaves it perfectly usable. Without this,
/// `.lock().expect(...)` would turn one unlucky panic into a cascade: every
/// later serve/status/stop call would panic on the poisoned lock — under the
/// release `panic = "unwind"` that just errors the call, but recovering keeps
/// hosting fully operational. Prefer this over `.lock().expect(...)`
/// everywhere.
fn hosts_locked() -> std::sync::MutexGuard<'static, HashMap<String, LocalServe>> {
    hosts().lock().unwrap_or_else(|poison| poison.into_inner())
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

/// The process-wide host-config store (project roots + default project), opened
/// once and shared by every host — like [`config_store`], co-located under
/// CODEX_HOME so all hosts on this machine share one host config.
fn host_store() -> Result<Arc<pocket_codex_host_svc::store::HostStore>> {
    static STORE: OnceCell<Arc<pocket_codex_host_svc::store::HostStore>> = OnceCell::new();
    STORE
        .get_or_try_init(|| -> Result<Arc<pocket_codex_host_svc::store::HostStore>> {
            let path = pocket_codex_host_svc::store::default_host_config_path()?;
            let store = runtime::runtime()
                .block_on(pocket_codex_host_svc::store::HostStore::open(path))
                .context("opening the host config store")?;
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

/// Publish `local` on the relay for `kind`, holding the registration.
///
/// Not fallible for the caller's purposes unless the NAME is taken: a relay
/// that is merely unreachable still yields a handle whose worker keeps
/// retrying, so the service comes up on its own. That asymmetry is deliberate —
/// a name conflict cannot resolve itself and must stop hosting, while
/// everything else can.
///
/// Deliberately NOT wrapped in `tokio::time::timeout`: cancelling `publish`
/// drops the handle, and with it the SDK worker doing the retrying, so a slow
/// relay would leave the service permanently unpublished rather than merely
/// late. The bound comes from the SDK's own readiness budget instead, and
/// [`pocket_codex_pb::publish_pending`] hands the handle back either way.
fn register_service(
    transport: &Transport,
    device: &str,
    kind: ServiceKind,
    name: &str,
    local: SocketAddr,
) -> Result<Option<Published>> {
    let key = transport.key(&ServiceId::new(device, kind, name));
    let outcome =
        runtime::runtime().block_on(publish_pending(&transport.session, RegisterOptions {
            key: key.clone(),
            local_addr: local.to_string(),
            codec: false,
        }));
    match outcome {
        Ok((published, ready)) => {
            if !ready {
                tracing::info!(
                    key = %key,
                    "hosting started before the relay confirmed this service; it keeps retrying"
                );
            }
            Ok(Some(published))
        },
        // The one condition hosting must not proceed through: another live
        // instance owns the name, and publishing anyway makes the two evict each
        // other on the relay in an endless leapfrog.
        Err(PublishError::Conflict {
            detail, ..
        }) => bail!("`{name}` is already in use by another device: {detail}"),
        Err(PublishError::Other(err)) => {
            tracing::warn!(
                key = %key,
                error = %format!("{err:#}"),
                "publishing failed; hosting continues and will retry"
            );
            Ok(None)
        },
    }
}

/// `true` if there is no registration to hold — none was created, or the relay
/// permanently refused the one we have.
///
/// Deliberately NOT the same question as "is it serving right now": a handle in
/// `Retrying` is down but must not be replaced, because its worker is already
/// reconnecting and a second registration for one key is the eviction leapfrog.
/// [`is_published`] is what the UI shows; this is what decides whether to
/// publish again.
fn needs_publishing(published: &Option<Published>) -> bool {
    published.as_ref().is_none_or(|p| p.failure().is_some())
}

/// `true` if a service is reachable through the relay right now.
///
/// What the UI reports, so `Starting` and `Retrying` read as NOT published: the
/// key may still be indexed while nothing is forwarding, and showing that as
/// online is what would withhold the offline / re-register controls from a user
/// whose service is unreachable.
fn is_published(published: &Option<Published>) -> bool {
    published.as_ref().is_some_and(|p| p.is_live())
}

/// Re-publish whichever of `name`'s three services are not currently published.
///
/// Errors only on a name CONFLICT, which is the one condition a caller must act
/// on: the local host stays up either way, but a name another device owns can
/// never publish, so a re-host under it has to say so rather than report
/// success.
///
/// Takes the hosts lock only to read what needs publishing and to store the
/// results — never across the relay round trip, which would stall status polls.
fn republish_dropped(name: &str) -> Result<()> {
    let Some((device, pending)) = ({
        let guard = hosts_locked();
        guard.get(name).map(|ls| {
            let pending: Vec<(ServiceKind, SocketAddr)> = [
                (ServiceKind::App, ls.app_local, &ls.app_register),
                (ServiceKind::Api, ls.api_local, &ls.api_register),
                (ServiceKind::Meta, ls.meta_local, &ls.meta_register),
            ]
            .into_iter()
            .filter(|(_, _, slot)| needs_publishing(slot))
            .map(|(kind, local, _)| (kind, local))
            .collect();
            (ls.device.clone(), pending)
        })
    }) else {
        return Ok(());
    };
    if pending.is_empty() {
        return Ok(());
    }
    let transport = transport::resolve_blocking()?;

    let mut conflict = None;
    for (kind, local) in pending {
        // Drop the refused registration BEFORE publishing again, so the relay
        // releases the old key and the fresh publish is not competing with its
        // own predecessor for the name.
        take_registration(name, kind);
        let published = match register_service(&transport, &device, kind, name, local) {
            Ok(published) => published,
            Err(err) => {
                // Keep going: a conflict on one kind (typically `app`) should not
                // leave the other two unpublished, and the caller wants the app
                // conflict reported once everything else is up.
                conflict = conflict.or(Some(err));
                continue;
            },
        };
        store_registration(name, kind, published);
    }
    match conflict {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Take a host's registration for `kind` out of the map and drop it, releasing
/// the relay key. No-op for an unknown host.
fn take_registration(name: &str, kind: ServiceKind) {
    let mut guard = hosts_locked();
    if let Some(ls) = guard.get_mut(name) {
        *registration_slot(ls, kind) = None;
    }
}

/// Store a freshly published registration, or clear the slot when publishing
/// did not produce one. Drops it immediately if the host vanished while we
/// published.
fn store_registration(name: &str, kind: ServiceKind, published: Option<Published>) {
    let mut guard = hosts_locked();
    match guard.get_mut(name) {
        Some(ls) => *registration_slot(ls, kind) = published,
        // The host was stopped while we were publishing; unpublish rather than
        // leaving a key on the relay with nothing behind it.
        None => drop(published),
    }
}

fn registration_slot(ls: &mut LocalServe, kind: ServiceKind) -> &mut Option<Published> {
    match kind {
        ServiceKind::Api => &mut ls.api_register,
        ServiceKind::Meta => &mut ls.meta_register,
        // `App` and any future kind share the app slot: a host publishes exactly
        // these three, and routing an unknown kind here is better than panicking
        // in a code path that holds the hosts lock.
        _ => &mut ls.app_register,
    }
}

/// Start hosting a local codex app-server **and** a Responses API proxy under
/// the signed-in account, publishing both `app:<name>` and `api:<name>`.
/// Re-hosting a name whose codex is still alive just re-registers any dropped
/// tunnels (no restart). `proxy` is the upstream proxy both codex and the API
/// proxy use to reach chatgpt.com (`None` = inherit the app's environment).
/// Run codex's app-server in-process on `listen_url`, restarting it if it ever
/// exits, until the task is aborted on stop. This is the embedded-mode
/// equivalent of the spawned `codex` binary. It only reacts to the task
/// EXITING (clean return, error, panic); a wedged-but-alive server never
/// trips it — that is [`embedded_health_watchdog`]'s job.
#[cfg(any(target_os = "windows", target_os = "macos"))]
async fn run_embedded_supervised(listen_url: String) {
    // Bounded restart backoff so a persistently-broken embedded codex (bad
    // state, a bind that never frees) can't hot-loop at ~1 Hz; a run that lasts
    // a while resets it so a one-off crash still restarts promptly.
    let mut failures: u32 = 0;
    loop {
        let started = std::time::Instant::now();
        // Run the in-process app-server as its OWN task and await its handle, so
        // a PANIC in codex's top-level accept future is delivered here as a
        // JoinError instead of unwinding through (and, in a release build,
        // aborting) the whole desktop. We log and restart, exactly as for a
        // clean exit or an error. (Panics inside codex's per-connection subtasks
        // are already contained by the tokio runtime under the unwind panic
        // strategy — see the workspace `[profile.release] panic = "unwind"`
        // note; this guards the supervisor's own future too so the embedded
        // server auto-recovers rather than the service silently dying.)
        let url = listen_url.clone();
        let handle =
            runtime::runtime().spawn(async move { pocket_codex_codex::embedded::run(&url).await });
        // Aborting THIS supervisor must stop the server too: dropping a
        // JoinHandle DETACHES its task, so without this guard an abort (a
        // failed start, `serve_stop`) would leave the inner app-server
        // serving the port forever — unsupervised and unpublished.
        let _abort_inner = AbortOnDrop(handle.abort_handle());
        match handle.await {
            Ok(Ok(())) => {
                tracing::warn!(%listen_url, "embedded codex app-server exited; restarting")
            },
            Ok(Err(e)) => {
                eprintln!("[embedded codex] failed on {listen_url}: {e:#}; restarting");
                tracing::error!(%listen_url, "embedded codex app-server failed: {e:#}; restarting")
            },
            Err(join_err) if join_err.is_panic() => {
                eprintln!("[embedded codex] PANICKED on {listen_url}: {join_err}; restarting");
                tracing::error!(%listen_url, "embedded codex app-server PANICKED: {join_err}; restarting")
            },
            Err(join_err) => {
                // Cancelled (e.g. runtime shutdown) — don't hot-loop.
                tracing::warn!(%listen_url, "embedded codex app-server task ended: {join_err}");
            },
        }
        // A healthy run (up long enough to have served) clears the backoff so a
        // transient crash restarts fast; repeated fast failures back off.
        failures = if started.elapsed() >= Duration::from_secs(30) {
            0
        } else {
            failures.saturating_add(1)
        };
        tokio::time::sleep(proxy_restart_backoff(failures)).await;
    }
}

/// Aborts the wrapped task when dropped. Guards a spawned inner task across
/// an `.await` in its supervisor: if the supervisor is itself aborted at that
/// point, the drop aborts the inner task too, where dropping the bare
/// `JoinHandle` would silently DETACH it. Dropping after a completed task is
/// a no-op abort.
#[cfg(any(target_os = "windows", target_os = "macos"))]
struct AbortOnDrop(tokio::task::AbortHandle);

#[cfg(any(target_os = "windows", target_os = "macos"))]
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Block until `host:port` accepts a TCP connection (the embedded WS listener
/// is up) or `timeout` elapses.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn wait_for_listener(host: &str, port: u16, timeout: Duration) -> Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if tcp_port_open(host, port) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    bail!("embedded codex app-server never listened on {host}:{port} within {timeout:?}")
}

/// Reserve a free loopback port (when the caller passed 0 for an automatic
/// one).
fn free_loopback_port() -> Result<u16> {
    let l = std::net::TcpListener::bind("127.0.0.1:0").context("reserving a loopback port")?;
    Ok(l.local_addr()?.port())
}

/// Prove the user-chosen port is actually bindable by binding-and-dropping it,
/// BEFORE the embedded supervisor exists. The supervisor swallows its bind
/// failure into a silent retry loop, and neither TCP wait can tell our
/// listener from a foreign one — this probe fails a taken port in
/// milliseconds, with the true cause, and also catches holders that never
/// accept at all (classically a leaked WSL mirrored-networking lease, which
/// would otherwise burn the whole listener wait and die with a generic
/// "never listened").
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn probe_port_free(port: u16) -> Result<()> {
    match std::net::TcpListener::bind(("127.0.0.1", port)) {
        Ok(_) => Ok(()),
        Err(cause) => Err(embedded_bind_conflict_error(port, &cause)),
    }
}

/// Size at which a running host's log file is truncated by its tailer.
///
/// Deliberately a SIZE bound and not a time window. "Keep the last six hours"
/// sounds like the right rule and isn't: the burst that prompted this peaked at
/// 72 MB/min, which is 25 GB over six hours — a time rule bounds nothing on a
/// disk. 8 MB is far more than the in-app viewer can show (its ring holds 2000
/// lines) and comfortably more than a normal session writes, while staying
/// negligible on any disk.
const MAX_HOST_LOG_BYTES: u64 = 8 * 1024 * 1024;

/// Whether a host log of `len` bytes has outgrown [`MAX_HOST_LOG_BYTES`].
/// Trivial, but split out so the threshold is asserted somewhere rather than
/// living only inside an async poll loop no unit test can reach.
fn over_log_cap(len: u64) -> bool {
    len > MAX_HOST_LOG_BYTES
}

/// Truncate a live host's log to zero, returning whether it worked.
///
/// `set_len(0)` and not `remove_file`: the child holds this file open, so
/// unlinking would leave it appending to a deleted inode and the blocks would
/// stay allocated until the host stopped — the opposite of what a disk-pressure
/// guard should do. Truncating frees them now, and the child's next append
/// lands at offset 0, which the caller's `len < pos` resync already handles.
async fn truncate_host_log(log_file: &std::path::Path) -> bool {
    match tokio::fs::OpenOptions::new()
        .write(true)
        .open(log_file)
        .await
    {
        Ok(f) => f.set_len(0).await.is_ok(),
        Err(cause) => {
            tracing::debug!(?log_file, %cause, "could not truncate oversized host log");
            false
        },
    }
}

/// This host's own codex log file (each external host gets its own, so one
/// tailer never picks up another host's — or an earlier run's — interleaved
/// output from the shared default file).
fn per_host_log_file(name: &str) -> Result<std::path::PathBuf> {
    let safe: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    Ok(runtime::support_dir()?
        .join("codex-logs")
        .join(format!("codex-{safe}.log")))
}

/// Tail an external codex process's log file (its own `tracing` output) into
/// the in-app log stream, so the viewer shows a spawned (外接) app-server's
/// logs the same way the in-process (自带) one already does. Starts at
/// `start_pos` (this run's first byte), polls for appended content, and emits
/// each complete new line under a `codex(<name>)` target. Aborted on stop.
fn tail_codex_log(log_file: std::path::PathBuf, start_pos: u64, name: String) -> JoinHandle<()> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    // Flush a runaway partial line so a newline-less blob can't grow `carry`
    // without bound.
    const CARRY_CAP: usize = 64 * 1024;
    let target = format!("codex({name})");
    runtime::runtime().spawn(async move {
        let mut pos = start_pos;
        // Bytes, not a String: a multi-byte UTF-8 char split across a read
        // boundary must be held intact until its line completes, then decoded —
        // decoding the partial chunk would corrupt it into replacement chars.
        let mut carry: Vec<u8> = Vec::new();
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let Ok(meta) = tokio::fs::metadata(&log_file).await else {
                continue;
            };
            let mut len = meta.len();
            // Enforce the size cap HERE, not only at spawn. The spawn-time check
            // alone is useless against the case that actually hurt: one host
            // running for hours. The burst that prompted this wrote 4.9 GB in a
            // single ~12h session, peaking at 72 MB/min — a check that only runs
            // between sessions never fired once.
            //
            // Truncate in place rather than unlink: the child holds this file
            // open, so removing it would leave the writer appending to a deleted
            // inode (the space never comes back until the host stops, which is
            // exactly the wrong behaviour for a disk-pressure guard). `set_len(0)`
            // frees the blocks immediately and the child's next append lands at 0.
            if over_log_cap(len) && truncate_host_log(&log_file).await {
                tracing::info!(?log_file, bytes = len, "host log passed its size cap; truncated");
                len = 0;
            }
            if len < pos {
                // Truncated / rotated — resync to the new end (don't replay the
                // whole file).
                pos = len;
                carry.clear();
            }
            if len == pos {
                continue;
            }
            let Ok(mut f) = tokio::fs::File::open(&log_file).await else {
                continue;
            };
            if f.seek(std::io::SeekFrom::Start(pos)).await.is_err() {
                continue;
            }
            let mut buf = Vec::with_capacity((len - pos) as usize);
            if f.take(len - pos).read_to_end(&mut buf).await.is_err() {
                continue;
            }
            pos = len;
            carry.extend_from_slice(&buf);
            // Emit complete lines; keep any trailing partial (possibly mid-char)
            // bytes for next round. `from_utf8_lossy` runs on a whole line, so no
            // char is ever split.
            while let Some(nl) = carry.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = carry.drain(..=nl).collect();
                let text = String::from_utf8_lossy(&line);
                let text = text.trim_end();
                if !text.is_empty() {
                    logging::push_external(&target, text);
                }
            }
            if carry.len() > CARRY_CAP {
                let text = String::from_utf8_lossy(&carry);
                let text = text.trim_end();
                if !text.is_empty() {
                    logging::push_external(&target, text);
                }
                carry.clear();
            }
        }
    })
}

/// Make the host's proxy visible to in-process codex via the process
/// environment (a spawned child gets it via its command env instead). Loopback
/// is kept direct so codex's own WS and our local services aren't proxied.
#[cfg(any(target_os = "windows", target_os = "macos"))]
#[allow(
    deprecated_safe_2024,
    reason = "set_var is safe in edition 2021; called once at host start before concurrent env use"
)]
fn set_proxy_env(proxy: &str) {
    for key in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]
    {
        std::env::set_var(key, proxy);
    }
    if std::env::var_os("NO_PROXY").is_none() && std::env::var_os("no_proxy").is_none() {
        std::env::set_var("NO_PROXY", "localhost,127.0.0.1,::1");
        std::env::set_var("no_proxy", "localhost,127.0.0.1,::1");
    }
}

/// Which codex binary to spawn: explicit override → persisted config → `$PATH`.
/// `None` for an embedded host, which runs codex in-process and has no binary
/// to find.
///
/// Persists an override that differs from what is saved, so the next start
/// finds the same codex without the caller passing it again.
fn resolve_codex_binary(
    embedded: bool,
    binary_override: Option<String>,
    support: &std::path::Path,
    config: &mut pocket_codex_core::config::Config,
) -> Result<Option<std::path::PathBuf>> {
    if embedded {
        return Ok(None);
    }
    let override_trimmed = binary_override
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let candidate = override_trimmed
        .map(str::to_string)
        .or_else(|| config.codex.binary.clone());
    let resolved = locate_binary(candidate.as_deref()).ok_or_else(|| {
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
            save_config(support, config)?;
        }
    }
    Ok(Some(resolved))
}

/// Decide what an existing host under `name` means for this start, under the
/// hosts lock:
///
/// - same name, codex alive → re-register any dropped tunnels; `Some(report)`,
///   and the caller returns without spawning.
/// - same name, codex dead  → retire the stale entry; `None`, spawn fresh.
/// - requested port taken   → error.
///
/// Runs before anything is spawned, so a re-host never leaves two codex
/// processes fighting over one port.
fn reuse_or_retire_host(name: &str, port: u16) -> Result<Option<ServeReport>> {
    let mut guard = hosts_locked();
    if let Some(ls) = guard.get_mut(name) {
        if listen_addr_open(&ls.app_local.to_string()) {
            let report = ServeReport {
                device: ls.device.clone(),
                name: ls.name.clone(),
                app_service_key: ls.app_key.clone(),
                app_listen_addr: ls.app_local.to_string(),
                api_service_key: ls.api_key.clone(),
                api_listen_addr: ls.api_local.to_string(),
                meta_service_key: ls.meta_key.clone(),
                meta_listen_addr: ls.meta_local.to_string(),
                pid: ls.pid,
                reused: true,
            };
            // Re-publish OUTSIDE the hosts lock: publishing waits on the relay,
            // and holding the lock across it would stall every status poll for
            // as long as the relay takes to answer.
            drop(guard);
            republish_dropped(name)?;
            return Ok(Some(report));
        }
        // codex dead → retire the stale entry and fall through to spawn.
        if let Some(stale) = guard.remove(name) {
            stop_host_tasks(stale);
        }
    }
    if port != 0 && guard.values().any(|ls| ls.app_local.port() == port) {
        bail!("port {port} is already used by another local host");
    }
    Ok(None)
}

/// Log, but do not refuse, a host with no codex credential yet.
///
/// The API proxy reuses the host's codex login (`CODEX_ACCESS_TOKEN` or
/// `~/.codex/auth.json`). This used to be a hard gate, but first-run onboarding
/// must host BEFORE a login exists: either a custom provider (which authorizes
/// turns on its own) is configured, or the user drives codex's ChatGPT login
/// over the app-server we are about to start. So a missing login is non-fatal —
/// the app-server and meta service host fine and the API proxy supervisor keeps
/// retrying until a credential appears. The onboarding UI surfaces the "no
/// credential yet" state via `codex_setup_status`.
fn warn_if_no_codex_login() {
    if let Err(e) = runtime::runtime().block_on(pocket_codex_api_proxy::check_auth()) {
        if pocket_codex_codex::setup::has_custom_provider() {
            tracing::info!("no codex login, but a custom provider is configured; hosting proceeds");
        } else {
            tracing::warn!(
                error = %format!("{e:#}"),
                "hosting without a codex login yet; configure a provider or complete codex login. \
                 The API proxy will fail upstream until a credential exists."
            );
        }
    }
}

pub fn serve_start(
    port: u16,
    binary_override: Option<String>,
    name: Option<String>,
    proxy: Option<String>,
    embedded: bool,
) -> Result<ServeReport> {
    let support = runtime::support_dir()?;
    let mut config = load_config(&support)?;
    if config.account_token().is_none() {
        bail!("sign in with GitHub before hosting a local app-server");
    }

    let binary = resolve_codex_binary(embedded, binary_override, &support, &mut config)?;
    // Capture the resolved path for the host details before `binary` is moved
    // into the spawn options below (`None` for an embedded host).
    let codex_binary_display = binary.as_ref().map(|p| p.display().to_string());

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

    if let Some(report) = reuse_or_retire_host(&name, port)? {
        return Ok(report);
    }

    warn_if_no_codex_login();

    // Open the (process-global) meta config store before spawning anything, so an
    // unwritable CODEX_HOME surfaces as a hosting error here instead of after a
    // codex child is already running (or via a panic in the supervisor).
    let config_store = config_store()?;
    let host_config_store = host_store()?;

    // Resolve an automatic port (0) HERE, for both paths: the external spawn
    // can't take 0 (codex would bind an ephemeral port that neither `spawn`'s
    // port wait nor `verify_ready` ever learns), and the hosting form + the
    // startup-failure hint both promise 0 works.
    let port = if port == 0 { free_loopback_port()? } else { port };

    // Bring up codex serving 127.0.0.1:<port>: in-process (embedded) or as a
    // spawned child binary. Both yield the local app-server socket, a pid (our
    // own for embedded), the embedded task handle (`None` for external), and a
    // watchdog keeping codex alive, and (external only) a log-file tailer. Runs
    // on the flutter_rust_bridge worker thread, so the blocking port poll is fine.
    #[allow(
        clippy::type_complexity,
        reason = "a local bring-up tuple immediately destructured into named bindings"
    )]
    let (app_local, pid, embedded_task, watchdog, log_tail, adopted): (
        SocketAddr,
        u32,
        Option<JoinHandle<()>>,
        JoinHandle<()>,
        Option<JoinHandle<()>>,
        bool,
    ) = if embedded {
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        {
            // In-process codex reaches chatgpt via the host's proxy from the
            // process environment (the spawned child gets it via its command env).
            if let Some(px) = proxy.as_deref() {
                set_proxy_env(px);
            }
            // Fail a taken port BEFORE the supervisor exists — its own bind
            // failure disappears into a silent retry loop, and neither TCP
            // wait below can tell our listener from a foreign process's.
            probe_port_free(port)?;
            let listen_url = format!("ws://127.0.0.1:{port}");
            let task = runtime::runtime().spawn(run_embedded_supervised(listen_url));
            if let Err(e) = wait_for_listener("127.0.0.1", port, Duration::from_secs(30)) {
                // Abort the supervisor, or it would keep retrying the bind
                // forever after this start already failed — and could later
                // come up unpublished and out of reach of `serve_stop`.
                task.abort();
                return Err(e);
            }
            // An accepting port is still NOT proof our embedded codex is the
            // one serving it (the probe→bind handoff has a window a foreign
            // process could grab; embedded's bind would then quietly retry in
            // the supervisor while the FOREIGN listener satisfies the TCP
            // wait) — and the app/api/meta tunnels would publish an unrelated
            // server. Only a 2xx /readyz proves a live codex app-server
            // answers before anything is published.
            if !pocket_codex_codex::wait_for_readyz("127.0.0.1", port, READY_TIMEOUT) {
                task.abort();
                return Err(embedded_not_ready_error(port));
            }
            let app_local: SocketAddr = format!("127.0.0.1:{port}")
                .parse()
                .expect("loopback socket addr");
            // The supervisor only restarts the in-process server when its task
            // EXITS; a wedged-but-alive server (hung accept loop that stops
            // answering /readyz) never exits, so — exactly like the external
            // path — a watchdog probes /readyz and abort+respawns the
            // supervisor when it goes quiet.
            let watchdog = runtime::runtime()
                .spawn(embedded_health_watchdog(name.clone(), app_local.to_string()));
            // Embedded codex's logs already stream through the in-process tracing
            // layer, so there's no separate log file to tail.
            (app_local, std::process::id(), Some(task), watchdog, None, false)
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            bail!("this build has no embedded codex; use an external codex binary");
        }
    } else {
        let spawn_opts = SpawnOptions {
            binary,
            listen: ListenSpec::WebSocket {
                host: "127.0.0.1".to_string(),
                port,
            },
            extra_args: Vec::new(),
            // A per-host log file so this host's tailer captures only its own
            // codex's output, never another host's from the shared default file.
            log_file: Some(per_host_log_file(&name)?),
            proxy: proxy.clone(),
        };
        // Spawn + readiness in one step: don't publish tunnels to a child
        // that died on boot (classically a bind failure on an already-taken
        // port) — fail the start now, with the child's own error output.
        // Blocking here is fine — this runs on the frb worker thread, like
        // the port poll inside `spawn`. On failure the tokio tasks need no
        // teardown (watchdog/tailer/proxy are only spawned after this), but
        // the fresh child itself does: reap it if it is still coming up, or
        // a failed start leaves an unsupervised codex squatting on the port,
        // out of reach of `serve_stop` (which only knows registered hosts).
        // A pre-existing server adopted by `spawn` is never killed.
        let report = match spawn_ready(spawn_opts.clone(), READY_TIMEOUT) {
            Ok(report) => report,
            Err(SpawnReadyError::Spawn(e)) => {
                return Err(anyhow::Error::new(e).context("spawning codex app-server"));
            },
            Err(SpawnReadyError::NotReady {
                report,
                failure,
            }) => {
                if !report.reused && !failure.process_exited {
                    if pid_running(report.info.pid) {
                        send_sigterm(report.info.pid);
                    }
                    if let Some(addr) = spawn_opts.listen.as_socket_addr() {
                        stop_codex_at(&addr);
                    }
                }
                return Err(startup_failure_error(*failure));
            },
        };
        let listen_addr = report
            .info
            .listen
            .strip_prefix("ws://")
            .map(str::to_string)
            .ok_or_else(|| {
                anyhow!("codex listen `{}` is not a ws:// address", report.info.listen)
            })?;
        let app_local: SocketAddr = listen_addr
            .parse()
            .with_context(|| format!("codex listen `{listen_addr}` is not a socket address"))?;
        // Pin the watchdog's respawn to the resolved codex port.
        let mut watchdog_opts = spawn_opts;
        watchdog_opts.listen = ListenSpec::WebSocket {
            host: app_local.ip().to_string(),
            port: app_local.port(),
        };
        let watchdog =
            runtime::runtime().spawn(health_watchdog(app_local.to_string(), watchdog_opts));
        // Surface the spawned codex's own logs in the in-app viewer by tailing
        // the file its stdout/stderr are redirected to, from this run's offset.
        let log_tail =
            tail_codex_log(report.info.log_file.clone(), report.log_offset, name.clone());
        (app_local, report.info.pid, None, watchdog, Some(log_tail), report.reused)
    };

    // The rest of the start can still fail (loopback listener binds, relay
    // transport). Run those fallible steps as ONE block so a single failure
    // path can tear the just-brought-up codex back down — a bare `?` here
    // would DROP (and thereby detach, not abort) the supervisor + watchdog
    // handles, leaving a live server squatting on the port, out of reach of
    // `serve_stop` (which only knows registered hosts).
    //
    // Reserves an ephemeral loopback port each for the in-process API proxy
    // and meta service and learns them (so we can register those ports);
    // supervisor tasks (spawned below, infallibly) keep both alive by
    // re-binding + restarting with backoff, so the registered `api:<name>` /
    // `meta:<name>` tunnels keep forwarding to live servers rather than dead
    // sockets.
    let bringup_tail = (|| -> Result<_> {
        let api_std = std::net::TcpListener::bind("127.0.0.1:0")
            .context("binding the in-app API proxy listener")?;
        api_std
            .set_nonblocking(true)
            .context("setting the API proxy listener non-blocking")?;
        let api_local: SocketAddr = api_std
            .local_addr()
            .context("reading the API proxy listener address")?;
        let meta_std = std::net::TcpListener::bind("127.0.0.1:0")
            .context("binding the in-app meta service listener")?;
        meta_std
            .set_nonblocking(true)
            .context("setting the meta service listener non-blocking")?;
        let meta_local: SocketAddr = meta_std
            .local_addr()
            .context("reading the meta service listener address")?;
        let transport = transport::resolve_blocking()?;
        Ok((api_std, api_local, meta_std, meta_local, transport))
    })();
    let (api_std, api_local, meta_std, meta_local, transport) = match bringup_tail {
        Ok(tail) => tail,
        Err(e) => {
            watchdog.abort();
            if let Some(t) = &log_tail {
                t.abort();
            }
            match &embedded_task {
                // In-process: aborting the supervisor stops the inner server
                // (its AbortOnDrop guard) and frees the port.
                Some(t) => t.abort(),
                // External: reap the fresh child. An ADOPTED (pre-existing,
                // reused) server is never ours to kill — mirrors the
                // verify_ready failure path above.
                None if !adopted => stop_codex_at(&app_local.to_string()),
                None => {},
            }
            return Err(e);
        },
    };
    let api_proxy =
        runtime::runtime().spawn(api_proxy_supervisor(api_local, api_std, proxy.clone()));
    // The meta service resumes into the codex we just brought up (`app_local`)
    // and shares the host-global config store.
    let meta_svc = runtime::runtime().spawn(meta_svc_supervisor(
        meta_local,
        meta_std,
        app_local,
        config_store,
        host_config_store,
    ));
    // The api and meta services are best-effort: one that could not publish is
    // retried on the next re-host, and neither blocks remote control. The APP
    // service is published last (below), once the host struct exists — its
    // outcome IS decisive, so a name conflict has to be able to tear everything
    // built here back down through the one teardown path.
    let api_register = register_service(&transport, &device, ServiceKind::Api, &name, api_local)
        .ok()
        .flatten();
    let meta_register = register_service(&transport, &device, ServiceKind::Meta, &name, meta_local)
        .ok()
        .flatten();

    let mut host = LocalServe {
        device: device.clone(),
        name: name.clone(),
        app_key: app_key.clone(),
        app_local,
        pid,
        // Filled in immediately below.
        app_register: None,
        watchdog,
        embedded: embedded_task,
        log_tail,
        api_key: api_key.clone(),
        api_local,
        api_proxy,
        api_register,
        meta_key: meta_key.clone(),
        meta_local,
        meta_svc,
        meta_register,
        codex_binary: codex_binary_display,
        proxy: proxy.clone(),
    };

    // Publish the app service last, and treat its outcome as decisive: if another
    // live instance already owns this name, tear everything just built back down
    // (codex included) and fail with the reason, rather than leave a host that can
    // never publish silently fighting for the key. An unreachable relay proceeds
    // optimistically — the SDK keeps retrying, and a re-host re-publishes.
    match register_service(&transport, &device, ServiceKind::App, &name, app_local) {
        Ok(published) => host.app_register = published,
        Err(conflict) => {
            stop_host_tasks(host);
            return Err(conflict);
        },
    }

    hosts_locked().insert(name.clone(), host);

    Ok(ServeReport {
        device,
        name,
        app_service_key: app_key,
        app_listen_addr: app_local.to_string(),
        api_service_key: api_key,
        api_listen_addr: api_local.to_string(),
        meta_service_key: meta_key,
        meta_listen_addr: meta_local.to_string(),
        pid,
        reused: false,
    })
}

/// Snapshot of every local host, sorted by name for a stable UI order.
///
/// Also the recovery tick. The UI polls this, and a service the relay
/// PERMANENTLY refused has no worker left retrying it — the SDK reconnects a
/// dropped tunnel, but not one it was told to stop. Re-publishing those here is
/// what lets hosting heal on its own instead of waiting for a user to notice
/// and re-host. A handle that is merely `Retrying` is left alone
/// ([`needs_publishing`] excludes it) so we never race the SDK's own reconnect.
pub fn serve_status() -> Vec<ServeStatus> {
    // Before reading status, so the snapshot reflects any recovery just made. Its
    // own errors are the UI's to discover on the next call: `serve_status` has no
    // way to report one, and a name conflict here means another device took over,
    // which the `false` registered flag already conveys.
    republish_refused();

    let guard = hosts_locked();
    let mut out: Vec<ServeStatus> = guard
        .values()
        .map(|ls| ServeStatus {
            name: ls.name.clone(),
            device: ls.device.clone(),
            pid: Some(ls.pid),
            alive: listen_addr_open(&ls.app_local.to_string()),
            app_listen_addr: ls.app_local.to_string(),
            app_service_key: ls.app_key.clone(),
            app_registered: is_published(&ls.app_register),
            api_listen_addr: ls.api_local.to_string(),
            api_service_key: ls.api_key.clone(),
            api_registered: is_published(&ls.api_register),
            meta_listen_addr: ls.meta_local.to_string(),
            meta_service_key: ls.meta_key.clone(),
            meta_registered: is_published(&ls.meta_register),
            embedded: ls.embedded.is_some(),
            codex_binary: ls.codex_binary.clone(),
            proxy: ls.proxy.clone(),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Re-publish every host's permanently-refused services, quietly.
///
/// The self-healing half of [`serve_status`]. Best-effort by construction: it
/// cannot report to anyone, so a failure just leaves the service unpublished
/// for the next tick to retry. Rate-limited because status is polled often and
/// each attempt is a relay round trip — without that, a relay that is down
/// would turn a 1 Hz UI poll into a 1 Hz publish storm, which is the shape of
/// the 2026-07-07 outage.
fn republish_refused() {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    /// Minimum gap between recovery sweeps.
    const RECOVERY_INTERVAL: Duration = Duration::from_secs(30);

    {
        let mut last = LAST.lock().unwrap_or_else(|poison| poison.into_inner());
        if last.is_some_and(|at| at.elapsed() < RECOVERY_INTERVAL) {
            return;
        }
        *last = Some(Instant::now());
    }

    let names: Vec<String> = hosts_locked()
        .values()
        .filter(|ls| {
            needs_publishing(&ls.app_register)
                || needs_publishing(&ls.api_register)
                || needs_publishing(&ls.meta_register)
        })
        .map(|ls| ls.name.clone())
        .collect();
    for name in names {
        if let Err(err) = republish_dropped(&name) {
            tracing::debug!(
                service = %name,
                error = %format!("{err:#}"),
                "re-publishing a refused service failed; will retry"
            );
        }
    }
}

/// Take one service (`kind` = `"app"`/`"api"`/`"meta"`) off the relay without
/// stopping the host. Reversible via [`serve_reregister`].
///
/// Dropping the registration IS the deregistration now, and it releases the
/// relay key as it goes. It used to need a `DELETE /v1/services` on top,
/// because the local end of the tunnel was a task the app could abort while the
/// BACKEND still held the relay-side registration open — so the key lingered
/// until its lease expired unless the backend was asked to retire it. With the
/// app registering directly there is no second party holding anything.
pub fn serve_deregister(name: &str, kind: &str) -> Result<()> {
    let kind: ServiceKind = kind
        .parse()
        .map_err(|_| anyhow!("invalid service kind `{kind}`"))?;
    let published = {
        let mut guard = hosts_locked();
        let ls = guard
            .get_mut(name)
            .ok_or_else(|| anyhow!("`{name}` is not hosting locally"))?;
        registration_slot(ls, kind).take()
    };
    // Awaited outside the hosts lock, and awaited rather than dropped, so the
    // relay frees the key before a re-register could race its own predecessor.
    if let Some(published) = published {
        if let Err(e) = runtime::runtime().block_on(published.stop()) {
            tracing::warn!(
                error = %format!("{e:#}"),
                service = %name,
                kind = %kind.as_key_segment(),
                "releasing the relay key on deregister failed; it lingers until lease expiry"
            );
        }
    }
    Ok(())
}

/// Re-publish a previously [`serve_deregister`]'d service, forwarding to the
/// still-running process. No-op if already published.
pub fn serve_reregister(name: &str, kind: &str) -> Result<()> {
    let kind: ServiceKind = kind
        .parse()
        .map_err(|_| anyhow!("invalid service kind `{kind}`"))?;
    let Some((device, local)) = ({
        let guard = hosts_locked();
        let ls = guard
            .get(name)
            .ok_or_else(|| anyhow!("`{name}` is not hosting locally"))?;
        let (local, slot) = match kind {
            ServiceKind::Api => (ls.api_local, &ls.api_register),
            ServiceKind::Meta => (ls.meta_local, &ls.meta_register),
            _ => (ls.app_local, &ls.app_register),
        };
        needs_publishing(slot).then(|| (ls.device.clone(), local))
    }) else {
        return Ok(());
    };
    // Publishing waits on the relay, so it happens with the hosts lock RELEASED —
    // holding it would stall every status poll for as long as the relay takes.
    let transport = transport::resolve_blocking()?;
    let published = register_service(&transport, &device, kind, name, local)?;
    store_registration(name, kind, published);
    Ok(())
}

/// Fully stop one host by name: release its three relay keys, abort the
/// watchdog, API proxy, and meta service tasks, and stop its codex.
/// Best-effort and idempotent — a no-op when that name isn't hosting.
pub fn serve_stop(name: &str) -> Result<()> {
    let removed = hosts_locked().remove(name);
    if let Some(ls) = removed {
        stop_host_tasks(ls);
    }
    Ok(())
}

/// Stop every host (called on app quit so a real quit leaves no orphan codex).
pub fn serve_stop_all() {
    let all: Vec<LocalServe> = hosts_locked().drain().map(|(_, ls)| ls).collect();
    for ls in all {
        stop_host_tasks(ls);
    }
}

/// Abort a host's background tasks (all register tunnels, watchdog, API proxy,
/// meta service) and stop its codex. Does not touch the relay (callers that
/// need an immediate relay drop force-deregister separately).
fn stop_host_tasks(ls: LocalServe) {
    // Awaited rather than dropped, so the relay releases all three keys NOW: an
    // immediate re-host under the same name would otherwise race registrations
    // this one abandoned but the relay still held.
    runtime::runtime().block_on(async {
        for published in [ls.app_register, ls.api_register, ls.meta_register]
            .into_iter()
            .flatten()
        {
            if let Err(e) = published.stop().await {
                tracing::warn!(
                    error = %format!("{e:#}"),
                    service = %ls.name,
                    "releasing a relay key on stop failed; it lingers until lease expiry"
                );
            }
        }
    });
    ls.watchdog.abort();
    ls.api_proxy.abort();
    ls.meta_svc.abort();
    if let Some(h) = ls.log_tail {
        h.abort();
    }
    if let Some(h) = ls.embedded {
        // Embedded codex is an in-process task: abort it (its supervisor stops
        // and the WS listener closes). Skip the port-targeted process kill — the
        // listener is our own process. The abort is only DELIVERED at the
        // task's next await, so wait (bounded, like the external path's
        // stop_codex_at) for the port to actually close: an immediate re-host
        // on the same port would otherwise race the dying listener and fail
        // its probe_port_free. Normally this returns in one connect probe.
        h.abort();
        wait_for_port_closed(&ls.app_local.to_string(), Duration::from_secs(6));
    } else {
        stop_codex_at(&ls.app_local.to_string());
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
/// codex at `app_ws_addr` and shares the host-global `store` + `host` config.
async fn meta_svc_supervisor(
    addr: SocketAddr,
    first: std::net::TcpListener,
    app_ws_addr: SocketAddr,
    store: Arc<pocket_codex_host_svc::store::ConfigStore>,
    host: Arc<pocket_codex_host_svc::store::HostStore>,
) {
    supervise("the in-app meta service", addr, first, move |listener| {
        let store = store.clone();
        let host = host.clone();
        async move { pocket_codex_host_svc::serve(listener, app_ws_addr, store, host).await }
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
    let Some(client) = probe_client() else {
        return;
    };
    let mut consecutive: u32 = 0;
    let mut restart_failures: u32 = 0;
    loop {
        tokio::time::sleep(HEALTH_INTERVAL).await;
        if probe_ready(&client, &url).await && probe_thread_rpc(&local_addr).await {
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
        tokio::time::sleep(health_restart_backoff(restart_failures)).await;
    }
}

/// Grace before probing resumes after a watchdog-driven restart, doubling per
/// consecutive failed recovery up to [`MAX_RESTART_BACKOFF`] — shared by the
/// external and embedded watchdogs so their recovery pacing can't drift apart.
fn health_restart_backoff(restart_failures: u32) -> Duration {
    HEALTH_RESTART_GRACE
        .saturating_mul(1u32 << restart_failures.min(5))
        .min(MAX_RESTART_BACKOFF)
}

/// The HTTP client both watchdogs probe `/readyz` with. Built with
/// `.no_proxy()`: probes only ever target our own loopback server, and the
/// process environment can carry an upstream proxy (the user's own, or the one
/// `set_proxy_env` installs for an embedded host) whose `NO_PROXY` doesn't
/// cover loopback — a proxied probe then fails against a perfectly healthy
/// server and the watchdog would keep "recovering" it forever.
fn probe_client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(HEALTH_TIMEOUT)
        .no_proxy()
        .build()
        .ok()
}

/// One `/readyz` probe over `client`: `true` on any 2xx.
async fn probe_ready(client: &reqwest::Client, url: &str) -> bool {
    matches!(client.get(url).send().await, Ok(resp) if resp.status().is_success())
}

async fn probe_thread_rpc(local_addr: &str) -> bool {
    let result =
        pocket_codex_codex::readiness::probe_rpc(&format!("ws://{local_addr}"), READY_TIMEOUT)
            .await;
    if let Err(error) = &result {
        tracing::warn!(%local_addr, error = %format!("{error:#}"), "app-server functional health probe failed");
    }
    result.is_ok()
}

/// Poll `/readyz` until it answers 2xx or `timeout` elapses — the async,
/// in-runtime sibling of `pocket_codex_codex::wait_for_readyz` (which parks a
/// thread).
#[cfg(any(target_os = "windows", target_os = "macos"))]
async fn wait_ready(client: &reqwest::Client, url: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if probe_ready(client, url).await {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Probe the EMBEDDED codex app-server's `/readyz` and, when it stops
/// answering, abort + respawn its supervisor task — the embedded twin of
/// [`health_watchdog`], with the same probe cadence, failure threshold, and
/// restart backoff.
///
/// [`run_embedded_supervised`] only restarts the in-process server when its
/// task EXITS; a wedged-but-not-exited server (hung accept loop that stops
/// answering `/readyz`) never trips it, and without this watchdog the
/// published app/api/meta tunnels would forward to a dead server until the
/// user restarted hosting by hand. Limit: task abortion is cooperative — an
/// inner task that never reaches an await can't be cancelled and then keeps
/// the port bound (the fresh supervisor just retries its bind); every wedge
/// that still yields recovers.
///
/// Returns (stops probing) once its host entry is gone or belongs to a
/// successor host — `serve_stop` aborts this task anyway; the checks inside
/// [`respawn_embedded_supervisor`] are the backstop for the window between
/// entry removal and that abort.
#[cfg(any(target_os = "windows", target_os = "macos"))]
async fn embedded_health_watchdog(name: String, local_addr: String) {
    let url = format!("http://{local_addr}/readyz");
    let Some(client) = probe_client() else {
        return;
    };
    let mut consecutive: u32 = 0;
    let mut restart_failures: u32 = 0;
    loop {
        tokio::time::sleep(HEALTH_INTERVAL).await;
        if probe_ready(&client, &url).await && probe_thread_rpc(&local_addr).await {
            consecutive = 0;
            restart_failures = 0;
            continue;
        }
        consecutive += 1;
        if consecutive < HEALTH_FAILURES {
            continue;
        }
        tracing::warn!(
            %local_addr,
            "embedded codex app-server failed {HEALTH_FAILURES} health checks; restarting it"
        );
        if !respawn_embedded_supervisor(&name, &local_addr) {
            return;
        }
        // The respawn itself cannot fail (it only spawns a task); whether the
        // server actually came back is only visible on /readyz. Verify that
        // bounded — the embedded analogue of the external path's
        // `verify_ready` — so repeated failed recoveries engage the restart
        // backoff instead of re-aborting at probe cadence.
        if wait_ready(&client, &url, READY_TIMEOUT).await {
            tracing::info!(%local_addr, "embedded codex app-server restarted");
            restart_failures = 0;
        } else {
            restart_failures += 1;
            tracing::warn!(
                %local_addr,
                attempt = restart_failures,
                "restarted embedded codex app-server did not become ready"
            );
        }
        consecutive = 0;
        tokio::time::sleep(health_restart_backoff(restart_failures)).await;
    }
}

/// Abort the embedded supervisor of host `name` and install a fresh one on
/// the same listen URL, swapping the handle in the host entry so a later
/// `serve_stop` aborts the LIVE task rather than a dead handle. Aborting the
/// supervisor also stops the inner app-server (its `AbortOnDrop` guard),
/// which frees the port for the fresh supervisor's bind-retry loop. Returns
/// `false` — the calling watchdog must stand down — when the host is gone or
/// is no longer this embedded server on this address (stopped or replaced
/// concurrently: an aborted watchdog still runs sync code until its next
/// await, so it must never touch a successor host's entry).
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn respawn_embedded_supervisor(name: &str, local_addr: &str) -> bool {
    let mut guard = hosts_locked();
    let Some(ls) = guard.get_mut(name) else {
        return false;
    };
    if ls.app_local.to_string() != local_addr {
        return false;
    }
    let Some(old) = ls.embedded.take() else {
        return false;
    };
    old.abort();
    let listen_url = format!("ws://{local_addr}");
    ls.embedded = Some(runtime::runtime().spawn(run_embedded_supervised(listen_url)));
    true
}

/// Stop the wedged codex and spawn a fresh one on the same port (escalating to
/// a hard kill if it ignores the graceful stop). Blocking work runs off the
/// async runtime.
async fn restart_codex(spawn_opts: SpawnOptions) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        if let Some(addr) = spawn_opts.listen.as_socket_addr() {
            stop_codex_at(&addr);
        }
        let report = spawn_ready(spawn_opts, READY_TIMEOUT).map_err(|err| match err {
            SpawnReadyError::Spawn(e) => {
                anyhow::Error::new(e).context("respawning the codex app-server")
            },
            // An adopted (reused) listener means spawn found the OLD process
            // still bound — the restart did not take effect; report that
            // precisely rather than as a readiness failure. Otherwise a
            // respawn that dies on boot (e.g. something else grabbed the
            // port while codex was down) must count as a FAILED restart, so
            // the watchdog's restart backoff engages instead of resetting and
            // hammering a bind that can never succeed.
            SpawnReadyError::NotReady {
                report,
                failure,
            } => {
                if report.reused {
                    anyhow::anyhow!(
                        "codex is still holding the listen port; restart did not take effect"
                    )
                } else {
                    startup_failure_error(*failure)
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

/// The caller's phrasing of the pick-another-port remedy (the hosting form's
/// port field), shared by the embedded startup errors and
/// [`startup_failure_error`].
fn port_retry_hint(port: u16) -> String {
    match port.checked_add(1) {
        Some(next) => format!("retry with port {next}, or 0 to pick a free port automatically"),
        None => "retry with a different port".to_string(),
    }
}

/// The hosting error for an embedded start whose port failed the pre-spawn
/// bind probe: definitively held by another process (or an invisible
/// port-lease holder), so the in-process app-server can never serve there.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn embedded_bind_conflict_error(port: u16, cause: &std::io::Error) -> anyhow::Error {
    let retry = port_retry_hint(port);
    // Mirror StartupFailure::diagnosis's Windows hint: the usual invisible
    // holder is a WSL mirrored-networking port lease — nothing shows in
    // netstat, yet binds fail with 10048.
    #[cfg(windows)]
    let free_hint = " — or free it (a leaked WSL mirrored-networking lease holds ports invisibly; \
                     `wsl --shutdown` releases them)";
    #[cfg(not(windows))]
    let free_hint = "";
    anyhow::anyhow!(
        "port {port} is already in use (binding 127.0.0.1:{port} failed: {cause}), so the \
         embedded codex app-server cannot serve there — {retry}{free_hint}"
    )
}

/// The hosting error for an embedded start whose port passed the bind probe
/// and got a listener, but where `/readyz` never answered 2xx: either another
/// process grabbed the port in the probe→bind handoff, or our own app-server
/// came up wedged. Publishing tunnels to it would be wrong either way.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn embedded_not_ready_error(port: u16) -> anyhow::Error {
    let retry = port_retry_hint(port);
    anyhow::anyhow!(
        "the embedded codex app-server did not become ready on 127.0.0.1:{port} in time (the \
         listener there never answered codex's /readyz — another process may have grabbed the \
         port, or the app-server failed to start) — {retry}"
    )
}

/// Render a [`StartupFailure`] as the hosting error surfaced to the UI —
/// [`StartupFailure::diagnosis`] with the remedy phrased as the hosting
/// form's port field (the CLI sibling phrases it as the `--port` flag).
fn startup_failure_error(failure: StartupFailure) -> anyhow::Error {
    let retry = match failure.port {
        Some(port) => port_retry_hint(port),
        None => "retry with a different port".to_string(),
    };
    anyhow::anyhow!(failure.diagnosis(&retry))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_failure_error_carries_log_tail_and_port_hint() {
        let failure = StartupFailure {
            rpc_error: None,
            process_exited: true,
            port_in_use: true,
            listen: "ws://127.0.0.1:18080".to_string(),
            port: Some(18080),
            log_file: std::path::PathBuf::from("codex-app-server.log"),
            log_tail: vec!["Error: Address already in use (os error 10048)".to_string()],
        };
        let msg = startup_failure_error(failure).to_string();
        assert!(msg.contains("exited during startup"));
        assert!(msg.contains("codex-app-server.log"));
        assert!(msg.contains("os error 10048"));
        assert!(
            msg.contains("port 18081, or 0 to pick a free port automatically"),
            "should suggest the next port or an automatic one: {msg}"
        );
    }

    #[test]
    fn host_log_cap_triggers_just_past_the_threshold() {
        assert!(!over_log_cap(0));
        assert!(!over_log_cap(MAX_HOST_LOG_BYTES), "at the cap is still fine");
        assert!(over_log_cap(MAX_HOST_LOG_BYTES + 1), "one byte over trips it");
        // The threshold itself is a size rather than a time window on purpose:
        // the burst that prompted this peaked at 72 MB/min, so "keep the last
        // six hours" would permit ~25 GB. Asserting the constant's value here
        // would be a tautology (clippy rightly flags it) — the guard that keeps
        // it honest is that this cap is measured in bytes at all.
    }

    #[test]
    fn truncating_a_live_host_log_frees_it_in_place() {
        let dir = std::env::temp_dir().join(format!("pcx-logcap-{}", std::process::id()));
        // This test can run before every other runtime-using test. Initialize
        // explicitly instead of depending on the test runner's random order.
        runtime::init(std::env::temp_dir()).expect("init runtime");
        std::fs::create_dir_all(&dir).expect("tmp dir");
        let log = dir.join("codex-default.log");
        std::fs::write(&log, vec![b'x'; 1024]).expect("write");

        // Hold the file open the way the spawned child does, so this exercises
        // the case that rules out `remove_file`.
        let writer = std::fs::OpenOptions::new()
            .append(true)
            .open(&log)
            .expect("open writer");

        assert!(runtime::runtime().block_on(truncate_host_log(&log)));
        assert_eq!(std::fs::metadata(&log).expect("meta").len(), 0, "space is freed now");

        // The still-open writer keeps working, and its next append lands at 0 —
        // which the tailer's `len < pos` branch resyncs to.
        use std::io::Write as _;
        let mut writer = writer;
        writer.write_all(b"after\n").expect("append after truncate");
        writer.flush().expect("flush");
        assert_eq!(std::fs::read(&log).expect("read"), b"after\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[test]
    fn probe_port_free_fails_a_held_port_with_cause_and_remedy() {
        // Hold a port, then probe it: must fail with the true cause and the
        // pick-another-port remedy (matching the external path's phrasing).
        let held = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a probe target");
        let port = held.local_addr().expect("local addr").port();
        let msg = probe_port_free(port)
            .expect_err("probing a held port must fail")
            .to_string();
        assert!(msg.contains(&format!("port {port} is already in use")), "{msg}");
        assert!(
            msg.contains(&port_retry_hint(port)),
            "should suggest the next port or an automatic one: {msg}"
        );
        #[cfg(windows)]
        assert!(msg.contains("wsl --shutdown"), "should carry the WSL-lease hint: {msg}");

        // A freed port probes clean.
        drop(held);
        assert!(probe_port_free(port).is_ok());
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[test]
    fn embedded_not_ready_error_names_both_causes_and_the_remedy() {
        let msg = embedded_not_ready_error(18080).to_string();
        assert!(msg.contains("did not become ready on 127.0.0.1:18080"), "{msg}");
        assert!(msg.contains("/readyz"), "{msg}");
        assert!(
            msg.contains("retry with port 18081, or 0 to pick a free port automatically"),
            "should suggest the next port or an automatic one: {msg}"
        );
        // The next-port hint must not overflow at the port ceiling.
        let msg = embedded_not_ready_error(u16::MAX).to_string();
        assert!(msg.contains("retry with a different port"), "{msg}");
    }

    /// A host entry whose task slots are inert pending futures — enough
    /// structure to exercise the respawn swap against the real hosts map.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn fake_embedded_host(name: &str, app_local: SocketAddr) -> LocalServe {
        let rt = runtime::runtime();
        let dummy: SocketAddr = "127.0.0.1:1".parse().expect("dummy addr");
        LocalServe {
            device: "test-device".to_string(),
            name: name.to_string(),
            app_key: format!("pcx:test-device:app:{name}"),
            app_local,
            pid: std::process::id(),
            app_register: None,
            watchdog: rt.spawn(std::future::pending::<()>()),
            embedded: Some(rt.spawn(std::future::pending::<()>())),
            log_tail: None,
            api_key: format!("pcx:test-device:api:{name}"),
            api_local: dummy,
            api_proxy: rt.spawn(std::future::pending::<()>()),
            api_register: None,
            meta_key: format!("pcx:test-device:meta:{name}"),
            meta_local: dummy,
            meta_svc: rt.spawn(std::future::pending::<()>()),
            meta_register: None,
            codex_binary: None,
            proxy: None,
        }
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[test]
    fn respawn_embedded_supervisor_swaps_in_a_live_task() {
        crate::engine::runtime::init(std::env::temp_dir()).expect("init runtime");
        // Hold the app port for the whole test so the respawned supervisor's
        // inner bind fails-and-retries instead of starting a real app-server
        // inside the test process.
        let held = std::net::TcpListener::bind("127.0.0.1:0").expect("hold the app port");
        let app_local = held.local_addr().expect("local addr");
        let name = format!("respawn-swap-test-{}", std::process::id());
        let host = fake_embedded_host(&name, app_local);
        let old_task = host
            .embedded
            .as_ref()
            .expect("embedded slot")
            .abort_handle();
        hosts_locked().insert(name.clone(), host);

        assert!(
            respawn_embedded_supervisor(&name, &app_local.to_string()),
            "a live matching host must be respawned"
        );

        // The wedged (here: pending-forever) supervisor must get aborted...
        let deadline = Instant::now() + Duration::from_secs(5);
        while !old_task.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(old_task.is_finished(), "the old supervisor should be aborted");
        // ...and the entry must now hold a LIVE replacement — the handle a
        // later serve_stop aborts.
        {
            let guard = hosts_locked();
            let fresh = guard
                .get(&name)
                .expect("host entry survives the swap")
                .embedded
                .as_ref()
                .expect("a fresh embedded task is installed")
                .abort_handle();
            assert!(!fresh.is_finished(), "the replacement supervisor should be running");
        }
        if let Some(ls) = hosts_locked().remove(&name) {
            stop_host_tasks(ls);
        }
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[test]
    fn respawn_embedded_supervisor_stands_down_when_host_is_gone_or_replaced() {
        crate::engine::runtime::init(std::env::temp_dir()).expect("init runtime");
        // Unknown name: the host was stopped — nothing to respawn.
        assert!(!respawn_embedded_supervisor("no-such-host", "127.0.0.1:1"));

        // Same name on a DIFFERENT address (a successor host): a stale
        // watchdog must not abort the successor's supervisor.
        let name = format!("respawn-guard-test-{}", std::process::id());
        let app_local: SocketAddr = "127.0.0.1:2".parse().expect("addr");
        let host = fake_embedded_host(&name, app_local);
        let successor = host
            .embedded
            .as_ref()
            .expect("embedded slot")
            .abort_handle();
        hosts_locked().insert(name.clone(), host);
        assert!(!respawn_embedded_supervisor(&name, "127.0.0.1:3"));
        assert!(
            !successor.is_finished(),
            "a stale watchdog must never abort a successor host's supervisor"
        );
        if let Some(ls) = hosts_locked().remove(&name) {
            stop_host_tasks(ls);
        }
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[test]
    fn wait_ready_sees_a_2xx_and_times_out_on_a_dead_port() {
        crate::engine::runtime::init(std::env::temp_dir()).expect("init runtime");
        // A fake /readyz answering 200 for every connection (the poll loop
        // probes repeatedly). The serving thread leaks; fine for a test.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind fake readyz");
        let addr = listener.local_addr().expect("local addr");
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut buf = [0u8; 512];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                );
            }
        });
        // Via `probe_client` rather than a hand-rolled builder, so the test
        // exercises the SAME client production probes with. Building one here
        // without `.no_proxy()` made this test fail on any machine with a system
        // HTTP proxy whose exceptions miss loopback: the probe went to the proxy
        // instead of the test's own listener — precisely the trap
        // `probe_client`'s doc comment describes.
        let client = probe_client().expect("build probe client");
        let url = format!("http://{addr}/readyz");
        assert!(runtime::runtime().block_on(wait_ready(&client, &url, Duration::from_secs(5))));

        // Nothing listening: false at the deadline, not a hang.
        let dead = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
            l.local_addr().expect("local addr")
        };
        let url = format!("http://{dead}/readyz");
        assert!(!runtime::runtime().block_on(wait_ready(
            &client,
            &url,
            Duration::from_millis(400)
        )));
    }

    #[test]
    fn hosts_locked_recovers_a_poisoned_lock() {
        // Poison the process-global host mutex the way a real panic-while-holding
        // would: a thread panics with the guard held. (Quiet the hook so the
        // intentional panic doesn't spam the test log.)
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let _ = std::thread::spawn(|| {
            let _guard = hosts().lock().expect("acquire before poisoning");
            panic!("intentional: poison the hosts mutex");
        })
        .join();
        std::panic::set_hook(prev);

        // The raw lock is now poisoned...
        assert!(hosts().lock().is_err(), "the mutex should be poisoned after the panic");
        // ...but the recovering accessor still hands back a usable guard instead
        // of panicking (which, cascading across every serve/status/stop call,
        // is what would take hosting down after one unlucky panic).
        let mut guard = hosts_locked();
        let before = guard.len();
        // It's a normal, mutable map — prove it's fully usable post-recovery.
        guard.retain(|_, _| true);
        assert_eq!(guard.len(), before);
    }
}
