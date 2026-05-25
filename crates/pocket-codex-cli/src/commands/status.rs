//! `pocket-codex status` unified runtime status output.

use anyhow::Result;
use pocket_codex_codex::status as codex_status;
use pocket_codex_core::{process::pid_alive, state::RuntimeState};

/// Print the status of all Pocket-Codex managed sessions.
pub fn run() -> Result<()> {
    let codex = codex_status()?;
    match codex.recorded {
        Some(info) if codex.alive => println!(
            "codex: alive pid={} listen={} log={} since {}",
            info.pid,
            info.listen,
            info.log_file.display(),
            info.started_at
        ),
        Some(info) => println!(
            "codex: stale pid={} listen={} log={} since {}",
            info.pid,
            info.listen,
            info.log_file.display(),
            info.started_at
        ),
        None => println!("codex: not managed"),
    }

    let state = RuntimeState::load()?;
    if state.api.is_empty() {
        println!("api: no managed sessions");
    } else {
        for session in &state.api {
            let state = if pid_alive(session.pid) { "alive" } else { "stale" };
            println!(
                "api proxy: {} pid={} key={} local={} log={} since {}",
                state,
                session.pid,
                session.key,
                session.local_addr,
                session.log_file.display(),
                session.started_at
            );
        }
    }

    if state.pb.is_empty() {
        println!("pb: no managed sessions");
    } else {
        for session in state.pb {
            let state = if pid_alive(session.pid) { "alive" } else { "stale" };
            println!(
                "pb {:?}: {} pid={} key={} local={} relay={} codec={} log={} since {}",
                session.role,
                state,
                session.pid,
                session.key,
                session.local_addr,
                session.relay_addr,
                session.codec,
                session.log_file.display(),
                session.started_at
            );
        }
    }
    Ok(())
}
