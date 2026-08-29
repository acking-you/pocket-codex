//! Relay address + credential resolution for `pocket-codex` commands.
//!
//! Precedence is `flag > config > $PB_MAPPER_SERVER`. The credential is bound
//! to the *configured* relay: [`resolve_relay`] pairs the resolved address with
//! `config.key` only when that address IS the configured one, so an explicit
//! `--relay <other>` uses whatever `$MSG_HEADER_KEY` the caller exported for
//! that relay rather than silently presenting the saved key to a stranger.
//!
//! That rule used to be enforced by conditionally mutating pb-mapper's
//! process-global key. It is now a returned value, which is a better fit for
//! the same intent: there is no window in which the process holds a credential
//! for a relay it is not talking to.

use anyhow::{anyhow, Result};
use pocket_codex_core::config::Config;
use pocket_codex_pb::RelaySession;

/// Environment variable pb-mapper and the CLI both read for the relay.
const RELAY_ENV: &str = "PB_MAPPER_SERVER";

/// Environment variable carrying a relay credential — the administrator key or
/// a `pbmt1_` temporary credential. Named for pb-mapper's own variable so an
/// operator's existing export keeps working.
pub(crate) const CREDENTIAL_ENV: &str = "MSG_HEADER_KEY";

/// Pure precedence resolver, factored out for testing.
fn resolve_relay_from(
    flag: Option<&str>,
    config: Option<&str>,
    env: Option<&str>,
) -> Result<String> {
    [flag, config, env]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| {
            anyhow!("no relay configured; run `pocket-codex init` or pass --relay <host:port>")
        })
}

/// Resolve the effective relay address (`flag > config > $PB_MAPPER_SERVER`).
pub(crate) fn resolve_relay(flag: Option<&str>, config: &Config) -> Result<String> {
    let env = std::env::var(RELAY_ENV).ok();
    resolve_relay_from(flag, config.relay(), env.as_deref())
}

/// Resolve the relay to talk to AND the credential to present to it.
///
/// The saved key is used only for the configured relay; for any other address
/// the caller's `$MSG_HEADER_KEY` applies. Errors when neither yields one,
/// because pb-mapper 0.5 fails closed on a missing credential — reporting that
/// here names the actual problem instead of surfacing it as a connect failure.
pub(crate) fn resolve_session(flag: Option<&str>, config: &Config) -> Result<RelaySession> {
    let relay = resolve_relay(flag, config)?;
    let env = std::env::var(CREDENTIAL_ENV).ok();
    let credential = if config_key_applies(&relay, config.relay()) {
        config.relay_key().map(ToString::to_string).or(env)
    } else {
        env
    };
    let credential = credential
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "no relay credential for {relay}; run `pocket-codex init` or export \
                 {CREDENTIAL_ENV}"
            )
        })?;
    Ok(RelaySession::new(relay, credential))
}

/// Whether the configured `MSG_HEADER_KEY` should be applied: only when the
/// resolved relay is exactly the configured relay. A flag/env relay that
/// differs — or the absence of a configured relay — leaves the ambient
/// `$MSG_HEADER_KEY` untouched.
fn config_key_applies(resolved: &str, config_relay: Option<&str>) -> bool {
    config_relay.is_some_and(|cfg| cfg == resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_wins_over_config_and_env() {
        let r = resolve_relay_from(Some("flag:1"), Some("cfg:2"), Some("env:3")).expect("resolves");
        assert_eq!(r, "flag:1");
    }

    #[test]
    fn config_wins_over_env_when_no_flag() {
        let r = resolve_relay_from(None, Some("cfg:2"), Some("env:3")).expect("resolves");
        assert_eq!(r, "cfg:2");
    }

    #[test]
    fn env_used_when_no_flag_or_config() {
        let r = resolve_relay_from(None, None, Some("env:3")).expect("resolves");
        assert_eq!(r, "env:3");
    }

    #[test]
    fn blank_candidates_are_skipped_then_error() {
        assert_eq!(
            resolve_relay_from(Some("  "), None, Some("env:3")).expect("falls back to env"),
            "env:3"
        );
        assert!(resolve_relay_from(None, None, None).is_err());
        assert!(resolve_relay_from(Some(""), Some("  "), Some("")).is_err());
    }

    #[test]
    fn config_key_applies_only_when_relay_matches_config() {
        // Resolved relay is the configured one (no flag, or flag == config).
        assert!(config_key_applies("relay-a:7666", Some("relay-a:7666")));
        // Explicit --relay to a different relay: keep the ambient env key.
        assert!(!config_key_applies("relay-b:7666", Some("relay-a:7666")));
        // No configured relay at all: nothing to bind.
        assert!(!config_key_applies("relay-a:7666", None));
    }
}
