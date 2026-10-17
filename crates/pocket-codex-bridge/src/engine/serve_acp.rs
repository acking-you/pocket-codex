//! Host an **owned** ACP agent from the app: launch the configured
//! executable + argument vector, be its ACP client, and publish the
//! Pocket-Codex gateway as `acp:<name>` plus a provider-neutral `meta:<name>`
//! service (host files, uploads, project folders, session file links).
//!
//! Hosting works in both self-host and account mode through the shared
//! transport; it does not require a hosted-account sign-in. Stopping hosting
//! (or quitting the app) stops the agent and cleans up its process tree.
//! Owned ACP hosting is only available on Unix desktops (see
//! `pocket_codex_host_svc::process`); controllers on any platform can
//! use a remote ACP host.
//!
//! # Ownership and quitting
//!
//! ```text
//!            owners table (one entry per owned agent)
//! start ──► [starting] ──► [hosted] ──stop──► [retiring] ──► cleaned up ──► removed
//!   ▲ registered before launch                                 ▲
//!   └ refused once stop_all closed the barrier                 │
//! stop_all (app quit): close barrier, stop every entry ────────┘
//! ```
//!
//! An owner leaves [`owners`] only after its agent was stopped.
//! [`AgentHost::stop`] joins a stop already in progress, so every overlapping
//! caller (a retirement, a second quit) returns only once that agent's process
//! cleanup is done. Processes are stopped first, all at once and each within a
//! bounded time; relay keys are withdrawn afterwards within their own bound,
//! so a slow relay never delays process cleanup.
//!
//! # Publication
//!
//! Re-publishing runs without the host lock (it waits for the relay). Its
//! result is installed only into the same host incarnation, in the same
//! transport context, and only if that service's slot was not changed
//! meanwhile (by a deregistration or another publication); otherwise it is
//! dropped, which withdraws it. A late result never replaces a newer host's
//! registration under the same name.

use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use once_cell::sync::OnceCell;
use pocket_codex_core::service::{ServiceId, ServiceKind};
use pocket_codex_host_svc::{
    acp::{self, claude, spec, AgentHost, AgentSpec, HostOptions, SessionStore, SourceId},
    file_links::SessionDirResolver,
    process,
};
use pocket_codex_pb::Published;
use tokio::task::JoinHandle;

use super::{
    claude_setup,
    host_lease::{self, Lease, Op},
    host_names, host_profiles,
    host_services::{self, bind_loopback},
    hosting, runtime,
    serve::ServeStatus,
    transport,
};

#[path = "serve_acp_launch.rs"]
mod launch;

use self::launch::Captured;
pub(super) use self::launch::{launch_prepared, prepare, Prepared};

struct AcpServe {
    device: String,
    name: String,
    key: String,
    meta_key: String,
    gateway_local: SocketAddr,
    meta_local: SocketAddr,
    gateway_task: JoinHandle<()>,
    meta_task: JoinHandle<()>,
    register: Slot,
    meta_register: Slot,
    host: AgentHost,
    program: String,
    /// The transport context the services were published under: only a
    /// controller in the same context may reach them on loopback.
    context: transport::Context,
    /// The model-service configuration the agent was launched with, kept so
    /// an apply that fails can restart it as it was.
    config: Option<host_profiles::Resolved>,
    /// Exactly how the agent was launched, for starting it again as it ran.
    launch: Option<Captured>,
    /// This agent's entry in [`owners`]; released when this is dropped,
    /// which happens only after the agent was stopped.
    _owner: Owner,
}

/// One published service of a host and the revision of its last change
/// (see the module docs).
#[derive(Default)]
struct Slot {
    published: Option<Published>,
    revision: u64,
}

impl Slot {
    /// Take the publication out (a deregistration); a publication still in
    /// flight then no longer applies.
    fn take(&mut self) -> Option<Published> {
        self.revision += 1;
        self.published.take()
    }

    /// Install `published` if nothing changed since `revision`; otherwise
    /// hand it back. Returns what is to be dropped.
    fn install(&mut self, revision: u64, published: Option<Published>) -> Option<Published> {
        if self.revision != revision {
            return published;
        }
        self.revision += 1;
        std::mem::replace(&mut self.published, published)
    }
}

/// Result of [`start`].
#[derive(Debug, Clone)]
pub struct AcpServeReport {
    /// Device id.
    pub device: String,
    /// Instance name.
    pub name: String,
    /// `pcx:<device>:acp:<name>`.
    pub service_key: String,
    /// Loopback gateway address.
    pub listen_addr: String,
    /// `pcx:<device>:meta:<name>`.
    pub meta_service_key: String,
    /// Profile id.
    pub profile_id: String,
    /// Display name.
    pub display_name: String,
    /// Whether an existing host was reused.
    pub reused: bool,
}

/// How long quitting waits for relay keys to be withdrawn.
const WITHDRAW_GRACE: Duration = Duration::from_secs(2);

fn hosts() -> MutexGuard<'static, HashMap<String, AcpServe>> {
    static HOSTS: OnceCell<Mutex<HashMap<String, AcpServe>>> = OnceCell::new();
    HOSTS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

/// Every owned agent whose cleanup has not finished, and the quit barrier
/// (see the module docs). Lock order: this, then [`hosts`].
struct Owners<T> {
    closing: bool,
    next: u64,
    /// Starting, hosted and retiring agents.
    agents: HashMap<u64, T>,
    /// Those that have not entered the host table yet.
    starting: HashSet<u64>,
}

impl<T> Default for Owners<T> {
    fn default() -> Self {
        Self {
            closing: false,
            next: 0,
            agents: HashMap::new(),
            starting: HashSet::new(),
        }
    }
}

impl<T: Clone> Owners<T> {
    /// Own `agent` from the start of its launch; `None` once the barrier
    /// is closed.
    fn begin(&mut self, agent: T) -> Option<u64> {
        if self.closing {
            return None;
        }
        self.next += 1;
        self.agents.insert(self.next, agent);
        self.starting.insert(self.next);
        Some(self.next)
    }

    /// Agent `id` wants to enter the host table: `true` when it may, and it
    /// must do so before this lock is released. Either way it stays owned
    /// until [`Owners::cleaned`].
    fn commit(&mut self, id: u64) -> bool {
        self.starting.remove(&id);
        !self.closing
    }

    /// Agent `id` was stopped: it is no longer owned.
    fn cleaned(&mut self, id: u64) {
        self.starting.remove(&id);
        self.agents.remove(&id);
    }

    /// Close the barrier; every agent not yet cleaned up, to be stopped.
    fn close(&mut self) -> Vec<T> {
        self.closing = true;
        self.agents.values().cloned().collect()
    }
}

fn owners() -> MutexGuard<'static, Owners<AgentHost>> {
    static OWNERS: OnceCell<Mutex<Owners<AgentHost>>> = OnceCell::new();
    OWNERS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

/// One agent's entry in [`owners`]. Dropped only once its agent was
/// stopped — never while [`owners`] or [`hosts`] is held.
struct Owner(u64);

impl Drop for Owner {
    fn drop(&mut self) {
        owners().cleaned(self.0);
    }
}

/// Test hook: hold the caller at `at` until the test releases it.
#[cfg(test)]
fn pause_point(at: &'static str) {
    let pause = test_pauses().remove(at);
    if let Some((reached, resume)) = pause {
        reached.wait();
        resume.wait();
    }
}

#[cfg(not(test))]
fn pause_point(_: &'static str) {}

#[cfg(test)]
type TestPause = (Arc<std::sync::Barrier>, Arc<std::sync::Barrier>);

#[cfg(test)]
fn test_pauses() -> MutexGuard<'static, HashMap<&'static str, TestPause>> {
    static PAUSES: OnceCell<Mutex<HashMap<&'static str, TestPause>>> = OnceCell::new();
    PAUSES
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

/// Whether `name` is an ACP host of this process.
pub fn is_hosting(name: &str) -> bool {
    hosts().contains_key(name)
}

/// Whether this platform can host ACP agents.
pub fn hosting_supported() -> bool {
    process::hosting_supported()
}

/// Resolve the executable a spec would launch: an explicit path as is, a
/// bare name on the child `PATH`, then the preset's install locations. The
/// Claude preset's own program resolves to its ACP adapter (see
/// [`super::claude_setup`]), never to the native `claude` CLI.
pub fn locate(program: &str, profile_id: Option<&str>) -> Option<PathBuf> {
    if profile_id == Some(claude::PROFILE_ID) && program.trim() == claude::ADAPTER_BIN {
        return claude_setup::environment()
            .ok()?
            .adapter
            .map(|adapter| adapter.bin);
    }
    let search = pocket_codex_codex::external_tool_path();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let fallback = profile_id
        .and_then(spec::preset)
        .map_or(&[][..], |p| p.fallback_dirs);
    spec::resolve_program(program.trim(), search.as_ref(), home.as_deref(), fallback).ok()
}

fn report(host: &AcpServe, reused: bool) -> AcpServeReport {
    let profile = host.host.spec();
    AcpServeReport {
        device: host.device.clone(),
        name: host.name.clone(),
        service_key: host.key.clone(),
        listen_addr: host.gateway_local.to_string(),
        meta_service_key: host.meta_key.clone(),
        profile_id: profile.profile_id.clone(),
        display_name: profile.display_name.clone(),
        reused,
    }
}

/// Host `spec` under `name` (default: the profile id). A running host with
/// the same spec is reused; another agent under the same name is refused.
pub fn start(name: Option<String>, spec: AgentSpec) -> Result<AcpServeReport> {
    let name = host_names::canonical(&host_names::requested(name.as_deref(), &spec.profile_id))?;
    let lease = host_lease::acquire(&name, Op::Start)?;
    let profiles = host_profiles::root()?;
    let restore = host_profiles::Runtime::Acp(spec.clone());
    let result = start_locked(&lease, spec, None, &profiles)?;
    super::host_lifecycle::record_running_in(&profiles, lease.name(), restore);
    Ok(result)
}

/// [`start`] under the held lease of its instance, with the model service
/// named (`None`: the saved one in `profiles`); a rollback restarts with
/// the one a host ran with.
pub(super) fn start_locked(
    lease: &Lease,
    spec: AgentSpec,
    provider: Option<host_profiles::Resolved>,
    profiles: &Path,
) -> Result<AcpServeReport> {
    spec.validate().map_err(|error| anyhow!("{error}"))?;
    if let Some(found) = reuse(lease.name(), &spec)? {
        return Ok(found);
    }
    launch_prepared(prepare(lease.name(), spec, provider, profiles)?)
}

fn reuse(name: &str, spec: &AgentSpec) -> Result<Option<AcpServeReport>> {
    let found = {
        let guard = hosts();
        let Some(host) = guard.get(name) else { return Ok(None) };
        if host.host.spec() != spec {
            bail!(
                "`{name}` is already hosting a different agent; stop it first or choose another \
                 name"
            );
        }
        report(host, true)
    };
    let _ = reregister(name, "acp");
    let _ = reregister(name, "meta");
    Ok(Some(found))
}

/// Restart the agent of host `name` (a fresh process and generation).
pub fn restart(name: &str) -> Result<()> {
    let lease = host_lease::acquire(name, Op::Restart)?;
    // The table is keyed canonically, like the lease.
    let host = hosts()
        .get(lease.name())
        .map(|h| h.host.clone())
        .ok_or_else(|| anyhow!("`{name}` is not hosting an ACP agent"))?;
    runtime::runtime()
        .block_on(host.restart())
        .map_err(|error| anyhow!("{error}"))
}

/// How a running ACP host was launched, for restarting it in place.
#[derive(Debug, Clone)]
pub(super) struct AcpRelaunch {
    /// Its spec.
    pub(super) spec: AgentSpec,
    /// Its model service.
    pub(super) provider: host_profiles::Resolved,
    /// Its agent host's identity.
    pub(super) incarnation: String,
}

/// How the ACP host `name` was launched.
pub(super) fn relaunch(name: &str) -> Option<AcpRelaunch> {
    hosts().get(name).map(|host| AcpRelaunch {
        spec: host.host.spec().clone(),
        provider: host
            .config
            .clone()
            .unwrap_or_else(host_profiles::Resolved::inherited),
        incarnation: host.host.host_id().to_string(),
    })
}

/// Status rows for every ACP host, in the shared [`ServeStatus`] shape.
pub(super) fn status() -> Vec<ServeStatus> {
    hosts()
        .values()
        .map(|host| {
            let info = host.host.info();
            let phase = info["phase"]["state"]
                .as_str()
                .unwrap_or("starting")
                .to_string();
            ServeStatus {
                name: host.name.clone(),
                device: host.device.clone(),
                pid: None,
                alive: phase == "ready",
                app_listen_addr: host.gateway_local.to_string(),
                app_service_key: host.key.clone(),
                app_registered: host_services::is_published(&host.register.published),
                api_listen_addr: String::new(),
                api_service_key: String::new(),
                api_registered: false,
                meta_listen_addr: host.meta_local.to_string(),
                meta_service_key: host.meta_key.clone(),
                meta_registered: host_services::is_published(&host.meta_register.published),
                embedded: false,
                codex_binary: Some(host.program.clone()),
                proxy: None,
                provider: "acp".to_string(),
                provider_version: info["agent"]["version"].as_str().map(str::to_string),
                protocol: "acp".to_string(),
                provider_name: host.host.spec().display_name.clone(),
                profile_id: Some(host.host.spec().profile_id.clone()),
                agent_phase: Some(phase),
                agent_error: info["phase"]["reason"].as_str().map(str::to_string),
                auth_required: info["authRequired"] == true,
                opencode: None,
                config_source: host.config.as_ref().map(|c| c.source.as_str().to_string()),
                config_revision: host.config.as_ref().map(|c| c.revision),
                incarnation: Some(host.host.host_id().to_string()),
                busy: host_lease::holder(&host.name).map(|op| op.label().to_string()),
            }
        })
        .collect()
}

/// Loopback gateway + meta addresses of a locally hosted ACP key, when it
/// was published under the current transport context. In another context
/// the same logical key names someone else's service, and a key namespaced
/// to another account never resolves here.
pub(super) fn local_endpoints(service_key: &str) -> Option<(String, String)> {
    let logical = super::session_engine::logical_key(service_key)?;
    let context = transport::current_context()?;
    if !context.admits(service_key) {
        return None;
    }
    hosts()
        .values()
        .find(|host| host.key == logical && host.context == context)
        .map(|host| (host.gateway_local.to_string(), host.meta_local.to_string()))
}

/// The loopback gateway of logical key `logical` published under `context`.
pub(super) fn local_gateway(logical: &str, context: &transport::Context) -> Option<String> {
    hosts()
        .values()
        .find(|host| host.key == logical && &host.context == context)
        .map(|host| host.gateway_local.to_string())
}

fn slot(host: &mut AcpServe, kind: ServiceKind) -> Result<&mut Slot> {
    match kind {
        ServiceKind::Acp => Ok(&mut host.register),
        ServiceKind::Meta => Ok(&mut host.meta_register),
        other => bail!("an ACP host has no `{other}` service"),
    }
}

fn withdraw(published: Option<Published>) {
    if let Some(published) = published {
        if let Err(e) = runtime::runtime().block_on(published.stop()) {
            tracing::warn!(error = %format!("{e:#}"), "releasing the relay key failed");
        }
    }
}

/// Unpublish one service (`acp` / `meta`) without stopping the host. A
/// publication of it still in flight is discarded when it finishes.
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
    withdraw(published);
    Ok(())
}

/// What a publication was started for (see the module docs).
struct Ticket {
    device: String,
    local: SocketAddr,
    incarnation: String,
    context: transport::Context,
    revision: u64,
}

/// Re-publish one service (`acp` / `meta`) of a running host.
pub(super) fn reregister(name: &str, kind: &str) -> Result<()> {
    reregister_with(
        name,
        kind,
        transport::resolve_owned_blocking,
        |owned, device, kind, name, local| {
            host_services::register_service(&owned.transport, device, kind, name, local)
        },
    )
}

/// [`reregister`] with its transport resolution and relay publication
/// supplied (tests use fixtures).
fn reregister_with(
    name: &str,
    kind: &str,
    resolve: impl FnOnce() -> Result<transport::Owned>,
    publish: impl FnOnce(
        &transport::Owned,
        &str,
        ServiceKind,
        &str,
        SocketAddr,
    ) -> Result<Option<Published>>,
) -> Result<()> {
    let kind: ServiceKind = kind
        .parse()
        .map_err(|_| anyhow!("invalid service kind `{kind}`"))?;
    let Some(ticket) = ({
        let mut guard = hosts();
        let host = guard
            .get_mut(name)
            .ok_or_else(|| anyhow!("`{name}` is not hosting locally"))?;
        let local = if kind == ServiceKind::Meta { host.meta_local } else { host.gateway_local };
        let (device, incarnation, context) =
            (host.device.clone(), host.host.host_id().to_string(), host.context.clone());
        let slot = slot(host, kind)?;
        host_services::needs_publishing(&slot.published).then_some(Ticket {
            device,
            local,
            incarnation,
            context,
            revision: slot.revision,
        })
    }) else {
        return Ok(());
    };
    let owned = resolve()?;
    if transport::Context::of(&owned.transport) != ticket.context {
        bail!(
            "the account or relay changed since `{name}` was published; stop and start hosting \
             again"
        );
    }
    let published = publish(&owned, &ticket.device, kind, name, ticket.local)?;
    let refused = {
        let mut guard = hosts();
        match guard.get_mut(name) {
            Some(host)
                if host.host.host_id() == ticket.incarnation
                    && host.context == ticket.context
                    && transport::is_current(owned.epoch) =>
            {
                slot(host, kind)?.install(ticket.revision, published)
            },
            // Another host (or none) owns the name now.
            _ => published,
        }
    };
    // Whatever was superseded is withdrawn outside the lock.
    withdraw(refused);
    Ok(())
}

/// Stop hosting `name`: stop the gateway and meta service, stop the agent
/// and clean up its process tree, then withdraw its relay keys. The agent
/// stays in [`owners`] until it was stopped, so a quit meanwhile still
/// waits for it.
pub(super) fn stop(name: &str) {
    let removed = hosts().remove(name);
    if let Some(host) = removed {
        pause_point("retire");
        stop_host(host);
    }
}

/// [`stop`], handing back the exact launch the host ran with so a failed
/// apply can start it again as it
/// was; `None` when it was not hosting or its launch was not kept.
pub(super) fn retire(name: &str) -> Option<Prepared> {
    let mut host = hosts().remove(name)?;
    let previous = host.launch.take().map(|captured| {
        let provider = host
            .config
            .clone()
            .unwrap_or_else(host_profiles::Resolved::inherited);
        Prepared::again(name, host.host.spec().clone(), captured, provider)
    });
    pause_point("retire");
    stop_host(host);
    previous
}

/// Stop every ACP agent for good (app quit; see the module docs). Returns
/// once every owned agent process — starting, hosted, or being retired by
/// an ordinary stop or by another quit — has been stopped; relay keys of the
/// hosts drained here get [`WITHDRAW_GRACE`] after that.
pub(super) fn stop_all() {
    let (agents, drained) = {
        let mut barrier = owners();
        let agents = barrier.close();
        let drained: Vec<AcpServe> = hosts().drain().map(|(_, host)| host).collect();
        (agents, drained)
    };
    pause_point("quit");
    let names: Vec<String> = drained.iter().map(|host| host.name.clone()).collect();
    runtime::runtime().block_on(async {
        // Joins a stop already in progress elsewhere.
        futures::future::join_all(agents.iter().map(AgentHost::stop)).await;
        let mut withdrawals = Vec::new();
        for host in drained {
            host.gateway_task.abort();
            host.meta_task.abort();
            withdrawals.extend(
                [host.register.published, host.meta_register.published]
                    .into_iter()
                    .flatten(),
            );
            // The rest of `host`, its owner included, is dropped here: its
            // agent was stopped above.
        }
        let withdrawn = futures::future::join_all(withdrawals.into_iter().map(Published::stop));
        if tokio::time::timeout(WITHDRAW_GRACE, withdrawn)
            .await
            .is_err()
        {
            tracing::warn!("releasing relay keys did not finish while quitting");
        }
    });
    for name in names {
        hosting::release(&name);
    }
}

fn stop_host(host: AcpServe) {
    let name = host.name.clone();
    runtime::runtime().block_on(async {
        // The owned process first (bounded), then the relay keys.
        host.gateway_task.abort();
        host.meta_task.abort();
        host.host.stop().await;
        for published in [host.register.published, host.meta_register.published]
            .into_iter()
            .flatten()
        {
            if let Err(e) = published.stop().await {
                tracing::warn!(error = %format!("{e:#}"), "releasing a relay key failed");
            }
        }
    });
    // `host` (and its owner) is gone now, after its agent stopped.
    hosting::release(&name);
}

#[cfg(test)]
#[path = "serve_acp_tests.rs"]
mod tests;
