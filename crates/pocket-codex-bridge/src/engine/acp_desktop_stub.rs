//! Mobile stand-ins for the desktop-only ACP hosting modules (TRD §1): the
//! same signatures, returning "ACP hosting is only available on desktop".

fn desktop_only() -> anyhow::Error {
    anyhow::anyhow!("ACP hosting is only available on desktop")
}

/// Stub of `engine::serve_acp`.
pub mod serve_acp {
    use anyhow::Result;

    pub use crate::engine::acp::AcpServeReport;
    use crate::engine::serve::ServeStatus;

    /// Always false on mobile.
    pub(crate) fn is_hosting(_name: &str) -> bool {
        false
    }

    /// No ACP hosts on mobile.
    pub(crate) fn status() -> Vec<ServeStatus> {
        Vec::new()
    }

    /// No ACP hosts on mobile.
    pub(crate) fn local_endpoints(_service_key: &str) -> Option<(String, String)> {
        None
    }

    /// Refused on mobile.
    pub(crate) fn start(_name: Option<String>, _agent_id: String) -> Result<AcpServeReport> {
        Err(super::desktop_only())
    }

    /// Refused on mobile.
    pub(crate) fn deregister(_name: &str, _kind: &str) -> Result<()> {
        Err(super::desktop_only())
    }

    /// Refused on mobile.
    pub(crate) fn reregister(_name: &str, _kind: &str) -> Result<()> {
        Err(super::desktop_only())
    }

    /// Nothing to stop on mobile.
    pub(crate) fn stop(_name: &str) {}

    /// Nothing to stop on mobile.
    pub(crate) fn stop_all() {}
}

/// Stub of `engine::acp_manage`.
pub mod acp_manage {
    use anyhow::Result;
    use pocket_codex_core::acp::pcx::{
        AcpSettingsView, AgentStatus, AuthState, CustomAgentDef, JobProgress,
    };

    /// Nothing to register on mobile.
    pub(crate) fn register() {}

    /// Refused on mobile.
    pub(crate) fn agents() -> Result<Vec<AgentStatus>> {
        Err(super::desktop_only())
    }

    /// Refused on mobile.
    pub(crate) fn install(_agent_id: &str, _version: Option<&str>) -> Result<String> {
        Err(super::desktop_only())
    }

    /// No local jobs on mobile.
    pub(crate) fn job(_id: &str) -> Option<JobProgress> {
        None
    }

    /// Refused on mobile.
    pub(crate) fn uninstall(_agent_id: &str) -> Result<()> {
        Err(super::desktop_only())
    }

    /// Refused on mobile.
    pub(crate) fn settings() -> Result<AcpSettingsView> {
        Err(super::desktop_only())
    }

    /// Refused on mobile.
    pub(crate) fn save_settings(
        _view: AcpSettingsView,
        _force: bool,
    ) -> Result<(bool, Vec<String>)> {
        Err(super::desktop_only())
    }

    /// Refused on mobile.
    pub(crate) fn custom_agents() -> Result<Vec<CustomAgentDef>> {
        Err(super::desktop_only())
    }

    /// Refused on mobile.
    pub(crate) fn put_custom_agent(_def: CustomAgentDef) -> Result<()> {
        Err(super::desktop_only())
    }

    /// Refused on mobile.
    pub(crate) fn delete_custom_agent(_id: &str) -> Result<()> {
        Err(super::desktop_only())
    }

    /// Refused on mobile.
    pub(crate) fn auth_terminal(_name: &str, _method_id: &str) -> Result<()> {
        Err(super::desktop_only())
    }

    /// Refused on mobile.
    pub(crate) fn auth_agent(_name: &str, _method_id: &str) -> Result<AuthState> {
        Err(super::desktop_only())
    }

    /// Refused on mobile.
    pub(crate) fn auth_recheck(_name: &str) -> Result<AuthState> {
        Err(super::desktop_only())
    }
}
