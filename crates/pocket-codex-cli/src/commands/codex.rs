//! `pocket-codex codex …` subcommand handlers.

use anyhow::Result;
use pocket_codex_codex::{spawn, status, stop, ListenSpec, SpawnOptions, StopOutcome};

use crate::{
    cli::{CodexCmd, CodexStartArgs},
    commands::ui,
};

/// Dispatch the `codex` subcommand group.
pub async fn run(cmd: CodexCmd) -> Result<()> {
    match cmd {
        CodexCmd::Start(args) => start(args),
        CodexCmd::Stop => stop_cmd(),
        CodexCmd::Status => status_cmd(),
    }
}

fn start(args: CodexStartArgs) -> Result<()> {
    let host = args.host.clone();
    let port = args.port;
    let opts = SpawnOptions {
        binary: args.binary,
        listen: ListenSpec::WebSocket {
            host: args.host,
            port: args.port,
        },
        extra_args: args.extra,
        log_file: None,
    };
    let report = spawn(opts)?;
    ui::headline(ui::Tone::Ok, "codex app-server running");
    ui::field("pid", &report.info.pid.to_string());
    ui::field("listen", &report.info.listen);
    ui::field("log", &report.info.log_file.display().to_string());
    ui::headline(ui::Tone::Action, "next step");
    ui::code(&format!(
        "pocket-codex pb register --key codex --local-addr {host}:{port} --relay <relay-host:7666>"
    ));
    Ok(())
}

fn stop_cmd() -> Result<()> {
    match stop()? {
        StopOutcome::NoRecord => {
            ui::muted("no codex app-server is currently supervised by pocket-codex");
        },
        StopOutcome::StaleRecord {
            pid,
        } => {
            ui::headline(ui::Tone::Muted, "codex stale cleared");
            ui::field("pid", &pid.to_string());
        },
        StopOutcome::Stopped {
            pid,
        } => {
            ui::headline(ui::Tone::Ok, "codex stopped");
            ui::field("pid", &pid.to_string());
        },
    }
    Ok(())
}

fn status_cmd() -> Result<()> {
    let report = status()?;
    match report.recorded {
        Some(info) if report.alive => {
            ui::headline(ui::Tone::Ok, "codex app-server alive");
            ui::field("pid", &info.pid.to_string());
            ui::field("listen", &info.listen);
            ui::field("log", &info.log_file.display().to_string());
            ui::field("uptime", &ui::relative_time(&info.started_at));
        },
        Some(info) => {
            ui::headline(ui::Tone::Muted, "codex app-server stale");
            ui::field("pid", &info.pid.to_string());
            ui::field("listen", &info.listen);
            ui::field("started", &info.started_at);
        },
        None => ui::muted("no codex app-server is currently supervised by pocket-codex"),
    }
    Ok(())
}
