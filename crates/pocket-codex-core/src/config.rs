//! Configuration schema for Pocket-Codex.
//!
//! The CLI loads a TOML file (default location:
//! `$XDG_CONFIG_HOME/pocket-codex/config.toml`) and merges it with
//! command-line flags. This module only defines the *shape* of that
//! configuration; loading helpers will be added once the CLI grows
//! beyond the bootstrap skeleton.

use serde::{Deserialize, Serialize};

/// Top-level Pocket-Codex configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Settings for the local `codex app-server` process.
    #[serde(default)]
    pub codex: CodexConfig,

    /// Settings for the `pb-mapper` register/subscribe layer.
    #[serde(default)]
    pub pb_mapper: PbMapperConfig,
}

/// Configuration for managing the local `codex app-server` process.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexConfig {
    /// Optional explicit path to the `codex` binary. If unset, the
    /// process manager will look it up on `PATH`.
    pub binary: Option<String>,
}

/// Configuration for the `pb-mapper` integration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PbMapperConfig {
    /// URL of the upstream `pb-mapper` relay (e.g.
    /// `tcp://relay.example.com:7800`).
    pub relay: Option<String>,
}
