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
//! the credential and key namespace [`Transport`](transport::Transport)
//! carries. Reconnecting a dropped registration is the pb-mapper SDK's job, so
//! the only failure this module acts on is a name CONFLICT — see
//! [`register_service`].
//!
//! The external codex process (log tail, watchdog, restarts) lives in
//! `serve_codex.rs`, stopping and the quit fence in `serve_lifecycle.rs`, and
//! relay registrations in `serve_registration.rs`; the steps ACP hosting
//! shares are in `host_services.rs`.
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
    locate_binary, spawn_ready_redacted, ListenSpec, SpawnOptions, SpawnReadyError, StartupFailure,
    READY_TIMEOUT,
};
use pocket_codex_core::{
    process::tcp_port_open,
    service::{ServiceId, ServiceKind},
};
use pocket_codex_pb::Published;
use tokio::task::JoinHandle;

use crate::engine::{
    config::{load_config, update_config},
    host_lease::{self, Lease, Op},
    host_names, host_profiles, logging,
    owned_child::{OwnedChild, EXIT_WAIT},
    runtime, transport,
};

#[path = "serve_codex.rs"]
mod codex;
#[path = "serve_lifecycle.rs"]
mod lifecycle;
#[path = "serve_registration.rs"]
mod registration;

#[cfg(test)]
use self::codex::{over_log_cap, truncate_host_log, MAX_HOST_LOG_BYTES};
#[cfg(test)]
pub use self::lifecycle::serve_stop;
pub(super) use self::lifecycle::{serve_stop_locked, serve_stop_remembered_checked};
use self::{
    codex::{health_watchdog, per_host_log_file, startup_failure_error, tail_codex_log, Watched},
    lifecycle::{
        commit_host, not_exited, quitting, settle_retained, stop_host_tasks, NATIVE, NOT_EXITED,
    },
    registration::{republish_dropped, republish_refused},
};
pub use self::{
    lifecycle::{serve_stop_all, serve_stop_remembered},
    registration::{serve_deregister, serve_reregister},
};
use super::host_services::{
    bind_loopback, config_store, host_store, is_published, needs_publishing, register_service,
    supervise, MAX_RESTART_BACKOFF,
};

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
    /// The exact app-server process this app started (and the shim in
    /// front of it), identified at the spawn: the only one a stop or
    /// restart signals; `None` when adopted.
    child: Option<super::owned_child::OwnedChild>,
    /// `Some` while the app service is published; `None` once deregistered.
    /// Holding it IS the publication — dropping it unpublishes.
    app_register: Option<Published>,
    watchdog: JoinHandle<()>,
    /// Tails the external codex log into the in-app viewer. Aborted on stop.
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
    /// The resolved external codex binary path, surfaced in host details.
    codex_binary: Option<String>,
    /// That exact executable, for restarting the host as it ran.
    binary_path: Option<std::path::PathBuf>,
    /// Identifies this start: a later start under the same name is another
    /// incarnation, which an apply opened for this one never touches.
    incarnation: String,
    /// The app-server was already running (adopted, not started here): it
    /// is not this host's to restart.
    adopted: bool,
    /// The upstream proxy codex + the API proxy were started with, or `None`
    /// when they inherit the app's own environment. Surfaced for debugging.
    proxy: Option<String>,
    /// The model-service configuration this codex was launched with, kept
    /// so an apply that fails can restart it as it was.
    config: host_profiles::Resolved,
}

/// How a running Codex host was launched, for restarting it in place.
#[derive(Debug, Clone)]
pub(super) struct CodexRelaunch {
    /// Its name in the host table.
    pub(super) name: String,
    /// Its app-server port.
    pub(super) port: u16,
    /// Its exact codex executable.
    pub(super) binary: Option<std::path::PathBuf>,
    /// Its upstream proxy.
    pub(super) proxy: Option<String>,
    /// Its model service.
    pub(super) provider: host_profiles::Resolved,
    /// This start's identity.
    pub(super) incarnation: String,
    /// Its app-server was adopted rather than started here.
    pub(super) adopted: bool,
}

/// How the running Codex host named like `name` (canonically) was launched.
pub(super) fn codex_relaunch(name: &str) -> Option<CodexRelaunch> {
    hosts_locked()
        .values()
        .find(|ls| host_names::same(&ls.name, name))
        .map(|ls| CodexRelaunch {
            name: ls.name.clone(),
            port: ls.app_local.port(),
            binary: ls.binary_path.clone(),
            proxy: ls.proxy.clone(),
            provider: ls.config.clone(),
            incarnation: ls.incarnation.clone(),
            adopted: ls.adopted,
        })
}

/// Which codex executable a start uses.
#[derive(Debug, Clone)]
pub(super) enum Binary {
    /// Resolved as a user's start does (explicit, saved, then `PATH`); an
    /// explicit path is saved for later starts.
    Requested(Option<String>),
    /// Exactly this executable, never saved: an apply or a restore.
    Exact(std::path::PathBuf),
}

/// One Codex start.
#[derive(Debug, Clone)]
pub(super) struct CodexStart {
    /// The app-server port (`0`: automatic).
    pub(super) port: u16,
    /// The executable.
    pub(super) binary: Binary,
    /// The requested instance name (trimmed; `default` when none).
    pub(super) name: String,
    /// The upstream proxy.
    pub(super) proxy: Option<String>,
    /// The model service; `None`: the instance's saved one.
    pub(super) provider: Option<host_profiles::Resolved>,
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
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
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
    /// Legacy runtime flag; always false for external Codex hosts.
    pub embedded: bool,
    /// The resolved external codex binary path.
    pub codex_binary: Option<String>,
    /// Upstream proxy codex + the API proxy were started with, or `None` when
    /// they inherit the app's environment.
    pub proxy: Option<String>,
    /// Service provider family: `codex` or `acp`.
    pub provider: String,
    /// Provider version, when known (ACP agents report it).
    pub provider_version: Option<String>,

    /// Wire protocol: `codex-app-server` or `acp`.
    pub protocol: String,
    /// Human-readable provider / agent name.
    pub provider_name: String,
    /// Agent profile id (ACP hosts).
    pub profile_id: Option<String>,
    /// Agent lifecycle phase (ACP): `starting`, `ready`, `failed`, `stopped`.
    pub agent_phase: Option<String>,
    /// Why the agent is not running (ACP), safe to show; for a Codex host,
    /// that a codex it owned did not exit (stop it again).
    pub agent_error: Option<String>,
    /// The agent reported that authentication is required (ACP).
    pub auth_required: bool,
    /// Native OpenCode runtime and catalog status.
    pub opencode: Option<super::serve_opencode::OpenCodeStatus>,
    /// Where the running host gets its model service (`inherited`,
    /// `official`, `custom`), as launched. `None` when unknown.
    pub config_source: Option<String>,
    /// The saved configuration revision the running host was launched with
    /// (`0`: never saved). Differs from the saved one until it is applied.
    pub config_revision: Option<u64>,
    /// Identifies this run of the host: an apply names it, so it never
    /// restarts a host that was replaced meanwhile.
    pub incarnation: Option<String>,
    /// What holds the host right now (`starting`, `stopping`, `restarting`,
    /// `applying`), when anything does.
    pub busy: Option<String>,
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

/// The resolved codex binary path (explicit override → persisted config →
/// PATH), or `None` when none resolve so the UI can prompt for one.
pub fn codex_locate() -> Option<String> {
    let configured = runtime::support_dir()
        .ok()
        .and_then(|dir| load_config(&dir).ok())
        .and_then(|c| c.codex.binary.clone());
    locate_binary(configured.as_deref()).map(|p| p.display().to_string())
}

/// Reserve a free loopback port (when the caller passed 0 for an automatic
/// one).
pub(super) fn free_loopback_port() -> Result<u16> {
    let l = std::net::TcpListener::bind("127.0.0.1:0").context("reserving a loopback port")?;
    Ok(l.local_addr()?.port())
}

/// Which codex binary to spawn: explicit override → persisted config → `$PATH`.
/// Persists an override that differs from what is saved, so the next start
/// finds the same codex without the caller passing it again.
fn resolve_codex_binary(
    binary: Binary,
    support: &std::path::Path,
    config: &mut pocket_codex_core::config::Config,
) -> Result<Option<std::path::PathBuf>> {
    let binary_override = match binary {
        Binary::Exact(path) => return Ok(Some(path)),
        Binary::Requested(requested) => requested,
    };
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
            *config = update_config(support, |latest| {
                latest.codex.binary = Some(ov.to_string());
                Ok(latest.clone())
            })?;
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
    // Another spelling of the same instance would publish the same relay
    // keys and share its saved configuration.
    if let Some(other) = guard
        .keys()
        .find(|other| other.as_str() != name && host_names::same(other, name))
    {
        bail!("`{other}` is already hosting under the same name; choose another name");
    }
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

/// Host an external Codex process and publish its app, API, and meta services.
/// The legacy `embedded` option is rejected before configuration or startup.
pub fn serve_start(
    port: u16,
    binary_override: Option<String>,
    name: Option<String>,
    proxy: Option<String>,
    embedded: bool,
) -> Result<ServeReport> {
    if embedded {
        bail!("the built-in engine is not implemented; use an external codex binary");
    }
    let start = CodexStart {
        port,
        binary: Binary::Requested(binary_override),
        name: host_names::requested(name.as_deref(), "default"),
        proxy,
        provider: None,
    };
    serve_start_in(&runtime::support_dir()?, &host_profiles::root()?, start)
}

/// [`serve_start`] reading its configuration (and saving an explicit codex
/// path) in `support` and the saved host configuration in `profiles`, under
/// the instance's lifecycle lease. Publishing still resolves the
/// process-wide transport.
pub(super) fn serve_start_in(
    support: &std::path::Path,
    profiles: &std::path::Path,
    start: CodexStart,
) -> Result<ServeReport> {
    let lease = host_lease::acquire(&start.name, Op::Start)?;
    let result = serve_start_locked(&lease, support, profiles, start)?;
    // A repeated start can reuse a launch with different options.
    if let Some(active) = super::host_apply::active(lease.name()) {
        super::host_lifecycle::record_running_in(profiles, lease.name(), active.runtime);
    }
    Ok(result)
}

/// [`serve_start_in`] with the instance's lease already held (an apply).
pub(super) fn serve_start_locked(
    lease: &Lease,
    support: &std::path::Path,
    profiles: &std::path::Path,
    start: CodexStart,
) -> Result<ServeReport> {
    debug_assert!(host_names::same(lease.name(), &start.name), "the lease names this host");
    // The instance's model service. An unreadable saved configuration fails
    // the start rather than silently launching with another provider.
    let provider = match start.provider.clone() {
        Some(provider) => provider,
        None => host_profiles::resolve_in(profiles, &start.name)?,
    };
    // Child output (in a startup failure) is shown only with the key's
    // lines withheld.
    launch_codex(support, start, &provider)
        .map_err(|e| anyhow!(host_profiles::redact(&format!("{e:#}"), &provider)))
}

fn launch_codex(
    support: &std::path::Path,
    start: CodexStart,
    provider: &host_profiles::Resolved,
) -> Result<ServeReport> {
    let CodexStart {
        port,
        binary,
        name,
        proxy,
        ..
    } = start;
    let provider = provider.clone();
    // No account gate: publishing goes through the shared transport, which
    // resolves self-host (relay + key) and account mode alike.
    let mut config = load_config(support)?;

    let binary = resolve_codex_binary(binary, support, &mut config)?;
    // Capture the resolved path for the host details before `binary` is moved
    // into the spawn options below.
    let codex_binary_display = binary.as_ref().map(|p| p.display().to_string());
    let binary_path = binary.clone();

    let device = runtime::device_id();
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

    // As child-only `-c` arguments and environment.
    let launch = host_profiles::codex(&provider, &name)?;
    let expected = launch.expected.clone();
    let incarnation = uuid::Uuid::new_v4().to_string();

    // A meta key is derived from the instance name, so one name cannot serve
    // two providers on this device. The claim lasts until this start returns
    // (after the host is recorded), so no other provider can start under the
    // name meanwhile; concurrent Codex starts share it.
    let _claim = super::hosting::claim(&name, super::hosting::Provider::Codex)?;
    if let Some(report) = reuse_or_retire_host(&name, port)? {
        return Ok(report);
    }
    // Nothing launches beside an earlier codex of this name still running.
    settle_retained(&name)?;

    warn_if_no_codex_login();

    // Open the (process-global) meta config store before spawning anything, so an
    // unwritable CODEX_HOME surfaces as a hosting error here instead of after a
    // codex child is already running (or via a panic in the supervisor).
    let config_store = config_store()?;
    let host_config_store = host_store()?;

    // Resolve an automatic port (0) here: the external spawn
    // can't take 0 (codex would bind an ephemeral port that neither `spawn`'s
    // port wait nor `verify_ready` ever learns), and the hosting form + the
    // startup-failure hint both promise 0 works.
    let port = if port == 0 { free_loopback_port()? } else { port };

    let (app_local, pid, child, watchdog, log_tail, adopted) = {
        let spawn_opts = SpawnOptions {
            binary,
            listen: ListenSpec::WebSocket {
                host: "127.0.0.1".to_string(),
                port,
            },
            extra_args: launch.args,
            // A per-host log file so this host's tailer captures only its own
            // codex's output, never another host's from the shared default file.
            log_file: Some(per_host_log_file(&name)?),
            proxy: proxy.clone(),
        };
        let secret_env = launch.env;
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
        // A child with a key writes its output through the redactor, so its
        // log file never holds a line that holds the key.
        let redactor = host_profiles::redactor(&provider);
        let report =
            match spawn_ready_redacted(spawn_opts.clone(), &secret_env, &redactor, READY_TIMEOUT) {
                Ok(report) => report,
                Err(SpawnReadyError::Spawn(e)) => {
                    return Err(anyhow::Error::new(e).context("spawning codex app-server"));
                },
                Err(SpawnReadyError::NotReady {
                    report,
                    failure,
                }) => {
                    // Only what this start launched, as identified at its
                    // spawn: a codex someone else started on the port is never
                    // signalled, and one that does not exit stays owned.
                    let exited = OwnedChild::of_report(&report)
                        .is_none_or(|child| NATIVE.stop(&name, child, EXIT_WAIT));
                    return Err(kept(startup_failure_error(*failure), exited));
                },
            };
        // An adopted app-server was not launched with this instance's
        // provider arguments: refuse it (never killed) rather than publish a
        // host that silently uses another model service.
        if report.reused && provider.source != host_profiles::Source::Inherited {
            bail!(
                "port {port} is already served by a codex this host did not start, so its model \
                 service cannot be applied; stop it or choose another port"
            );
        }
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
        // As identified at the spawn, never looked up by its address.
        let child = OwnedChild::of_report(&report);
        // Managed configuration and requirements are merged above `-c`: use
        // the host only if the app-server's effective configuration routes it
        // as selected, and stop the child this start launched otherwise.
        if let (false, Some(expected)) = (report.reused, &expected) {
            let checked = pocket_codex_codex::provider_launch::check_effective_blocking(
                &report.info.listen,
                expected,
                READY_TIMEOUT,
            );
            if let Err(refusal) = checked {
                let exited = child.is_none_or(|child| NATIVE.stop(&name, child, EXIT_WAIT));
                bail!("{refusal}{}", not_exited(exited));
            }
        }
        // Observed again held by the server readiness attributed: a second
        // point-in-time check, not proof that every answer since was its.
        if !pocket_codex_codex::still_owned(&report) {
            let exited = child.is_none_or(|child| NATIVE.stop(&name, child, EXIT_WAIT));
            bail!(
                "port {port} is no longer served by the codex this host started{}",
                not_exited(exited)
            );
        }
        // Pin the watchdog's respawn to the resolved codex port.
        let mut watchdog_opts = spawn_opts;
        watchdog_opts.listen = ListenSpec::WebSocket {
            host: app_local.ip().to_string(),
            port: app_local.port(),
        };
        // An adopted app-server is not this app's to restart.
        let watchdog = if report.reused {
            runtime::runtime().spawn(async {})
        } else {
            runtime::runtime().spawn(health_watchdog(Watched {
                name: name.clone(),
                incarnation: incarnation.clone(),
                local_addr: app_local.to_string(),
                spawn_opts: watchdog_opts,
                secret_env,
                expected,
                redactor: redactor.clone(),
            }))
        };
        // Surface the spawned codex's own logs in the in-app viewer by tailing
        // the file its stdout/stderr are redirected to, from this run's offset,
        // with every line holding the key withheld.
        let log_tail =
            tail_codex_log(report.info.log_file.clone(), report.log_offset, name.clone(), redactor);
        (app_local, report.info.pid, child, watchdog, Some(log_tail), report.reused)
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
        let (api_std, api_local) = bind_loopback("in-app API proxy")?;
        let (meta_std, meta_local) = bind_loopback("in-app meta service")?;
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
            let exited = child.is_none_or(|child| NATIVE.stop(&name, child, EXIT_WAIT));
            return Err(kept(e, exited));
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
        child,
        // Filled in immediately below.
        app_register: None,
        watchdog,
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
        binary_path,
        incarnation,
        adopted,
        proxy: proxy.clone(),
        config: provider,
    };

    // Publish the app service last, and treat its outcome as decisive: if another
    // live instance already owns this name, tear everything just built back down
    // (codex included) and fail with the reason, rather than leave a host that can
    // never publish silently fighting for the key. An unreachable relay proceeds
    // optimistically — the SDK keeps retrying, and a re-host re-publishes.
    match register_service(&transport, &device, ServiceKind::App, &name, app_local) {
        Ok(published) => host.app_register = published,
        Err(conflict) => {
            let exited = stop_host_tasks(host);
            return Err(kept(conflict, exited));
        },
    }

    // Recorded only while the app is not quitting; otherwise torn down here.
    commit_host(host)?;

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

/// `error`, saying so when the codex its start launched did not exit (it
/// stays owned, and the next stop or quit tries again).
fn kept(error: anyhow::Error, exited: bool) -> anyhow::Error {
    if exited {
        error
    } else {
        error.context("its codex did not exit; stop the host again")
    }
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
    let mut out = codex_status();
    out.extend(super::serve_acp::status());
    out.extend(super::serve_opencode::status());
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The Codex rows of [`serve_status`], without its relay recovery.
fn codex_status() -> Vec<ServeStatus> {
    let guard = hosts_locked();
    // Hosts whose owned codex did not exit, whether or not their record is
    // still here: listed (and so stoppable again) until it exited.
    let kept = NATIVE.owners(|_| true);
    let stuck = |name: &str| kept.iter().any(|owner| host_names::same(owner, name));
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
            embedded: false,
            codex_binary: ls.codex_binary.clone(),
            proxy: ls.proxy.clone(),
            provider: "codex".to_string(),
            provider_version: None,
            protocol: "codex-app-server".to_string(),
            provider_name: "Codex".to_string(),
            config_source: Some(ls.config.source.as_str().to_string()),
            config_revision: Some(ls.config.revision),
            incarnation: Some(ls.incarnation.clone()),
            busy: host_lease::holder(&ls.name).map(|op| op.label().to_string()),
            agent_error: stuck(&ls.name).then(|| NOT_EXITED.to_string()),
            ..ServeStatus::default()
        })
        .collect();
    out.extend(
        kept.iter()
            .filter(|owner| !guard.keys().any(|name| host_names::same(name, owner)))
            .map(|owner| ServeStatus {
                name: owner.clone(),
                // As its start named it, so it is listed under this device.
                device: runtime::device_id(),
                provider: "codex".to_string(),
                protocol: "codex-app-server".to_string(),
                provider_name: "Codex".to_string(),
                busy: host_lease::holder(owner).map(|op| op.label().to_string()),
                agent_error: Some(NOT_EXITED.to_string()),
                ..ServeStatus::default()
            }),
    );
    drop(guard);
    out
}

/// Whether `name` is a Codex host of this process, including one whose
/// owned codex was not seen to exit (its record may be gone): the name is
/// not free for another provider until it exited.
pub(super) fn is_hosting_codex(name: &str) -> bool {
    let hosts = hosts_locked();
    hosts.contains_key(name)
        || !NATIVE
            .owners(|owner| host_names::same(owner, name))
            .is_empty()
}

/// Resolve this process's own session (app-server or ACP
/// gateway) and meta listeners without relay probes.
pub(super) fn local_endpoints(service_key: &str) -> Option<(String, String)> {
    let codex = hosts_locked()
        .values()
        .find(|host| host.app_key == service_key)
        .map(|host| (host.app_local.to_string(), host.meta_local.to_string()));
    codex
        .or_else(|| super::serve_acp::local_endpoints(service_key))
        .or_else(|| super::serve_opencode::local_endpoints(service_key))
}

/// `true` if something is accepting TCP on a `host:port` listen address.
pub(super) fn listen_addr_open(listen_addr: &str) -> bool {
    match listen_addr.rsplit_once(':') {
        Some((host, port)) => port
            .parse::<u16>()
            .map(|p| tcp_port_open(host, p))
            .unwrap_or(false),
        None => false,
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

#[cfg(test)]
#[path = "serve_tests.rs"]
mod tests;
