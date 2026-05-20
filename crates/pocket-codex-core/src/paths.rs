//! Well-known filesystem paths used by Pocket-Codex.
//!
//! These helpers centralise the lookups so the rest of the codebase
//! never spells out a path literal. We use [`directories`] to follow
//! the conventional XDG / macOS layouts.

use std::path::PathBuf;

use directories::ProjectDirs;

use crate::error::{Error, Result};

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

/// Default config file location (`config.toml` next to the state dir).
pub fn config_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}
