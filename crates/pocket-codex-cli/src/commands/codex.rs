//! `pocket-codex codex …` subcommand handlers.

use anyhow::Result;
use pocket_codex_codex::{
    spawn_ready, status, stop, ListenSpec, SpawnOptions, SpawnReadyError, StartupFailure,
    StopOutcome, READY_TIMEOUT,
};

use crate::{
    cli::{CodexCmd, CodexStartArgs},
    commands::{api_proxy, ui},
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
    // The spawned app-server reads proxy settings only from its environment,
    // never from codex's config.toml, so resolve the effective proxy (explicit
    // flag or env) and inject it via SpawnOptions. Only an explicit `--proxy`
    // is validated eagerly (see resolve_app_server_proxy).
    let proxy_requested = args.proxy.is_some();
    let effective_proxy = api_proxy::resolve_app_server_proxy(args.proxy.as_deref())?;

    let opts = SpawnOptions {
        binary: args.binary,
        listen: ListenSpec::WebSocket {
            host: args.host,
            port: args.port,
        },
        extra_args: args.extra,
        log_file: None,
        proxy: effective_proxy.clone(),
    };
    // Spawn + readiness in one step: don't print success for a child that
    // died on boot (classically a bind failure on an already-taken port) —
    // fail now, with the child's own error output.
    let report = spawn_ready(opts, READY_TIMEOUT).map_err(spawn_ready_error)?;
    ui::headline(ui::Tone::Ok, "codex app-server running");
    ui::field("pid", &report.info.pid.to_string());
    ui::field("listen", &report.info.listen);
    ui::field("log", &report.info.log_file.display().to_string());
    api_proxy::print_proxy_status(
        effective_proxy.as_deref(),
        proxy_requested,
        report.reused,
        api_proxy::SpawnCommand::CodexStart,
    );
    ui::headline(ui::Tone::Action, "next step");
    // Use the *resolved* listen address (`ws://host:port` minus the scheme) so
    // the hint shows the real port even when `--port 0` auto-selected one.
    let local_addr = report
        .info
        .listen
        .strip_prefix("ws://")
        .unwrap_or(&report.info.listen);
    ui::code(&format!(
        "pocket-codex pb register --key codex --local-addr {local_addr} --relay <relay-host:7666>"
    ));
    Ok(())
}

/// Render a [`SpawnReadyError`] as the launch command's error: a spawn-phase
/// error passes through unchanged (already an actionable message), a
/// readiness failure renders via [`startup_failure_error`]. Shared by
/// `codex start` and `serve` (both launch the app-server and take `--port`).
pub(crate) fn spawn_ready_error(err: SpawnReadyError) -> anyhow::Error {
    match err {
        SpawnReadyError::Spawn(e) => e.into(),
        SpawnReadyError::NotReady {
            failure, ..
        } => startup_failure_error(*failure),
    }
}

/// Render a [`StartupFailure`] as the launch command's error —
/// [`StartupFailure::diagnosis`] with the remedy phrased as the `--port`
/// flag. Shared by `codex start` and `serve` (both spawn the app-server and
/// both take `--port`).
pub(crate) fn startup_failure_error(failure: StartupFailure) -> anyhow::Error {
    let retry = match failure.port.and_then(|port| port.checked_add(1)) {
        Some(next) => format!("retry with `--port {next}` (or any free port)"),
        None => "retry with `--port <other>`".to_string(),
    };
    anyhow::anyhow!(failure.diagnosis(&retry))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_failure_error_carries_log_tail_and_port_hint() {
        let failure = StartupFailure {
            rpc_error: None,
            process_exited: true,
            port_in_use: true,
            listen: "ws://127.0.0.1:18080".to_string(),
            port: Some(18080),
            log_file: std::path::PathBuf::from("codex-app-server.log"),
            log_tail: vec!["Error: Address already in use (os error 10048)".to_string()],
        };
        let msg = startup_failure_error(failure).to_string();
        assert!(msg.contains("exited during startup"));
        assert!(msg.contains("codex-app-server.log"));
        assert!(msg.contains("os error 10048"));
        assert!(msg.contains("--port 18081"), "should suggest the next port: {msg}");
    }
}
