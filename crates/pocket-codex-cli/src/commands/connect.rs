//! `pocket-codex connect` high-level client-side orchestration.

use anyhow::Result;
use pocket_codex_core::{
    config::Config,
    service::ServiceKind,
    state::{PbRole, RuntimeState},
};

use crate::{
    cli::ConnectArgs,
    commands::{
        managed_pb::{self, EnsureOutcome, PbWorkerSpec},
        service_target::{choose_target, discover_services, TargetRequest},
    },
};

/// Run the client-side setup flow.
pub async fn run(args: ConnectArgs) -> Result<()> {
    let request = TargetRequest {
        key: args.key,
        device: args.device,
        name: args.name,
    };
    let needs_discovery = request.key.is_none() && request.device.is_none();
    let config = Config::load()?;
    let state = RuntimeState::load()?;
    let has_local_default = config.default_service(ServiceKind::App).is_some()
        || state.selected_service(ServiceKind::App).is_some();
    let discovered = if needs_discovery && !has_local_default {
        discover_services(&args.relay.relay).await?
    } else {
        Vec::new()
    };
    let target = choose_target(ServiceKind::App, request, &config, &state, &discovered)?;
    let outcome = managed_pb::ensure(PbWorkerSpec {
        role: PbRole::Subscribe,
        key: target.key,
        local_addr: args.local_addr,
        relay_addr: args.relay.relay,
        codec: false,
    })?;
    if let Some(service_id) = target.service_id {
        let mut state = RuntimeState::load()?;
        state.record_selected_service(ServiceKind::App, service_id.device, service_id.name);
        state.save()?;
    }
    print_connect_summary(&outcome);
    Ok(())
}

fn print_connect_summary(outcome: &EnsureOutcome) {
    let session = match outcome {
        EnsureOutcome::Reused(session) => {
            println!(
                "pb subscribe reused: pid={} key={} relay={} log={}",
                session.pid,
                session.key,
                session.relay_addr,
                session.log_file.display()
            );
            session
        },
        EnsureOutcome::Replaced {
            stale_pid,
            session,
        } => {
            println!(
                "pb subscribe replaced stale pid {} with pid={} key={} relay={} log={}",
                stale_pid,
                session.pid,
                session.key,
                session.relay_addr,
                session.log_file.display()
            );
            session
        },
        EnsureOutcome::Spawned(session) => {
            println!(
                "pb subscribe started: pid={} key={} relay={} log={}",
                session.pid,
                session.key,
                session.relay_addr,
                session.log_file.display()
            );
            session
        },
    };
    println!("codex remote: {}", codex_remote_command(&session.local_addr));
}

pub(crate) fn remote_ws_url(local_addr: &str) -> String {
    format!("ws://{local_addr}")
}

pub(crate) fn codex_remote_command(local_addr: &str) -> String {
    format!("codex --remote {}", remote_ws_url(local_addr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_remote_command_uses_local_subscriber_listener() {
        assert_eq!(codex_remote_command("127.0.0.1:28080"), "codex --remote ws://127.0.0.1:28080");
    }
}
