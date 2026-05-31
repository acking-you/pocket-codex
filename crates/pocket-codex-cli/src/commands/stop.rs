//! `pocket-codex stop` unified runtime stop command.

use anyhow::Result;
use pocket_codex_codex::{stop as stop_codex, StopOutcome as CodexStopOutcome};

use crate::{
    cli::StopArgs,
    commands::{
        managed_api::{self, StopOutcome as ApiStopOutcome},
        managed_pb::{self, StopFilter, StopOutcome as PbStopOutcome},
        ui,
    },
};

/// Stop Pocket-Codex managed sessions.
pub fn run(args: StopArgs) -> Result<()> {
    let filtering_pb = args.key.is_some() || args.role.is_some();
    if !filtering_pb {
        print_codex_stop(stop_codex()?);
        print_api_stops(managed_api::stop_all()?);
    }

    let outcomes = managed_pb::stop_matching(StopFilter {
        role: args.role.map(Into::into),
        key: args.key,
    })?;
    print_pb_stops(outcomes, filtering_pb);
    Ok(())
}

fn print_api_stops(outcomes: Vec<ApiStopOutcome>) {
    if outcomes.is_empty() {
        ui::muted("api: no managed sessions");
        return;
    }
    for outcome in outcomes {
        match outcome {
            ApiStopOutcome::Stopped(session) => {
                ui::headline(ui::Tone::Ok, "api proxy stopped");
                ui::field("pid", &session.pid.to_string());
                ui::field("key", &session.key);
            },
            ApiStopOutcome::Stale(session) => {
                ui::headline(ui::Tone::Muted, "api proxy stale cleared");
                ui::field("pid", &session.pid.to_string());
                ui::field("key", &session.key);
            },
        }
    }
}

fn print_codex_stop(outcome: CodexStopOutcome) {
    match outcome {
        CodexStopOutcome::NoRecord => ui::muted("codex: not managed"),
        CodexStopOutcome::StaleRecord {
            pid,
        } => {
            ui::headline(ui::Tone::Muted, "codex stale cleared");
            ui::field("pid", &pid.to_string());
        },
        CodexStopOutcome::Stopped {
            pid,
        } => {
            ui::headline(ui::Tone::Ok, "codex stopped");
            ui::field("pid", &pid.to_string());
        },
    }
}

fn print_pb_stops(outcomes: Vec<PbStopOutcome>, filtered: bool) {
    if outcomes.is_empty() {
        if filtered {
            ui::muted("pb: no matching managed sessions");
        } else {
            ui::muted("pb: no managed sessions");
        }
        return;
    }

    for outcome in outcomes {
        match outcome {
            PbStopOutcome::Stopped(session) => {
                ui::headline(ui::Tone::Ok, &format!("pb {} stopped", session.role));
                ui::field("pid", &session.pid.to_string());
                ui::field("key", &session.key);
            },
            PbStopOutcome::Stale(session) => {
                ui::headline(ui::Tone::Muted, &format!("pb {} stale cleared", session.role));
                ui::field("pid", &session.pid.to_string());
                ui::field("key", &session.key);
            },
        }
    }
}
