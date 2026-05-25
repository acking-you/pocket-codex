//! Service target resolution shared by app-server and API connect flows.
//!
//! ```text
//!   choose_target(kind, request, config, state, discovered)
//!                              │
//!                              ▼
//!         ┌─── request.key set? ────────────────────┐
//!         │                                          │ yes
//!         │ no                                       ▼
//!         │                                ResolvedTarget {
//!         │                                  key: <verbatim>,
//!         │                                  service_id: parse_key(key)
//!         │                                }
//!         ▼
//!   ┌─ request.device set? ─┐
//!   │                        │ yes ─▶ ServiceId::new(device, kind, name)
//!   │ no                                        │
//!   ▼                                           ▼
//!   config.default_service(kind) ─▶ ServiceId from device + name
//!         │ none
//!         ▼
//!   state.selected_service(kind) ─▶ ServiceId from device + name
//!         │ none
//!         ▼
//!   discovered.filter(kind):
//!         ├─ exactly one ─▶ ResolvedTarget(only)
//!         ├─ zero        ─▶ bail!("no {kind} services found …")
//!         └─ many        ─▶ bail!("multiple {kind} services found …")
//! ```
//!
//! Discovery hits the relay through [`pocket_codex_pb::keys`] and is
//! gated by the call sites: `connect` only queries the relay when the
//! caller passed neither `--key` nor `--device` *and* no local default
//! exists, so the common reuse path stays offline.

use anyhow::{bail, Result};
use pocket_codex_core::{
    config::Config,
    service::{default_device_id, ServiceId, ServiceKind},
    state::RuntimeState,
};
use tokio::net::lookup_host;

#[derive(Debug, Clone, Default)]
pub(crate) struct TargetRequest {
    pub key: Option<String>,
    pub device: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedTarget {
    pub key: String,
    pub service_id: Option<ServiceId>,
}

pub(crate) fn explicit_target(kind: ServiceKind, request: TargetRequest) -> Option<ResolvedTarget> {
    if let Some(key) = request.key {
        return Some(ResolvedTarget {
            service_id: ServiceId::parse_key(&key),
            key,
        });
    }
    request.device.map(|device| {
        let service_id = ServiceId::new(device, kind, request.name);
        ResolvedTarget {
            key: service_id.key(),
            service_id: Some(service_id),
        }
    })
}

pub(crate) fn generated_target(
    kind: ServiceKind,
    device: Option<String>,
    name: String,
) -> ResolvedTarget {
    let service_id = ServiceId::new(device.unwrap_or_else(default_device_id), kind, name);
    ResolvedTarget {
        key: service_id.key(),
        service_id: Some(service_id),
    }
}

pub(crate) fn choose_target(
    kind: ServiceKind,
    request: TargetRequest,
    config: &Config,
    state: &RuntimeState,
    discovered: &[ServiceId],
) -> Result<ResolvedTarget> {
    if let Some(target) = explicit_target(kind, request) {
        return Ok(target);
    }

    if let Some(default) = config.default_service(kind) {
        return Ok(generated_target(kind, Some(default.device.clone()), default.name.clone()));
    }

    if let Some(selected) = state.selected_service(kind) {
        return Ok(generated_target(kind, Some(selected.device.clone()), selected.name.clone()));
    }

    let candidates: Vec<ServiceId> = discovered
        .iter()
        .filter(|id| id.kind == kind)
        .cloned()
        .collect();
    match candidates.as_slice() {
        [only] => Ok(ResolvedTarget {
            key: only.key(),
            service_id: Some(only.clone()),
        }),
        [] => bail!("no {kind} services found on the relay; pass --device or --key"),
        many => {
            let list = many
                .iter()
                .map(|id| format!("{} ({})", id.key(), id.device))
                .collect::<Vec<_>>()
                .join(", ");
            bail!("multiple {kind} services found; pass --device or --key. candidates: {list}")
        },
    }
}

pub(crate) async fn discover_services(relay: &str) -> Result<Vec<ServiceId>> {
    let relay_addr = resolve_one(relay).await?;
    let keys = pocket_codex_pb::keys(relay_addr).await?;
    Ok(keys
        .into_iter()
        .filter_map(|key| ServiceId::parse_key(&key))
        .collect())
}

async fn resolve_one(addr: &str) -> Result<std::net::SocketAddr> {
    let mut iter = lookup_host(addr).await?;
    iter.next()
        .ok_or_else(|| anyhow::anyhow!("relay address `{addr}` resolved to no entries"))
}

#[cfg(test)]
mod tests {
    use pocket_codex_core::{
        config::Config,
        service::{ServiceId, ServiceKind},
        state::RuntimeState,
    };

    use super::*;

    #[test]
    fn explicit_key_wins_over_other_target_sources() {
        let request = TargetRequest {
            key: Some("custom".to_string()),
            device: Some("ignored".to_string()),
            name: "ignored".to_string(),
        };

        let resolved = choose_target(
            ServiceKind::App,
            request,
            &Config::default(),
            &RuntimeState::default(),
            &[ServiceId::new("studio", ServiceKind::App, "default")],
        )
        .expect("target");

        assert_eq!(resolved.key, "custom");
        assert!(resolved.service_id.is_none());
    }

    #[test]
    fn configured_default_wins_over_last_selection() {
        let mut config = Config::default();
        config.set_default_service(ServiceKind::App, "studio", "work");
        let mut state = RuntimeState::default();
        state.record_selected_service(ServiceKind::App, "old", "default");

        let resolved =
            choose_target(ServiceKind::App, TargetRequest::default(), &config, &state, &[])
                .expect("target");

        assert_eq!(resolved.key, "pcx:studio:app:work");
    }

    #[test]
    fn one_discovered_service_is_used_when_no_default_exists() {
        let resolved = choose_target(
            ServiceKind::Api,
            TargetRequest::default(),
            &Config::default(),
            &RuntimeState::default(),
            &[ServiceId::new("studio", ServiceKind::Api, "default")],
        )
        .expect("target");

        assert_eq!(resolved.key, "pcx:studio:api:default");
    }

    #[test]
    fn multiple_discovered_services_require_selection() {
        let err = choose_target(
            ServiceKind::App,
            TargetRequest::default(),
            &Config::default(),
            &RuntimeState::default(),
            &[
                ServiceId::new("macbook", ServiceKind::App, "default"),
                ServiceId::new("studio", ServiceKind::App, "default"),
            ],
        )
        .expect_err("ambiguous target");

        assert!(err.to_string().contains("multiple app services"));
    }
}
