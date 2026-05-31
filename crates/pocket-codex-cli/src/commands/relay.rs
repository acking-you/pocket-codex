//! Relay address + shared-key resolution for `pocket-codex` commands.
//!
//! Precedence is `flag > config > $PB_MAPPER_SERVER`. The shared
//! `MSG_HEADER_KEY` is bound to the *configured* relay: [`resolve_relay`]
//! applies `config.key` to the process only when the relay it resolves is
//! the configured one, so both in-process queries and spawned `__worker`
//! children authenticate with it. An explicit `--relay <other>` therefore
//! keeps whatever `$MSG_HEADER_KEY` the caller exported for that relay.

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

/// Resolve the effective relay (`flag > config > $PB_MAPPER_SERVER`) and,
/// when that relay is the configured one, bind the process to the
/// configured `MSG_HEADER_KEY` so the upcoming query and any spawned
/// `__worker` child authenticate with it.
///
/// Applying the key here — rather than unconditionally at startup — keeps
/// `--relay <other>` honest: pointing at a different relay leaves any
/// caller-exported `$MSG_HEADER_KEY` in place instead of clobbering it
/// with the saved config key. A bad key is logged, not fatal, so it never
/// blocks the command from reporting its own clearer error.
pub(crate) fn resolve_relay(flag: Option<&str>, config: &Config) -> Result<String> {
    let env = std::env::var(RELAY_ENV).ok();
    let relay = resolve_relay_from(flag, config.relay(), env.as_deref())?;
    if config_key_applies(&relay, config.relay()) {
        if let Some(key) = config.relay_key() {
            if let Err(err) = pocket_codex_pb::set_msg_header_key(Some(key)) {
                tracing::warn!("ignoring configured relay key: {err}");
            }
        }
    }
    Ok(relay)
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
