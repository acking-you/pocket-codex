//! `pocket-codex status` unified runtime status output.

use anyhow::Result;
use comfy_table::Cell;
use pocket_codex_codex::status as codex_status;
use pocket_codex_core::{process::pid_alive, state::RuntimeState};

use crate::commands::ui;

/// Print the status of all Pocket-Codex managed sessions.
pub fn run() -> Result<()> {
    let codex = codex_status()?;
    let state = RuntimeState::load()?;

    if codex.recorded.is_none() && state.api.is_empty() && state.pb.is_empty() {
        ui::muted("no Pocket-Codex sessions are running");
        return Ok(());
    }

    // All workers share one log directory; derive it from whichever
    // session we happen to have so the footer can point at it.
    let logs_dir = codex
        .recorded
        .as_ref()
        .map(|info| info.log_file.clone())
        .or_else(|| state.api.first().map(|s| s.log_file.clone()))
        .or_else(|| state.pb.first().map(|s| s.log_file.clone()))
        .and_then(|path| path.parent().map(|dir| dir.display().to_string()));
    let relay = state.pb.first().map(|session| session.relay_addr.clone());

    ui::banner("Pocket-Codex runtime status", relay.as_deref());
    let mut table = ui::new_table(&["COMPONENT", "STATE", "PID", "ENDPOINT", "KEY", "UPTIME"]);

    if let Some(info) = &codex.recorded {
        table.add_row(vec![
            Cell::new("codex"),
            ui::state_cell(state_label(codex.alive), codex.alive),
            Cell::new(info.pid),
            Cell::new(&info.listen),
            Cell::new("—"),
            Cell::new(ui::relative_time(&info.started_at)),
        ]);
    }

    for session in &state.api {
        let alive = pid_alive(session.pid);
        table.add_row(vec![
            Cell::new("api proxy"),
            ui::state_cell(state_label(alive), alive),
            Cell::new(session.pid),
            Cell::new(&session.local_addr),
            Cell::new(&session.key),
            Cell::new(ui::relative_time(&session.started_at)),
        ]);
    }

    for session in &state.pb {
        let alive = pid_alive(session.pid);
        table.add_row(vec![
            Cell::new(format!("pb {}", session.role)),
            ui::state_cell(state_label(alive), alive),
            Cell::new(session.pid),
            Cell::new(&session.local_addr),
            Cell::new(&session.key),
            Cell::new(ui::relative_time(&session.started_at)),
        ]);
    }

    println!("{table}");
    if let Some(dir) = logs_dir {
        ui::footer("logs", &dir);
    }
    Ok(())
}

/// Map liveness to the STATE column label.
fn state_label(alive: bool) -> &'static str {
    if alive {
        "alive"
    } else {
        "stale"
    }
}
