//! Pb-mapper register/subscribe glue used by Pocket-Codex.
//!
//! Pocket-Codex reuses the upstream [`pb-mapper`](https://github.com/acking-you/pb-mapper)
//! library (vendored under `deps/pb-mapper`) to:
//!
//! * **Register** a local `codex app-server` listener with a remote `pb-mapper`
//!   relay so other devices can reach it.
//! * **Subscribe** to a remote `codex app-server` from a client device, mapping
//!   it onto a local TCP endpoint.
//!
//! These are async functions that *do not return* under normal
//! operation — they keep the relay session alive until the future is
//! cancelled or an unrecoverable error occurs. Higher-level callers
//! typically `tokio::spawn` them, persist a
//! [`pocket_codex_core::state::PbSessionInfo`], and watch the resulting
//! [`tokio::task::JoinHandle`].

#![forbid(unsafe_code)]

/// Register / subscribe primitives.
pub mod session;

pub use session::{
    keys, register, service_connections, set_msg_header_key, status, subscribe, RegisterOptions,
    StatusKind, SubscribeOptions,
};
