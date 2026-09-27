//! Read-only attachment to the official local OpenCode service registration.

use std::{
    fs::OpenOptions,
    io::Read,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use super::{v2::V2Client, BasicCredentials, Error, Result};

const MAX_REGISTRATION_BYTES: u64 = 16 * 1024;

#[derive(Deserialize)]
struct Registration {
    version: String,
    url: String,
    pid: u32,
    password: Option<String>,
}

/// Discover the user's existing official OpenCode background service.
///
/// Reads `$XDG_STATE_HOME/opencode/service.json`, or
/// `$HOME/.local/state/opencode/service.json` when XDG state is unset. Neither
/// missing nor incompatible services trigger process startup or configuration
/// changes. State directory overrides must be absolute, nonempty paths.
pub async fn discover(directory: &str) -> Result<V2Client> {
    let state = match std::env::var_os("XDG_STATE_HOME") {
        Some(value) => PathBuf::from(value),
        None => {
            let home = std::env::var_os("HOME").ok_or(Error::InvalidInput)?;
            let home = PathBuf::from(home);
            if !home.is_absolute() {
                return Err(Error::InvalidInput);
            }
            home.join(".local/state")
        },
    };
    if !state.is_absolute() {
        return Err(Error::InvalidInput);
    }
    discover_from_file(&state.join("opencode/service.json"), directory).await
}

/// Attach to an existing service advertised by a local registration file.
///
/// The registration and its credentials remain private to this Rust client.
/// This function never starts, stops or reconfigures the service.
/// Only numeric loopback origins and matching 2.0.18 process identities are
/// accepted. Unix registrations must be owned by the effective user, readable
/// by that owner and inaccessible to group/other users; symlinks are rejected.
pub async fn discover_from_file(path: &Path, directory: &str) -> Result<V2Client> {
    let path = path.to_owned();
    let registration = tokio::task::spawn_blocking(move || read_registration(&path))
        .await
        .map_err(|_| Error::InvalidInput)??;
    if registration.version != "2.0.18" {
        return Err(Error::Protocol);
    }
    if registration.pid == 0 {
        return Err(Error::InvalidInput);
    }
    validate_origin(&registration.url)?;
    let credentials = registration
        .password
        .map(|password| BasicCredentials::new("opencode", password));
    let client = V2Client::new(&registration.url, directory, credentials)?;
    let info = client.connect().await?;
    if info.version != registration.version || info.pid != u64::from(registration.pid) {
        return Err(Error::Protocol);
    }
    Ok(client)
}

fn validate_origin(origin: &str) -> Result<()> {
    let origin = url::Url::parse(origin).map_err(|_| Error::InvalidInput)?;
    let loopback = match origin.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    if !loopback
        || !matches!(origin.scheme(), "http" | "https")
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.path() != "/"
        || origin.query().is_some()
        || origin.fragment().is_some()
    {
        return Err(Error::InvalidInput);
    }
    Ok(())
}

fn read_registration(path: &Path) -> Result<Registration> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| Error::InvalidInput)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(Error::InvalidInput);
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Reject replacements with symlinks or blocking special files at open time.
        options.custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path).map_err(|_| Error::InvalidInput)?;
    let metadata = file.metadata().map_err(|_| Error::InvalidInput)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(Error::InvalidInput);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != nix::unistd::geteuid().as_raw()
            || metadata.mode() & 0o077 != 0
            || metadata.mode() & 0o400 == 0
        {
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
