//! Launch specs of installed and custom agents (TRD §4.3.8).

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use super::{
    super::{
        error::AcpError,
        launch::{AgentConnector, LaunchSpec, ProcessConnector},
    },
    catalog::{catalog, ArchiveTarget, Catalog, CatalogAgent, Release, ReleaseKind},
    fetch::FetchPolicy,
    node,
    npm::{NpmRunner, ProcessNpm},
    platform::{self, Platform},
    settings::Settings,
    store::{ensure_private_dir, read_installed, InstalledAgent, Layout},
};

/// Predicate: (agent id, version) is used by a hosted instance.
pub type InUse = Arc<dyn Fn(&str, &str) -> bool + Send + Sync>;
/// Major version of the user's own OpenCode, for D19 `auto`.
pub type OpenCodeMajor = Arc<dyn Fn() -> Option<u64> + Send + Sync>;

/// Everything the installer touches, injected so tests never use the real
/// state directory, network, npm or agents. Production:
/// [`InstallContext::production`].
#[derive(Clone)]
pub struct InstallContext {
    /// Managed directory; production = `paths::state_dir()?/acp`.
    pub root: PathBuf,
    /// Production = `catalog()` (embedded); tests pass `parse_catalog(..)`.
    pub catalog: Arc<Catalog>,
    /// Lock name → package-lock.json text; production = `locks::LOCKS`.
    pub locks: Arc<BTreeMap<String, String>>,
    /// Runs npm.
    pub npm: Arc<dyn NpmRunner>,
    /// Download policy.
    pub fetch: FetchPolicy,
    /// Used by install validation; production = `ProcessConnector`.
    pub connector: Arc<dyn AgentConnector>,
    /// Production = `platform::detect()`. An `Err` (e.g. musl Linux) does not
    /// make the context fail: catalog install/resolve paths return it, while
    /// custom agents, settings and `agents_status` keep working (T13).
    pub platform: Result<Platform, AcpError>,
    /// The user's codex (saved config → PATH), from the bridge.
    pub codex_binary: Option<PathBuf>,
    /// Whether a hosted instance currently uses (agent id, version).
    pub in_use: InUse,
    /// D19 detection; production runs the user's `opencode --version`.
    pub opencode_major: OpenCodeMajor,
}

impl InstallContext {
    /// The production context rooted at `paths::state_dir()?/acp`.
    pub fn production(codex_binary: Option<PathBuf>, in_use: InUse) -> Result<Self, AcpError> {
        let state =
            pocket_codex_core::paths::state_dir().map_err(|e| AcpError::Io(e.to_string()))?;
        let root = state.join("acp");
        ensure_private_dir(&root)?;
        let locks = super::locks::LOCKS
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Ok(Self {
            root,
            catalog: Arc::new(catalog().clone()),
            locks: Arc::new(locks),
            npm: Arc::new(ProcessNpm),
            fetch: FetchPolicy::production(),
            connector: Arc::new(ProcessConnector),
            platform: platform::detect(),
            codex_binary,
            in_use,
            opencode_major: Arc::new(detect_opencode_major),
        })
    }

    /// Layout of the managed directory.
    pub fn layout(&self) -> Layout {
        Layout::new(&self.root)
    }

    /// Catalog agent by id.
    pub fn agent(&self, id: &str) -> Option<&CatalogAgent> {
        self.catalog.agents.iter().find(|a| a.id == id)
    }
}

/// First `\d+.\d+.\d+` in `text`.
pub fn parse_version(text: &str) -> Option<semver::Version> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            let mut dots = 0;
            let mut j = i;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || (bytes[j] == b'.' && dots < 2)) {
                if bytes[j] == b'.' {
                    dots += 1;
                }
                j += 1;
            }
            let candidate = text[start..j].trim_end_matches('.');
            if candidate.matches('.').count() == 2 {
                if let Ok(v) = semver::Version::parse(candidate) {
                    return Some(v);
                }
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    None
}

/// Run `<binary> <args>` (5 s) and parse its version.
pub fn engine_version(binary: &Path, args: &[String]) -> Result<semver::Version, AcpError> {
    let mut child = std::process::Command::new(binary)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| AcpError::EngineMissing(format!("{}: {e}", binary.display())))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AcpError::EngineMissing(format!(
                    "{} --version timed out",
                    binary.display()
                )));
            },
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|e| AcpError::EngineMissing(e.to_string()))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    parse_version(&text).ok_or_else(|| {
        AcpError::EngineMissing(format!("cannot read the version of {}", binary.display()))
    })
}

fn detect_opencode_major() -> Option<u64> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        let exe = if cfg!(windows) { "opencode.exe" } else { "opencode" };
        candidates.extend(std::env::split_paths(&path).map(|d| d.join(exe)));
    }
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        candidates.push(
            PathBuf::from(home)
                .join(".opencode")
                .join("bin")
                .join("opencode"),
        );
    }
    let binary = candidates.into_iter().find(|p| p.is_file())?;
    engine_version(&binary, &["--version".to_string()])
        .ok()
        .map(|v| v.major)
}

/// The engine path an external-engine agent uses.
pub fn engine_path(
    agent: &CatalogAgent,
    settings: &Settings,
    ctx: &InstallContext,
) -> Option<PathBuf> {
    let engine = agent.external_engine.as_ref()?;
    settings
        .agent_str(&agent.id, &engine.setting)
        .map(PathBuf::from)
        .or_else(|| ctx.codex_binary.clone())
}

/// Release matching the engine version, newest first.
pub fn release_for_engine<'a>(
    agent: &'a CatalogAgent,
    version: &semver::Version,
) -> Option<&'a Release> {
    agent.releases.iter().find(|r| {
        r.engine_range
            .as_deref()
            .and_then(|range| semver::VersionReq::parse(range).ok())
            .is_some_and(|req| req.matches(version))
    })
}

/// Archive target for `platform` (baseline first without AVX2).
pub fn archive_target<'a>(
    targets: &'a BTreeMap<String, ArchiveTarget>,
    platform: &Platform,
) -> Option<&'a ArchiveTarget> {
    platform
        .archive_keys()
        .iter()
        .find_map(|key| targets.get(key))
}

/// 32 random hex bytes (two v4 uuids).
fn random_hex() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

/// Launch a specific release from a specific directory; used by
/// [`resolve_launch`] and by install validation (on the staging directory).
pub fn resolve_release(
    agent: &CatalogAgent,
    release: &Release,
    dir: &Path,
    ctx: &InstallContext,
) -> Result<LaunchSpec, AcpError> {
    let settings = Settings::load(&ctx.layout())?;
    let platform = ctx.platform.clone()?;
    let (program, mut args) = match &release.kind {
        ReleaseKind::Npm {
            entry, ..
        } => {
            let runtime = node::installed_runtime(ctx)
                .ok_or_else(|| AcpError::NotInstalled("the private Node runtime".into()))?;
            (runtime.node, vec![dir.join(entry).to_string_lossy().into_owned()])
        },
        ReleaseKind::Archive {
            targets,
        } => {
            let target = archive_target(targets, &platform).ok_or_else(|| {
                AcpError::UnsupportedPlatform(format!(
                    "{} has no build for {}",
                    agent.name, platform.key
                ))
            })?;
            let cmd = target.cmd.clone().unwrap_or_else(|| agent.id.clone());
            (dir.join(cmd), Vec::new())
        },
    };
    args.extend(release.args.iter().cloned());
    let mut env = release.env.clone();
    for name in &release.random_env {
        env.insert(name.clone(), random_hex());
    }
    if let Some(engine) = &agent.external_engine {
        let path = engine_path(agent, &settings, ctx).ok_or_else(|| {
            AcpError::EngineMissing(format!("{} needs `{}`", agent.name, engine.binary))
        })?;
        let version = engine_version(&path, &engine.version_args)?;
        if let Some(range) = &release.engine_range {
            let req = semver::VersionReq::parse(range)
                .map_err(|e| AcpError::Internal(format!("bad engine range: {e}")))?;
            if !req.matches(&version) {
                return Err(AcpError::EngineIncompatible {
                    found: version.to_string(),
                    required: range.clone(),
                    suggested: release_for_engine(agent, &version).map(|r| r.version.clone()),
                });
            }
        }
        env.insert(engine.env.clone(), path.to_string_lossy().into_owned());
    }
    if let Some(override_) = &agent.engine_override {
        if let Some(path) = settings.agent_str(&agent.id, &override_.setting) {
            env.insert(override_.env.clone(), path);
        }
    }
    let mut launch_only_args = Vec::new();
    for conditional in &release.conditional_args {
        let value = settings
            .agent_bool(&agent.id, &conditional.setting)
            .unwrap_or(conditional.default);
        launch_only_args.extend(if value {
            conditional.when_true.clone()
        } else {
            conditional.when_false.clone()
        });
    }
    if let Some(family) = &release.data_family {
        let mode = settings.data_mode(family);
        let isolated = match mode.as_str() {
            "shared" => false,
            "isolated" => true,
            _ => !family_matches(family, (ctx.opencode_major)()),
        };
        if isolated {
            let data = ctx.layout().data(family);
            ensure_private_dir(&data)?;
            env.insert(
                "OPENCODE_DB".into(),
                data.join("opencode.db").to_string_lossy().into_owned(),
            );
        }
    }
    Ok(LaunchSpec {
        agent_id: agent.id.clone(),
        display_name: agent.name.clone(),
        program,
        args,
        env,
        env_remove: Vec::new(),
        version: Some(release.version.clone()),
        pinned: agent.release(&release.version).is_some(),
        cwd: None,
        launch_only_args,
    })
}

/// `opencode-v<major>` matches the detected major version.
fn family_matches(family: &str, major: Option<u64>) -> bool {
    let wanted = family
        .strip_prefix("opencode-v")
        .and_then(|n| n.parse::<u64>().ok());
    wanted.is_some() && wanted == major
}

/// Synthesize the release of a registry-sourced install.
fn registry_release(agent: &CatalogAgent, record: &InstalledAgent, platform: &Platform) -> Release {
    let template = agent.releases.first();
    let kind = if record.kind == "npm" {
        ReleaseKind::Npm {
            package: String::new(),
            entry: record.entry.clone().unwrap_or_default(),
            lock: String::new(),
            omit: Vec::new(),
            platforms: Vec::new(),
        }
    } else {
        let target = ArchiveTarget {
            cmd: record.cmd.clone(),
            ..ArchiveTarget::default()
        };
        ReleaseKind::Archive {
            targets: [(platform.key.to_string(), target)].into(),
        }
    };
    Release {
        version: record.version.clone(),
        engine_range: None,
        kind,
        args: if record.args.is_empty() {
            template.map(|r| r.args.clone()).unwrap_or_default()
        } else {
            record.args.clone()
        },
        env: template.map(|r| r.env.clone()).unwrap_or_default(),
        random_env: template.map(|r| r.random_env.clone()).unwrap_or_default(),
        data_family: template.and_then(|r| r.data_family.clone()),
        conditional_args: template
            .map(|r| r.conditional_args.clone())
            .unwrap_or_default(),
    }
}

/// Launch an installed catalog agent or a custom agent.
pub fn resolve_launch(agent_id: &str, ctx: &InstallContext) -> Result<LaunchSpec, AcpError> {
    let layout = ctx.layout();
    let settings = Settings::load(&layout)?;
    if let Some(custom) = settings.custom().into_iter().find(|c| c.id == agent_id) {
        let program = PathBuf::from(&custom.command);
        if !program.is_absolute() || !program.is_file() {
            return Err(AcpError::NotInstalled(format!("{} ({})", custom.name, custom.command)));
        }
        return Ok(LaunchSpec {
            agent_id: custom.id.clone(),
            display_name: if custom.name.is_empty() {
                custom.id.clone()
            } else {
                custom.name.clone()
            },
            program,
            args: custom.args.clone(),
            env: custom.env.iter().cloned().collect(),
            env_remove: Vec::new(),
            version: None,
            pinned: false,
            cwd: None,
            launch_only_args: Vec::new(),
        });
    }
    let agent = ctx
        .agent(agent_id)
        .ok_or_else(|| AcpError::UnknownAgent(agent_id.into()))?;
    let platform = ctx.platform.clone()?;
    let record = read_installed(&layout)
        .agents
        .get(agent_id)
        .cloned()
        .ok_or_else(|| AcpError::NotInstalled(agent.name.clone()))?;
    let dir = layout.absolute(&record.path);
    let mut spec = match (record.source.as_str(), agent.release(&record.version)) {
        ("catalog", Some(release)) => resolve_release(agent, release, &dir, ctx)?,
        _ => {
            let mut spec =
                resolve_release(agent, &registry_release(agent, &record, &platform), &dir, ctx)?;
            spec.pinned = false;
            spec
        },
    };
    if matches!(agent.release(&record.version).map(|r| &r.kind), Some(ReleaseKind::Archive { .. }))
        || record.kind == "archive"
    {
        if let Some(binary) = settings.agent_str(agent_id, "binary") {
            spec.program = PathBuf::from(binary);
        }
    }
    Ok(spec)
}
