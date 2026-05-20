//! `pocket-codex` command-line entrypoint.
//!
//! Pocket-Codex orchestrates a local `codex app-server` process and a
//! `pb-mapper` register/subscribe session so users can drive Codex
//! from any device. This binary wires the three library crates
//! (`pocket-codex-core`, `pocket-codex-codex`, `pocket-codex-pb`)
//! into the user-facing subcommands documented in `AGENTS.md`.

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

mod cli;
mod commands;

/// Default tracing filter when `RUST_LOG` is unset.
const DEFAULT_LOG_FILTER: &str = "warn,pocket_codex=info,pocket_codex_cli=info";

#[tokio::main]
async fn main() -> Result<()> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    let cli = cli::Cli::parse();
    commands::dispatch(cli).await
}
