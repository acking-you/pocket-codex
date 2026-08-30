//! Pb-mapper register/subscribe glue used by Pocket-Codex.
//!
//! Pocket-Codex drives the published
//! [`pb-mapper`](https://github.com/acking-you/pb-mapper) client SDK to:
//!
//! * **Register** a local `codex app-server` listener with a remote `pb-mapper`
//!   relay so other devices can reach it.
//! * **Subscribe** to a remote `codex app-server` from a client device, mapping
//!   it onto a local TCP endpoint.
//!
//! # What changed, and why it matters to callers
//!
//! This crate wrapped the pb-mapper **submodule** at 0.2.14 and exposed
//! never-returning futures plus a process-global `set_msg_header_key`. Two
//! consequences drove the rewrite:
//!
//! * **The relay key was ambient.** Every process that wanted a tunnel had to
//!   hold the relay's shared key, because the credential lived in a global
//!   rather than in an argument. The SDK's credential is per-session, so the
//!   backend can hold the administrator key while a client holds only a
//!   short-lived credential the relay issued it — see [`admin`].
//! * **A pinned-old client drifts from the relay.** Production already ran
//!   0.5.0, which answers an over-quota register with a structured error frame.
//!   A 0.2.x client cannot decode that frame, so it read a permanent refusal as
//!   a transport fault and reconnected without backoff. Tracking the release
//!   removes the whole class of skew.
//!
//! [`register`] and [`subscribe`] now resolve once the tunnel is actually up
//! and return a handle that owns it, instead of a future the caller had to
//! spawn and could not interrogate. Hold the handle for as long as the tunnel
//! should live.
//!
//! # Module map
//!
//! * [`session`] — [`RelaySession`] (an address plus a credential) and the
//!   register / subscribe / status primitives.
//! * [`publish`] — [`publish`] on top of `register`, naming the one failure a
//!   caller must not retry: a name already held by another live publisher.
//! * [`admin`] — issuing, renewing, and revoking the temporary credentials
//!   clients actually use. Administrator-only.
//! * [`keepalive`] — renewing an issued credential under a long-lived tunnel,
//!   because the relay cancels a lapsed credential's tunnels.
//! * [`transport`] — [`Transport`], which pairs a session with an optional
//!   account namespace and is the only difference between account and self-host
//!   mode. Shared so the CLI and the app cannot disagree on a key's shape.

#![forbid(unsafe_code)]

/// Administrator operations: issuing and revoking temporary credentials.
pub mod admin;
/// Keeping an issued credential alive under a long-lived tunnel.
pub mod keepalive;
/// Publishing a local service, and the one failure a caller must not retry.
pub mod publish;
/// Register / subscribe primitives.
pub mod session;
/// Which relay to talk to, with which credential and key namespace.
pub mod transport;

pub use admin::{
    all_services, issue_credential, live_credentials, renew_credential, retire_service,
    revoke_credential, CredentialRecord, IssuedCredential, ServiceRecord,
};
pub use keepalive::keep_credential_alive;
pub use publish::{publish, publish_pending, PublishError, Published};
pub use session::{
    keys, parse_relay_addr, register, remote_id, service_connections, subscribe, RegisterOptions,
    RelaySession, SubscribeOptions, TUNNEL_READY_TIMEOUT,
};
pub use transport::Transport;
