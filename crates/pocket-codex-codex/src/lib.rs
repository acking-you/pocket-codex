//! Process management and protocol bridge for the upstream
//! `codex app-server`.
//!
//! Pocket-Codex spawns the user's existing `codex` binary in
//! `app-server` mode and exposes a typed handle that the CLI / UI
//! layers can use to:
//!
//! * locate the `codex` binary on `$PATH`,
//! * spawn `codex app-server --listen <url>` as a detached child and stream its
//!   stdout/stderr to a log file,
//! * inspect supervised processes (alive? same pid?) and gracefully stop them.
//!
//! JSON-RPC 2.0 envelopes that the app-server speaks are defined in
//! [`protocol`]; this crate stays transport-agnostic so callers can
//! decide whether to talk over stdio, a unix socket, or a websocket.

#![forbid(unsafe_code)]

/// JSON-RPC 2.0 envelopes used by the codex app-server.
pub mod protocol;

/// Async WebSocket JSON-RPC client for a `codex app-server`.
pub mod client;

/// Spawn / inspect / stop the supervised `codex app-server` process.
pub mod process;

pub use process::{spawn, status, stop, ListenSpec, SpawnOptions, StatusReport, StopOutcome};
