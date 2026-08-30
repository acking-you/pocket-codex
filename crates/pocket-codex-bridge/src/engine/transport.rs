//! How the app resolves its [`Transport`] from persisted configuration.
//!
//! ```text
//!   signed in?  ─yes─▶ account:   GET /v1/relay, keys under `pcxu:<user>:`
//!         │ no
//!         ▼
//!                      self-host: configured relay + stored key, `pcx:` keys
//! ```
//!
//! The type itself, and the key-shape rules that must hold identically here and
//! in the CLI, live in [`pocket_codex_pb::transport`]. This module is only the
//! app's half: reading the support directory and, in account mode, fetching and
//! refreshing a credential. The CLI resolves the same type from flags instead.

use anyhow::{anyhow, Result};
use pocket_codex_core::config::Mode;
use pocket_codex_pb::RelaySession;
// Re-exported so callers keep saying `engine::transport::Transport`: within the
// engine, the resolver and the type it returns are one concept.
pub use pocket_codex_pb::Transport;

use crate::engine::{account, config::load_config, runtime};

/// Resolve this device's transport from its persisted configuration.
///
/// Async because account mode fetches a credential from the backend; that
/// result is cached (see [`account::relay_credential`]), so repeated calls are
/// cheap.
pub async fn resolve() -> Result<Transport> {
    let support = runtime::support_dir()?;
    let config = load_config(&support)?;
    if config.account_mode() == Mode::Account {
        let (session, expires_at) = account::relay_session(&support).await?;
        // A registration outlives the call that made it, and the relay cancels a
        // lapsed credential's tunnels — so the refresher has to be running for
        // hosting to survive its TTL. Idempotent, hence unconditional here.
        account::start_credential_refresh(&support, expires_at);
        return Ok(Transport {
            session,
            namespace: Some(account::relay_credential(&support).await?.namespace),
        });
    }
    let relay = config
        .relay()
        .ok_or_else(|| anyhow!("no relay configured"))?
        .to_string();
    let key = config
        .relay_key()
        .ok_or_else(|| anyhow!("no key configured"))?;
    // Length-checked here so a hand-edited config.toml cannot reach the SDK's own
    // error, which echoes the raw key into its message.
    if key.len() != 32 {
        return Err(anyhow!("stored MSG_HEADER_KEY is not 32 bytes; re-run setup"));
    }
    Ok(Transport {
        session: RelaySession::new(relay, key),
        namespace: None,
    })
}

/// Resolve the transport from the engine runtime, for the synchronous
/// flutter_rust_bridge entrypoints.
pub fn resolve_blocking() -> Result<Transport> {
    runtime::runtime().block_on(resolve())
}
