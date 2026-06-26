//! Shared wire types and pb-mapper key namespacing for the Pocket-Codex hosted
//! account model.
//!
//! The hosted backend lets users authenticate with GitHub and reach their own
//! pb-mapper services through a TLS broker, without ever holding the relay's
//! global `MSG_HEADER_KEY`. This crate is the *contract* between the backend
//! and its clients (the `pocket-codex-cli` and the Flutter bridge):
//!
//! - [`key`] — per-user relay-key namespacing
//!   (`pcxu:<user>:<device>:<kind>:<name>`) layered on top of
//!   [`pocket_codex_core::service::ServiceId`].
//! - [`http`] — the JSON request/response bodies for the backend's HTTP API
//!   (GitHub device flow, session credentials, `/v1/me`, `/v1/services`).
//! - [`broker`] — the `HELLO`/`Ack` handshake exchanged over the broker tunnel.
//! - [`frame`] (behind the `frame` feature) — length-prefixed JSON framing used
//!   by the broker client and server to exchange the handshake on a byte
//!   stream.
//!
//! The crate is deliberately I/O-free by default (just serde types); enabling
//! `frame` adds the small async read/write helpers.

#![forbid(unsafe_code)]

pub mod broker;
pub mod http;
pub mod key;
pub mod params;

#[cfg(feature = "frame")]
pub mod frame;

pub use broker::{BrokerAck, BrokerControl, BrokerHello, BrokerRole, TunnelPurpose};
pub use key::{NamespacedServiceId, SERVICE_NS_PREFIX};
pub use params::RetryBackoff;
