//! Host an **attached** OpenCode server from the app: publish the user's own
//! OpenCode background service on the relay as `opencode:<name>` (through a
//! loopback, credential-injecting gateway) plus a provider-neutral
//! `meta:<name>` service (host files, uploads, project folders).
//!
//! Attached hosting never owns the OpenCode process: when no service is
//! running it asks OpenCode to start its own background service
//! (`opencode service start`), and stopping hosting only withdraws what this
//! app published. The Basic password stays inside this process.
//!
//! Instance names share one namespace with Codex hosts on this device, because
//! both derive `meta:<name>` from it.

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use once_cell::sync::OnceCell;
use pocket_codex_core::service::{default_device_id, ServiceId, ServiceKind};
use pocket_codex_host_svc::{
    file_links::SessionDirResolver,
    opencode::{discovery, gateway, Error as OcError, SessionDirs},
};
use pocket_codex_pb::Published;
use tokio::task::JoinHandle;

use super::{
    config::load_config,
    runtime,
    serve::{self, ServeStatus},
    transport,
};

/// Default instance name for OpenCode hosts (distinct from Codex's `default`).
pub const DEFAULT_OPENCODE_NAME: &str = "opencode";
/// How often the attached server is probed.
const HEALTH_INTERVAL: Duration = Duration::from_secs(15);
/// How long `opencode service start` may take before we re-probe anyway.
const SERVICE_START_TIMEOUT: Duration = Duration::from_secs(20);

struct OpenCodeServe {
    device: String,
    name: String,
    key: String,
    gateway_local: SocketAddr,
    gateway_task: JoinHandle<()>,
    register: Option<Published>,
    meta_key: String,
    meta_local: SocketAddr,
    meta_task: JoinHandle<()>,
    meta_register: Option<Published>,
    watchdog: JoinHandle<()>,
    alive: Arc<AtomicBool>,
    version: Arc<Mutex<String>>,
    verified: Arc<AtomicBool>,
    binary: Option<String>,
}

/// Result of [`start`], surfaced to the UI.
#[derive(Debug, Clone)]
pub struct OpenCodeServeReport {
    /// Device id the services were registered under.
    pub device: String,
    /// Instance name.
    pub name: String,
    /// `pcx:<device>:opencode:<name>`.
    pub service_key: String,
    /// Loopback gateway address.
    pub listen_addr: String,
    /// `pcx:<device>:meta:<name>`.
    pub meta_service_key: String,
    /// OpenCode version.
    pub version: String,
    /// Whether that version is the verified one.
    pub verified: bool,
    /// Whether an existing host was reused.
    pub reused: bool,
    /// Whether this call asked OpenCode to start its background service.
    pub started_service: bool,
}

fn hosts() -> std::sync::MutexGuard<'static, HashMap<String, OpenCodeServe>> {
    static HOSTS: OnceCell<Mutex<HashMap<String, OpenCodeServe>>> = OnceCell::new();
    HOSTS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

/// Whether `name` is an OpenCode host of this process.
pub fn is_hosting(name: &str) -> bool {
    hosts().contains_key(name)
}

/// Resolve the `opencode` executable: explicit path → `PATH` → the official
/// installer location `~/.opencode/bin/opencode`.
pub fn locate(binary_override: Option<&str>) -> Option<PathBuf> {
    if let Some(explicit) = binary_override.map(str::trim).filter(|s| !s.is_empty()) {
        let path = PathBuf::from(explicit);
        return path.is_file().then_some(path);
    }
    let exe = if cfg!(windows) { "opencode.exe" } else { "opencode" };
    let from_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(exe))
            .find(|candidate| candidate.is_file())
    });
    from_path.or_else(|| {
        let home = std::env::var_os("HOME")?;
        let installed = PathBuf::from(home).join(".opencode/bin").join(exe);
        installed.is_file().then_some(installed)
    })
}

/// Host the local OpenCode service under `name`.
pub fn start(name: Option<String>, binary_override: Option<String>) -> Result<OpenCodeServeReport> {
    let support = runtime::support_dir()?;
    let config = load_config(&support)?;
    if config.account_token().is_none() {
        bail!("sign in with GitHub before hosting a local OpenCode service");
    }
    let name = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| DEFAULT_OPENCODE_NAME.to_string());
    let name = pocket_codex_core::service::sanitize_component(&name);
    if serve::is_hosting_codex(&name) {
        bail!("`{name}` is already hosting Codex on this device; choose another name");
    }
    if super::serve_acp::is_hosting(&name) {
        bail!("`{name}` is already hosting an ACP agent on this device; choose another name");
    }
    if let Some(report) = reuse(&name) {
        return Ok(report);
    }

    let (attached, started_service) = attach_or_start(binary_override.as_deref())?;
    let binary = locate(binary_override.as_deref()).map(|p| p.display().to_string());
    let device = default_device_id();
    let key = ServiceId::new(&device, ServiceKind::OpenCode, &name).key();
    let meta_key = ServiceId::new(&device, ServiceKind::Meta, &name).key();

    let gateway = gateway::Gateway::new(attached.upstream.clone())
        .map_err(|e| anyhow!("preparing the OpenCode gateway: {e}"))?;
    let (gateway_std, gateway_local) = serve::bind_loopback("OpenCode gateway")?;
    let (meta_std, meta_local) = serve::bind_loopback("meta service")?;
    let store = serve::config_store()?;
    let host_store = serve::host_store()?;
    let uploads = pocket_codex_core::paths::state_dir()
        .context("resolving the state directory")?
        .join("opencode-uploads");

    let session_dirs = SessionDirs::new(attached.client.clone());

    let rt = runtime::runtime();
    let gateway_task = {
        let gateway = gateway.clone();
        rt.spawn(serve::supervise(
            "the OpenCode gateway",
            gateway_local,
            gateway_std,
            move |listener| {
                let gateway = gateway.clone();
                async move { gateway::serve(listener, gateway).await }
            },
        ))
    };
    let meta_task = {
        let session_dirs = session_dirs.clone();
        rt.spawn(serve::supervise(
            "the OpenCode meta service",
            meta_local,
            meta_std,
            move |listener| {
                let (store, host_store, uploads) =
                    (store.clone(), host_store.clone(), uploads.clone());
                let session_dirs: Arc<dyn SessionDirResolver> = Arc::new(session_dirs.clone());
                async move {
                    pocket_codex_host_svc::serve_generic(
                        listener,
                        store,
                        host_store,
                        uploads,
                        session_dirs,
                    )
                    .await
                }
            },
        ))
    };
    let alive = Arc::new(AtomicBool::new(true));
    let version = Arc::new(Mutex::new(attached.info.version.clone()));
    let verified = Arc::new(AtomicBool::new(attached.verified));
    let watchdog = rt.spawn(health_watchdog(
        gateway.clone(),
        session_dirs,
        attached.clone(),
        alive.clone(),
        version.clone(),
        verified.clone(),
    ));

    let abort = |tasks: [&JoinHandle<()>; 3]| tasks.iter().for_each(|t| t.abort());
    let transport = match transport::resolve_blocking() {
        Ok(transport) => transport,
        Err(e) => {
            abort([&gateway_task, &meta_task, &watchdog]);
            return Err(e);
        },
    };
    let meta_register =
        serve::register_service(&transport, &device, ServiceKind::Meta, &name, meta_local)
            .ok()
            .flatten();
    let register = match serve::register_service(
        &transport,
        &device,
        ServiceKind::OpenCode,
        &name,
        gateway_local,
    ) {
        Ok(register) => register,
        Err(e) => {
            abort([&gateway_task, &meta_task, &watchdog]);
            drop(meta_register);
            return Err(e);
        },
    };
    let report = OpenCodeServeReport {
        device: device.clone(),
        name: name.clone(),
        service_key: key.clone(),
        listen_addr: gateway_local.to_string(),
        meta_service_key: meta_key.clone(),
        version: attached.info.version.clone(),
        verified: attached.verified,
        reused: false,
        started_service,
    };
    hosts().insert(name.clone(), OpenCodeServe {
        device,
        name,
        key,
        gateway_local,
        gateway_task,
        register,
        meta_key,
        meta_local,
        meta_task,
        meta_register,
        watchdog,
        alive,
        version,
        verified,
        binary,
    });
    Ok(report)
}

fn reuse(name: &str) -> Option<OpenCodeServeReport> {
    let report = {
        let guard = hosts();
        let host = guard.get(name)?;
        OpenCodeServeReport {
            device: host.device.clone(),
            name: host.name.clone(),
            service_key: host.key.clone(),
            listen_addr: host.gateway_local.to_string(),
            meta_service_key: host.meta_key.clone(),
            version: host.version.lock().map(|v| v.clone()).unwrap_or_default(),
            verified: host.verified.load(Ordering::Relaxed),
            reused: true,
            started_service: false,
        }
    };
    // Re-publish anything that dropped; a conflict surfaces on the next status.
    let _ = reregister(name, "opencode");
    let _ = reregister(name, "meta");
    Some(report)
}

/// Attach to the running service, or ask OpenCode to start its own background
/// service once and attach to that.
fn attach_or_start(binary_override: Option<&str>) -> Result<(discovery::Attached, bool)> {
    let rt = runtime::runtime();
    match rt.block_on(discovery::discover()) {
        Ok(attached) => return Ok((attached, false)),
        Err(OcError::NotRunning) => {},
        Err(e) => return Err(describe(e)),
    }
    let binary = locate(binary_override).ok_or_else(|| {
        anyhow!(
            "OpenCode is not running and the `opencode` executable was not found; install \
             OpenCode or choose its path"
        )
    })?;
    tracing::info!(binary = %binary.display(), "starting the OpenCode background service");
    let mut child = std::process::Command::new(&binary)
        .args(["service", "start"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("running `{} service start`", binary.display()))?;
    let deadline = std::time::Instant::now() + SERVICE_START_TIMEOUT;
    loop {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        if std::time::Instant::now() >= deadline {
            // The CLI may keep running; the service it launched is OpenCode's own.
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        match rt.block_on(discovery::discover()) {
            Ok(attached) => return Ok((attached, true)),
            Err(OcError::NotRunning) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(500));
            },
            Err(e) => return Err(describe(e)),
        }
    }
}

fn describe(error: OcError) -> anyhow::Error {
    match error {
        OcError::NotRunning => {
            anyhow!("no running OpenCode service found (try `opencode service start`)")
        },
        OcError::InvalidInput => anyhow!(
            "the OpenCode service registration is not usable (it must be a private file of this \
             user with a loopback address)"
        ),
        other => anyhow!("attaching to OpenCode failed: {other}"),
    }
}

/// Probe the attached server; when it stops answering or restarted (new pid,
/// port or password), re-read its registration and repoint the gateway and the
/// meta service's session-directory lookups.
async fn health_watchdog(
    gateway: gateway::Gateway,
    session_dirs: SessionDirs,
    mut current: discovery::Attached,
    alive: Arc<AtomicBool>,
    version: Arc<Mutex<String>>,
    verified: Arc<AtomicBool>,
) {
    loop {
        tokio::time::sleep(HEALTH_INTERVAL).await;
        let healthy = matches!(
            current.client.info().await,
            Ok(info) if info.pid == current.info.pid
        );
        if healthy {
            alive.store(true, Ordering::Relaxed);
            continue;
        }
        match discovery::discover().await {
            Ok(attached) => {
                tracing::info!("OpenCode service changed; repointing the gateway");
                gateway.set_upstream(attached.upstream.clone());
                session_dirs.set_client(attached.client.clone());
                if let Ok(mut v) = version.lock() {
                    v.clone_from(&attached.info.version);
                }
                verified.store(attached.verified, Ordering::Relaxed);
                alive.store(true, Ordering::Relaxed);
                current = attached;
            },
            Err(e) => {
                tracing::debug!(error = %e, "OpenCode health probe failed");
                alive.store(false, Ordering::Relaxed);
            },
        }
    }
}

/// Status rows for every OpenCode host, in the shared [`ServeStatus`] shape.
pub(super) fn status() -> Vec<ServeStatus> {
    hosts()
        .values()
        .map(|host| ServeStatus {
            name: host.name.clone(),
            device: host.device.clone(),
            pid: None,
            alive: host.alive.load(Ordering::Relaxed),
            app_listen_addr: host.gateway_local.to_string(),
            app_service_key: host.key.clone(),
            app_registered: serve::is_published(&host.register),
            api_listen_addr: String::new(),
            api_service_key: String::new(),
            api_registered: false,
            meta_listen_addr: host.meta_local.to_string(),
            meta_service_key: host.meta_key.clone(),
            meta_registered: serve::is_published(&host.meta_register),
            embedded: false,
            codex_binary: host.binary.clone(),
            proxy: None,
            provider: "opencode".to_string(),
            provider_version: host.version.lock().ok().map(|v| v.clone()),
            provider_verified: host.verified.load(Ordering::Relaxed),
            agent_id: None,
            agent_name: None,
        })
        .collect()
}

/// Loopback gateway + meta addresses for a locally hosted OpenCode key.
pub(super) fn local_endpoints(service_key: &str) -> Option<(String, String)> {
    hosts()
        .values()
        .find(|host| host.key == service_key)
        .map(|host| (host.gateway_local.to_string(), host.meta_local.to_string()))
}

fn slot(host: &mut OpenCodeServe, kind: ServiceKind) -> Result<&mut Option<Published>> {
    match kind {
        ServiceKind::OpenCode => Ok(&mut host.register),
        ServiceKind::Meta => Ok(&mut host.meta_register),
        other => bail!("an OpenCode host has no `{other}` service"),
    }
}

/// Unpublish one service (`opencode` / `meta`) without stopping the host.
pub(super) fn deregister(name: &str, kind: &str) -> Result<()> {
    let kind: ServiceKind = kind
        .parse()
        .map_err(|_| anyhow!("invalid service kind `{kind}`"))?;
    let published = {
        let mut guard = hosts();
        let host = guard
            .get_mut(name)
            .ok_or_else(|| anyhow!("`{name}` is not hosting locally"))?;
        slot(host, kind)?.take()
    };
    if let Some(published) = published {
        if let Err(e) = runtime::runtime().block_on(published.stop()) {
            tracing::warn!(error = %format!("{e:#}"), "releasing the relay key failed");
        }
    }
    Ok(())
}

/// Re-publish one service (`opencode` / `meta`) of a running host.
pub(super) fn reregister(name: &str, kind: &str) -> Result<()> {
    let kind: ServiceKind = kind
        .parse()
        .map_err(|_| anyhow!("invalid service kind `{kind}`"))?;
    let Some((device, local)) = ({
        let mut guard = hosts();
        let host = guard
            .get_mut(name)
            .ok_or_else(|| anyhow!("`{name}` is not hosting locally"))?;
        let local = if kind == ServiceKind::Meta { host.meta_local } else { host.gateway_local };
        let device = host.device.clone();
        serve::needs_publishing(slot(host, kind)?).then_some((device, local))
    }) else {
        return Ok(());
    };
    let transport = transport::resolve_blocking()?;
    let published = serve::register_service(&transport, &device, kind, name, local)?;
    let mut guard = hosts();
    match guard.get_mut(name) {
        Some(host) => *slot(host, kind)? = published,
        None => drop(published),
    }
    Ok(())
}

/// Stop hosting `name`: withdraw its relay keys and stop the gateway, meta
/// service and watchdog. OpenCode itself keeps running.
pub(super) fn stop(name: &str) {
    let removed = hosts().remove(name);
    if let Some(host) = removed {
        stop_tasks(host);
    }
}

/// Stop every OpenCode host.
pub(super) fn stop_all() {
    let all: Vec<OpenCodeServe> = hosts().drain().map(|(_, host)| host).collect();
    for host in all {
        stop_tasks(host);
    }
}

fn stop_tasks(host: OpenCodeServe) {
    runtime::runtime().block_on(async {
        for published in [host.register, host.meta_register].into_iter().flatten() {
            if let Err(e) = published.stop().await {
                tracing::warn!(error = %format!("{e:#}"), service = %host.name, "releasing a relay key failed");
            }
        }
    });
    host.watchdog.abort();
    host.gateway_task.abort();
    host.meta_task.abort();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locate_prefers_an_explicit_existing_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let exe = dir.path().join("opencode");
        std::fs::write(&exe, b"").expect("write");
        assert_eq!(locate(Some(exe.to_str().expect("utf8"))), Some(exe));
        assert_eq!(locate(Some("/definitely/not/here/opencode")), None);
    }

    #[test]
    fn unknown_hosts_are_not_hosting() {
        assert!(!is_hosting("no-such-opencode-host"));
        assert!(local_endpoints("pcx:dev:opencode:no-such").is_none());
        assert!(deregister("no-such-opencode-host", "opencode").is_err());
    }
}
