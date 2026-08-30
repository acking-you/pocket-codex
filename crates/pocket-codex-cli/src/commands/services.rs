//! `pocket-codex services …` subcommand handlers.

use anyhow::{anyhow, Result};
use comfy_table::Cell;
use pocket_codex_core::{config::Config, service::ServiceKind};

use crate::{
    cli::{ServicesCmd, ServicesDefaultCmd, ServicesDefaultSetArgs, ServicesListArgs},
    commands::{service_target::discover_services, transport, ui},
};

/// Dispatch the `services` subcommand group.
pub async fn run(cmd: ServicesCmd) -> Result<()> {
    match cmd {
        ServicesCmd::List(args) => list(args).await,
        ServicesCmd::Default(ServicesDefaultCmd::Set(args)) => default_set(args),
    }
}

/// List the services this device can reach — the relay's own listing in both
/// modes, since an account credential already sees only its own namespace.
async fn list(args: ServicesListArgs) -> Result<()> {
    let wanted = args.kind.map(ServiceKind::from);
    let config = Config::load()?;
    let transport =
        transport::resolve_transport(args.relay.relay.as_deref(), None, &config).await?;

    let mut services = Vec::new();
    for kind in wanted.map(|k| vec![k]).unwrap_or_else(all_kinds) {
        services.extend(discover_services(&transport, kind).await?);
    }
    services.sort_by_key(|id| id.key());

    if services.is_empty() {
        ui::muted(match transport.is_account() {
            true => "no services in your account",
            false => "no Pocket-Codex services found",
        });
        return Ok(());
    }

    let mut table = ui::new_table(&["KEY", "DEVICE", "KIND", "NAME"]);
    for service in services {
        table.add_row(vec![
            Cell::new(transport.key(&service)),
            Cell::new(&service.device),
            ui::kind_cell(service.kind.as_key_segment(), service.kind),
            Cell::new(&service.name),
        ]);
    }
    println!("{table}");
    Ok(())
}

/// Every kind a listing shows when `--kind` is absent.
///
/// `Meta` is deliberately excluded: it is colocated with an app service as an
/// implementation detail, and listing it invites a user to connect to something
/// that is not a codex endpoint.
fn all_kinds() -> Vec<ServiceKind> {
    vec![ServiceKind::App, ServiceKind::Api]
}

fn default_set(args: ServicesDefaultSetArgs) -> Result<()> {
    let kind = ServiceKind::from(args.kind);
    let mut config = Config::load()?;
    config.set_default_service(kind, &args.device, &args.name);
    config.save()?;
    let target = config
        .default_service(kind)
        .ok_or_else(|| anyhow!("default target missing after setting {kind} service"))?;
    ui::headline(ui::Tone::Ok, &format!("default {kind} service"));
    ui::field("target", &format!("pcx:{}:{}:{}", target.device, kind, target.name));
    Ok(())
}
