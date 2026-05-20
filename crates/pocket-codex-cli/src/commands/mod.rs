//! Subcommand dispatcher.
//!
//! Each module in this directory implements one verb of the
//! `pocket-codex` CLI and is responsible for its own I/O. Keeping
//! them small and side-effect-local makes them easy to unit test.

use anyhow::Result;

use crate::cli::{Cli, Command};

mod codex;
mod pb;
mod remote_hint;
mod version;

/// Dispatch a parsed [`Cli`] invocation to the matching subcommand.
pub async fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Version => version::run(),
        Command::Codex(cmd) => codex::run(cmd).await,
        Command::Pb(cmd) => pb::run(cmd).await,
        Command::RemoteHint(args) => remote_hint::run(args),
    }
}
