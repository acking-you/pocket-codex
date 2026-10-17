//! Core primitives shared across the Pocket-Codex crates.
//!
//! This crate intentionally has a small surface area: it exposes the
//! configuration schema, persisted runtime state, well-known filesystem
//! paths, and a handful of error / id helpers reused by the higher-level
//! `pocket-codex-codex`, `pocket-codex-pb` and `pocket-codex-cli`
//! crates.
//!
//! See `AGENTS.md` for the development roadmap.

#![forbid(unsafe_code)]

/// Metadata identifying the environment that owns a registered service.
pub mod device;

/// Library-level error type re-exported from individual modules.
pub mod error;

/// Configuration schema loaded from `pocket-codex.toml` or the CLI.
pub mod config;

/// Well-known filesystem paths used by the CLI / daemon.
pub mod paths;

/// Shared process inspection and signalling helpers.
pub mod process;

/// Persistent runtime state (PID, listen URL, pb-mapper sessions).
pub mod state;

/// Pocket-Codex service identifiers and relay key helpers.
pub mod service;

/// Provider-independent, restart-safe history window synchronization.
pub mod history_sync;

/// Custom model providers (protocol, base URL, key) for agent hosts.
pub mod provider;

/// Keeping a supplied key out of child output shown or logged.
pub mod redact;

/// Which processes hold the listening sockets a local TCP endpoint reaches.
pub mod listener;

pub use error::{Error, Result};
