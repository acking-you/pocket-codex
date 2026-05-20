//! `pocket-codex codex …` subcommand handlers.

use anyhow::Result;
use pocket_codex_codex::{spawn, status, stop, ListenSpec, SpawnOptions, StopOutcome};

use crate::cli::{CodexCmd, CodexStartArgs};

/// Dispatch the `codex` subcommand group.
pub async fn run(cmd: CodexCmd) -> Result<()> {
    match cmd {
        CodexCmd::Start(args) => start(args),
        CodexCmd::Stop => stop_cmd(),
        CodexCmd::Status => status_cmd(),
    }
}

fn start(args: CodexStartArgs) -> Result<()> {
    let opts = SpawnOptions {
        binary: args.binary,
        listen: ListenSpec::WebSocket {
            host: args.host.clone(),
            port: args.port,
        },
        extra_args: args.extra,
        log_file: None,
    };
    let report = spawn(opts)?;
    println!(
        "codex app-server running: pid={} listen={} log={}",
        report.info.pid,
        report.info.listen,
        report.info.log_file.display(),
    );
    println!(
        "next steps: `pocket-codex pb register --key codex --local-addr {host}:{port} --relay \
         <relay-host:7666>`",
        host = args.host,
        port = args.port,
    );
    Ok(())
}

fn stop_cmd() -> Result<()> {
    match stop()? {
        StopOutcome::NoRecord => {
            println!("no codex app-server is currently supervised by pocket-codex");
        },
        StopOutcome::StaleRecord {
            pid,
        } => {
            println!("recorded pid {pid} was already gone; cleared state");
        },
        StopOutcome::Stopped {
            pid,
        } => {
            println!("sent SIGTERM to pid {pid}");
        },
    }
    Ok(())
}

fn status_cmd() -> Result<()> {
    let report = status()?;
    match report.recorded {
        Some(info) if report.alive => {
            println!(
                "alive: pid={} listen={} log={} since {}",
                info.pid,
                info.listen,
                info.log_file.display(),
                info.started_at,
            );
        },
        Some(info) => {
            println!(
                "stale: recorded pid={} listen={} is gone (started at {})",
                info.pid, info.listen, info.started_at,
            );
        },
        None => println!("no codex app-server is currently supervised by pocket-codex"),
    }
    Ok(())
}
