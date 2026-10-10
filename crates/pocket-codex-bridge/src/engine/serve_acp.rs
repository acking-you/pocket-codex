//! Host an **owned** ACP agent from the app: launch the configured
//! executable + argument vector, be its ACP client, and publish the
//! Pocket-Codex gateway as `acp:<name>` plus a provider-neutral `meta:<name>`
//! service (host files, uploads, project folders, session file links).
//!
//! Hosting works in both self-host and account mode through the shared
//! transport; it does not require a hosted-account sign-in. Stopping hosting
//! (or quitting the app) stops the agent and cleans up its process tree.
//! Owned ACP hosting is only available on Unix desktops (see
//! `pocket_codex_host_svc::acp::process`); controllers on any platform can
//! use a remote ACP host.
//!
//! # Ownership and quitting
//!
//! One table ([`owners`]) holds every agent this process owns until its
//! cleanup has finished: registered when its start begins (*before* the
//! agent is launched), kept while it is hosted, and kept while an ordinary
//! stop is still retiring it — an owner leaves the table only after its
//! agent was stopped. [`stop_all`] (app quit) closes the table's barrier, so
//! no start begins and none can enter the host table afterwards, and then
//! stops every agent still in the table — starting, hosted or retiring.
//! [`AgentHost::stop`] joins a stop already in progress, so every
//! overlapping caller (a retirement, a second quit) returns only once that
//! agent's process cleanup is done. Agent processes are stopped first, all at
//! once and each within a bounded time; relay keys are withdrawn afterwards,
//! within their own bound, so a slow relay never delays process cleanup.
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
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use once_cell::sync::OnceCell;
use pocket_codex_core::service::{default_device_id, sanitize_component, ServiceId, ServiceKind};
use pocket_codex_host_svc::{
    acp::{self, process, spec, AgentHost, AgentSpec, HostOptions, SessionStore, SourceId},
    file_links::SessionDirResolver,
};
use pocket_codex_pb::Published;
use tokio::task::JoinHandle;

use super::{
    hosting, runtime,
    serve::{self, ServeStatus},
    transport,
};

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
/// bare name on the child `PATH`, then the preset's install locations.
pub fn locate(program: &str, profile_id: Option<&str>) -> Option<PathBuf> {
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

fn bind_loopback(label: &str) -> Result<(std::net::TcpListener, SocketAddr)> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .with_context(|| format!("binding the {label}"))?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    Ok((listener, addr))
}

/// Host `spec` under `name` (default: the profile id). A running host with
/// the same spec is reused; another agent under the same name is refused.
pub fn start(name: Option<String>, spec: AgentSpec) -> Result<AcpServeReport> {
    if !hosting_supported() {
        bail!("hosting ACP agents is not supported on this platform yet");
    }
    spec.validate().map_err(|error| anyhow!("{error}"))?;
    let name = sanitize_component(
        name.as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or(&spec.profile_id),
    );
    if let Some(found) = reuse(&name, &spec)? {
        return Ok(found);
    }
    let reservation = hosting::reserve(&name)?;
    let program = locate(&spec.program, Some(&spec.profile_id)).ok_or_else(|| {
        anyhow!("the agent executable was not found; install it or choose its full path")
    })?;
    let state = pocket_codex_core::paths::state_dir().context("resolving the state directory")?;
    // Session authority belongs to exactly this invocation (resolved
    // executable and arguments), never to the name or profile label.
    let acp_root = state.join("acp");
    let source = SourceId::derive(&acp_root, &program, &spec.args)
        .context("preparing the agent's session record")?;
    let store = SessionStore::open(&acp_root, &name, source);
    let uploads = state.join("acp-uploads");
    let rt = runtime::runtime();
    let host = {
        let _runtime = rt.enter();
        AgentHost::create(HostOptions {
            spec: spec.clone(),
            program: program.clone(),
            path: pocket_codex_codex::external_tool_path(),
            store,
        })
    };
    // Visible to a quit before the agent exists, so it can never escape it.
    let Some(id) = owners().begin(host.clone()) else {
        bail!("the app is quitting");
    };
    // Released only after the agent was stopped: an early return below
    // stops it first, and a hosted agent's owner lives in its `AcpServe`.
    let owner = Owner(id);
    if let Err(error) = rt.block_on(host.launch()) {
        rt.block_on(host.stop());
        drop(owner);
        return Err(anyhow!("{error}"));
    }
    let serving = match publish(&name, &host, &program, uploads, owner) {
        Ok(serving) => serving,
        Err((error, owner)) => {
            // Roll back: the agent stops and the reservation drops.
            rt.block_on(host.stop());
            drop(owner);
            return Err(error);
        },
    };
    let out = report(&serving, false);
    let refused = {
        let mut barrier = owners();
        if barrier.commit(id) {
            let replaced = hosts().insert(name, serving);
            debug_assert!(replaced.is_none(), "the name reservation excludes a second host");
            drop(barrier);
            // Not expected (the reservation), but never dropped under a lock.
            if let Some(replaced) = replaced {
                stop_host(replaced);
            }
            None
        } else {
            Some(serving)
        }
    };
    if let Some(serving) = refused {
        // A quit began meanwhile; it stops this agent too.
        stop_host(serving);
        bail!("the app is quitting");
    }
    reservation.commit();
    Ok(out)
}

/// Serve the gateway and meta service for a launched `host` and publish
/// both. On error everything started here is stopped again and `owner` is
/// handed back (the caller still has to stop the agent).
fn publish(
    name: &str,
    host: &AgentHost,
    program: &std::path::Path,
    uploads: PathBuf,
    owner: Owner,
) -> Result<AcpServe, (anyhow::Error, Owner)> {
    let prepared = (|| {
        Ok::<_, anyhow::Error>((
            serve::config_store()?,
            serve::host_store()?,
            bind_loopback("ACP gateway")?,
            bind_loopback("meta service")?,
        ))
    })();
    let (config_store, host_store, (gateway_std, gateway_local), (meta_std, meta_local)) =
        match prepared {
            Ok(prepared) => prepared,
            Err(error) => return Err((error, owner)),
        };
    let rt = runtime::runtime();
    let device = default_device_id();
    let gateway_host = host.clone();
    let gateway_task = rt.spawn(serve::supervise(
        "the ACP gateway",
        gateway_local,
        gateway_std,
        move |listener| {
            let host = gateway_host.clone();
            async move { acp::api::serve(listener, host).await }
        },
    ));
    let meta_host = host.clone();
    let meta_task =
        rt.spawn(serve::supervise("the ACP meta service", meta_local, meta_std, move |listener| {
            let (store, host_store, uploads) =
                (config_store.clone(), host_store.clone(), uploads.clone());
            let dirs: Arc<dyn SessionDirResolver> = Arc::new(meta_host.clone());
            async move {
                pocket_codex_host_svc::serve_generic(listener, store, host_store, uploads, dirs)
                    .await
            }
        }));
    let abort = || {
        gateway_task.abort();
        meta_task.abort();
    };
    let transport = match transport::resolve_blocking() {
        Ok(transport) => transport,
        Err(error) => {
            abort();
            return Err((error, owner));
        },
    };
    let meta_register =
        serve::register_service(&transport, &device, ServiceKind::Meta, name, meta_local)
            .ok()
            .flatten();
    let register =
        match serve::register_service(&transport, &device, ServiceKind::Acp, name, gateway_local) {
            Ok(register) => register,
            Err(error) => {
                abort();
                drop(meta_register);
                return Err((error, owner));
            },
        };
    Ok(AcpServe {
        key: ServiceId::new(&device, ServiceKind::Acp, name).key(),
        meta_key: ServiceId::new(&device, ServiceKind::Meta, name).key(),
        device,
        name: name.to_string(),
        gateway_local,
        meta_local,
        gateway_task,
        meta_task,
        register: Slot {
            published: register,
            revision: 0,
        },
        meta_register: Slot {
            published: meta_register,
            revision: 0,
        },
        host: host.clone(),
        program: program.display().to_string(),
        context: transport::Context::of(&transport),
        _owner: owner,
    })
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
    let host = hosts()
        .get(name)
        .map(|h| h.host.clone())
        .ok_or_else(|| anyhow!("`{name}` is not hosting an ACP agent"))?;
    runtime::runtime()
        .block_on(host.restart())
        .map_err(|error| anyhow!("{error}"))
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
                app_registered: serve::is_published(&host.register.published),
                api_listen_addr: String::new(),
                api_service_key: String::new(),
                api_registered: false,
                meta_listen_addr: host.meta_local.to_string(),
                meta_service_key: host.meta_key.clone(),
                meta_registered: serve::is_published(&host.meta_register.published),
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
            serve::register_service(&owned.transport, device, kind, name, local)
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
        serve::needs_publishing(&slot.published).then_some(Ticket {
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
mod tests {
    use super::*;

    #[test]
    fn unknown_hosts_are_not_hosting() {
        assert!(!is_hosting("no-such-acp-host"));
        assert!(local_endpoints("pcx:dev:acp:no-such").is_none());
        assert!(deregister("no-such-acp-host", "acp").is_err());
        assert!(restart("no-such-acp-host").is_err());
    }

    /// A start paused just before entering the host table while the app
    /// quits: the quit sees (and stops) its host, and the start may not
    /// enter the table afterwards. Nothing starts once the barrier closed.
    #[test]
    fn a_quit_sees_every_start_in_flight_and_none_enters_afterwards() {
        use std::sync::Barrier;

        let barrier = Arc::new(Mutex::new(Owners::<&'static str>::default()));
        let table = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let paused = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        let starter = std::thread::spawn({
            let (barrier, table) = (barrier.clone(), table.clone());
            let (paused, resume) = (paused.clone(), resume.clone());
            move || {
                let id = barrier
                    .lock()
                    .expect("lock")
                    .begin("agent-1")
                    .expect("open");
                // Launched and published; about to enter the table.
                paused.wait();
                resume.wait();
                let mut barrier = barrier.lock().expect("lock");
                if barrier.commit(id) {
                    table.lock().expect("table").push("agent-1");
                    true
                } else {
                    false
                }
            }
        });
        paused.wait();
        let to_stop = barrier.lock().expect("lock").close();
        assert_eq!(to_stop, vec!["agent-1"], "the quit stops the start in flight");
        resume.wait();
        assert!(!starter.join().expect("starter"), "the start is refused");
        assert!(table.lock().expect("table").is_empty(), "nothing entered after the quit");
        assert_eq!(barrier.lock().expect("lock").begin("agent-2"), None, "no new start");
        // A refused start stays owned (a later quit still sees it) until it
        // was cleaned up.
        assert_eq!(barrier.lock().expect("lock").close(), vec!["agent-1"]);
        barrier.lock().expect("lock").cleaned(1);
        assert!(barrier.lock().expect("lock").close().is_empty());
    }

    // ---- The production table, with agents that never launch -----------

    fn fixture_context(relay: &str) -> transport::Context {
        transport::Context::of(&fixture_transport(relay))
    }

    fn fixture_transport(relay: &str) -> transport::Transport {
        transport::Transport {
            session: pocket_codex_pb::RelaySession::for_test(relay),
            namespace: None,
        }
    }

    /// A hosted entry whose agent was created (and is owned) but never
    /// launched: stopping it touches no process.
    fn fixture_host(name: &str, context: transport::Context) -> AcpServe {
        let rt = runtime::runtime();
        let _runtime = rt.enter();
        let program = PathBuf::from("/nonexistent/fixture-agent");
        let host = AgentHost::create(HostOptions {
            spec: AgentSpec {
                profile_id: "fixture".into(),
                display_name: "Fixture".into(),
                program: program.display().to_string(),
                args: Vec::new(),
            },
            program: program.clone(),
            path: None,
            store: SessionStore::memory(SourceId::with_key("fixture", &program, &[])),
        });
        let id = owners().begin(host.clone()).expect("the barrier is open");
        assert!(owners().commit(id));
        AcpServe {
            device: "dev".into(),
            name: name.into(),
            key: ServiceId::new("dev", ServiceKind::Acp, name).key(),
            meta_key: ServiceId::new("dev", ServiceKind::Meta, name).key(),
            gateway_local: "127.0.0.1:9".parse().expect("addr"),
            meta_local: "127.0.0.1:9".parse().expect("addr"),
            gateway_task: rt.spawn(std::future::pending::<()>()),
            meta_task: rt.spawn(std::future::pending::<()>()),
            register: Slot::default(),
            meta_register: Slot::default(),
            host,
            program: program.display().to_string(),
            context,
            _owner: Owner(id),
        }
    }

    fn phase(host: &AgentHost) -> String {
        host.info()["phase"]["state"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    fn pause(at: &'static str) -> (Arc<std::sync::Barrier>, Arc<std::sync::Barrier>) {
        let pair = (Arc::new(std::sync::Barrier::new(2)), Arc::new(std::sync::Barrier::new(2)));
        test_pauses().insert(at, pair.clone());
        pair
    }

    /// Tests of the quit barrier reopen it afterwards.
    fn reopen() {
        owners().closing = false;
    }

    /// An ordinary stop removed its host from the table and is paused
    /// before it stops the agent, when the app quits: the quit must not
    /// return before that agent is stopped.
    #[test]
    fn a_quit_waits_for_an_agent_still_being_retired() {
        let _serial = transport::test_serial();
        runtime::init(std::env::temp_dir()).expect("init");
        let serving = fixture_host("retire-quit-test", fixture_context("relay.test:1"));
        let agent = serving.host.clone();
        hosts().insert("retire-quit-test".into(), serving);
        let (reached, resume) = pause("retire");
        let retiring = std::thread::spawn(|| stop("retire-quit-test"));
        reached.wait();
        assert!(!is_hosting("retire-quit-test"), "out of the table, not yet cleaned up");
        assert_ne!(phase(&agent), "stopped");
        stop_all();
        assert_eq!(phase(&agent), "stopped", "the quit stopped the retiring agent");
        resume.wait();
        retiring.join().expect("retirement");
        assert!(owners().agents.is_empty(), "every owner was released after cleanup");
        reopen();
    }

    /// Two quits overlap: the second must not return before the agents the
    /// first one drained are stopped.
    #[test]
    fn overlapping_quits_both_wait_for_every_agent() {
        let _serial = transport::test_serial();
        runtime::init(std::env::temp_dir()).expect("init");
        let serving = fixture_host("double-quit-test", fixture_context("relay.test:1"));
        let agent = serving.host.clone();
        hosts().insert("double-quit-test".into(), serving);
        let (reached, resume) = pause("quit");
        let first = std::thread::spawn(stop_all);
        reached.wait();
        assert!(!is_hosting("double-quit-test"), "the first quit drained the table");
        stop_all();
        assert_eq!(phase(&agent), "stopped", "the second quit waited for it too");
        resume.wait();
        first.join().expect("first quit");
        assert!(owners().agents.is_empty());
        reopen();
    }

    /// Fixture resolution in `context`'s relay, current now.
    fn resolved(relay: &str) -> Result<transport::Owned> {
        Ok(transport::Owned {
            transport: fixture_transport(relay),
            epoch: transport::epoch(),
        })
    }

    fn revision(name: &str) -> (u64, String) {
        let hosts = hosts();
        let host = hosts.get(name).expect("hosted");
        (host.register.revision, host.host.host_id().to_string())
    }

    /// A publication of host A is still waiting for the relay when A is
    /// stopped and B starts under the same name in another context; a
    /// deregistration overlapping a publication is the same case. The late
    /// result never touches the newer state.
    #[test]
    fn a_late_publication_never_replaces_a_newer_hosts_registration() {
        let _serial = transport::test_serial();
        runtime::init(std::env::temp_dir()).expect("init");
        let name = "publish-race-test";
        let a = fixture_host(name, fixture_context("relay-a.test:1"));
        hosts().insert(name.into(), a);
        let reached = Arc::new(std::sync::Barrier::new(2));
        let resume = Arc::new(std::sync::Barrier::new(2));
        let late = std::thread::spawn({
            let (reached, resume) = (reached.clone(), resume.clone());
            move || {
                reregister_with(
                    name,
                    "acp",
                    || resolved("relay-a.test:1"),
                    |_, _, _, _, _| {
                        reached.wait();
                        resume.wait();
                        Ok(None)
                    },
                )
            }
        });
        reached.wait();
        stop(name);
        let b = fixture_host(name, fixture_context("relay-b.test:1"));
        hosts().insert(name.into(), b);
        let before = revision(name);
        resume.wait();
        late.join()
            .expect("join")
            .expect("a late result is dropped, not an error");
        assert_eq!(revision(name), before, "B's registration is untouched");

        // A publication of B itself is installed.
        reregister_with(name, "acp", || resolved("relay-b.test:1"), |_, _, _, _, _| Ok(None))
            .expect("publish");
        let installed = revision(name);
        assert_eq!(installed.0, before.0 + 1);
        // B, resolved in another context, is refused before publishing.
        assert!(reregister_with(
            name,
            "acp",
            || resolved("relay-a.test:1"),
            |_, _, _, _, _| { panic!("never published into another context") }
        )
        .is_err());

        // A deregistration while a publication is in flight wins.
        let late = std::thread::spawn({
            let (reached, resume) = (reached.clone(), resume.clone());
            move || {
                reregister_with(
                    name,
                    "acp",
                    || resolved("relay-b.test:1"),
                    |_, _, _, _, _| {
                        reached.wait();
                        resume.wait();
                        Ok(None)
                    },
                )
            }
        });
        reached.wait();
        deregister(name, "acp").expect("deregister");
        let deregistered = revision(name);
        resume.wait();
        late.join().expect("join").expect("dropped");
        assert_eq!(revision(name), deregistered, "the deregistration stands");
        stop(name);
        assert!(owners().agents.is_empty());
    }

    #[test]
    fn presets_resolve_only_existing_programs() {
        assert!(locate("/definitely/not/here/agent", Some("opencode")).is_none());
        assert!(locate("definitely-not-an-installed-agent-xyz", None).is_none());
    }
}
