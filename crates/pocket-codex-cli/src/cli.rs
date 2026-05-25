//! Command-line argument schema for `pocket-codex`.
//!
//! All subcommands are declared here so that argument parsing,
//! `--help` text and shell completion stay in one file. The actual
//! work is delegated to `commands::*` so this module remains free of
//! side effects.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use pocket_codex_core::{service::ServiceKind, state::PbRole};

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

    /// Manage the direct Responses API proxy service.
    #[command(subcommand)]
    Api(ApiCmd),

    /// Discover and configure relay-exposed Pocket-Codex services.
    #[command(subcommand)]
    Services(ServicesCmd),

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
    /// Exact pb-mapper key to register. When omitted, Pocket-Codex builds
    /// `pcx:<device>:app:<name>`.
    #[arg(long)]
    pub key: Option<String>,

    /// Device id used in generated Pocket-Codex service keys.
    #[arg(long)]
    pub device: Option<String>,

    /// Service instance name used in generated Pocket-Codex service keys.
    #[arg(long, default_value = "default")]
    pub name: String,

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
    /// Exact pb-mapper key to attach to. When omitted, Pocket-Codex resolves
    /// the target from `--device`, local defaults, or relay discovery.
    #[arg(long)]
    pub key: Option<String>,

    /// Device id to connect to.
    #[arg(long)]
    pub device: Option<String>,

    /// Service instance name to connect to. When omitted, Pocket-Codex
    /// uses the stored default target name or falls back to `default`.
    #[arg(long)]
    pub name: Option<String>,

    /// `host:port` to bind the local subscriber listener on.
    #[arg(long, default_value = "127.0.0.1:28080")]
    pub local_addr: String,

    #[command(flatten)]
    pub relay: PbRelayArgs,
}

/// Subcommands under `pocket-codex api …`.
#[derive(Debug, Subcommand)]
pub enum ApiCmd {
    /// Start the local Responses API proxy and register it with a relay.
    Serve(ApiServeArgs),
    /// Subscribe to a relay-exposed Responses API proxy and print Codex config.
    Connect(ApiConnectArgs),
}

/// Args for `pocket-codex api serve`.
#[derive(Debug, Args)]
pub struct ApiServeArgs {
    /// Exact pb-mapper key to register. When omitted, Pocket-Codex builds
    /// `pcx:<device>:api:<name>`.
    #[arg(long)]
    pub key: Option<String>,

    /// Device id used in generated Pocket-Codex service keys.
    #[arg(long)]
    pub device: Option<String>,

    /// Service instance name used in generated Pocket-Codex service keys.
    #[arg(long, default_value = "default")]
    pub name: String,

    /// Bind host for the local Responses API proxy.
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Bind port for the local Responses API proxy.
    #[arg(long, default_value_t = 18180)]
    pub port: u16,

    /// Enable AES-256-GCM end-to-end encryption in pb-mapper.
    #[arg(long)]
    pub codec: bool,

    #[command(flatten)]
    pub relay: PbRelayArgs,
}

/// Args for `pocket-codex api connect`.
#[derive(Debug, Args)]
pub struct ApiConnectArgs {
    /// Exact pb-mapper key to attach to. When omitted, Pocket-Codex resolves
    /// the target from `--device`, local defaults, or relay discovery.
    #[arg(long)]
    pub key: Option<String>,

    /// Device id to connect to.
    #[arg(long)]
    pub device: Option<String>,

    /// Service instance name to connect to. When omitted, Pocket-Codex
    /// uses the stored default target name or falls back to `default`.
    #[arg(long)]
    pub name: Option<String>,

    /// `host:port` to bind the local subscriber listener on.
    #[arg(long, default_value = "127.0.0.1:28180")]
    pub local_addr: String,

    #[command(flatten)]
    pub relay: PbRelayArgs,
}

/// Subcommands under `pocket-codex services …`.
#[derive(Debug, Subcommand)]
pub enum ServicesCmd {
    /// List relay-exposed Pocket-Codex service keys.
    List(ServicesListArgs),
    /// Manage local default service targets.
    #[command(subcommand)]
    Default(ServicesDefaultCmd),
}

/// Args for `pocket-codex services list`.
#[derive(Debug, Args)]
pub struct ServicesListArgs {
    /// Limit output to one service kind.
    #[arg(long)]
    pub kind: Option<ServiceKindArg>,

    #[command(flatten)]
    pub relay: PbRelayArgs,
}

/// Subcommands under `pocket-codex services default …`.
#[derive(Debug, Subcommand)]
pub enum ServicesDefaultCmd {
    /// Set the local default target for a service kind.
    Set(ServicesDefaultSetArgs),
}

/// Args for `pocket-codex services default set`.
#[derive(Debug, Args)]
pub struct ServicesDefaultSetArgs {
    /// Service kind to configure.
    #[arg(long)]
    pub kind: ServiceKindArg,

    /// Device id to use by default.
    #[arg(long)]
    pub device: String,

    /// Service instance name to use by default.
    #[arg(long, default_value = "default")]
    pub name: String,
}

/// Service kind selector used by user-facing CLI commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ServiceKindArg {
    /// Codex app-server remote-control service.
    App,
    /// Responses API proxy service.
    Api,
}

impl From<ServiceKindArg> for ServiceKind {
    fn from(value: ServiceKindArg) -> Self {
        match value {
            ServiceKindArg::App => Self::App,
            ServiceKindArg::Api => Self::Api,
        }
    }
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
    /// Run a foreground Responses API proxy worker.
    ApiProxy(ApiProxyArgs),
}

/// Args for the hidden Responses API proxy worker.
#[derive(Debug, Args)]
pub struct ApiProxyArgs {
    /// `host:port` to bind the proxy listener on.
    #[arg(long)]
    pub listen: String,
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

        assert!(args.key.is_none());
        assert!(args.device.is_none());
        assert_eq!(args.name, "default");
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

        assert!(args.key.is_none());
        assert!(args.device.is_none());
        assert!(args.name.is_none());
        assert_eq!(args.local_addr, "127.0.0.1:28080");
        assert_eq!(args.relay.relay, "relay.example:7666");
    }

    #[test]
    fn api_serve_parses_device_service_defaults() {
        let cli =
            Cli::parse_from(["pocket-codex", "api", "serve", "--relay", "relay.example:7666"]);

        let Command::Api(ApiCmd::Serve(args)) = cli.command else {
            panic!("expected api serve command");
        };

        assert!(args.key.is_none());
        assert!(args.device.is_none());
        assert_eq!(args.name, "default");
        assert_eq!(args.host, "127.0.0.1");
        assert_eq!(args.port, 18180);
        assert_eq!(args.relay.relay, "relay.example:7666");
    }

    #[test]
    fn services_default_set_parses_target() {
        let cli = Cli::parse_from([
            "pocket-codex",
            "services",
            "default",
            "set",
            "--kind",
            "app",
            "--device",
            "studio",
            "--name",
            "work",
        ]);

        let Command::Services(ServicesCmd::Default(ServicesDefaultCmd::Set(args))) = cli.command
        else {
            panic!("expected services default set command");
        };

        assert_eq!(args.kind, ServiceKindArg::App);
        assert_eq!(args.device, "studio");
        assert_eq!(args.name, "work");
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
