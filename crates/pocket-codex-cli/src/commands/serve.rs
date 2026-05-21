//! `pocket-codex serve` high-level host-side orchestration.

use anyhow::{Context, Result};
use pocket_codex_codex::{spawn, ListenSpec, SpawnOptions};
use pocket_codex_core::state::PbRole;

use crate::{
    cli::ServeArgs,
    commands::managed_pb::{self, EnsureOutcome, PbWorkerSpec},
};

/// Run the host-side one-shot setup flow.
pub async fn run(args: ServeArgs) -> Result<()> {
    let requested_listen = ListenSpec::WebSocket {
        host: args.host,
        port: args.port,
    };
    let report = spawn(SpawnOptions {
        binary: args.codex_binary,
        listen: requested_listen,
        extra_args: args.extra,
        log_file: None,
    })?;
    let local_addr = websocket_listen_addr(&report.info.listen).with_context(|| {
        format!("codex listen URL `{}` is not relayable TCP", report.info.listen)
    })?;

    let outcome = managed_pb::ensure(PbWorkerSpec {
        role: PbRole::Register,
        key: args.key.clone(),
        local_addr,
        relay_addr: args.relay.relay.clone(),
        codec: args.codec,
    })?;
    print_serve_summary(&report.info, &outcome, &args.key, &args.relay.relay);
    Ok(())
}

fn print_serve_summary(
    codex: &pocket_codex_core::state::CodexProcessInfo,
    pb: &EnsureOutcome,
    key: &str,
    relay: &str,
) {
    println!(
        "codex app-server: pid={} listen={} log={}",
        codex.pid,
        codex.listen,
        codex.log_file.display()
    );
    match pb {
        EnsureOutcome::Reused(session) => println!(
            "pb register reused: pid={} key={} relay={} log={}",
            session.pid,
            session.key,
            session.relay_addr,
            session.log_file.display()
        ),
        EnsureOutcome::Replaced {
            stale_pid,
            session,
        } => println!(
            "pb register replaced stale pid {} with pid={} key={} relay={} log={}",
            stale_pid,
            session.pid,
            session.key,
            session.relay_addr,
            session.log_file.display()
        ),
        EnsureOutcome::Spawned(session) => println!(
            "pb register started: pid={} key={} relay={} log={}",
            session.pid,
            session.key,
            session.relay_addr,
            session.log_file.display()
        ),
    }
    println!("client setup: pocket-codex connect --key {key} --relay {relay}");
}

fn websocket_listen_addr(listen: &str) -> Option<String> {
    listen
        .strip_prefix("ws://")
        .filter(|addr| !addr.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_listen_addr_extracts_tcp_addr() {
        assert_eq!(
            websocket_listen_addr("ws://127.0.0.1:18080").as_deref(),
            Some("127.0.0.1:18080")
        );
        assert_eq!(websocket_listen_addr("unix:///tmp/codex.sock"), None);
    }
}
