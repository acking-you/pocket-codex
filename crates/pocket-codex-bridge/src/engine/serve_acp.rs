//! Host an ACP agent from the app (TRD §4.4.1): run an [`AcpHub`] over the
//! agent process, serve it on loopback, and publish `acp:<name>` plus a
//! `meta:<name>` service on the relay. Owned hosting: stopping hosting ends
//! the agent process.
//!
//! Instance names share one namespace with Codex and OpenCode hosts on this
//! device, because every provider derives `meta:<name>` from it.

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use once_cell::sync::OnceCell;
use pocket_codex_core::{
    acp::pcx::ProcessState,
    service::{default_device_id, sanitize_component, ServiceId, ServiceKind},
};
use pocket_codex_host_svc::{
    acp::{
        install, AcpError, AcpHub, AgentConnector, GatewayAuth, HubOptions, LaunchProvider,
        LaunchSpec, ProcessConnector,
    },
    store::{ConfigStore, HostStore},
};
use pocket_codex_pb::Published;
use tokio::task::JoinHandle;

pub use super::acp::AcpServeReport;
use super::{
    acp_manage,
    acp_terminal::DesktopTerminal,
    config::load_config,
    runtime,
    serve::{self, ServeStatus},
    transport,
};

/// Grace period for running turns when hosting stops.
pub const ACP_STOP_GRACE: Duration = Duration::from_secs(5);

/// Publishes one service on the relay.
pub type RegisterFn =
    Arc<dyn Fn(ServiceKind, &str, SocketAddr) -> Result<Option<Published>> + Send + Sync>;
/// Resolves an agent id to its launch spec.
pub type ResolveFn = Arc<dyn Fn(&str) -> Result<LaunchSpec> + Send + Sync>;
/// The gateway login configured for an agent id.
pub type GatewayFn = Arc<dyn Fn(&str) -> Option<GatewayAuth> + Send + Sync>;
/// Which other provider already hosts a name.
pub type OtherHostingFn = Box<dyn Fn(&str) -> Option<&'static str> + Send + Sync>;

struct AcpServe {
    device: String,
    name: String,
    key: String,
    agent_id: String,
    /// `LaunchSpec.version` (the installed release); used by
    /// `InstallContext.in_use`.
    agent_version: Option<String>,
    register_fn: RegisterFn,
    hub: Arc<AcpHub>,
    ws_local: SocketAddr,
    ws_task: JoinHandle<()>,
    register: Option<Published>,
    meta_key: String,
    meta_local: SocketAddr,
    meta_task: JoinHandle<()>,
    meta_register: Option<Published>,
    after_stop: Option<Arc<dyn Fn() + Send + Sync>>,
}

fn hosts() -> std::sync::MutexGuard<'static, HashMap<String, AcpServe>> {
    static HOSTS: OnceCell<Mutex<HashMap<String, AcpServe>>> = OnceCell::new();
    HOSTS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

/// Whether `name` is an ACP host of this process.
pub fn is_hosting(name: &str) -> bool {
    hosts().contains_key(name)
}

/// The hub of host `name`.
pub fn hub(name: &str) -> Option<Arc<AcpHub>> {
    hosts().get(name).map(|h| h.hub.clone())
}

/// Instance names hosting `agent_id`.
pub fn hosted_names(agent_id: &str) -> Vec<String> {
    hosts()
        .values()
        .filter(|h| h.agent_id == agent_id)
        .map(|h| h.name.clone())
        .collect()
}

/// Whether a host uses (`agent_id`, `version`); an empty version matches any.
pub fn uses(agent_id: &str, version: &str) -> bool {
    hosts().values().any(|h| {
        h.agent_id == agent_id
            && (version.is_empty() || h.agent_version.as_deref() == Some(version))
    })
}

/// Injected dependencies (T19). `start` = `start_with(..,
/// StartDeps::production())`.
pub struct StartDeps {
    /// Device id.
    pub device: String,
    /// Production: require a signed-in account (same check as OpenCode).
    pub require_account: bool,
    /// Starts the agent.
    pub connector: Arc<dyn AgentConnector>,
    /// Publishes a service (production resolves the transport lazily).
    pub register: RegisterFn,
    /// Production: `install::resolve_launch`; tests return a fixed spec.
    pub resolve: ResolveFn,
    /// Production: the agent's gateway in `agents.toml`.
    pub gateway: GatewayFn,
    /// Name → provider already hosting it (`"Codex"` / `"OpenCode"`).
    pub other_hosting: OtherHostingFn,
    /// Meta stores.
    pub stores: (Arc<ConfigStore>, Arc<HostStore>),
    /// Production: `paths::state_dir()?` (logs, acp/uploads, acp/run,
    /// acp/sessions).
    pub state_dir: PathBuf,
    /// Production: `install::gc_sweep` once the agent process has stopped.
    pub after_stop: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl StartDeps {
    /// Fails when the stores or the state directory cannot be opened.
    pub fn production() -> Result<Self> {
        let device = default_device_id();
        let register_device = device.clone();
        Ok(Self {
            device,
            require_account: true,
            connector: Arc::new(ProcessConnector),
            register: Arc::new(move |kind, name, local| {
                let transport = transport::resolve_blocking()?;
                serve::register_service(&transport, &register_device, kind, name, local)
            }),
            resolve: Arc::new(|agent_id| {
                let ctx = acp_manage::install_context()?;
                install::resolve_launch(agent_id, &ctx).map_err(|e| anyhow!(e))
            }),
            gateway: Arc::new(|agent_id| {
                acp_manage::install_context()
                    .ok()
                    .and_then(|ctx| install::gateway_auth(&ctx, agent_id))
            }),
            other_hosting: Box::new(|name| {
                if serve::is_hosting_codex(name) {
                    Some("Codex")
                } else if super::serve_opencode::is_hosting(name) {
                    Some("OpenCode")
                } else {
                    None
                }
            }),
            stores: (serve::config_store()?, serve::host_store()?),
            state_dir: pocket_codex_core::paths::state_dir()
                .context("resolving the state directory")?,
            after_stop: Some(Arc::new(|| {
                if let Ok(ctx) = acp_manage::install_context() {
                    install::gc_sweep(&ctx);
                }
            })),
        })
    }
}

/// Host `agent_id` under `name` (default: the agent id).
pub fn start(name: Option<String>, agent_id: String) -> Result<AcpServeReport> {
    start_with(name, agent_id, StartDeps::production()?)
}

fn conflict(name: &str, provider: &str) -> anyhow::Error {
    anyhow!("`{name}` is already hosting {provider} on this device; choose another name")
}

fn to_acp(error: anyhow::Error) -> AcpError {
    match error.downcast::<AcpError>() {
        Ok(e) => e,
        Err(e) => AcpError::Internal(format!("{e:#}")),
    }
}

/// [`start`] with injected dependencies.
pub fn start_with(
    name: Option<String>,
    agent_id: String,
    deps: StartDeps,
) -> Result<AcpServeReport> {
    if deps.require_account {
        let config = load_config(&runtime::support_dir()?)?;
        if config.account_token().is_none() {
            bail!("sign in with GitHub before hosting an ACP agent");
        }
    }
    let name = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| agent_id.clone());
    let name = sanitize_component(&name);
    if let Some(provider) = (deps.other_hosting)(&name) {
        return Err(conflict(&name, provider));
    }
    if let Some(report) = reuse(&name, &agent_id)? {
        return Ok(report);
    }
    let spec = (deps.resolve)(&agent_id)?;
    let resolve = deps.resolve.clone();
    let gateway = deps.gateway.clone();
    let launch_id = agent_id.clone();
    let launch: LaunchProvider = Arc::new(move || {
        let spec = resolve(&launch_id).map_err(to_acp)?;
        Ok((spec, gateway(&launch_id)))
    });
    let acp_dir = deps.state_dir.join("acp");
    let options = HubOptions {
        instance: name.clone(),
        launch,
        state_dir: acp_dir.clone(),
        log_file: Some(deps.state_dir.join("logs").join(format!("acp-{name}.log"))),
        connector: deps.connector.clone(),
        terminal: Some(Arc::new(DesktopTerminal)),
    };
    let rt = runtime::runtime();
    let hub = rt.block_on(AcpHub::start(options)).map_err(|e| match &e {
        AcpError::AgentStartFailed {
            stderr_tail, ..
        }
        | AcpError::InitializeTimeout {
            stderr_tail,
        } if !stderr_tail.is_empty() => {
            anyhow!("{e}\n{stderr_tail}")
        },
        _ => anyhow!(e),
    })?;
    let (ws_std, ws_local) = serve::bind_loopback("ACP hub")?;
    let (meta_std, meta_local) = serve::bind_loopback("ACP meta service")?;
    let (store, host_store) = deps.stores.clone();
    let uploads = acp_dir.join("uploads").join(&name);
    let ws_task = {
        let hub = hub.clone();
        rt.spawn(serve::supervise("the ACP hub", ws_local, ws_std, move |listener| {
            let hub = hub.clone();
            async move { pocket_codex_host_svc::acp::serve_ws(listener, hub).await }
        }))
    };
    let meta_task = {
        let hub = hub.clone();
        rt.spawn(serve::supervise("the ACP meta service", meta_local, meta_std, move |listener| {
            let (store, host_store, uploads, hub) =
                (store.clone(), host_store.clone(), uploads.clone(), hub.clone());
            async move {
                pocket_codex_host_svc::acp::serve_meta(listener, store, host_store, uploads, hub)
                    .await
            }
        }))
    };
    let meta_register = (deps.register)(ServiceKind::Meta, &name, meta_local)
        .ok()
        .flatten();
    let register = match (deps.register)(ServiceKind::Acp, &name, ws_local) {
        Ok(register) => register,
        Err(e) => {
            ws_task.abort();
            meta_task.abort();
            drop(meta_register);
            rt.block_on(hub.shutdown(ACP_STOP_GRACE));
            return Err(e);
        },
    };
    let info = hub.info();
    let key = ServiceId::new(&deps.device, ServiceKind::Acp, &name).key();
    let meta_key = ServiceId::new(&deps.device, ServiceKind::Meta, &name).key();
    let report = AcpServeReport {
        device: deps.device.clone(),
        name: name.clone(),
        service_key: key.clone(),
        listen_addr: ws_local.to_string(),
        meta_service_key: meta_key.clone(),
        agent_id: agent_id.clone(),
        agent_name: info.agent_name.clone(),
        agent_version: spec.version.clone().unwrap_or_default(),
        auth: info.auth.clone(),
        reused: false,
    };
    hosts().insert(name.clone(), AcpServe {
        device: deps.device,
        name,
        key,
        agent_id,
        agent_version: spec.version,
        register_fn: deps.register,
        hub,
        ws_local,
        ws_task,
        register,
        meta_key,
        meta_local,
        meta_task,
        meta_register,
        after_stop: deps.after_stop,
    });
    Ok(report)
}

/// Reuse a host of the same agent (re-publishing dropped services); refuse a
/// different agent under the same name.
fn reuse(name: &str, agent_id: &str) -> Result<Option<AcpServeReport>> {
    let report = {
        let guard = hosts();
        let Some(host) = guard.get(name) else { return Ok(None) };
        if host.agent_id != agent_id {
            let other = host.hub.info().agent_name;
            return Err(conflict(name, &format!("{other} (ACP)")));
        }
        let info = host.hub.info();
        AcpServeReport {
            device: host.device.clone(),
            name: host.name.clone(),
            service_key: host.key.clone(),
            listen_addr: host.ws_local.to_string(),
            meta_service_key: host.meta_key.clone(),
            agent_id: host.agent_id.clone(),
            agent_name: info.agent_name,
            agent_version: host.agent_version.clone().unwrap_or_default(),
            auth: info.auth,
            reused: true,
        }
    };
    let _ = reregister(name, "acp");
    let _ = reregister(name, "meta");
    Ok(Some(report))
}

/// Status rows for every ACP host.
pub fn status() -> Vec<ServeStatus> {
    hosts()
        .values()
        .map(|host| {
            let info = host.hub.info();
            ServeStatus {
                name: host.name.clone(),
                device: host.device.clone(),
                pid: info.pid,
                alive: info.process == ProcessState::Ready,
                app_listen_addr: host.ws_local.to_string(),
                app_service_key: host.key.clone(),
                app_registered: serve::is_published(&host.register),
                api_listen_addr: String::new(),
                api_service_key: String::new(),
                api_registered: false,
                meta_listen_addr: host.meta_local.to_string(),
                meta_service_key: host.meta_key.clone(),
                meta_registered: serve::is_published(&host.meta_register),
                embedded: false,
                codex_binary: Some(info.program.display().to_string()),
                proxy: None,
                provider: "acp".to_string(),
                provider_version: info.agent_version.clone(),
                provider_verified: info.pinned,
                agent_id: Some(host.agent_id.clone()),
                agent_name: Some(info.agent_name),
            }
        })
        .collect()
}

/// Loopback hub + meta addresses for a locally hosted ACP key.
pub fn local_endpoints(service_key: &str) -> Option<(String, String)> {
    hosts()
        .values()
        .find(|host| host.key == service_key)
        .map(|host| (host.ws_local.to_string(), host.meta_local.to_string()))
}

fn slot(host: &mut AcpServe, kind: ServiceKind) -> Result<&mut Option<Published>> {
    match kind {
        ServiceKind::Acp => Ok(&mut host.register),
        ServiceKind::Meta => Ok(&mut host.meta_register),
        other => bail!("an ACP host has no `{other}` service"),
    }
}

/// Unpublish one service (`acp` / `meta`) without stopping the host.
pub fn deregister(name: &str, kind: &str) -> Result<()> {
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

/// Re-publish one service (`acp` / `meta`) of a running host.
pub fn reregister(name: &str, kind: &str) -> Result<()> {
    let kind: ServiceKind = kind
        .parse()
        .map_err(|_| anyhow!("invalid service kind `{kind}`"))?;
    let Some((register, local)) = ({
        let mut guard = hosts();
        let host = guard
            .get_mut(name)
            .ok_or_else(|| anyhow!("`{name}` is not hosting locally"))?;
        let local = if kind == ServiceKind::Meta { host.meta_local } else { host.ws_local };
        let register = host.register_fn.clone();
        serve::needs_publishing(slot(host, kind)?).then_some((register, local))
    }) else {
        return Ok(());
    };
    let published = register(kind, name, local)?;
    let mut guard = hosts();
    match guard.get_mut(name) {
        Some(host) => *slot(host, kind)? = published,
        None => drop(published),
    }
    Ok(())
}

/// Stop hosting `name`: end the agent (after a grace period for running
/// turns), stop the hub and meta servers and withdraw both relay keys.
pub fn stop(name: &str) {
    let removed = hosts().remove(name);
    if let Some(host) = removed {
        stop_host(host);
    }
}

/// Stop every ACP host.
pub fn stop_all() {
    let all: Vec<AcpServe> = hosts().drain().map(|(_, host)| host).collect();
    for host in all {
        stop_host(host);
    }
}

fn stop_host(host: AcpServe) {
    let after_stop = host.after_stop.clone();
    runtime::runtime().block_on(async {
        host.hub.shutdown(ACP_STOP_GRACE).await;
        for published in [host.register, host.meta_register].into_iter().flatten() {
            if let Err(e) = published.stop().await {
                tracing::warn!(error = %format!("{e:#}"), service = %host.name, "releasing a relay key failed");
            }
        }
    });
    host.ws_task.abort();
    host.meta_task.abort();
    if let Some(after_stop) = after_stop {
        after_stop();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use pocket_codex_host_svc::acp::testing::{FakeAgent, FakeHandle, FakeScript};

    use super::*;

    fn test_deps(
        dir: &std::path::Path,
        fake: FakeScript,
        kinds: Arc<StdMutex<Vec<ServiceKind>>>,
    ) -> (StartDeps, FakeHandle) {
        runtime::init(std::env::temp_dir()).expect("runtime");
        let (connector, handle) = FakeAgent::spawn(fake);
        let rt = runtime::runtime();
        let store = Arc::new(
            rt.block_on(ConfigStore::open(dir.join("threads.json")))
                .expect("store"),
        );
        let host = Arc::new(
            rt.block_on(HostStore::open(dir.join("host.json")))
                .expect("host"),
        );
        let deps = StartDeps {
            device: "testdev".into(),
            require_account: false,
            connector: Arc::new(connector),
            register: Arc::new(move |kind, _name, _local| {
                kinds.lock().expect("kinds").push(kind);
                Ok(None)
            }),
            resolve: Arc::new(|agent_id| {
                Ok(LaunchSpec {
                    agent_id: agent_id.into(),
                    display_name: format!("{agent_id} agent"),
                    program: PathBuf::from("/opt/fake/agent"),
                    version: Some("1.2.3".into()),
                    pinned: true,
                    ..LaunchSpec::default()
                })
            }),
            gateway: Arc::new(|_| None),
            other_hosting: Box::new(|name| (name == "taken").then_some("Codex")),
            stores: (store, host),
            state_dir: dir.to_path_buf(),
            after_stop: None,
        };
        (deps, handle)
    }

    fn start_test(
        name: &str,
        agent: &str,
    ) -> (AcpServeReport, FakeHandle, Arc<StdMutex<Vec<ServiceKind>>>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("dir");
        let kinds = Arc::new(StdMutex::new(Vec::new()));
        let (deps, handle) = test_deps(dir.path(), FakeScript::default(), kinds.clone());
        let report = start_with(Some(name.into()), agent.into(), deps).expect("start");
        (report, handle, kinds, dir)
    }

    #[test]
    fn start_registers_meta_then_acp() {
        let (report, _fake, kinds, _dir) = start_test("acp-order", "fake-a");
        assert_eq!(*kinds.lock().expect("kinds"), vec![ServiceKind::Meta, ServiceKind::Acp]);
        assert_eq!(report.service_key, "pcx:testdev:acp:acp-order");
        assert_eq!(report.meta_service_key, "pcx:testdev:meta:acp-order");
        assert!(!report.reused);
        stop("acp-order");
    }

    #[test]
    fn name_reported_by_other_hosting_is_refused() {
        let dir = tempfile::tempdir().expect("dir");
        let (deps, _fake) = test_deps(dir.path(), FakeScript::default(), Arc::default());
        let error = start_with(Some("taken".into()), "fake-a".into(), deps).expect_err("refused");
        assert!(error.to_string().contains("already hosting Codex"), "{error}");
        assert!(!is_hosting("taken"));
    }

    #[test]
    fn is_hosting_is_true_after_start_with() {
        let (_report, _fake, _kinds, _dir) = start_test("acp-hosting", "fake-hosting");
        assert!(is_hosting("acp-hosting"));
        assert!(uses("fake-hosting", "1.2.3"));
        assert!(uses("fake-hosting", ""));
        assert!(!uses("fake-hosting", "9.9.9"));
        assert_eq!(hosted_names("fake-hosting"), vec!["acp-hosting".to_string()]);
        assert!(local_endpoints("pcx:testdev:acp:acp-hosting").is_some());
        stop("acp-hosting");
    }

    #[test]
    fn same_name_same_agent_is_reused() {
        let (_report, _fake, _kinds, dir) = start_test("acp-reuse", "fake-a");
        let (deps, _fake2) = test_deps(dir.path(), FakeScript::default(), Arc::default());
        let again = start_with(Some("acp-reuse".into()), "fake-a".into(), deps).expect("reuse");
        assert!(again.reused);
        let (deps, _fake3) = test_deps(dir.path(), FakeScript::default(), Arc::default());
        let other =
            start_with(Some("acp-reuse".into()), "fake-b".into(), deps).expect_err("conflict");
        assert!(other.to_string().contains("already hosting"), "{other}");
        stop("acp-reuse");
    }

    #[test]
    fn stop_shuts_down_hub_and_forgets_host() {
        let (_report, fake, _kinds, _dir) = start_test("acp-stop", "fake-a");
        assert!(fake.running());
        stop("acp-stop");
        assert!(!is_hosting("acp-stop"));
        runtime::runtime().block_on(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                while fake.running() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("agent stdin closed");
        });
    }

    #[test]
    fn status_reports_agent_fields() {
        let (_report, _fake, _kinds, _dir) = start_test("acp-status", "fake-a");
        let row = status()
            .into_iter()
            .find(|s| s.name == "acp-status")
            .expect("row");
        assert_eq!(row.provider, "acp");
        assert_eq!(row.agent_id.as_deref(), Some("fake-a"));
        assert_eq!(row.agent_name.as_deref(), Some("fake-a agent"));
        assert_eq!(row.provider_version.as_deref(), Some("1.2.3"));
        assert!(row.provider_verified);
        assert!(row.alive);
        assert_eq!(row.app_service_key, "pcx:testdev:acp:acp-status");
        assert_eq!(row.meta_service_key, "pcx:testdev:meta:acp-status");
        assert!(row.api_service_key.is_empty());
        assert_eq!(row.codex_binary.as_deref(), Some("/opt/fake/agent"));
        stop("acp-status");
    }
}
