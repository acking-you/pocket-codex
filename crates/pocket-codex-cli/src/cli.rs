//! Command-line argument schema for `pocket-codex`.
//!
//! All subcommands are declared here so that argument parsing,
//! `--help` text and shell completion stay in one file. The actual
//! work is delegated to `commands::*` so this module remains free of
//! side effects.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use pocket_codex_core::state::PbRole;

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
    /// Start local codex app-server and register it with a relay.
    Serve(ServeArgs),

    /// Subscribe to a relay-exposed app-server and print `codex --remote`.
    Connect(ConnectArgs),

    /// Show all Pocket-Codex managed runtime sessions.
    Status,

    /// Stop Pocket-Codex managed runtime sessions.
    Stop(StopArgs),

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

    /// Internal worker entrypoints spawned by high-level commands.
    #[command(name = "__worker", hide = true, subcommand)]
    Worker(WorkerCmd),
}

/// Args for `pocket-codex serve`.
#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Service key the relay will index this registration under.
    #[arg(long, default_value = "codex")]
    pub key: String,

    /// Optional explicit path to the `codex` binary.
    #[arg(long = "codex-binary")]
    pub codex_binary: Option<PathBuf>,

    /// Bind host for the local websocket app-server.
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Bind port for the local websocket app-server.
    #[arg(long, default_value_t = 18080)]
    pub port: u16,

    /// Enable AES-256-GCM end-to-end encryption in pb-mapper.
    #[arg(long)]
    pub codec: bool,

    #[command(flatten)]
    pub relay: PbRelayArgs,

    /// Extra arguments forwarded to `codex app-server`.
    #[arg(last = true)]
    pub extra: Vec<String>,
}

/// Args for `pocket-codex connect`.
#[derive(Debug, Args)]
pub struct ConnectArgs {
    /// Service key to attach to.
    #[arg(long, default_value = "codex")]
    pub key: String,

    /// `host:port` to bind the local subscriber listener on.
    #[arg(long, default_value = "127.0.0.1:28080")]
    pub local_addr: String,

    #[command(flatten)]
    pub relay: PbRelayArgs,
}

/// Args for `pocket-codex stop`.
#[derive(Debug, Args)]
pub struct StopArgs {
    /// Limit pb-mapper stopping to one service key. When set, codex is
    /// left running; use `pocket-codex codex stop` for codex-only stops.
    #[arg(long)]
    pub key: Option<String>,

    /// Limit pb-mapper stopping to one role.
    #[arg(long)]
    pub role: Option<PbRoleArg>,
}

/// pb-mapper role selector for CLI filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum PbRoleArg {
    /// Local service registration session.
    Register,
    /// Local subscriber session.
    Subscribe,
}

impl From<PbRoleArg> for PbRole {
    fn from(value: PbRoleArg) -> Self {
        match value {
            PbRoleArg::Register => Self::Register,
            PbRoleArg::Subscribe => Self::Subscribe,
        }
    }
}

/// Internal worker subcommands.
#[derive(Debug, Subcommand)]
pub enum WorkerCmd {
    /// Run a foreground pb-mapper register worker.
    PbRegister(PbRegisterArgs),
    /// Run a foreground pb-mapper subscribe worker.
    PbSubscribe(PbSubscribeArgs),
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

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};
    use pocket_codex_core::state::PbRole;

    use super::*;

    #[test]
    fn serve_parses_high_level_host_flow_defaults() {
        let cli = Cli::parse_from(["pocket-codex", "serve", "--relay", "relay.example:7666"]);

        let Command::Serve(args) = cli.command else {
            panic!("expected serve command");
        };

        assert_eq!(args.key, "codex");
        assert_eq!(args.host, "127.0.0.1");
        assert_eq!(args.port, 18080);
        assert_eq!(args.relay.relay, "relay.example:7666");
        assert!(!args.codec);
        assert!(args.codex_binary.is_none());
        assert!(args.extra.is_empty());
    }

    #[test]
    fn connect_parses_high_level_client_flow_defaults() {
        let cli = Cli::parse_from(["pocket-codex", "connect", "--relay", "relay.example:7666"]);

        let Command::Connect(args) = cli.command else {
            panic!("expected connect command");
        };

        assert_eq!(args.key, "codex");
        assert_eq!(args.local_addr, "127.0.0.1:28080");
        assert_eq!(args.relay.relay, "relay.example:7666");
    }

    #[test]
    fn hidden_worker_parses_pb_register_args() {
        let cli = Cli::parse_from([
            "pocket-codex",
            "__worker",
            "pb-register",
            "--key",
            "demo",
            "--local-addr",
            "127.0.0.1:18080",
            "--relay",
            "relay.example:7666",
            "--codec",
        ]);

        let Command::Worker(WorkerCmd::PbRegister(args)) = cli.command else {
            panic!("expected hidden pb-register worker");
        };

        assert_eq!(args.key, "demo");
        assert_eq!(args.local_addr, "127.0.0.1:18080");
        assert_eq!(args.relay.relay, "relay.example:7666");
        assert!(args.codec);
    }

    #[test]
    fn stop_filter_role_maps_to_runtime_role() {
        let cli =
            Cli::parse_from(["pocket-codex", "stop", "--role", "subscribe", "--key", "codex"]);

        let Command::Stop(args) = cli.command else {
            panic!("expected stop command");
        };

        assert_eq!(args.key.as_deref(), Some("codex"));
        assert_eq!(args.role.map(PbRole::from), Some(PbRole::Subscribe));
    }

    #[test]
    fn hidden_worker_does_not_show_in_help() {
        let help = Cli::command().render_long_help().to_string();

        assert!(help.contains("serve"));
        assert!(help.contains("connect"));
        assert!(!help.contains("__worker"));
    }
}
