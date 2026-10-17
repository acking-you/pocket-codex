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
    let managed = pocket_codex_bridge::headless::read_status(&super::managed_host::state_dir()?)?;

    if codex.recorded.is_none() && state.api.is_empty() && state.pb.is_empty() && managed.is_none()
    {
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
    let mut table =
        ui::new_table(&["COMPONENT", "STATE", "PID", "ENDPOINT", "KEY", "UPTIME", "PB SDK"]);

    if let Some(managed) = managed {
        table.add_row(vec![
            Cell::new("computer management"),
            ui::state_cell(
                if managed.management.registered { "connected" } else { "pending" },
                true,
            ),
            Cell::new(managed.pid),
            Cell::new(managed.management.listen_addr.as_deref().unwrap_or("—")),
            Cell::new(managed.management.service_key.as_deref().unwrap_or("—")),
            Cell::new("—"),
            Cell::new(pocket_codex_pb::SDK_VERSION),
        ]);
        for service in managed.services {
            table.add_row(vec![
                Cell::new(format!("managed {}", service.name)),
                ui::state_cell(state_label(service.alive), service.alive),
                Cell::new(
                    service
                        .pid
                        .map_or_else(|| "—".into(), |pid| pid.to_string()),
                ),
                Cell::new(service.endpoint),
                Cell::new(service.key),
                Cell::new("—"),
                Cell::new(pocket_codex_pb::SDK_VERSION),
            ]);
        }
    }

    if let Some(info) = &codex.recorded {
        table.add_row(vec![
            Cell::new("codex"),
            ui::state_cell(state_label(codex.alive), codex.alive),
            Cell::new(info.pid),
            Cell::new(&info.listen),
            Cell::new("—"),
            Cell::new(ui::relative_time(&info.started_at)),
            Cell::new("—"),
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
            Cell::new("—"),
        ]);
    }

    for session in &state.pb {
        let alive = pocket_codex_core::process::pid_running(session.pid);
        let health = crate::commands::worker_health::read(session);
        table.add_row(vec![
            Cell::new(format!("pb {}", session.role)),
            ui::state_cell(
                health
                    .as_ref()
                    .map_or(state_label(alive), |h| h.status.as_str()),
                alive,
            ),
            Cell::new(session.pid),
            Cell::new(&session.local_addr),
            Cell::new(&session.key),
            Cell::new(ui::relative_time(&session.started_at)),
            Cell::new(
                health
                    .as_ref()
                    .map_or("unknown", |h| h.sdk_version.as_str()),
            ),
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
