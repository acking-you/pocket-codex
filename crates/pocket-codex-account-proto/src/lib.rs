//! Shared wire types and pb-mapper key namespacing for the Pocket-Codex hosted
//! account model.
//!
//! The hosted backend lets users authenticate with GitHub and reach their own
//! pb-mapper services without ever holding the relay's administrator key: it
//! vends each account a short-lived credential, and clients take it straight to
//! the relay. This crate is the *contract* between the backend and its clients
//! (the `pocket-codex-cli` and the Flutter bridge):
//!
//! - [`key`] — per-user relay-key namespacing
//!   (`pcxu:<user>:<device>:<kind>:<name>`) layered on top of
//!   [`pocket_codex_core::service::ServiceId`].
//! - [`http`] — the JSON request/response bodies for the backend's HTTP API
//!   (GitHub login, session credentials, `/v1/me`, `/v1/services`, and
//!   `/v1/relay` — the credential handoff after which clients no longer need
//!   the backend).
//! - [`pkce`] — client-side PKCE + CSRF helpers for the web
//!   (authorization-code) login flow.
//! - [`params`] — shared retry/backoff bounds.
//!
//! I/O-free by construction: it is serde types and pure helpers. It used to
//! also carry a broker tunnel protocol (`HELLO`/`Ack` plus length-prefixed
//! framing) back when every byte of a client's traffic passed through the
//! backend; direct relay access removed the tunnel and with it that whole
//! surface.

#![forbid(unsafe_code)]

pub mod http;
pub mod key;
pub mod params;
pub mod pkce;

pub use key::{NamespacedServiceId, SERVICE_NS_PREFIX};
pub use params::{BoundedRetry, RetryBackoff, MAX_ATTEMPTS};
