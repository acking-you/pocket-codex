//! Async wrappers around the published `pb-mapper` client SDK.
//!
//! ```text
//!     Pocket-Codex helper           pb-mapper SDK entrypoint
//!     ──────────────────────────    ────────────────────────────────────────
//!     register(RegisterOptions)   → Client::register(RegisterRequest)
//!     subscribe(SubscribeOptions) → Client::connect(ConnectRequest)
//!     keys(relay, credential)     → Client::list_keys
//!     service_connections(…)      → Client::service_status
//!
//!         caller ── (relay_addr + credential) ──▶  pb-mapper relay
//!                                                   │
//!                                                   ▼
//!                                          registered service keys /
//!                                          per-key connection list
//! ```
//!
//! # Why the credential is a parameter now
//!
//! This module used to call `set_process_msg_header_key`, a process-global
//! mutation, and every entrypoint relied on it having happened first. That made
//! the relay's shared key ambient state in whatever process wanted to tunnel —
//! including the Flutter app's bridge. The SDK's credential is per-[`Client`]
//! instead, so it travels as an argument and two sessions in one process may
//! hold different credentials. That is what lets the backend keep the admin key
//! while a client only ever holds a short-lived issued one.
//!
//! # Registrations are handles, not futures
//!
//! `register`/`subscribe` used to return a future that ran until the session
//! died, so callers `tokio::spawn`ed it and had no way to ask whether the tunnel
//! was actually up. The SDK spawns its own worker and hands back a handle
//! carrying readiness and a `stop()`. We surface that handle so a caller can
//! wait for the relay to accept the registration and tear it down deliberately
//! — dropping it aborts the tunnel.

use std::{net::SocketAddr, time::Duration};

use anyhow::{anyhow, Context, Result};
use pb_mapper::{
    Client, ClientConfig, ConnectRequest, Connection, RegisterRequest, Registration,
    ServiceConnection, Transport,
};

/// Relay address plus the credential to present to it.
///
/// The credential is either the relay's 32-byte administrator key or a
/// `pbmt1_`-prefixed temporary credential the relay issued. The SDK decides
/// which from the string's shape; only the admin one may drive admin RPCs.
#[derive(Debug, Clone)]
pub struct RelaySession {
    /// `host:port` of the relay (`pb-mapper server`).
    pub relay_addr: String,
    /// Administrator key or `pbmt1_…` temporary credential.
    pub credential: String,
    /// Whether to enable TCP keep-alive on relay connections.
    pub keep_alive: bool,
}

impl RelaySession {
    /// A session against `relay_addr` presenting `credential`, keep-alive on.
    ///
    /// Keep-alive defaults on because every tunnel we open is long-lived: a
    /// registration outlives the app that published it, and a dead peer that
    /// never sends a FIN would otherwise hold the slot until the relay's own
    /// lease sweep noticed.
    pub fn new(relay_addr: impl Into<String>, credential: impl Into<String>) -> Self {
        Self {
            relay_addr: relay_addr.into(),
            credential: credential.into(),
            keep_alive: true,
        }
    }

    /// The admin RPC surface for this session.
    ///
    /// Fails locally, before any I/O, unless the credential is the
    /// administrator key — so a client session cannot reach these even by
    /// accident, and the failure is a programming error surfaced immediately
    /// rather than a permission error surfaced over the wire.
    pub(crate) fn admin(&self) -> Result<pb_mapper::Admin> {
        self.client()?
            .admin()
            .map_err(|err| anyhow!("admin access on {}: {err}", self.relay_addr))
    }

    /// Build the SDK client for this session.
    fn client(&self) -> Result<Client> {
        Client::new(ClientConfig {
            server: self.relay_addr.clone(),
            credential: self.credential.clone(),
            keep_alive: self.keep_alive,
            // Temporary credentials are scoped to their own key id by the
            // relay; only an admin credential could target another namespace,
            // and nothing here has a reason to.
            namespace: None,
        })
        .map_err(|err| anyhow!("building pb-mapper client for {}: {err}", self.relay_addr))
    }
}

/// Options for registering a local TCP service with a remote relay.
#[derive(Debug, Clone)]
pub struct RegisterOptions {
    /// Service key under which the relay should index the registration.
    pub key: String,
    /// `host:port` of the local service to expose.
    pub local_addr: String,
    /// Enable AES-256-GCM end-to-end encryption (matches `--codec`).
    pub codec: bool,
}

/// Options for subscribing to a remote service from a client device.
#[derive(Debug, Clone)]
pub struct SubscribeOptions {
    /// Service key the client wants to attach to.
    pub key: String,
    /// `host:port` of the local listener to expose.
    pub local_addr: String,
}

/// How long to wait for the relay to accept a registration before treating it
/// as failed.
///
/// The SDK reports a registration `failed` only once EVERY worker in its
/// control-connection pool has permanently given up, so an unbounded wait can
/// sit on a partially-wedged pool. Bounding it means a caller learns about a
/// relay that is refusing us — an over-quota service, a revoked credential —
/// instead of hanging.
pub const TUNNEL_READY_TIMEOUT: Duration = Duration::from_secs(30);

/// Publish a local TCP service on the relay.
///
/// Returns once the relay has accepted the registration. The returned handle
/// owns the tunnel: hold it for as long as the service should stay published,
/// and prefer [`Registration::stop`] over dropping it so the teardown is
/// awaited rather than aborted.
pub async fn register(session: &RelaySession, opts: RegisterOptions) -> Result<Registration> {
    let registration = session
        .client()?
        .register(RegisterRequest {
            key: opts.key.clone(),
            local_addr: opts.local_addr.clone(),
            transport: Transport::Tcp,
            codec: opts.codec,
            // Never force a namespace: a temporary credential owns exactly one,
            // and forcing is an admin-only override we have no use for.
            force_namespace: false,
        })
        .await
        .with_context(|| format!("registering `{}` on {}", opts.key, session.relay_addr))?;
    registration
        .wait_ready_timeout(TUNNEL_READY_TIMEOUT)
        .await
        .with_context(|| format!("waiting for `{}` to register on {}", opts.key, session.relay_addr))?;
    Ok(registration)
}

/// Subscribe to a remote service and expose it on a local TCP port.
///
/// Returns once the local listener is bound AND the relay has confirmed the
/// service, so a caller told the tunnel is ready can immediately dial
/// `opts.local_addr`.
pub async fn subscribe(session: &RelaySession, opts: SubscribeOptions) -> Result<Connection> {
    let connection = session
        .client()?
        .connect(ConnectRequest {
            key: opts.key.clone(),
            local_addr: opts.local_addr.clone(),
            transport: Transport::Tcp,
        })
        .await
        .with_context(|| format!("subscribing to `{}` on {}", opts.key, session.relay_addr))?;
    connection
        .wait_ready_timeout(TUNNEL_READY_TIMEOUT)
        .await
        .with_context(|| format!("waiting for `{}` to subscribe on {}", opts.key, session.relay_addr))?;
    Ok(connection)
}

/// Query the service keys visible to this session's credential.
///
/// A temporary credential sees only its own namespace, so this is the tenant's
/// own inventory rather than the relay's whole listing.
pub async fn keys(session: &RelaySession) -> Result<Vec<String>> {
    session
        .client()?
        .list_keys()
        .await
        .with_context(|| format!("listing keys on {}", session.relay_addr))
}

/// Query connection health for one registered service key.
pub async fn service_connections(
    session: &RelaySession,
    key: impl Into<String>,
) -> Result<Vec<ServiceConnection>> {
    let key = key.into();
    session
        .client()?
        .service_status(key.clone())
        .await
        .with_context(|| format!("querying `{key}` status on {}", session.relay_addr))
}

/// Whether `addr` parses as a relay address this session could dial.
///
/// Kept as a free function because callers validate user input (a CLI flag, a
/// settings field) before they have a credential to build a session with.
pub fn parse_relay_addr(addr: &str) -> Result<SocketAddr> {
    addr.parse()
        .with_context(|| format!("`{addr}` is not a host:port relay address"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_rejects_an_empty_credential() {
        // The SDK validates before any I/O, which is what makes this testable
        // without a relay — and is why a misconfigured key surfaces at build
        // time rather than as a connection failure minutes later.
        let session = RelaySession::new("127.0.0.1:7666", "");
        assert!(session.client().is_err());
    }

    #[test]
    fn a_session_rejects_a_wrong_length_admin_key() {
        // Anything that is not a `pbmt1_` credential must be exactly 32 bytes.
        let session = RelaySession::new("127.0.0.1:7666", "short");
        assert!(session.client().is_err());
    }

    #[test]
    fn a_session_accepts_a_32_byte_admin_key() {
        let session = RelaySession::new("127.0.0.1:7666", "a".repeat(32));
        assert!(session.client().is_ok());
    }

    #[test]
    fn a_session_accepts_a_temporary_credential() {
        // Shape-checked by the SDK: version byte, non-zero key id, and a
        // checksum over the payload. This one was issued by a real relay.
        let session = RelaySession::new(
            "127.0.0.1:7666",
            "pbmt1_AQAAAAEAAAAAFNsL8qDUIBWtvMkHusQ9A6kEm0APq_BDwMZTCuoBCzqAsdCp",
        );
        assert!(session.client().is_ok());
    }

    #[test]
    fn an_empty_relay_address_is_rejected() {
        let session = RelaySession::new("", "a".repeat(32));
        assert!(session.client().is_err());
    }

    #[test]
    fn parse_relay_addr_rejects_a_bare_host() {
        assert!(parse_relay_addr("127.0.0.1:7666").is_ok());
        assert!(parse_relay_addr("lb7666.top").is_err());
    }
}
