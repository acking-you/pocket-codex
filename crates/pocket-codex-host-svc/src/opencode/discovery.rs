//! Read-only attachment to the user's official OpenCode background service.
//!
//! OpenCode records its background server in
//! `$XDG_STATE_HOME/opencode/service.json` (default
//! `~/.local/state/opencode/service.json`) with the URL, pid, version and a
//! Basic-auth password. The registration and its password stay in this
//! process; nothing here starts, stops or reconfigures the service.

use std::{
    fs::OpenOptions,
    io::Read,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use super::{validate_origin, BasicCredentials, Client, Error, Result, ServerInfo};

const MAX_REGISTRATION_BYTES: u64 = 16 * 1024;

#[derive(Deserialize)]
struct Registration {
    url: String,
    pid: u64,
    #[serde(default)]
    password: Option<String>,
}

/// An attached, contract-checked OpenCode service.
#[derive(Clone, Debug)]
pub struct Attached {
    /// Authenticated client for the real server (host-local only).
    pub client: Client,
    /// The same origin and credentials, for a [`super::gateway::Gateway`].
    pub upstream: super::gateway::Upstream,
    /// Server identity.
    pub info: ServerInfo,
    /// Whether `info.version` is the release this build was verified against.
    pub verified: bool,
}

/// The default registration file path.
pub fn registration_path() -> Result<PathBuf> {
    let state = match std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
        Some(value) => PathBuf::from(value),
        None => {
            let home = PathBuf::from(std::env::var_os("HOME").ok_or(Error::InvalidInput)?);
            if !home.is_absolute() {
                return Err(Error::InvalidInput);
            }
            home.join(".local/state")
        },
    };
    if !state.is_absolute() {
        return Err(Error::InvalidInput);
    }
    Ok(state.join("opencode/service.json"))
}

/// Attach to the service at the default registration path.
pub async fn discover() -> Result<Attached> {
    discover_from_file(&registration_path()?).await
}

/// Attach to the service advertised by `path`.
///
/// Only loopback origins are accepted. On Unix the file must be a regular,
/// owner-only file of the effective user (symlinks are rejected). The server's
/// `/api/info` pid must match the registration, and its API must satisfy the
/// contract.
pub async fn discover_from_file(path: &Path) -> Result<Attached> {
    let path = path.to_owned();
    let registration = tokio::task::spawn_blocking(move || read_registration(&path))
        .await
        .map_err(|_| Error::InvalidInput)??;
    validate_origin(&registration.url, true)?;
    let credentials = registration
        .password
        .map(|password| BasicCredentials::new("opencode", password));
    let upstream = super::gateway::Upstream::new(&registration.url, credentials.clone())?;
    let client = Client::new(&registration.url, credentials)?;
    let (info, verified) = match client.connect().await {
        Ok(ok) => ok,
        Err(Error::Transport) => return Err(Error::NotRunning),
        Err(error) => return Err(error),
    };
    if registration.pid != 0 && info.pid != 0 && info.pid != registration.pid {
        return Err(Error::NotRunning);
    }
    Ok(Attached {
        client,
        upstream,
        info,
        verified,
    })
}

fn read_registration(path: &Path) -> Result<Registration> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::NotRunning)
        },
        Err(_) => return Err(Error::InvalidInput),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(Error::InvalidInput);
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Reject a symlink or blocking special file swapped in after the check.
        options.custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|_| Error::InvalidInput)?;
    let metadata = file.metadata().map_err(|_| Error::InvalidInput)?;
    if !metadata.is_file() {
        return Err(Error::InvalidInput);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != nix::unistd::geteuid().as_raw() || metadata.mode() & 0o077 != 0 {
            return Err(Error::InvalidInput);
        }
    }
    if metadata.len() > MAX_REGISTRATION_BYTES {
        return Err(Error::Limit);
    }
    let mut bytes = Vec::new();
    file.take(MAX_REGISTRATION_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::InvalidInput)?;
    if bytes.len() as u64 > MAX_REGISTRATION_BYTES {
        return Err(Error::Limit);
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::InvalidInput)
}
