//! Install jobs (TRD §4.3.9).
//!
//! ```text
//! queued → downloading → verifying → extracting → installing → validating → done
//!                                any step fails ↘ failed
//! ```

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use pocket_codex_core::acp::{methods, pcx::JobProgress, InitializeResponse, PROTOCOL_VERSION};
use serde_json::json;
use tokio::sync::Semaphore;

use super::{
    super::{error::AcpError, hub::client_initialize, launch::LaunchSpec, peer::AgentPeer},
    archive,
    audit::{self, AuditEntry},
    catalog::{CatalogAgent, Release, ReleaseKind},
    fetch, node,
    npm::{self, NpmArgs},
    registry_hint::{self, RegistryPlan},
    resolve::{self, archive_target, InstallContext},
    settings::Settings,
    store::{now_rfc3339, update_installed, InstalledAgent, Layout},
};

/// Finished jobs are kept this long.
const KEEP_FINISHED: Duration = Duration::from_secs(3600);
/// Concurrent jobs.
const MAX_CONCURRENT: usize = 2;
const VALIDATE_TIMEOUT: Duration = Duration::from_secs(60);

struct Entry {
    progress: JobProgress,
    finished: Option<Instant>,
}

fn jobs() -> std::sync::MutexGuard<'static, HashMap<String, Entry>> {
    static JOBS: OnceLock<Mutex<HashMap<String, Entry>>> = OnceLock::new();
    let guard = JOBS.get_or_init(|| Mutex::new(HashMap::new())).lock();
    let mut guard = guard.unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.retain(|_, e| e.finished.is_none_or(|at| at.elapsed() < KEEP_FINISHED));
    guard
}

fn permits() -> Arc<Semaphore> {
    static PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    PERMITS
        .get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT)))
        .clone()
}

/// Progress of job `id`.
pub fn job(id: &str) -> Option<JobProgress> {
    jobs().get(id).map(|e| e.progress.clone())
}

/// Running job of `agent_id` (of `kind`), if any.
pub fn running_job(agent_id: &str, kind: &str) -> Option<String> {
    jobs()
        .values()
        .find(|e| {
            e.finished.is_none() && e.progress.agent_id == agent_id && e.progress.kind == kind
        })
        .map(|e| e.progress.id.clone())
}

/// Most recent failed install of `agent_id`.
pub fn last_failure(agent_id: &str) -> Option<JobProgress> {
    jobs()
        .values()
        .filter(|e| {
            e.progress.agent_id == agent_id
                && e.progress.kind == "install"
                && e.progress.state == "failed"
        })
        .max_by_key(|e| e.finished)
        .map(|e| e.progress.clone())
}

/// Register a new job; returns its id.
pub fn new_job(kind: &str, agent_id: &str, version: &str) -> String {
    let id = format!("j{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    let progress = JobProgress {
        id: id.clone(),
        kind: kind.into(),
        agent_id: agent_id.into(),
        version: version.into(),
        state: "queued".into(),
        ..JobProgress::default()
    };
    jobs().insert(id.clone(), Entry {
        progress,
        finished: None,
    });
    id
}

/// Update job `id`.
pub fn update(id: &str, edit: impl FnOnce(&mut JobProgress)) {
    if let Some(entry) = jobs().get_mut(id) {
        edit(&mut entry.progress);
    }
}

/// Finish job `id`; `Ok(service_key)` for host jobs.
pub fn finish(id: &str, result: Result<Option<String>, AcpError>) {
    if let Some(entry) = jobs().get_mut(id) {
        match result {
            Ok(service_key) => {
                entry.progress.state = "done".into();
                entry.progress.service_key = service_key;
                entry.progress.message = None;
            },
            Err(e) => {
                entry.progress.state = "failed".into();
                entry.progress.error_code = Some(e.code().into());
                entry.progress.message = Some(e.detail());
            },
        }
        entry.finished = Some(Instant::now());
    }
}

/// What a job installs.
#[derive(Clone, Debug)]
enum Plan {
    Catalog(Release),
    Registry(RegistryPlan),
}

impl Plan {
    fn version(&self) -> String {
        match self {
            Self::Catalog(r) => r.version.clone(),
            Self::Registry(p) => p.version.clone(),
        }
    }
}

#[cfg_attr(
    unix,
    allow(
        clippy::useless_conversion,
        reason = "statvfs field widths differ across macOS, 64-bit and 32-bit Linux"
    )
)]
fn free_mb(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        let stat = nix::sys::statvfs::statvfs(path).ok()?;
        let bytes =
            u64::from(stat.blocks_available()).saturating_mul(u64::from(stat.fragment_size()));
        Some(bytes / (1024 * 1024))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// Choose what to install.
fn plan(
    agent: &CatalogAgent,
    version: Option<&str>,
    ctx: &InstallContext,
) -> Result<Plan, AcpError> {
    if let Some(version) = version {
        if let Some(release) = agent.release(version) {
            return Ok(Plan::Catalog(release.clone()));
        }
        return registry_hint::install_plan(ctx, agent, version).map(Plan::Registry);
    }
    if let Some(engine) = &agent.external_engine {
        let settings = Settings::load(&ctx.layout())?;
        let path = resolve::engine_path(agent, &settings, ctx).ok_or_else(|| {
            AcpError::EngineMissing(format!("{} needs `{}`", agent.name, engine.binary))
        })?;
        let found = resolve::engine_version(&path, &engine.version_args)?;
        return resolve::release_for_engine(agent, &found)
            .cloned()
            .map(Plan::Catalog)
            .ok_or_else(|| AcpError::EngineIncompatible {
                found: found.to_string(),
                required: agent
                    .releases
                    .iter()
                    .filter_map(|r| r.engine_range.clone())
                    .collect::<Vec<_>>()
                    .join(" | "),
                suggested: None,
            });
    }
    agent
        .releases
        .first()
        .cloned()
        .map(Plan::Catalog)
        .ok_or_else(|| AcpError::UnknownAgent(agent.id.clone()))
}

fn check_platform(plan: &Plan, ctx: &InstallContext, agent: &CatalogAgent) -> Result<(), AcpError> {
    let platform = ctx.platform.clone()?;
    match plan {
        Plan::Catalog(release) => match &release.kind {
            ReleaseKind::Npm {
                lock,
                platforms,
                ..
            } => {
                if !platforms.is_empty() && !platforms.iter().any(|p| p == platform.key) {
                    return Err(AcpError::UnsupportedPlatform(format!(
                        "{} has no build for {}",
                        agent.name, platform.key
                    )));
                }
                if !ctx.locks.contains_key(lock) {
                    return Err(AcpError::Internal(format!("missing lockfile {lock}")));
                }
                Ok(())
            },
            ReleaseKind::Archive {
                targets,
            } => archive_target(targets, &platform)
                .map(|_| ())
                .ok_or_else(|| {
                    AcpError::UnsupportedPlatform(format!(
                        "{} has no build for {}",
                        agent.name, platform.key
                    ))
                }),
        },
        Plan::Registry(_) => Ok(()),
    }
}

/// Start installing `agent_id` (`version` = catalog pin when `None`);
/// returns the job id.
pub async fn start_install(
    agent_id: &str,
    version: Option<&str>,
    remote: bool,
    ctx: &InstallContext,
) -> Result<String, AcpError> {
    let result = prepare(agent_id, version, remote, ctx);
    let mut entry = AuditEntry::new("install", agent_id, remote).outcome(&result);
    entry.version = result
        .as_ref()
        .ok()
        .map(|(_, plan)| plan.version())
        .or(version.map(str::to_string));
    audit::record(&ctx.layout(), &entry);
    let (agent, plan) = result?;
    let id = new_job("install", agent_id, &plan.version());
    let ctx = ctx.clone();
    let job_id = id.clone();
    tokio::spawn(async move {
        let permit = permits().acquire_owned().await;
        let result = run(&ctx, &agent, &plan, &job_id).await;
        drop(permit);
        finish(&job_id, result.map(|()| None));
    });
    Ok(id)
}

fn prepare(
    agent_id: &str,
    version: Option<&str>,
    remote: bool,
    ctx: &InstallContext,
) -> Result<(CatalogAgent, Plan), AcpError> {
    let agent = ctx
        .agent(agent_id)
        .cloned()
        .ok_or_else(|| AcpError::UnknownAgent(agent_id.into()))?;
    if remote {
        if !Settings::load(&ctx.layout())?.remote_management() {
            return Err(AcpError::RemoteManagementDisabled);
        }
        if version.is_some_and(|v| agent.release(v).is_none()) {
            return Err(AcpError::VersionNotPinned(format!(
                "{} can only be installed remotely at a pinned version",
                agent.name
            )));
        }
    }
    ctx.platform.clone()?;
    let plan = plan(&agent, version, ctx)?;
    check_platform(&plan, ctx, &agent)?;
    if running_job(agent_id, "install").is_some() {
        return Err(AcpError::JobRunning(format!("{} is already being installed", agent.name)));
    }
    let needed = u64::from(agent.approx_size_mb) * 2;
    std::fs::create_dir_all(&ctx.root)?;
    if free_mb(&ctx.root).is_some_and(|free| free < needed) {
        return Err(AcpError::DiskSpace {
            needed_mb: needed,
        });
    }
    Ok((agent, plan))
}

fn state(id: &str, state: &'static str) {
    update(id, |p| {
        p.state = state.into();
        p.bytes = 0;
        p.total = None;
    });
}

async fn run(
    ctx: &InstallContext,
    agent: &CatalogAgent,
    plan: &Plan,
    id: &str,
) -> Result<(), AcpError> {
    let layout = ctx.layout();
    let platform = ctx.platform.clone()?;
    let version = plan.version();
    let progress_id = id.to_string();
    let progress = move |state: &'static str, bytes: u64, total: Option<u64>| {
        update(&progress_id, |p| {
            p.state = state.into();
            p.bytes = bytes;
            p.total = total;
        });
    };
    let final_dir = layout.agent_dir(&agent.id, &version);
    let staging = Layout::staging(&final_dir);
    let outcome = async {
        let (release, record) = match plan {
            Plan::Catalog(release) => match &release.kind {
                ReleaseKind::Npm {
                    package,
                    lock,
                    omit,
                    ..
                } => {
                    let runtime = node::ensure_node(ctx, &progress).await?;
                    state(id, "installing");
                    let lock_text = ctx.locks.get(lock).cloned().unwrap_or_default();
                    npm::write_package(&staging, &agent.id, package, &release.version, &lock_text)?;
                    let registry = Settings::load(&layout)?
                        .npm_registry()
                        .unwrap_or_else(|| npm::DEFAULT_REGISTRY.to_string());
                    let args = NpmArgs {
                        node: runtime.node.clone(),
                        npm_cli: runtime.npm_cli.clone(),
                        prefix: staging.clone(),
                        args: npm::ci_args(&staging, &platform, omit, &registry, &layout),
                        env: npm::npm_env(&runtime.node),
                        timeout: npm::NPM_TIMEOUT,
                    };
                    ctx.npm.ci(&args).await?;
                    let integrity = Some(fetch::sri("sha256", lock_text.as_bytes()));
                    (release.clone(), record("npm", "catalog", integrity, None, None, Vec::new()))
                },
                ReleaseKind::Archive {
                    targets,
                } => {
                    let target = archive_target(targets, &platform)
                        .ok_or_else(|| AcpError::UnsupportedPlatform(platform.key.into()))?;
                    download_extract(ctx, &target.url, &target.integrity, &staging, &progress)
                        .await?;
                    let integrity = Some(target.integrity.clone());
                    (
                        release.clone(),
                        record("archive", "catalog", integrity, None, None, Vec::new()),
                    )
                },
            },
            Plan::Registry(registry) => {
                let release = registry_hint::release_of(agent, registry, &platform);
                match registry {
                    RegistryPlan {
                        archive: Some((url, integrity, cmd)), ..
                    } => {
                        download_extract(ctx, url, integrity, &staging, &progress).await?;
                        let rec = record(
                            "archive",
                            "registry",
                            Some(integrity.clone()),
                            None,
                            Some(cmd.clone()),
                            registry.args.clone(),
                        );
                        (release, rec)
                    },
                    RegistryPlan {
                        npm_package: Some(package), ..
                    } => {
                        let runtime = node::ensure_node(ctx, &progress).await?;
                        state(id, "installing");
                        std::fs::create_dir_all(&staging)?;
                        std::fs::write(
                            staging.join("package.json"),
                            json!({"name": format!("pcx-{}", agent.id), "private": true})
                                .to_string(),
                        )?;
                        let registry_url = Settings::load(&layout)?
                            .npm_registry()
                            .unwrap_or_else(|| npm::DEFAULT_REGISTRY.to_string());
                        let mut args = vec![
                            "install".to_string(),
                            "--save-exact".into(),
                            "--ignore-scripts".into(),
                            "--prefix".into(),
                            staging.to_string_lossy().into_owned(),
                            format!("{package}@{}", registry.version),
                        ];
                        args.extend(npm::shared_args(&platform, &[], &registry_url, &layout));
                        let npm_args = NpmArgs {
                            node: runtime.node.clone(),
                            npm_cli: runtime.npm_cli.clone(),
                            prefix: staging.clone(),
                            args,
                            env: npm::npm_env(&runtime.node),
                            timeout: npm::NPM_TIMEOUT,
                        };
                        ctx.npm.ci(&npm_args).await?;
                        let entry = registry_hint::npm_entry(package);
                        let rec = record(
                            "npm",
                            "registry",
                            None,
                            Some(entry),
                            None,
                            registry.args.clone(),
                        );
                        (release, rec)
                    },
                    _ => {
                        return Err(AcpError::VersionNotPinned(format!(
                            "{} {}",
                            agent.name, registry.version
                        )))
                    },
                }
            },
        };
        state(id, "validating");
        let agent_version = validate(ctx, agent, &release, &staging).await?;
        commit(ctx, agent, &version, &staging, &final_dir, InstalledAgent {
            agent_version,
            ..record
        })
    }
    .await;
    if outcome.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    outcome
}

fn record(
    kind: &str,
    source: &str,
    integrity: Option<String>,
    entry: Option<String>,
    cmd: Option<String>,
    args: Vec<String>,
) -> InstalledAgent {
    InstalledAgent {
        kind: kind.into(),
        source: source.into(),
        integrity,
        entry,
        cmd,
        args,
        ..InstalledAgent::default()
    }
}

async fn download_extract(
    ctx: &InstallContext,
    url: &str,
    integrity: &str,
    staging: &Path,
    progress: &(dyn Fn(&'static str, u64, Option<u64>) + Send + Sync),
) -> Result<(), AcpError> {
    let layout = ctx.layout();
    let lower = url.to_lowercase();
    let suffix = if lower.ends_with(".zip") {
        "zip"
    } else if lower.ends_with(".tgz") {
        "tgz"
    } else {
        "tar.gz"
    };
    let file = layout
        .tmp()
        .join(format!("dl-{}.{suffix}", uuid::Uuid::new_v4().simple()));
    progress("downloading", 0, None);
    fetch::download(url, integrity, &file, &ctx.fetch, &|b, t| progress("downloading", b, t))
        .await?;
    progress("verifying", 0, None);
    progress("extracting", 0, None);
    let (archive_path, dest) = (file.clone(), staging.to_path_buf());
    let result = tokio::task::spawn_blocking(move || archive::extract(&archive_path, &dest))
        .await
        .map_err(|e| AcpError::Internal(e.to_string()))?;
    let _ = std::fs::remove_file(&file);
    result
}

/// Start the staged agent from an empty directory and run `initialize`.
async fn validate(
    ctx: &InstallContext,
    agent: &CatalogAgent,
    release: &Release,
    staging: &Path,
) -> Result<Option<String>, AcpError> {
    let mut spec: LaunchSpec = resolve::resolve_release(agent, release, staging, ctx)?;
    let empty = ctx
        .layout()
        .tmp()
        .join(format!("validate-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&empty)?;
    spec.cwd = Some(empty.clone());
    if release.data_family.is_some() {
        spec.env
            .insert("OPENCODE_DB".into(), empty.join("validate.db").to_string_lossy().into_owned());
    }
    let result = handshake(ctx, &spec).await;
    let _ = std::fs::remove_dir_all(&empty);
    result
}

async fn handshake(ctx: &InstallContext, spec: &LaunchSpec) -> Result<Option<String>, AcpError> {
    let mut io =
        ctx.connector.connect(spec).await.map_err(|e| {
            AcpError::ValidationFailed(format!("starting the agent: {}", e.detail()))
        })?;
    let child = io.child.take();
    let (peer, _join) = AgentPeer::start(io, None, Arc::new(|_| {}));
    let result = peer
        .request(methods::INITIALIZE, client_initialize(), Some(VALIDATE_TIMEOUT))
        .await;
    let stderr = peer.stderr_tail_bytes(2048);
    peer.close();
    if let Some(child) = child {
        child.terminate(Duration::from_secs(3)).await;
    }
    let value =
        result.map_err(|e| AcpError::ValidationFailed(format!("{}\n{stderr}", e.detail())))?;
    let init: InitializeResponse = serde_json::from_value(value)
        .map_err(|e| AcpError::ValidationFailed(format!("invalid initialize response: {e}")))?;
    if init.protocol_version != PROTOCOL_VERSION {
        return Err(AcpError::ValidationFailed(format!(
            "the agent speaks ACP version {}",
            init.protocol_version
        )));
    }
    Ok(init.agent_info.and_then(|i| i.version))
}

/// Move the staged install into place and record it.
fn commit(
    ctx: &InstallContext,
    agent: &CatalogAgent,
    version: &str,
    staging: &Path,
    final_dir: &Path,
    mut record: InstalledAgent,
) -> Result<(), AcpError> {
    let layout = ctx.layout();
    let mut displaced: Option<PathBuf> = None;
    if final_dir.exists() {
        if (ctx.in_use)(&agent.id, version) {
            let aside = final_dir
                .with_file_name(format!("{version}.old-{}", uuid::Uuid::new_v4().simple()));
            std::fs::rename(final_dir, &aside)?;
            displaced = Some(aside);
        } else {
            std::fs::remove_dir_all(final_dir)?;
        }
    }
    std::fs::rename(staging, final_dir)?;
    record.version = version.to_string();
    record.path = layout.relative(final_dir);
    record.installed_at = now_rfc3339();
    let in_use = ctx.in_use.clone();
    let agent_id = agent.id.clone();
    let previous = update_installed(&layout, |installed| {
        if let Some(aside) = &displaced {
            installed.gc_pending.push(layout.relative(aside));
        }
        let previous = installed.agents.insert(agent_id.clone(), record);
        let mut delete = None;
        if let Some(old) = previous.filter(|old| old.version != version) {
            if in_use(&agent_id, &old.version) {
                installed.gc_pending.push(old.path.clone());
            } else {
                delete = Some(old.path);
            }
        }
        delete
    })?;
    if let Some(old) = previous {
        let _ = std::fs::remove_dir_all(layout.absolute(&old));
    }
    Ok(())
}

/// Version of a `agents/<id>/<version>[.old-…]` directory.
fn path_version(path: &str) -> Option<(String, String)> {
    let mut parts = path.split('/');
    if parts.next()? != "agents" {
        return None;
    }
    let id = parts.next()?.to_string();
    let dir = parts.next()?;
    let version = dir.split(".old-").next().unwrap_or(dir).to_string();
    Some((id, version))
}

/// Delete `gcPending` directories no hosted instance uses.
pub fn gc_sweep(ctx: &InstallContext) {
    let layout = ctx.layout();
    let in_use = ctx.in_use.clone();
    let removed = update_installed(&layout, |installed| {
        let mut removed = Vec::new();
        installed.gc_pending.retain(|path| {
            let busy = path_version(path).is_some_and(|(id, version)| in_use(&id, &version));
            if !busy {
                removed.push(path.clone());
            }
            busy
        });
        removed
    });
    for path in removed.unwrap_or_default() {
        let _ = std::fs::remove_dir_all(layout.absolute(&path));
    }
}

/// Empty `tmp/`, delete leftover `*.staging-*` directories, then
/// [`gc_sweep`].
pub fn startup_cleanup(ctx: &InstallContext) {
    let layout = ctx.layout();
    let _ = std::fs::remove_dir_all(layout.tmp());
    let _ = std::fs::create_dir_all(layout.tmp());
    let mut parents: Vec<PathBuf> = vec![layout.root.join("node")];
    if let Ok(agents) = std::fs::read_dir(layout.root.join("agents")) {
        parents.extend(agents.filter_map(Result::ok).map(|e| e.path()));
    }
    for parent in parents {
        let Ok(entries) = std::fs::read_dir(&parent) else { continue };
        for entry in entries.filter_map(Result::ok) {
            if entry.file_name().to_string_lossy().contains(".staging-") {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    gc_sweep(ctx);
}

/// Remove an installed agent (refused while hosted).
pub fn uninstall(agent_id: &str, ctx: &InstallContext) -> Result<(), AcpError> {
    let layout = ctx.layout();
    let result = (|| {
        let record = super::store::read_installed(&layout)
            .agents
            .get(agent_id)
            .cloned()
            .ok_or_else(|| AcpError::NotInstalled(agent_id.into()))?;
        if (ctx.in_use)(agent_id, &record.version) {
            return Err(AcpError::InUse(format!("{agent_id} is being hosted")));
        }
        update_installed(&layout, |installed| {
            installed.agents.remove(agent_id);
        })?;
        let _ = std::fs::remove_dir_all(layout.absolute(&record.path));
        Ok(record.version)
    })();
    let mut entry = AuditEntry::new("uninstall", agent_id, false).outcome(&result);
    entry.version = result.as_ref().ok().cloned();
    audit::record(&layout, &entry);
    result.map(|_| ())
}
