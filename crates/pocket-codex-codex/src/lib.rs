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

/// The `deps/codex` submodule commit the embedded (自带) app-server was built
/// from, baked at compile time by `build.rs`. codex's own crate version is a
/// `0.0.0` placeholder, so this short commit is the meaningful "version" of the
/// in-process codex. `"unknown"` if git was unavailable at build time.
pub const EMBEDDED_CODEX_COMMIT: &str = env!("EMBEDDED_CODEX_COMMIT");

/// JSON-RPC 2.0 envelopes used by the codex app-server.
pub mod protocol;

/// Async WebSocket JSON-RPC client for a `codex app-server`.
pub mod client;

/// Spawn / inspect / stop the supervised `codex app-server` process.
pub mod process;

/// Read codex session rollout files from `CODEX_HOME` and classify their
/// most-recent-turn state.
pub mod rollout;

/// Bootstrap `CODEX_HOME` (custom provider / non-degraded prompt) from the
/// onboarding UI.
pub mod setup;

/// Detect whether a session's rollout is currently held open by a live
/// process, and enumerate codex app-server processes.
pub mod liveness;

/// Combine transcript + liveness into a resume-safety verdict and
/// implement force takeover of a held-open session.
pub mod takeover;

/// Desktop self-contained mode: run codex's app-server in-process (compiled in)
/// instead of spawning an external binary. Only built with `embedded-codex`.
#[cfg(feature = "embedded-codex")]
pub mod embedded;

pub use process::{
    locate_binary, spawn, status, stop, ListenSpec, SpawnOptions, SpawnReport, StatusReport,
    StopOutcome,
};
