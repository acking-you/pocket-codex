//! Config persistence + `pcx1:` share-string codec. Pure (no FRB, no
//! runtime), so it unit-tests standalone.
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use pocket_codex_core::config::Config;
use serde::{Deserialize, Serialize};

const PCX1_PREFIX: &str = "pcx1:";

/// Relay address + shared key carried by a `pcx1:` share string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharePayload {
    /// Relay `host:port`.
    pub relay: String,
    /// 32-byte `MSG_HEADER_KEY`.
    pub key: String,
}

/// Encode relay+key into a `pcx1:` + base64url(JSON) share string.
pub fn encode_pcx1(relay: &str, key: &str) -> Result<String> {
    let json = serde_json::to_vec(&SharePayload {
        relay: relay.to_string(),
        key: key.to_string(),
    })?;
    Ok(format!("{PCX1_PREFIX}{}", URL_SAFE_NO_PAD.encode(json)))
}

/// Decode a `pcx1:` share string; validates the key is exactly 32 bytes.
pub fn decode_pcx1(text: &str) -> Result<SharePayload> {
    let body = text
        .trim()
        .strip_prefix(PCX1_PREFIX)
        .ok_or_else(|| anyhow!("not a pcx1 share string (missing `pcx1:` prefix)"))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(body.trim())
        .context("invalid base64 in pcx1 string")?;
    let payload: SharePayload =
        serde_json::from_slice(&bytes).context("invalid JSON in pcx1 string")?;
    if payload.key.len() != 32 {
        bail!("MSG_HEADER_KEY must be exactly 32 bytes (got {})", payload.key.len());
    }
    Ok(payload)
}

/// Path to the persisted config TOML under the app-support dir.
pub fn config_path(support_dir: &Path) -> PathBuf {
    support_dir.join("config.toml")
}

/// Load the config from `<support_dir>/config.toml`. Missing file → default.
pub fn load_config(support_dir: &Path) -> Result<Config> {
    let path = config_path(support_dir);
    match std::fs::read_to_string(&path) {
        Ok(raw) => toml::from_str(&raw).context("parsing config.toml"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e).context("reading config.toml"),
    }
}

/// Persist the config. On unix the file is created 0o600 (it holds the
/// relay MSG_HEADER_KEY); permissions are set before the bytes are written.
pub fn save_config(support_dir: &Path, config: &Config) -> Result<()> {
    std::fs::create_dir_all(support_dir).context("creating support dir")?;
    let path = config_path(support_dir);
    let raw = toml::to_string_pretty(config)?;
    #[cfg(unix)]
    {
        use std::{
            io::Write as _,
            os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _},
        };
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)?;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        f.write_all(raw.as_bytes())?;
    }
    #[cfg(not(unix))]
    std::fs::write(&path, raw)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcx1_round_trips() {
        let key = "0123456789abcdef0123456789abcdef";
        let enc = encode_pcx1("lb7666.top:7666", key).expect("encode");
        assert!(enc.starts_with("pcx1:"));
        let got = decode_pcx1(&enc).expect("decode");
        assert_eq!(got.relay, "lb7666.top:7666");
        assert_eq!(got.key, key);
    }

    #[test]
    fn decode_rejects_bad_input() {
        assert!(decode_pcx1("lb7666.top:7666").is_err()); // no prefix
        assert!(decode_pcx1("pcx1:!!!notbase64").is_err());
        let short = encode_pcx1("r:1", "short").expect("encode");
        assert!(decode_pcx1(&short).is_err()); // key != 32 bytes
    }

    #[test]
    fn config_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("pcx-cfg-{}", std::process::id()));
        let mut cfg = Config::default();
        cfg.set_relay("lb7666.top:7666");
        cfg.set_relay_key("0123456789abcdef0123456789abcdef");
        save_config(&dir, &cfg).expect("save");
        let back = load_config(&dir).expect("load");
        assert_eq!(back.relay(), Some("lb7666.top:7666"));
        assert_eq!(back.relay_key(), Some("0123456789abcdef0123456789abcdef"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
