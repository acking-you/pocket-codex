//! Scoped HTTP/SSE access to an existing external OpenCode server.

mod client;
pub mod connection;
pub mod discovery;
pub mod gateway;
mod protocol;
mod sse;
pub mod v2;

pub use client::{BasicCredentials, OpenCodeClient};
pub use connection::Connection;
pub use gateway::{serve as serve_gateway, GatewayHandle, OpenCodeGateway};
pub use protocol::{
    Capabilities, Health, Message, MessageInfo, MessagePage, MessagePart, PermissionReply,
    PermissionRequest, PromptInput, PromptPart, Question, QuestionOption, QuestionRequest, Session,
};
pub use sse::{OpenCodeEvent, OpenCodeEventStream};

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
    /// A response exceeded the configured memory budget.
    #[error("OpenCode response exceeds the resource limit")]
    Limit,
    /// The object belongs to another directory or an unsupported workspace.
    #[error("OpenCode object is outside the selected project")]
    Scope,
    /// A mutation may have been accepted; callers must reconcile, never retry
    /// automatically.
    #[error("OpenCode submission result is unknown; refresh before retrying")]
    SubmissionUnknown,
    /// Event continuity was lost; the caller must resynchronize authoritative
    /// state.
    #[error("OpenCode event stream interrupted; refresh required")]
    Disconnected,
}

/// Result of an operation against the external OpenCode service.
pub type Result<T> = std::result::Result<T, Error>;
