//! Command-line argument schema for `pocket-codex`.
//!
//! All subcommands are declared here so that argument parsing,
//! `--help` text and shell completion stay in one file. The actual
//! work is delegated to `commands::*` so this module remains free of
//! side effects.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// `pocket-codex` orchestrates codex app-server + pb-mapper.
#[derive(Debug, Parser)]
#[command(
    name = "pocket-codex",
    version,
    about,
    long_about = None,
    propagate_version = true,
)]
pub struct Cli {
    /// Subcommand to dispatch.
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print build / runtime version info.
    Version,

    /// Manage the local `codex app-server` process.
    #[command(subcommand)]
    Codex(CodexCmd),

    /// Manage pb-mapper register / subscribe sessions.
    #[command(subcommand)]
    Pb(PbCmd),

    /// Print the `codex --remote …` style invocation a client device
    /// should use to attach to a relay-exposed app-server.
    RemoteHint(RemoteHintArgs),
}

/// Subcommands under `pocket-codex codex …`.
#[derive(Debug, Subcommand)]
pub enum CodexCmd {
    /// Start the local `codex app-server` as a detached child.
    Start(CodexStartArgs),
    /// Stop the supervised `codex app-server` (SIGTERM).
    Stop,
    /// Show whether the supervised process is alive and on what port.
    Status,
}

/// Args for `pocket-codex codex start`.
#[derive(Debug, Args)]
pub struct CodexStartArgs {
    /// Optional explicit path to the `codex` binary.
    #[arg(long)]
    pub binary: Option<PathBuf>,

    /// Bind host (websocket transport).
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Bind port (websocket transport).
    #[arg(long, default_value_t = 18080)]
    pub port: u16,

    /// Extra arguments forwarded to `codex app-server`.
    #[arg(last = true)]
    pub extra: Vec<String>,
}

/// Subcommands under `pocket-codex pb …`.
#[derive(Debug, Subcommand)]
pub enum PbCmd {
    /// Register a local TCP service with the relay (publisher side).
    Register(PbRegisterArgs),
    /// Subscribe to a remote service and expose it locally
    /// (subscriber side).
    Subscribe(PbSubscribeArgs),
    /// Query the relay for status info.
    Status(PbStatusArgs),
}

/// Common pb-mapper relay locator.
#[derive(Debug, Args, Clone)]
pub struct PbRelayArgs {
    /// `host:port` of the upstream pb-mapper relay. Falls back to
    /// `$PB_MAPPER_SERVER` when unset (matches the pb-mapper CLIs).
    #[arg(long, env = "PB_MAPPER_SERVER")]
    pub relay: String,
}

/// Args for `pocket-codex pb register`.
#[derive(Debug, Args)]
pub struct PbRegisterArgs {
    /// Service key the relay will index this registration under.
    #[arg(long, default_value = "codex")]
    pub key: String,

    /// `host:port` of the local service to publish.
    #[arg(long, default_value = "127.0.0.1:18080")]
    pub local_addr: String,

    /// Enable AES-256-GCM end-to-end encryption.
    #[arg(long)]
    pub codec: bool,

    #[command(flatten)]
    pub relay: PbRelayArgs,
}

/// Args for `pocket-codex pb subscribe`.
#[derive(Debug, Args)]
pub struct PbSubscribeArgs {
    /// Service key to attach to.
    #[arg(long, default_value = "codex")]
    pub key: String,

    /// `host:port` to bind the local listener on.
    #[arg(long, default_value = "127.0.0.1:28080")]
    pub local_addr: String,

    #[command(flatten)]
    pub relay: PbRelayArgs,
}

/// Args for `pocket-codex pb status`.
#[derive(Debug, Args)]
pub struct PbStatusArgs {
    /// What to query: `keys` (registered services) or `remote-id`
    /// (active connections).
    #[arg(long, default_value = "keys")]
    pub kind: PbStatusKind,

    #[command(flatten)]
    pub relay: PbRelayArgs,
}

/// Status query kind exposed by the CLI.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum PbStatusKind {
    /// List registered service keys.
    Keys,
    /// List active connection ids.
    RemoteId,
}

/// Args for `pocket-codex remote-hint`.
#[derive(Debug, Args)]
pub struct RemoteHintArgs {
    /// Service key the relay knows the app-server by.
    #[arg(long, default_value = "codex")]
    pub key: String,

    /// `host:port` the client should expose the subscriber listener on.
    #[arg(long, default_value = "127.0.0.1:28080")]
    pub local_addr: String,

    #[command(flatten)]
    pub relay: PbRelayArgs,
}
