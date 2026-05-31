//! `pocket-codex codex …` subcommand handlers.

use anyhow::Result;
use pocket_codex_codex::{spawn, status, stop, ListenSpec, SpawnOptions, StopOutcome};

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
    let host = args.host.clone();
    let port = args.port;

    // The spawned app-server reads proxy settings only from its environment,
    // never from codex's config.toml, so resolve the effective proxy (explicit
    // flag or env), fail fast on a bad scheme, and inject it via SpawnOptions.
    let proxy_requested = args.proxy.is_some();
    let effective_proxy = api_proxy::resolve_proxy(args.proxy.as_deref());
    if let Some(raw) = effective_proxy.as_deref() {
        api_proxy::validate_proxy(raw)?;
    }

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
    let report = spawn(opts)?;
    ui::headline(ui::Tone::Ok, "codex app-server running");
    ui::field("pid", &report.info.pid.to_string());
    ui::field("listen", &report.info.listen);
    ui::field("log", &report.info.log_file.display().to_string());
    print_proxy_status(effective_proxy.as_deref(), proxy_requested, report.reused);
    ui::headline(ui::Tone::Action, "next step");
    ui::code(&format!(
        "pocket-codex pb register --key codex --local-addr {host}:{port} --relay <relay-host:7666>"
    ));
    Ok(())
}

/// Surface the spawned app-server's proxy posture: confirm an injected
/// proxy, warn when none is set, flag SOCKS' HTTP blind spot, and note
/// when a `--proxy` could not take effect because the process was reused.
fn print_proxy_status(effective: Option<&str>, proxy_requested: bool, reused: bool) {
    match effective {
        Some(raw) => {
            ui::field("proxy", &api_proxy::redact_proxy(raw));
            if api_proxy::proxy_is_socks(raw) {
                ui::warn(
                    "socks5 proxy carries only the model WebSocket. codex's reqwest client has \
                     no SOCKS support, so codex_apps and plugin sync stay direct and will time \
                     out on a blocked network. Use an `http://` proxy to fix codex_apps.",
                );
            }
        },
        None => ui::warn(
            "no upstream proxy configured. The codex app-server reaches chatgpt.com directly and \
             will fail on networks that block it (codex_apps bootstrap times out, model calls \
             stall). Pass `--proxy http://host:port`, or export HTTPS_PROXY / ALL_PROXY / \
             HTTP_PROXY before running `pocket-codex codex start`.",
        ),
    }
    if reused && proxy_requested {
        ui::warn(
            "the codex app-server was already running, so this `--proxy` did not take effect. To \
             apply a new proxy, run `pocket-codex codex stop` first, then `pocket-codex codex \
             start --proxy …`.",
        );
    }
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
