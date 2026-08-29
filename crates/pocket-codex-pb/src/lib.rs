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
//! [`register`] and [`subscribe`] now resolve once the tunnel is actually up and
//! return a handle that owns it, instead of a future the caller had to spawn and
//! could not interrogate. Hold the handle for as long as the tunnel should live.

#![forbid(unsafe_code)]

/// Administrator operations: issuing and revoking temporary credentials.
pub mod admin;
/// Register / subscribe primitives.
pub mod session;

pub use admin::{issue_credential, revoke_credential, IssuedCredential};
pub use session::{
    keys, parse_relay_addr, register, service_connections, subscribe, RegisterOptions,
    RelaySession, SubscribeOptions, TUNNEL_READY_TIMEOUT,
};
