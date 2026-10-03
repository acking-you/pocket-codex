//! Access to an existing external OpenCode (v2 `/api/*`) server.
//!
//! The host uses [`discovery`] to attach to the user's background service and
//! [`gateway`] to publish a loopback, credential-injecting, route-allowlisted
//! proxy of it. Controllers talk to that gateway (or a relay tunnel to it) with
//! the same [`Client`], without credentials. Nothing here starts, stops, or
//! reconfigures OpenCode itself.

mod client;
pub mod contract;
pub mod discovery;
pub mod forms;
pub mod gateway;
mod protocol;
mod session_dirs;
mod sse;

use std::fmt;

pub use client::{Client, PromptRequest, SessionQuery};
pub use protocol::{
    FileDiff, Form, Location, Message, MessagePage, ModelRef, Permission, PermissionReply,
    PromptAcceptance, ServerInfo, Session, SessionPage,
};
pub use session_dirs::SessionDirs;
pub use sse::{Event, EventStream};

/// The OpenCode release whose runtime contract this build was verified
/// against. Other versions are accepted when their contract matches.
pub const VERIFIED_VERSION: &str = "2.0.18";

/// An OpenCode operation failure containing no upstream body, URL, or secret.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Invalid local configuration or request input.
    #[error("invalid OpenCode request or configuration")]
    InvalidInput,
    /// The server could not be reached or its response could not be read.
    #[error("OpenCode connection failed")]
    Transport,
    /// The server returned an unsuccessful HTTP status.
    #[error("OpenCode request rejected (HTTP {0})")]
    Rejected(u16),
    /// The response did not satisfy the supported protocol contract.
    #[error("unsupported OpenCode response")]
    Protocol,
    /// The server's API lacks routes or fields this build relies on.
    #[error("OpenCode {version} is incompatible: missing {missing}")]
    Incompatible {
        /// Version reported by the server.
        version: String,
        /// Comma-separated missing routes / schema fields.
        missing: String,
    },
    /// A response exceeded the configured memory budget.
    #[error("OpenCode response exceeds the resource limit")]
    Limit,
    /// A mutation may have been accepted; callers must reconcile, never retry
    /// automatically.
    #[error("OpenCode submission result is unknown; refresh before retrying")]
    SubmissionUnknown,
    /// Event continuity was lost; the caller must resynchronize authoritative
    /// state.
    #[error("OpenCode event stream interrupted; refresh required")]
    Disconnected,
    /// No usable local OpenCode background service was found.
    #[error("no running OpenCode service found")]
    NotRunning,
}

/// Result of an operation against the external OpenCode service.
pub type Result<T> = std::result::Result<T, Error>;

/// Memory-only HTTP Basic credentials. Debug output never exposes either field.
#[derive(Clone)]
pub struct BasicCredentials {
    username: String,
    password: String,
}

impl BasicCredentials {
    /// Credentials for the official service registration
    /// (`opencode:<password>`).
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }

    pub(crate) fn apply(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request.basic_auth(&self.username, Some(&self.password))
    }
}

impl fmt::Debug for BasicCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("BasicCredentials([redacted])")
    }
}

/// Validate a server origin: http(s), no userinfo, path, query or fragment.
/// With `loopback_only` the host must be a numeric loopback address or
/// `localhost`.
pub fn validate_origin(origin: &str, loopback_only: bool) -> Result<url::Url> {
    let url = url::Url::parse(origin).map_err(|_| Error::InvalidInput)?;
    let loopback = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(host)) => host == "localhost",
        None => false,
    };
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || (loopback_only && !loopback)
    {
        return Err(Error::InvalidInput);
    }
    Ok(url)
}

/// Validate a native identifier before it is interpolated into a path.
pub fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 512
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(Error::InvalidInput);
    }
    Ok(())
}
