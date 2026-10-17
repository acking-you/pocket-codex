//! Owned native OpenCode hosting, independent of ACP and Codex. Every start,
//! stop and apply holds the shared instance lease. Records and name claims
//! survive a failed stop; only confirmed exit permits a replacement.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, ensure, Context, Result};
use once_cell::sync::Lazy;
use pocket_codex_core::service::{ServiceId, ServiceKind};
use pocket_codex_host_svc::{
    file_links::SessionDirResolver,
    opencode::{
        self,
        catalog::Catalog,
        host::Host,
        runtime::{Launch, Server},
    },
    process,
};
use pocket_codex_pb::Published;
use tokio::task::JoinHandle;

use super::{
    host_lease, host_profiles, host_services, hosting, runtime, serve::ServeStatus, transport,
};

#[path = "serve_opencode_registration.rs"]
mod registration;
pub(super) use registration::{deregister, reregister};

#[derive(Default)]
struct Registry {
    closing: bool,
    hosts: HashMap<String, Serving>,
}
static HOSTS: Lazy<Mutex<Registry>> = Lazy::new(Mutex::default);
fn hosts() -> MutexGuard<'static, Registry> {
    HOSTS.lock().unwrap_or_else(|error| error.into_inner())
}

#[derive(Default)]
struct Slot {
    published: Option<Published>,
    revision: u64,
}
struct Serving {
    prepared: Prepared,
    host: Host,
    pid: u32,
    device: String,
    key: String,
    meta_key: String,
    local: SocketAddr,
    meta: SocketAddr,
    tasks: [JoinHandle<()>; 2],
    register: Slot,
    meta_register: Slot,
    context: transport::Context,
    status: OpenCodeStatus,
    stopped: bool,
}

/// Public startup/catalog status without native credentials or private URL.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeStatus {
    /// Installed executable version.
    pub version: String,
    /// Number of connected native providers.
    pub providers_connected: u32,
    /// Number of usable native models.
    pub models: u32,
    /// Captured custom default, or an authoritative native global selection.
    pub default_model: Option<String>,
    /// Visible primary agents.
    pub agents: Vec<String>,
    /// Safe catalog or lifecycle failure, if any.
    pub problem: Option<String>,
    /// The host keeps its sessions in a database of its own, not in
    /// OpenCode's default one.
    #[serde(default)]
    pub separate_history: bool,
}
impl OpenCodeStatus {
    fn new(
        version: String,
        catalog: Result<Catalog, opencode::host::Error>,
        default_model: Option<String>,
    ) -> Self {
        let mut out = Self {
            version,
            providers_connected: 0,
            models: 0,
            default_model,
            agents: Vec::new(),
            problem: None,
            separate_history: false,
        };
        match catalog {
            Ok(catalog) => {
                out.providers_connected =
                    u32::try_from(catalog.providers_connected).unwrap_or(u32::MAX);
                out.models = u32::try_from(catalog.models.len()).unwrap_or(u32::MAX);
                out.agents = catalog.agents.into_iter().map(|agent| agent.name).collect();
            },
            Err(error) => out.problem = Some(error.to_string()),
        }
        out
    }
}

/// Native host's gateway and meta service, never its private server endpoint.
#[derive(Debug, Clone)]
pub struct Report {
    /// Canonical instance name.
    pub name: String,
    /// Logical `opencode` service key.
    pub service_key: String,
}
fn report(host: &Serving) -> Report {
    Report {
        name: host.prepared.name.clone(),
        service_key: host.key.clone(),
    }
}

/// Exact resolved launch, retained for apply rollback.
#[derive(Clone)]
pub(super) struct Prepared {
    name: String,
    launch: Launch,
    discovery_problem: Option<String>,
    pub(super) provider: host_profiles::Resolved,
    /// Launched with its own database ([`history_database`]).
    pub(super) separate_history: bool,
}
/// Running host identity and exact launch.
pub(super) struct Relaunch {
    pub(super) prepared: Prepared,
    pub(super) incarnation: String,
}
impl Prepared {
    fn default_model(&self) -> Option<String> {
        self.provider
            .endpoint
            .as_ref()
            .and_then(|endpoint| endpoint.model.clone())
    }

    pub(super) fn binary(&self) -> String {
        self.launch.program.display().to_string()
    }
}

fn launch_for(binary: Option<&str>) -> Result<Launch> {
    ensure!(
        process::hosting_supported(),
        "native OpenCode hosting is unavailable on this platform"
    );
    let path = pocket_codex_codex::external_tool_path();
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let program = pocket_codex_host_svc::acp::spec::resolve_program(
        binary
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("opencode"),
        path.as_ref(),
        home.as_deref(),
        &[".opencode/bin"],
    )
    .map_err(|_| anyhow!("OpenCode was not found; install it or select its executable"))?;
    Ok(Launch {
        program,
        path,
        env: Vec::new(),
    })
}

/// The database file of a host that keeps its own history: in OpenCode's
/// data directory, beside its default database, named after the canonical
/// instance (so one instance keeps its history across stops and restarts).
/// `OPENCODE_DB` resolves a bare file name against that directory.
fn history_database(name: &str) -> Result<String> {
    let name = super::host_names::canonical(name)?;
    // A file name, whatever the instance name's length.
    Ok(if name.len() <= 96 {
        format!("pocket-codex-{name}.db")
    } else {
        let digest = pocket_codex_core::history_sync::digest_bytes(name.as_bytes());
        format!("pocket-codex-{}.db", &digest[..32])
    })
}

/// The running host that serves OpenCode's default database, if any: a
/// second host on it would be refused, so it keeps a history of its own.
pub(super) fn shared_history_host() -> Option<String> {
    hosts()
        .hosts
        .values()
        .find(|host| !host.prepared.separate_history)
        .map(|host| host.prepared.name.clone())
}

/// Resolve and inspect the installed external executable without starting a
/// host.
pub fn detect(binary: Option<&str>) -> Result<(String, String)> {
    let launch = launch_for(binary)?;
    let detection = runtime::runtime().block_on(launch.detect())?;
    Ok((launch.program.display().to_string(), detection.version))
}

pub(super) fn prepare(
    name: &str,
    binary: Option<&str>,
    separate_history: bool,
    provider: host_profiles::Resolved,
) -> Result<Prepared> {
    host_profiles::Engine::OpenCode
        .allows(provider.source, provider.endpoint.as_ref().map(|e| e.protocol))?;
    ensure!(
        provider.claude_env.is_empty(),
        "OpenCode uses its own native environment configuration"
    );
    let mut launch = launch_for(binary)?;
    let (environment, discovery_problem) =
        runtime::runtime().block_on(host_profiles::opencode::discover_environment(
            &provider,
            name,
            std::env::var_os("OPENCODE_CONFIG_CONTENT").as_deref(),
        ))?;
    launch.env = environment;
    if separate_history {
        // Detection, the source claim and the server all use this database.
        launch
            .env
            .push(("OPENCODE_DB".into(), history_database(name)?.into()));
    }
    runtime::runtime().block_on(launch.detect())?;
    Ok(Prepared {
        name: name.into(),
        launch,
        discovery_problem,
        provider,
        separate_history,
    })
}

pub(super) fn start_locked(
    lease: &host_lease::Lease,
    binary: Option<&str>,
    separate_history: bool,
    provider: host_profiles::Resolved,
) -> Result<Report> {
    launch_prepared(prepare(lease.name(), binary, separate_history, provider)?)
}

pub(super) fn launch_prepared(prepared: Prepared) -> Result<Report> {
    let reservation = hosting::reserve_for(&prepared.name, hosting::Provider::OpenCode)?;
    ensure!(!hosts().closing, "the app is quitting");
    // Fallible local/transport setup precedes the owned process.
    let owned = transport::resolve_owned_blocking()?;
    let config = host_services::config_store()?;
    let store = host_services::host_store()?;
    let uploads = pocket_codex_core::paths::state_dir()?.join("opencode-uploads");
    let (gateway, local) = host_services::bind_loopback("OpenCode gateway")?;
    let (meta_listener, meta) = host_services::bind_loopback("OpenCode meta")?;
    let rt = runtime::runtime();
    let (host, pid) = rt.block_on(async {
        let server = Server::start(&prepared.launch).await?;
        let pid = server.pid;
        Ok::<_, anyhow::Error>((Host::new(server).await?, pid))
    })?;
    let mut status = OpenCodeStatus::new(
        host.info().version,
        rt.block_on(host.catalog(None)),
        prepared.default_model(),
    );
    status.problem = status
        .problem
        .or_else(|| prepared.discovery_problem.clone());
    let gateway_host = host.clone();
    let gateway_task =
        rt.spawn(host_services::supervise("OpenCode gateway", local, gateway, move |listener| {
            let host = gateway_host.clone();
            async move { opencode::api::serve(listener, host).await }
        }));
    let meta_host = host.clone();
    let meta_task =
        rt.spawn(host_services::supervise("OpenCode meta", meta, meta_listener, move |listener| {
            let (config, store, uploads) = (config.clone(), store.clone(), uploads.clone());
            let dirs: Arc<dyn SessionDirResolver> = Arc::new(meta_host.clone());
            async move {
                pocket_codex_host_svc::serve_generic(listener, config, store, uploads, dirs).await
            }
        }));
    let name = prepared.name.clone();
    let device = runtime::device_id();
    let serving = Serving {
        key: ServiceId::new(&device, ServiceKind::OpenCode, &name).key(),
        meta_key: ServiceId::new(&device, ServiceKind::Meta, &name).key(),
        prepared,
        host,
        pid,
        device,
        local,
        meta,
        tasks: [gateway_task, meta_task],
        register: Slot::default(),
        meta_register: Slot::default(),
        context: transport::Context::of(&owned.transport),
        status,
        stopped: false,
    };
    let out = report(&serving);
    let closing = {
        let mut registry = hosts();
        let closing = registry.closing;
        registry.hosts.insert(name.clone(), serving);
        closing
    };
    // From here a failed stop retains both the record and this name.
    reservation.commit();
    let publish = if closing || !transport::is_current(owned.epoch) {
        Err(anyhow!("the app is quitting or its account/relay changed during OpenCode startup"))
    } else {
        reregister(&name, "opencode").and_then(|()| reregister(&name, "meta"))
    }
    .and_then(|()| {
        ensure!(
            transport::is_current(owned.epoch) && !hosts().closing,
            "the app is quitting or its account/relay changed during OpenCode startup"
        );
        Ok(())
    });
    if let Err(error) = publish {
        stop(&name)
            .context("OpenCode startup cleanup did not confirm exit; stop this host again")?;
        return Err(error);
    }
    Ok(out)
}

pub(super) fn is_hosting(name: &str) -> bool {
    hosts().hosts.contains_key(name)
}
pub(super) fn relaunch(name: &str) -> Option<Relaunch> {
    hosts().hosts.get(name).map(|host| Relaunch {
        prepared: host.prepared.clone(),
        incarnation: host.host.info().identity.host_id,
    })
}

/// Under the instance lease; retain ownership and listeners if exit is
/// uncertain.
pub(super) fn retire(name: &str) -> Result<Option<Prepared>> {
    let Some(host) = ({
        let mut registry = hosts();
        registry.hosts.get_mut(name).map(|host| {
            host.stopped = true;
            host.host.clone()
        })
    }) else {
        return Ok(None);
    };
    if let Err(error) = runtime::runtime().block_on(host.stop()) {
        if let Some(host) = hosts().hosts.get_mut(name) {
            host.status.problem = Some(error.to_string());
        }
        return Err(error);
    }
    let removed = hosts().hosts.remove(name);
    let Some(host) = removed else { return Ok(None) };
    runtime::runtime().block_on(async {
        for task in host.tasks {
            task.abort();
            let _ = task.await;
        }
        let withdrawals = [host.register.published, host.meta_register.published]
            .into_iter()
            .flatten()
            .map(Published::stop);
        let _ =
            tokio::time::timeout(Duration::from_secs(2), futures::future::join_all(withdrawals))
                .await;
    });
    hosting::release_for(name, hosting::Provider::OpenCode);
    Ok(Some(host.prepared))
}
pub(super) fn stop(name: &str) -> Result<()> {
    retire(name).map(drop)
}
pub(super) fn close_admission() {
    hosts().closing = true;
}
pub(super) fn stop_all(drained: bool) -> Vec<String> {
    close_admission();
    let names: Vec<_> = hosts().hosts.keys().cloned().collect();
    let mut left = Vec::new();
    for name in names {
        let wait = if drained { host_lease::LEASE_WAIT } else { Duration::ZERO };
        let result = host_lease::acquire_for_quit(&name, wait).and_then(|_lease| stop(&name));
        if let Err(error) = result {
            left.push(format!("OpenCode `{name}`: {error}"));
        }
    }
    if opencode::runtime::pending_cleanup() != 0 {
        left.push("an OpenCode startup is still stopping".into());
    }
    left
}

/// Refresh the native catalog for one exact running host. Requests and native
/// configuration reads happen outside the registry lock; a late result never
/// changes a replacement host or erases its stop failure.
pub fn refresh_catalog(name: &str) -> Result<()> {
    let name = super::host_names::canonical(name)?;
    let (host, default_model) = hosts()
        .hosts
        .get(&name)
        .filter(|host| !host.stopped)
        .map(|host| (host.host.clone(), host.prepared.default_model()))
        .ok_or_else(|| anyhow!("OpenCode host is not running"))?;
    let identity = host.info().identity;
    let status = OpenCodeStatus::new(
        host.info().version,
        runtime::runtime().block_on(host.catalog(None)),
        default_model,
    );
    let mut registry = hosts();
    let current = registry
        .hosts
        .get_mut(&name)
        .filter(|host| !host.stopped && host.host.info().identity == identity)
        .ok_or_else(|| anyhow!("OpenCode host changed during refresh"))?;
    current.status = status;
    current.status.problem = current
        .status
        .problem
        .take()
        .or_else(|| current.prepared.discovery_problem.clone());
    Ok(())
}

pub(super) fn status() -> Vec<ServeStatus> {
    registration::recover();
    hosts()
        .hosts
        .values()
        .map(|host| ServeStatus {
            name: host.prepared.name.clone(),
            device: host.device.clone(),
            pid: Some(host.pid),
            alive: !host.stopped
                && !host.tasks[0].is_finished()
                && pocket_codex_core::process::pid_running(host.pid),
            app_listen_addr: host.local.to_string(),
            app_service_key: host.key.clone(),
            app_registered: host_services::is_published(&host.register.published),
            meta_listen_addr: host.meta.to_string(),
            meta_service_key: host.meta_key.clone(),
            meta_registered: host_services::is_published(&host.meta_register.published),
            codex_binary: Some(host.prepared.binary()),
            provider: "opencode".into(),
            provider_version: Some(host.status.version.clone()),
            protocol: "opencode-http".into(),
            provider_name: "OpenCode".into(),
            config_source: Some("inherited".into()),
            config_revision: Some(host.prepared.provider.revision),
            incarnation: Some(host.host.info().identity.host_id),
            busy: host_lease::holder(&host.prepared.name).map(|op| op.label().into()),
            agent_error: host.status.problem.clone(),
            opencode: Some(OpenCodeStatus {
                separate_history: host.prepared.separate_history,
                ..host.status.clone()
            }),
            ..ServeStatus::default()
        })
        .collect()
}

pub(super) fn local_gateway(logical: &str, context: &transport::Context) -> Option<String> {
    hosts()
        .hosts
        .values()
        .find(|host| host.key == logical && &host.context == context && !host.stopped)
        .map(|host| host.local.to_string())
}
pub(super) fn local_endpoints(service: &str) -> Option<(String, String)> {
    let context = transport::current_context()?;
    if !context.admits(service) {
        return None;
    }
    let logical = super::session_engine::logical_key(service)?;
    hosts()
        .hosts
        .values()
        .find(|host| host.key == logical && host.context == context && !host.stopped)
        .map(|host| (host.local.to_string(), host.meta.to_string()))
}

#[cfg(test)]
#[path = "serve_opencode_tests.rs"]
mod tests;
