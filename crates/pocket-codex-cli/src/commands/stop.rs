//! `pocket-codex stop` unified runtime stop command.

use anyhow::Result;
use pocket_codex_codex::{stop as stop_codex, StopOutcome as CodexStopOutcome};

use crate::{
    cli::StopArgs,
    commands::{
        managed_api::{self, StopOutcome as ApiStopOutcome},
        managed_pb::{self, StopFilter, StopOutcome as PbStopOutcome},
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
        println!("api: no managed sessions");
        return;
    }
    for outcome in outcomes {
        match outcome {
            ApiStopOutcome::Stopped(session) => {
                println!("api proxy: sent SIGTERM to pid {} key={}", session.pid, session.key)
            },
            ApiStopOutcome::Stale(session) => {
                println!("api proxy: stale pid {} cleared key={}", session.pid, session.key)
            },
        }
    }
}

fn print_codex_stop(outcome: CodexStopOutcome) {
    match outcome {
        CodexStopOutcome::NoRecord => println!("codex: not managed"),
        CodexStopOutcome::StaleRecord {
            pid,
        } => println!("codex: stale pid {pid} cleared"),
        CodexStopOutcome::Stopped {
            pid,
        } => println!("codex: sent SIGTERM to pid {pid}"),
    }
}

fn print_pb_stops(outcomes: Vec<PbStopOutcome>, filtered: bool) {
    if outcomes.is_empty() {
        if filtered {
            println!("pb: no matching managed sessions");
        } else {
            println!("pb: no managed sessions");
        }
        return;
    }

    for outcome in outcomes {
        match outcome {
            PbStopOutcome::Stopped(session) => println!(
                "pb {:?}: sent SIGTERM to pid {} key={}",
                session.role, session.pid, session.key
            ),
            PbStopOutcome::Stale(session) => println!(
                "pb {:?}: stale pid {} cleared key={}",
                session.role, session.pid, session.key
            ),
        }
    }
}
