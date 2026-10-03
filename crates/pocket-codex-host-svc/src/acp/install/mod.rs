//! Agent installer (TRD §4.3): pinned catalog, private Node, verified
//! downloads, safe extraction, `npm ci`, launch specs, settings, audit and
//! remote management routes.
//!
//! Everything takes an [`InstallContext`] so tests never touch the real
//! state directory, network, npm or agents.

pub mod archive;
pub mod audit;
pub mod catalog;
pub mod fetch;
pub mod jobs;
pub mod locks;
pub mod manage;
pub mod node;
pub mod npm;
pub mod platform;
pub mod registry_hint;
pub mod resolve;
pub mod settings;
pub mod store;

pub use catalog::{catalog, parse_catalog, Catalog, CatalogAgent, Release, ReleaseKind};
pub use jobs::{gc_sweep, job, start_install, startup_cleanup, uninstall};
use pocket_codex_core::acp::pcx::{AcpSettingsView, AgentStatus, CustomAgentDef};
pub use resolve::{resolve_launch, resolve_release, InstallContext};

use self::{
    audit::{record, AuditEntry},
    settings::Settings,
    store::read_installed,
};
use super::error::AcpError;

/// State of every catalog and custom agent.
pub fn agents_status(ctx: &InstallContext) -> Vec<AgentStatus> {
    let layout = ctx.layout();
    let installed = read_installed(&layout);
    let settings = Settings::load(&layout).unwrap_or_default();
    let cache = registry_hint::cached(&layout);
    let remote = settings.remote_management();
    let mut out = Vec::new();
    for agent in &ctx.catalog.agents {
        let record = installed.agents.get(&agent.id);
        let running = jobs::running_job(&agent.id, "install");
        let mut status = AgentStatus {
            id: agent.id.clone(),
            name: agent.name.clone(),
            description: agent.description.clone(),
            source: "catalog".into(),
            pinned_version: agent.releases.first().map(|r| r.version.clone()),
            installed_version: record.map(|r| r.version.clone()),
            state: "not_installed".into(),
            registry_version: registry_hint::newer_version(agent, cache.as_ref()),
            approx_size_mb: agent.approx_size_mb,
            needs_node: agent.needs_node(),
            remote_install_allowed: remote && ctx.platform.is_ok(),
            ..AgentStatus::default()
        };
        if let Err(e) = &ctx.platform {
            status.state = "unsupported_platform".into();
            status.detail = Some(e.detail());
        } else if let Some(job) = running {
            status.state = "installing".into();
            status.job_id = Some(job);
        } else if record.is_some() {
            status.state = "installed".into();
            if agent.external_engine.is_some() {
                match resolve_launch(&agent.id, ctx) {
                    Err(e @ AcpError::EngineMissing(_)) => {
                        status.state = "engine_missing".into();
                        status.detail = Some(e.detail());
                    },
                    Err(
                        e @ AcpError::EngineIncompatible {
                            ..
                        },
                    ) => {
                        status.state = "engine_incompatible".into();
                        status.detail = Some(match &e {
                            AcpError::EngineIncompatible {
                                suggested: Some(s), ..
                            } => {
                                format!("{}; suggested {s}", e.detail())
                            },
                            _ => e.detail(),
                        });
                    },
                    _ => {},
                }
            }
        } else if let Some(failed) = jobs::last_failure(&agent.id) {
            status.state = "failed".into();
            status.detail = failed.message;
        }
        if agent.external_engine.is_some() && record.is_none() && status.state == "not_installed" {
            if let Some(path) = resolve::engine_path(agent, &settings, ctx) {
                if let Ok(version) = resolve::engine_version(
                    &path,
                    &agent
                        .external_engine
                        .as_ref()
                        .map(|e| e.version_args.clone())
                        .unwrap_or_default(),
                ) {
                    status.pinned_version = resolve::release_for_engine(agent, &version)
                        .map(|r| r.version.clone())
                        .or(status.pinned_version);
                }
            } else {
                status.state = "engine_missing".into();
            }
        }
        out.push(status);
    }
    for custom in settings.custom() {
        let exists = std::path::Path::new(&custom.command).is_file();
        out.push(AgentStatus {
            id: custom.id.clone(),
            name: if custom.name.is_empty() { custom.id.clone() } else { custom.name.clone() },
            source: "custom".into(),
            state: if exists { "installed" } else { "failed" }.into(),
            detail: (!exists).then(|| format!("{} does not exist", custom.command)),
            ..AgentStatus::default()
        });
    }
    out
}

/// Current settings (tokens never included).
pub fn settings(ctx: &InstallContext) -> Result<AcpSettingsView, AcpError> {
    Ok(settings::view(&Settings::load(&ctx.layout())?, &ctx.catalog))
}

/// Save settings. Returns `(saved, warnings)`; with warnings and
/// `force = false` nothing is saved (D16 coexistence prompt).
pub fn save_settings(
    ctx: &InstallContext,
    view: &AcpSettingsView,
    force: bool,
) -> Result<(bool, Vec<String>), AcpError> {
    let layout = ctx.layout();
    let mut current = Settings::load(&layout)?;
    let before = current.clone();
    let warnings = coexistence_warnings(ctx, &before, view);
    if !warnings.is_empty() && !force {
        return Ok((false, warnings));
    }
    let changed = settings::apply(&mut current, view, &ctx.catalog)?;
    if changed.is_empty() {
        return Ok((true, warnings));
    }
    let result = current.save(&layout);
    for (agent, item) in &changed {
        let mut entry = AuditEntry::new("settings", agent, false).outcome(&result);
        entry.item = Some(item.clone());
        record(&layout, &entry);
    }
    result.map(|()| (true, warnings))
}

/// D16: switching a data family to `shared` while an agent of another family
/// is installed.
fn coexistence_warnings(
    ctx: &InstallContext,
    before: &Settings,
    view: &AcpSettingsView,
) -> Vec<String> {
    let installed = read_installed(&ctx.layout());
    let family_of = |id: &str| -> Option<String> {
        let agent = ctx.agent(id)?;
        let version = &installed.agents.get(id)?.version;
        agent
            .release(version)
            .or(agent.releases.first())?
            .data_family
            .clone()
    };
    let installed_families: Vec<(String, String)> = installed
        .agents
        .keys()
        .filter_map(|id| family_of(id).map(|f| (id.clone(), f)))
        .collect();
    let mut warnings = Vec::new();
    for (family, mode) in &view.opencode_data {
        if mode != "shared" || before.data_mode(family) == "shared" {
            continue;
        }
        for (id, other) in &installed_families {
            if other != family {
                warnings.push(format!(
                    "{id} ({other}) is installed; sharing the {family} database with your own \
                     OpenCode may change data the other version also reads"
                ));
            }
        }
    }
    warnings
}

/// Custom agents.
pub fn custom_agents(ctx: &InstallContext) -> Result<Vec<CustomAgentDef>, AcpError> {
    Ok(Settings::load(&ctx.layout())?.custom())
}

/// Add or replace a custom agent (host desktop only).
pub fn put_custom_agent(ctx: &InstallContext, def: &CustomAgentDef) -> Result<(), AcpError> {
    let layout = ctx.layout();
    let result = (|| {
        settings::validate_custom(def, &ctx.catalog)?;
        let mut current = Settings::load(&layout)?;
        current.put_custom(def);
        current.save(&layout)
    })();
    record(&layout, &AuditEntry::new("custom_agent", &def.id, false).outcome(&result));
    result
}

/// Delete a custom agent (refused while hosted).
pub fn delete_custom_agent(ctx: &InstallContext, id: &str) -> Result<(), AcpError> {
    let layout = ctx.layout();
    let result = (|| {
        if (ctx.in_use)(id, "") {
            return Err(AcpError::InUse(format!("{id} is being hosted")));
        }
        let mut current = Settings::load(&layout)?;
        if !current.delete_custom(id) {
            return Err(AcpError::UnknownAgent(id.into()));
        }
        current.save(&layout)
    })();
    record(&layout, &AuditEntry::new("custom_agent", id, false).outcome(&result));
    result
}

/// The gateway login of `agent_id` configured in `agents.toml`.
pub fn gateway_auth(ctx: &InstallContext, agent_id: &str) -> Option<super::auth::GatewayAuth> {
    Settings::load(&ctx.layout()).ok()?.gateway_auth(agent_id)
}
