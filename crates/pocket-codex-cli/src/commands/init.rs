//! `pocket-codex init`: interactively persist the default relay + key.
//!
//! Stores `host:port` and the shared `MSG_HEADER_KEY` in `config.toml`
//! (0600) so later commands need neither `--relay` nor an exported
//! `$MSG_HEADER_KEY`. Verifies reachability before saving unless
//! `--no-verify` is passed.

use anyhow::{anyhow, bail, Result};
use pocket_codex_core::config::Config;

use crate::{
    cli::InitArgs,
    commands::{service_target::discover_services, ui},
};

/// Strip an optional `tcp://` scheme and validate `host:port`.
///
/// Splits on the last `:`, so DNS names, IPv4, and bracketed IPv6
/// (`[::1]:7666`) work. Bare IPv6 (`::1`) is not supported — wrap it in
/// brackets. Relay addresses in practice are DNS/IPv4.
pub(crate) fn normalize_relay(input: &str) -> Result<String> {
    let trimmed = input.trim();
    let bare = trimmed
        .strip_prefix("tcp://")
        .unwrap_or(trimmed)
        .trim_end_matches('/');
    let (host, port) = bare
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("relay `{input}` must be host:port"))?;
    if host.is_empty() {
        bail!("relay `{input}` is missing a host");
    }
    port.parse::<u16>()
        .map_err(|_| anyhow!("relay `{input}` has an invalid port"))?;
    Ok(bare.to_string())
}

/// Validate the shared key is exactly 32 bytes (pb-mapper's requirement).
pub(crate) fn validate_key(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.len() != 32 {
        bail!("MSG_HEADER_KEY must be exactly 32 bytes (got {})", trimmed.len());
    }
    Ok(trimmed.to_string())
}

/// Read one line for `label`, showing `default_hint` in brackets. Returns
/// the trimmed input, or `None` when the user accepts the default (blank).
/// Errors if stdin is not a TTY (so non-interactive callers fail clearly).
fn prompt(label: &str, default_hint: Option<&str>) -> Result<Option<String>> {
    use std::io::{stdin, stdout, IsTerminal, Write};
    if !stdin().is_terminal() {
        bail!("non-interactive environment: pass --relay and --key");
    }
    match default_hint {
        Some(hint) => print!("{label} [{hint}]: "),
        None => print!("{label}: "),
    }
    stdout().flush()?;
    let mut line = String::new();
    stdin().read_line(&mut line)?;
    let trimmed = line.trim();
    Ok((!trimmed.is_empty()).then(|| trimmed.to_string()))
}

/// Run `pocket-codex init`.
pub async fn run(args: InitArgs) -> Result<()> {
    let mut config = Config::load()?;

    // Relay: flag, else prompt (default = existing config value).
    let relay_input = match args.relay {
        Some(r) => r,
        None => prompt("relay (host:port)", config.relay())?
            .or_else(|| config.relay().map(str::to_string))
            .ok_or_else(|| anyhow!("a relay is required"))?,
    };
    let relay = normalize_relay(&relay_input)?;

    // Key: flag, else prompt. Existing key shown as "keep current".
    let key_hint = config.relay_key().map(|_| "keep current");
    let key_input = match args.key {
        Some(k) => k,
        None => prompt("MSG_HEADER_KEY (32 bytes)", key_hint)?
            .or_else(|| config.relay_key().map(str::to_string))
            .ok_or_else(|| anyhow!("a 32-byte MSG_HEADER_KEY is required"))?,
    };
    let key = validate_key(&key_input)?;

    if !args.no_verify {
        // Apply the new key to this process so discovery validates with it,
        // then probe the relay (bounded by the connect timeout from Task 2).
        pocket_codex_pb::set_msg_header_key(Some(&key))?;
        match discover_services(&relay).await {
            Ok(found) => {
                ui::field("verified", &format!("reached relay, {} service(s)", found.len()))
            },
            Err(err) => bail!(
                "could not reach relay `{relay}`: {err}\nfix the relay/key, or re-run with \
                 --no-verify to save anyway"
            ),
        }
    }

    config.set_relay(&relay);
    config.set_relay_key(&key);
    config.save()?;

    ui::headline(ui::Tone::Ok, "relay configured");
    ui::field("relay", &relay);
    ui::field("key", &format!("len={}", key.len()));
    ui::field(
        "config",
        &pocket_codex_core::paths::config_file()?
            .display()
            .to_string(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_relay_strips_scheme_and_keeps_host_port() {
        assert_eq!(normalize_relay("tcp://lb7666.top:7666").expect("ok"), "lb7666.top:7666");
        assert_eq!(normalize_relay("  lb7666.top:7666  ").expect("ok"), "lb7666.top:7666");
        assert_eq!(normalize_relay("tcp://1.2.3.4:7666/").expect("ok"), "1.2.3.4:7666");
    }

    #[test]
    fn normalize_relay_rejects_missing_port_or_host() {
        assert!(normalize_relay("lb7666.top").is_err());
        assert!(normalize_relay(":7666").is_err());
        assert!(normalize_relay("lb7666.top:notaport").is_err());
    }

    #[test]
    fn validate_key_requires_32_bytes() {
        assert!(validate_key("short").is_err());
        let ok = "0123456789abcdef0123456789abcdef";
        assert_eq!(validate_key(ok).expect("32 bytes"), ok);
        assert_eq!(ok.len(), 32);
    }
}
