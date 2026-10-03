//! Facade between the FRB functions and the desktop ACP installer / hubs
//! (TRD §4.4.1), plus the [`AcpManagement`] behind the `/acp/v1` routes.
//!
//! Only types that compile on every platform cross this boundary (the
//! `core::acp::pcx` views); mobile builds use `acp_desktop_stub::acp_manage`.

use std::{path::PathBuf, sync::Arc};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use pocket_codex_core::acp::pcx::{
    AcpSettingsView, AgentStatus, AuthState, CustomAgentDef, JobProgress,
};
use pocket_codex_host_svc::acp::{
    install::{
        self,
        audit::{self, AuditEntry},
        jobs,
        manage::{set_management, AcpManagement},
        InstallContext,
    },
    AcpError,
};

use super::{runtime, serve, serve_acp};

/// The installer context of this process (desktop only).
pub type InstallContextHandle = InstallContext;

/// Register the remote management implementation and clean up leftovers of
/// interrupted installs (once, at bridge start).
pub fn register() {
    set_management(Arc::new(Management));
    runtime::runtime().spawn(async {
        let ctx = match tokio::task::spawn_blocking(install_context).await {
            Ok(Ok(ctx)) => ctx,
            _ => return,
        };
        let cleanup = ctx.clone();
        let _ = tokio::task::spawn_blocking(move || install::startup_cleanup(&cleanup)).await;
        if let Err(e) = install::registry_hint::refresh(&ctx).await {
            tracing::debug!(error = %e, "ACP registry hint refresh failed");
        }
    });
}

/// `InstallContext::production` with the user's codex and the hosted agents.
pub fn install_context() -> Result<InstallContextHandle> {
    InstallContext::production(serve::codex_locate().map(PathBuf::from), Arc::new(serve_acp::uses))
        .map_err(|e| anyhow!(e))
}

/// Every catalog and custom agent, with the instances hosting it.
pub fn agents() -> Result<Vec<AgentStatus>> {
    let ctx = install_context()?;
    let mut agents = install::agents_status(&ctx);
    for agent in &mut agents {
        agent.hosted_names = serve_acp::hosted_names(&agent.id);
    }
    Ok(agents)
}

/// Start installing `agent_id` (local, so registry versions are allowed).
pub fn install(agent_id: &str, version: Option<&str>) -> Result<String> {
    let ctx = install_context()?;
    runtime::runtime()
        .block_on(install::start_install(agent_id, version, false, &ctx))
        .map_err(|e| anyhow!(e))
}

/// Progress of an install or host job.
pub fn job(id: &str) -> Option<JobProgress> {
    install::job(id)
}

/// Uninstall `agent_id` (refused while hosted).
pub fn uninstall(agent_id: &str) -> Result<()> {
    install::uninstall(agent_id, &install_context()?).map_err(|e| anyhow!(e))
}

/// Current settings (tokens withheld).
pub fn settings() -> Result<AcpSettingsView> {
    install::settings(&install_context()?).map_err(|e| anyhow!(e))
}

/// Save settings; `(saved, warnings)`.
pub fn save_settings(view: AcpSettingsView, force: bool) -> Result<(bool, Vec<String>)> {
    install::save_settings(&install_context()?, &view, force).map_err(|e| anyhow!(e))
}

/// Custom agents.
pub fn custom_agents() -> Result<Vec<CustomAgentDef>> {
    install::custom_agents(&install_context()?).map_err(|e| anyhow!(e))
}

/// Add or replace a custom agent.
pub fn put_custom_agent(def: CustomAgentDef) -> Result<()> {
    install::put_custom_agent(&install_context()?, &def).map_err(|e| anyhow!(e))
}

/// Delete a custom agent.
pub fn delete_custom_agent(id: &str) -> Result<()> {
    install::delete_custom_agent(&install_context()?, id).map_err(|e| anyhow!(e))
}

fn hub_of(name: &str) -> Result<Arc<pocket_codex_host_svc::acp::AcpHub>> {
    serve_acp::hub(name).ok_or_else(|| anyhow!("`{name}` is not hosting an ACP agent"))
}

/// Open the host terminal for a terminal login of host `name`.
pub fn auth_terminal(name: &str, method_id: &str) -> Result<()> {
    hub_of(name)?
        .terminal_login(method_id)
        .map_err(|e| anyhow!(e))
}

/// Start an agent-type login; returns `inProgress`.
pub fn auth_agent(name: &str, method_id: &str) -> Result<AuthState> {
    let hub = hub_of(name)?;
    runtime::runtime()
        .block_on(hub.authenticate(method_id))
        .map_err(|e| anyhow!(e))
}

/// Restart the agent of host `name` and re-detect authentication.
pub fn auth_recheck(name: &str) -> Result<AuthState> {
    let hub = hub_of(name)?;
    runtime::runtime()
        .block_on(hub.recheck_auth())
        .map_err(|e| anyhow!(e))
}

/// Backs the `/acp/v1` routes (always `remote = true` through the relay).
struct Management;

fn context() -> Result<InstallContext, AcpError> {
    install_context().map_err(|e| AcpError::Internal(format!("{e:#}")))
}

#[async_trait]
impl AcpManagement for Management {
    async fn agents(&self) -> Vec<AgentStatus> {
        tokio::task::spawn_blocking(agents)
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default()
    }

    async fn install(&self, agent_id: &str, remote: bool) -> Result<String, AcpError> {
        let ctx = context()?;
        install::start_install(agent_id, None, remote, &ctx).await
    }

    fn job(&self, id: &str) -> Option<JobProgress> {
        install::job(id)
    }

    async fn host(
        &self,
        agent_id: &str,
        name: Option<String>,
        remote: bool,
    ) -> Result<String, AcpError> {
        let ctx = context()?;
        let result = (|| {
            if remote && !install::settings(&ctx)?.remote_management {
                return Err(AcpError::RemoteManagementDisabled);
            }
            if ctx.agent(agent_id).is_none() {
                // Custom agents run user commands; they start only on the host.
                return Err(AcpError::UnknownAgent(agent_id.into()));
            }
            let spec = install::resolve_launch(agent_id, &ctx)?;
            let name = pocket_codex_core::service::sanitize_component(
                name.as_deref()
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or(agent_id),
            );
            let taken = if serve::is_hosting_codex(&name) {
                Some("Codex")
            } else if super::serve_opencode::is_hosting(&name) {
                Some("OpenCode")
            } else if serve_acp::is_hosting(&name)
                && !serve_acp::hosted_names(agent_id).contains(&name)
            {
                Some("another ACP agent")
            } else {
                None
            };
            if let Some(provider) = taken {
                return Err(AcpError::NameConflict(format!(
                    "`{name}` is already hosting {provider}"
                )));
            }
            Ok((name, spec.version.unwrap_or_default()))
        })();
        let mut entry = AuditEntry::new("host", agent_id, remote).outcome(&result);
        entry.version = result.as_ref().ok().map(|(_, v)| v.clone());
        audit::record(&ctx.layout(), &entry);
        let (name, version) = result?;
        let job = jobs::new_job("host", agent_id, &version);
        jobs::update(&job, |p| p.state = "starting".into());
        let (job_id, agent) = (job.clone(), agent_id.to_string());
        tokio::task::spawn_blocking(move || {
            let outcome = serve_acp::start(Some(name), agent)
                .map(|report| Some(report.service_key))
                .map_err(|e| match e.downcast::<AcpError>() {
                    Ok(e) => e,
                    Err(e) => AcpError::Internal(format!("{e:#}")),
                });
            jobs::finish(&job_id, outcome);
        });
        Ok(job)
    }

    fn remote_allowed(&self) -> bool {
        install_context()
            .ok()
            .and_then(|ctx| install::settings(&ctx).ok())
            .is_some_and(|s| s.remote_management)
    }
}
