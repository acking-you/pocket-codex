//! `/acp/v1/*` remote management routes (TRD §4.3.11, D14).
//!
//! Stateless: the desktop bridge registers an [`AcpManagement`] once; without
//! it every route answers 404 (for example in the CLI). Requests arrive
//! through the relay and are always treated as `remote = true`.

use std::sync::{Arc, OnceLock, RwLock};

use async_trait::async_trait;
use axum::{
    extract::{DefaultBodyLimit, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use pocket_codex_core::acp::pcx::{AgentStatus, JobProgress};
use serde::Deserialize;
use serde_json::json;

use super::{super::error::AcpError, settings::valid_id};

/// Request body limit.
const BODY_LIMIT: usize = 4 * 1024;

/// What the routes delegate to.
#[async_trait]
pub trait AcpManagement: Send + Sync {
    /// Catalog, custom and installed agents with their state.
    async fn agents(&self) -> Vec<AgentStatus>;
    /// Install the pinned version; returns a job id.
    async fn install(&self, agent_id: &str, remote: bool) -> Result<String, AcpError>;
    /// Job progress.
    fn job(&self, id: &str) -> Option<JobProgress>;
    /// Validates synchronously (policy, installed, name clash) and returns a
    /// `host` job id.
    async fn host(
        &self,
        agent_id: &str,
        name: Option<String>,
        remote: bool,
    ) -> Result<String, AcpError>;
    /// Remote management is switched on.
    fn remote_allowed(&self) -> bool;
}

fn slot() -> &'static RwLock<Option<Arc<dyn AcpManagement>>> {
    static SLOT: OnceLock<RwLock<Option<Arc<dyn AcpManagement>>>> = OnceLock::new();
    SLOT.get_or_init(|| RwLock::new(None))
}

/// Register the implementation (once, by the desktop bridge).
pub fn set_management(management: Arc<dyn AcpManagement>) {
    *slot()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(management);
}

fn management() -> Option<Arc<dyn AcpManagement>> {
    slot()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// The `/acp/v1` routes.
pub fn router() -> Router {
    Router::new()
        .route("/acp/v1/agents", get(agents))
        .route("/acp/v1/agents/{id}/install", post(install))
        .route("/acp/v1/agents/{id}/host", post(host))
        .route("/acp/v1/jobs/{id}", get(job))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
}

fn status_of(error: &AcpError) -> StatusCode {
    match error {
        AcpError::RemoteManagementDisabled | AcpError::VersionNotPinned(_) => StatusCode::FORBIDDEN,
        AcpError::UnknownAgent(_) | AcpError::UnknownJob(_) => StatusCode::NOT_FOUND,
        AcpError::JobRunning(_) | AcpError::NameConflict(_) | AcpError::InUse(_) => {
            StatusCode::CONFLICT
        },
        AcpError::NotInstalled(_) => StatusCode::FAILED_DEPENDENCY,
        AcpError::UnsupportedPlatform(_)
        | AcpError::EngineMissing(_)
        | AcpError::EngineIncompatible {
            ..
        }
        | AcpError::DiskSpace {
            ..
        }
        | AcpError::InvalidParams(_) => StatusCode::UNPROCESSABLE_ENTITY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn error(error: AcpError) -> Response {
    let body = json!({ "code": error.code(), "message": error.detail() });
    (status_of(&error), Json(body)).into_response()
}

fn not_registered() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

fn check_id(id: &str) -> Result<(), AcpError> {
    if valid_id(id) {
        Ok(())
    } else {
        Err(AcpError::UnknownAgent(id.into()))
    }
}

async fn agents() -> Response {
    let Some(m) = management() else { return not_registered() };
    let agents = m.agents().await;
    Json(json!({ "remoteManagement": m.remote_allowed(), "agents": agents })).into_response()
}

async fn install(Path(id): Path<String>) -> Response {
    let Some(m) = management() else { return not_registered() };
    if let Err(e) = check_id(&id) {
        return error(e);
    }
    match m.install(&id, true).await {
        Ok(job) => (StatusCode::ACCEPTED, Json(json!({ "jobId": job }))).into_response(),
        Err(e) => error(e),
    }
}

#[derive(Default, Deserialize)]
struct HostBody {
    #[serde(default)]
    name: Option<String>,
}

async fn host(Path(id): Path<String>, body: Option<Json<HostBody>>) -> Response {
    let Some(m) = management() else { return not_registered() };
    if let Err(e) = check_id(&id) {
        return error(e);
    }
    let name = body
        .and_then(|Json(b)| b.name)
        .filter(|n| !n.trim().is_empty());
    match m.host(&id, name, true).await {
        Ok(job) => (StatusCode::ACCEPTED, Json(json!({ "jobId": job }))).into_response(),
        Err(e) => error(e),
    }
}

async fn job(Path(id): Path<String>) -> Response {
    let Some(m) = management() else { return not_registered() };
    match m.job(&id) {
        Some(progress) => Json(progress).into_response(),
        None => error(AcpError::UnknownJob(id)),
    }
}
