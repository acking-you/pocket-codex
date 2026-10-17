//! Installation identity shared by CLI publishers and their detached workers.

use anyhow::Result;
use pocket_codex_core::{device, paths};

pub(super) fn directory() -> Result<std::path::PathBuf> {
    Ok(paths::state_dir()?.join("identity"))
}

pub(super) fn device(explicit: Option<String>) -> Result<String> {
    let identity = device::initialize(&directory()?)?;
    Ok(explicit.unwrap_or_else(|| identity.device().to_string()))
}
