//! Client side of the Pocket-Codex broker tunnel.
//!
//! Speaks the [`pocket_codex_account_proto`] broker protocol over TLS tunnels
//! to the backend; the backend does the pb-mapper work. The transport is
//! abstracted behind [`Connector`] (the CLI/bridge plug in `tokio-rustls`;
//! tests use plain TCP) and the session token behind [`TokenProvider`] (called
//! on every (re)connect, so a long-lived register survives JWT expiry —
//! broker-review D1).
//!
//! Two entry points, mirroring pb-mapper's publisher/subscriber:
//! - [`run_register`] — the controller: hold one control tunnel, heartbeat it,
//!   and for each `NewStream` signal dial a fresh data tunnel bridged to the
//!   local service. Reconnects with pb-mapper's backoff (plus jitter).
//! - [`run_subscribe`] — for each local connection, open a data tunnel.

#![forbid(unsafe_code)]

mod conn;
mod register;
mod subscribe;

pub use register::{run_register, RegisterConfig};
pub use subscribe::{run_subscribe, SubscribeConfig};
use tokio::io::{AsyncRead, AsyncWrite};

/// A connected byte stream to the backend broker (one TLS tunnel).
pub trait BrokerStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> BrokerStream for T {}

/// Opens a fresh connection (one TLS tunnel) to the backend broker. Implemented
/// by the CLI/bridge over `tokio-rustls`; by tests over plain TCP.
#[async_trait::async_trait]
pub trait Connector: Send + Sync + 'static {
    /// Connect and return a ready stream (TLS handshake already done).
    async fn connect(&self) -> Result<Box<dyn BrokerStream>, BrokerError>;
}

/// Supplies a currently-valid session token, refreshing as needed. Called on
/// every (re)connect so an expired JWT is renewed rather than retried forever.
#[async_trait::async_trait]
pub trait TokenProvider: Send + Sync + 'static {
    /// A valid bearer token, or an error if none can be obtained.
    async fn token(&self) -> Result<String, BrokerError>;
}

/// Errors from the broker client.
#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    /// Stream I/O failed.
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    /// Framing a control/handshake message failed.
    #[error("frame: {0}")]
    Frame(#[from] pocket_codex_account_proto::frame::FrameError),
    /// A timed operation exceeded its deadline.
    #[error("timed out: {0}")]
    Timeout(&'static str),
    /// Obtaining a session token failed.
    #[error("token: {0}")]
    Token(String),
    /// The backend rejected the tunnel (transient/relay reason).
    #[error("rejected: {0}")]
    Rejected(String),
}
