//! Errors of the ACP hub and installer (TRD §6).
//!
//! Every error renders as `[acp.<code>] <detail>`; [`AcpError::code`] returns
//! the `acp.<code>` part and [`AcpError::to_rpc`] builds the JSON-RPC error a
//! controller receives (T16).

use pocket_codex_core::acp::rpc::{code, pcx_error, RpcError};

/// ACP hub / installer error.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AcpError {
    /// The agent is not installed.
    #[error("[acp.not_installed] {0} is not installed")]
    NotInstalled(String),
    /// The platform or libc is not supported.
    #[error("[acp.unsupported_platform] {0}")]
    UnsupportedPlatform(String),
    /// The required engine (codex) was not found.
    #[error("[acp.engine_missing] {0}")]
    EngineMissing(String),
    /// The engine version is outside the supported range.
    #[error("[acp.engine_incompatible] found {found}, requires {required}")]
    EngineIncompatible {
        /// Version found.
        found: String,
        /// Supported range.
        required: String,
        /// Adapter version that would match, if any.
        suggested: Option<String>,
    },
    /// The agent negotiated an ACP version other than 1.
    #[error("[acp.protocol_unsupported] the agent speaks ACP version {0}")]
    ProtocolUnsupported(u16),
    /// The agent process could not be started.
    #[error("[acp.agent_start_failed] {message}")]
    AgentStartFailed {
        /// What failed.
        message: String,
        /// Last 2 KiB of stderr.
        stderr_tail: String,
    },
    /// `initialize` did not finish in time.
    #[error("[acp.initialize_timeout] the agent did not finish initialize in time")]
    InitializeTimeout {
        /// Last 2 KiB of stderr.
        stderr_tail: String,
    },
    /// The agent process is restarting or stopped.
    #[error("[acp.agent_unavailable] {0}")]
    AgentUnavailable(String),
    /// The agent requires authentication (`-32000`).
    #[error("[acp.auth_required] {0}")]
    AuthRequired(String),
    /// The operation can only be done on the host desktop.
    #[error("[acp.host_only] {0}")]
    HostOnly(String),
    /// The agent can neither load nor resume this session.
    #[error("[acp.session_not_loadable] {0}")]
    SessionNotLoadable(String),
    /// The transcript generation changed.
    #[error("[acp.generation_changed] the transcript changed; re-read the window")]
    GenerationChanged,
    /// A request timed out.
    #[error("[acp.timeout] {0}")]
    Timeout(String),
    /// The agent does not accept images.
    #[error("[acp.images_unsupported] this agent does not accept images")]
    ImagesUnsupported,
    /// A download failed or was redirected off the allowlist.
    #[error("[acp.download_failed] {0}")]
    DownloadFailed(String),
    /// Integrity check failed.
    #[error("[acp.integrity_mismatch] {0}")]
    IntegrityMismatch(String),
    /// An archive contained unsafe entries.
    #[error("[acp.archive_rejected] {0}")]
    ArchiveRejected(String),
    /// `npm ci` failed.
    #[error("[acp.npm_failed] {0}")]
    NpmFailed(String),
    /// Not enough disk space.
    #[error("[acp.disk_space] about {needed_mb} MB of free disk space is needed")]
    DiskSpace {
        /// Space needed.
        needed_mb: u64,
    },
    /// The post-install handshake failed.
    #[error("[acp.validation_failed] {0}")]
    ValidationFailed(String),
    /// A hosted instance still uses the agent.
    #[error("[acp.in_use] {0}")]
    InUse(String),
    /// A job for the agent is already running.
    #[error("[acp.job_running] {0}")]
    JobRunning(String),
    /// Remote management is turned off on the host.
    #[error("[acp.remote_management_disabled] remote management is turned off on this host")]
    RemoteManagementDisabled,
    /// The version cannot be installed remotely.
    #[error("[acp.version_not_pinned] {0}")]
    VersionNotPinned(String),
    /// No terminal program was found.
    #[error("[acp.no_terminal] no terminal program found; run this command manually: {command}")]
    NoTerminal {
        /// The command to run by hand.
        command: String,
    },
    /// Any other JSON-RPC error from the agent.
    #[error("[acp.agent_error] {message}")]
    AgentError {
        /// JSON-RPC code.
        code: i64,
        /// Agent message.
        message: String,
    },
    /// The session is still loading.
    #[error("[acp.session_loading] the session is still loading")]
    SessionLoading,
    /// No working directory was given and the host has no default project.
    #[error("[acp.cwd_required] choose a project directory first")]
    CwdRequired,
    /// Unknown agent id.
    #[error("[acp.unknown_agent] {0}")]
    UnknownAgent(String),
    /// Unknown job id.
    #[error("[acp.unknown_job] {0}")]
    UnknownJob(String),
    /// The instance name is taken.
    #[error("[acp.name_conflict] {0}")]
    NameConflict(String),
    /// Invalid parameters.
    #[error("[acp.invalid_params] {0}")]
    InvalidParams(String),
    /// A resource (session, file) was not found.
    #[error("[acp.not_found] {0}")]
    NotFound(String),
    /// I/O failure.
    #[error("[acp.io] {0}")]
    Io(String),
    /// Unexpected internal failure.
    #[error("[acp.internal] {0}")]
    Internal(String),
}

impl AcpError {
    /// `acp.<code>` of this error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotInstalled(_) => "acp.not_installed",
            Self::UnsupportedPlatform(_) => "acp.unsupported_platform",
            Self::EngineMissing(_) => "acp.engine_missing",
            Self::EngineIncompatible {
                ..
            } => "acp.engine_incompatible",
            Self::ProtocolUnsupported(_) => "acp.protocol_unsupported",
            Self::AgentStartFailed {
                ..
            } => "acp.agent_start_failed",
            Self::InitializeTimeout {
                ..
            } => "acp.initialize_timeout",
            Self::AgentUnavailable(_) => "acp.agent_unavailable",
            Self::AuthRequired(_) => "acp.auth_required",
            Self::HostOnly(_) => "acp.host_only",
            Self::SessionNotLoadable(_) => "acp.session_not_loadable",
            Self::GenerationChanged => "acp.generation_changed",
            Self::Timeout(_) => "acp.timeout",
            Self::ImagesUnsupported => "acp.images_unsupported",
            Self::DownloadFailed(_) => "acp.download_failed",
            Self::IntegrityMismatch(_) => "acp.integrity_mismatch",
            Self::ArchiveRejected(_) => "acp.archive_rejected",
            Self::NpmFailed(_) => "acp.npm_failed",
            Self::DiskSpace {
                ..
            } => "acp.disk_space",
            Self::ValidationFailed(_) => "acp.validation_failed",
            Self::InUse(_) => "acp.in_use",
            Self::JobRunning(_) => "acp.job_running",
            Self::RemoteManagementDisabled => "acp.remote_management_disabled",
            Self::VersionNotPinned(_) => "acp.version_not_pinned",
            Self::NoTerminal {
                ..
            } => "acp.no_terminal",
            Self::AgentError {
                ..
            } => "acp.agent_error",
            Self::SessionLoading => "acp.session_loading",
            Self::CwdRequired => "acp.cwd_required",
            Self::UnknownAgent(_) => "acp.unknown_agent",
            Self::UnknownJob(_) => "acp.unknown_job",
            Self::NameConflict(_) => "acp.name_conflict",
            Self::InvalidParams(_) => "acp.invalid_params",
            Self::NotFound(_) => "acp.not_found",
            Self::Io(_) => "acp.io",
            Self::Internal(_) => "acp.internal",
        }
    }

    /// The message without the `[acp.<code>] ` prefix.
    pub fn detail(&self) -> String {
        let rendered = self.to_string();
        match rendered.split_once("] ") {
            Some((_, rest)) => rest.to_string(),
            None => rendered,
        }
    }

    /// The JSON-RPC error a controller receives (T16).
    pub fn to_rpc(&self) -> RpcError {
        let number = match self {
            Self::AgentUnavailable(_) => code::AGENT_UNAVAILABLE,
            Self::AuthRequired(_) => code::AUTH_REQUIRED,
            Self::HostOnly(_) => code::HOST_ONLY,
            Self::SessionNotLoadable(_) => code::SESSION_NOT_LOADABLE,
            Self::GenerationChanged => code::GENERATION_CHANGED,
            Self::SessionLoading => code::SESSION_LOADING,
            Self::ImagesUnsupported | Self::CwdRequired | Self::InvalidParams(_) => {
                code::INVALID_PARAMS
            },
            Self::NotFound(_) => code::RESOURCE_NOT_FOUND,
            Self::AgentError {
                code, ..
            } => *code,
            _ => code::INTERNAL_ERROR,
        };
        pcx_error(number, self.code(), self.detail())
    }

    /// Classify an error returned by the agent.
    pub fn from_agent(error: RpcError) -> Self {
        if error.code == code::AUTH_REQUIRED {
            Self::AuthRequired(error.message)
        } else {
            Self::AgentError {
                code: error.code,
                message: error.message,
            }
        }
    }

    /// Whether the agent reported `-32000`.
    pub fn is_auth_required(&self) -> bool {
        matches!(self, Self::AuthRequired(_))
    }
}

impl From<std::io::Error> for AcpError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use pocket_codex_core::acp::rpc::pcx_code_of;

    use super::*;

    #[test]
    fn display_code_and_rpc_agree() {
        let error = AcpError::SessionLoading;
        assert_eq!(error.code(), "acp.session_loading");
        assert!(error.to_string().starts_with("[acp.session_loading] "));
        let rpc = error.to_rpc();
        assert_eq!(rpc.code, code::SESSION_LOADING);
        assert_eq!(pcx_code_of(&rpc.message), Some("acp.session_loading"));
        assert!(!rpc.message.contains("[acp.session_loading] [acp."));
        let agent = AcpError::from_agent(RpcError::new(-32000, "login first"));
        assert!(agent.is_auth_required());
        assert_eq!(agent.to_rpc().code, code::AUTH_REQUIRED);
        let other = AcpError::from_agent(RpcError::new(-32603, "boom"));
        assert_eq!(other.to_rpc().message, "[acp.agent_error] boom");
    }
}
