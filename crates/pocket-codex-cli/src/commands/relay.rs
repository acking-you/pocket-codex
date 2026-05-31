//! Relay address + shared-key resolution for `pocket-codex` commands.
//!
//! Precedence is `flag > config > $PB_MAPPER_SERVER`. The shared
//! `MSG_HEADER_KEY` is applied once per process in [`apply_configured_key`]
//! so both in-process relay queries and spawned `__worker` children agree
//! on it.

use anyhow::{anyhow, Result};
use pocket_codex_core::config::Config;

/// Environment variable pb-mapper and the CLI both read for the relay.
const RELAY_ENV: &str = "PB_MAPPER_SERVER";

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

/// Resolve the effective relay from an explicit flag and loaded config,
/// falling back to `$PB_MAPPER_SERVER`.
pub(crate) fn resolve_relay(flag: Option<&str>, config: &Config) -> Result<String> {
    let env = std::env::var(RELAY_ENV).ok();
    resolve_relay_from(flag, config.relay(), env.as_deref())
}

/// Apply the configured `MSG_HEADER_KEY` to this process once, before any
/// relay traffic or worker spawn. No-op when config has no key (the
/// existing `$MSG_HEADER_KEY` env, if any, then stands). Best-effort: a
/// broken config must not stop offline commands like `version`.
pub(crate) fn apply_configured_key() {
    let Ok(config) = Config::load() else {
        return;
    };
    if let Some(key) = config.relay_key() {
        if let Err(err) = pocket_codex_pb::set_msg_header_key(Some(key)) {
            tracing::warn!("ignoring configured relay key: {err}");
        }
    }
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
}
