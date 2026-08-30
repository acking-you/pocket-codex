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
//! died, so callers `tokio::spawn`ed it and had no way to ask whether the
//! tunnel was actually up. The SDK spawns its own worker and hands back a
//! handle carrying readiness and a `stop()`. We surface that handle so a caller
//! can wait for the relay to accept the registration and tear it down
//! deliberately — dropping it aborts the tunnel.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use pb_mapper::{
    Client, ClientConfig, ConnectRequest, Connection, RegisterRequest, Registration,
    ServiceConnection, Transport,
};

/// Relay address plus the credential to present to it.
///
/// The credential is either the relay's 32-byte administrator key or a
/// `pbmt1_`-prefixed temporary credential the relay issued. The SDK decides
/// which from the string's shape; only the admin one may drive admin RPCs.
///
/// `Debug` is hand-written to redact the credential. Deriving it would put the
/// relay's administrator key into any log line, panic message, or error chain
/// that happened to format a value containing one — and the backend holds
/// exactly that key.
#[derive(Clone, PartialEq, Eq)]
pub struct RelaySession {
    /// `host:port` of the relay (`pb-mapper server`).
    pub relay_addr: String,
    /// Administrator key or `pbmt1_…` temporary credential.
    pub credential: String,
    /// Whether to enable TCP keep-alive on relay connections.
    pub keep_alive: bool,
}

impl std::fmt::Debug for RelaySession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelaySession")
            .field("relay_addr", &self.relay_addr)
            // Which KIND of credential is diagnostically useful ("the backend is
            // presenting a temporary key" is a real bug report); the bytes never
            // are.
            .field("credential", &if self.credential.starts_with("pbmt1_") {
                "<temporary>"
            } else {
                "<admin>"
            })
            .field("keep_alive", &self.keep_alive)
            .finish()
    }
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

    /// A session against `relay_addr` with a syntactically valid throwaway
    /// credential, for tests that never reach the relay.
    ///
    /// Exists because the SDK validates the credential when the client is
    /// built, so a test exercising, say, token rejection would otherwise
    /// have to carry a 32-byte literal that has nothing to do with what it
    /// is testing.
    pub fn for_test(relay_addr: impl Into<String>) -> Self {
        Self::new(relay_addr, "0".repeat(32))
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
    let (registration, ready) = register_pending(session, opts).await?;
    ready?;
    Ok(registration)
}

/// Register, returning the handle AND the readiness outcome separately.
///
/// For a caller that must keep the registration even when it is not up yet: the
/// SDK's worker goes on retrying, so the handle is what lets the service appear
/// once the relay is reachable. [`register`] is this plus "treat not-ready as a
/// failure", which is right when the caller has nowhere to hold a pending
/// handle.
///
/// An `Err` from this function means the relay REFUSED the registration; the
/// inner `Err` means it has not confirmed it yet.
pub async fn register_pending(
    session: &RelaySession,
    opts: RegisterOptions,
) -> Result<(Registration, Result<()>)> {
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
    let ready = registration
        .wait_ready_timeout(TUNNEL_READY_TIMEOUT)
        .await
        .with_context(|| {
            format!("waiting for `{}` to register on {}", opts.key, session.relay_addr)
        });
    Ok((registration, ready))
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
        .with_context(|| {
            format!("waiting for `{}` to subscribe on {}", opts.key, session.relay_addr)
        })?;
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

/// The relay's id for this connection, as the relay reports it.
///
/// Useful only for diagnostics — it names the connection, not the service — but
/// it is what `pb status remote-id` answers, so it stays exposed.
pub async fn remote_id(session: &RelaySession) -> Result<String> {
    session
        .client()?
        .remote_id()
        .await
        .map(|id| format!("{id:?}"))
        .with_context(|| format!("querying remote id on {}", session.relay_addr))
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

/// Check that `addr` is a `host:port` a relay session could dial.
///
/// Deliberately NOT `SocketAddr::parse`: clients dial the relay by name
/// (`lb7666.top:7666`), so requiring a literal IP would reject every real
/// deployment. The SDK resolves the host itself; all we owe a caller is an
/// early "you typed that wrong" on a value that came from a CLI flag or a
/// settings field, before it becomes a connection error much later.
pub fn parse_relay_addr(addr: &str) -> Result<()> {
    let (host, port) = addr
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("`{addr}` is missing a `:port`"))?;
    // An IPv6 literal keeps its brackets (`[::1]:7666`); `rsplit_once` already
    // took the port off the end, so anything left is the host.
    if host.is_empty() {
        bail!("`{addr}` is missing a host");
    }
    port.parse::<u16>()
        .map(|_| ())
        .with_context(|| format!("`{addr}` has an invalid port"))
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
    fn debug_never_prints_the_credential() {
        // The backend holds the relay's ADMINISTRATOR key in one of these, and a
        // derived Debug would leak it through any log line or error chain that
        // formatted a value containing one.
        let admin = "S3CR3T-admin-key-padded-to-32ch!";
        assert_eq!(admin.len(), 32);
        let rendered = format!("{:?}", RelaySession::new("127.0.0.1:7666", admin));
        assert!(!rendered.contains(admin), "credential leaked: {rendered}");
        assert!(rendered.contains("<admin>"), "kind not shown: {rendered}");
        assert!(rendered.contains("127.0.0.1:7666"), "address hidden: {rendered}");

        let temp = "pbmt1_AQAAAAEAAAAAFNsL8qDUIBWtvMkHusQ9A6kEm0APq_BDwMZTCuoBCzqAsdCp";
        let rendered = format!("{:?}", RelaySession::new("127.0.0.1:7666", temp));
        assert!(!rendered.contains(temp), "credential leaked: {rendered}");
        assert!(rendered.contains("<temporary>"), "kind not shown: {rendered}");
    }

    #[test]
    fn parse_relay_addr_accepts_a_named_host_and_needs_a_port() {
        // A NAME must be accepted: with clients dialling the relay directly,
        // `lb7666.top:7666` is the production value, not an edge case.
        assert!(parse_relay_addr("lb7666.top:7666").is_ok());
        assert!(parse_relay_addr("127.0.0.1:7666").is_ok());
        assert!(parse_relay_addr("[::1]:7666").is_ok());
        assert!(parse_relay_addr("lb7666.top").is_err(), "a missing port is a typo");
        assert!(parse_relay_addr("lb7666.top:nope").is_err());
        assert!(parse_relay_addr(":7666").is_err());
    }
}
