//! Well-known filesystem paths used by Pocket-Codex.
//!
//! ```text
//!     ProjectDirs("io.github", "acking-you", "pocket-codex")
//!                            │
//!         ┌──────────────────┴──────────────────────┐
//!         ▼                                          ▼
//!      config_dir()                              state_dir()
//!  $XDG_CONFIG_HOME/pocket-codex             $XDG_STATE_HOME/pocket-codex
//!  ~/Library/Application Support/…           (macOS: falls back to data_dir)
//!         │                                          │
//!         ▼                                          ▼
//!      config.toml                       ┌───────────┴────────────┐
//!                                        ▼                        ▼
//!                                    state.toml                 logs/
//!                                                                  │
//!                                  ┌───────────────────────────────┼──────────────┐
//!                                  ▼                               ▼              ▼
//!                          codex-app-server.log     pb-{role}-{key}.log    api-proxy-{key}.log
//! ```
//!
//! Service keys reach the filesystem via `safe_file_component`,
//! which substitutes `_` for any character outside `[a-z0-9._-]`, so
//! a colon-bearing key like `pcx:studio:api:default` becomes
//! `pcx_studio_api_default` in the log filename. Empty inputs fall
//! back to `default` so the path stays well-formed.

use std::path::PathBuf;

use directories::ProjectDirs;

use crate::{
    error::{Error, Result},
    state::PbRole,
};

const QUALIFIER: &str = "io.github";
const ORG: &str = "acking-you";
const APP: &str = "pocket-codex";

/// Resolve the [`ProjectDirs`] handle, returning a typed error if the
/// host OS does not expose a usable home directory.
fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from(QUALIFIER, ORG, APP)
        .ok_or_else(|| Error::Path("cannot determine pocket-codex project directories".into()))
}

/// Configuration directory (e.g. `$XDG_CONFIG_HOME/pocket-codex`).
pub fn config_dir() -> Result<PathBuf> {
    Ok(project_dirs()?.config_dir().to_path_buf())
}

/// State directory used for runtime metadata (PID files, log files).
///
/// Falls back to the data dir on platforms where `directories` does not
/// expose a separate state dir (e.g. macOS).
pub fn state_dir() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    Ok(dirs
        .state_dir()
        .unwrap_or_else(|| dirs.data_dir())
        .to_path_buf())
}

/// Path to the persisted runtime state file (`state.toml`).
pub fn state_file() -> Result<PathBuf> {
    Ok(state_dir()?.join("state.toml"))
}

/// Path to the default codex app-server log file.
pub fn codex_log_file() -> Result<PathBuf> {
    Ok(state_dir()?.join("logs").join("codex-app-server.log"))
}

/// Path to the default pb-mapper worker log file for a role/key pair.
pub fn pb_log_file(role: PbRole, key: &str) -> Result<PathBuf> {
    let role_name = match role {
        PbRole::Register => "register",
        PbRole::Subscribe => "subscribe",
    };
    Ok(state_dir()?
        .join("logs")
        .join(format!("pb-{role_name}-{}.log", safe_file_component(key))))
}

/// Path to the default direct API proxy worker log file for a service key.
pub fn api_proxy_log_file(key: &str) -> Result<PathBuf> {
    Ok(state_dir()?
        .join("logs")
        .join(format!("api-proxy-{}.log", safe_file_component(key))))
}

/// Default config file location (`config.toml` next to the state dir).
pub fn config_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

fn safe_file_component(raw: &str) -> String {
    let sanitized: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "default".into()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pb_log_file_sanitizes_service_key() -> Result<()> {
        let path = pb_log_file(PbRole::Register, "team/codex:main")?;

        assert!(path.ends_with("logs/pb-register-team_codex_main.log"));
        Ok(())
    }

    #[test]
    fn empty_pb_log_file_key_uses_default_component() -> Result<()> {
        let path = pb_log_file(PbRole::Subscribe, "")?;

        assert!(path.ends_with("logs/pb-subscribe-default.log"));
        Ok(())
    }
}
