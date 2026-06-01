//! FRB-exposed bridge surface: config, discovery, API-service subscribe.
//! Thin glue over `crate::engine`; DTOs are plain (FRB-friendly) structs.
use std::path::PathBuf;

use anyhow::{anyhow, Result};

use crate::engine::{config, discovery, runtime};

/// View of persisted config for the UI; never exposes the raw key.
pub struct ConfigView {
    /// Configured relay `host:port`, if any.
    pub relay: Option<String>,
    /// Whether a 32-byte key is stored (value withheld).
    pub has_key: bool,
}

/// A discovered service, mirrored for Dart.
pub struct ServiceIdDto {
    /// Device id segment.
    pub device: String,
    /// `app` or `api`.
    pub kind: String,
    /// Instance name segment.
    pub name: String,
    /// Full relay key.
    pub key: String,
}

/// Status of one active subscription, mirrored for Dart.
pub struct SubStatusDto {
    /// Service key.
    pub key: String,
    /// Local `host:port`.
    pub local_addr: String,
    /// Task still running.
    pub alive: bool,
}

/// Initialise the engine with the platform app-support dir (from Dart's
/// path_provider). Must be called once after `RustLib.init()`.
pub fn init_bridge(support_dir: String) -> Result<()> {
    runtime::init(PathBuf::from(support_dir))
}

fn current_relay() -> Result<String> {
    let cfg = config::load_config(&runtime::support_dir()?)?;
    cfg.relay()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("no relay configured"))
}

/// Apply the stored MSG_HEADER_KEY to this process (relay validates it).
fn apply_key() -> Result<()> {
    let cfg = config::load_config(&runtime::support_dir()?)?;
    if let Some(k) = cfg.relay_key() {
        // Guard length here so a hand-edited config.toml can't reach the
        // upstream length error (which echoes the raw key into its message).
        if k.len() != 32 {
            return Err(anyhow!("stored MSG_HEADER_KEY is not 32 bytes; re-run setup"));
        }
        pocket_codex_pb::set_msg_header_key(Some(k)).map_err(|e| anyhow!("{e}"))?;
    }
    Ok(())
}

/// Current config view (relay + whether a key is set).
pub fn get_config() -> Result<ConfigView> {
    let cfg = config::load_config(&runtime::support_dir()?)?;
    Ok(ConfigView {
        relay: cfg.relay().map(str::to_string),
        has_key: cfg.relay_key().is_some(),
    })
}

/// Set the relay `host:port` and persist.
pub fn set_relay(relay: String) -> Result<()> {
    let dir = runtime::support_dir()?;
    let mut cfg = config::load_config(&dir)?;
    cfg.set_relay(&relay);
    config::save_config(&dir, &cfg)
}

/// Set the 32-byte MSG_HEADER_KEY and persist (validates length).
pub fn set_key(key: String) -> Result<()> {
    if key.len() != 32 {
        return Err(anyhow!("MSG_HEADER_KEY must be exactly 32 bytes (got {})", key.len()));
    }
    let dir = runtime::support_dir()?;
    let mut cfg = config::load_config(&dir)?;
    cfg.set_relay_key(&key);
    config::save_config(&dir, &cfg)
}

/// Import a `pcx1:` share string: decode, persist relay + key, return relay.
pub fn import_config(text: String) -> Result<String> {
    let payload = config::decode_pcx1(&text)?;
    let dir = runtime::support_dir()?;
    let mut cfg = config::load_config(&dir)?;
    cfg.set_relay(&payload.relay);
    cfg.set_relay_key(&payload.key);
    config::save_config(&dir, &cfg)?;
    Ok(payload.relay)
}

/// Export the current relay+key as a `pcx1:` share string.
pub fn export_config() -> Result<String> {
    let cfg = config::load_config(&runtime::support_dir()?)?;
    let relay = cfg.relay().ok_or_else(|| anyhow!("no relay configured"))?;
    let key = cfg
        .relay_key()
        .ok_or_else(|| anyhow!("no key configured"))?;
    config::encode_pcx1(relay, key)
}

/// Discover services on the configured relay (applies the stored key first).
pub fn discover_services() -> Result<Vec<ServiceIdDto>> {
    apply_key()?;
    let relay = current_relay()?;
    let found = runtime::runtime().block_on(discovery::discover(&relay))?;
    Ok(found
        .into_iter()
        .map(|s| ServiceIdDto {
            device: s.device,
            kind: s.kind,
            name: s.name,
            key: s.key,
        })
        .collect())
}

/// Subscribe to an API service, exposing it on `127.0.0.1:<local_port>`.
pub fn api_subscribe(service_key: String, local_port: u16) -> Result<SubStatusDto> {
    apply_key()?;
    let relay = current_relay()?;
    let s = runtime::subscribe_service(service_key, local_port, relay)?;
    Ok(SubStatusDto {
        key: s.key,
        local_addr: s.local_addr,
        alive: s.alive,
    })
}

/// Stop an API-service subscription.
pub fn api_unsubscribe(service_key: String) {
    runtime::unsubscribe_service(&service_key);
}

/// List all active subscriptions.
pub fn subscriptions() -> Vec<SubStatusDto> {
    runtime::list_subscriptions()
        .into_iter()
        .map(|s| SubStatusDto {
            key: s.key,
            local_addr: s.local_addr,
            alive: s.alive,
        })
        .collect()
}
