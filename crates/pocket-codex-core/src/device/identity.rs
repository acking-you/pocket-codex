//! Stable hosting identities, independent of hostname and publisher lifetime.

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::DeviceMetadata;
use crate::service::sanitize_component;

/// One hosting installation in one OS environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostIdentity {
    pub(super) root: PathBuf,
    id: String,
    device: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    id: String,
    device: String,
}

impl HostIdentity {
    /// Read or durably create an identity under a private state directory.
    /// Different roots and OS environments never share an identity implicitly.
    /// An unreadable identity fails instead of changing the registered keys.
    pub fn load_or_create(root: &Path, metadata: &DeviceMetadata) -> Result<Self> {
        fs::create_dir_all(root).context("creating host identity directory")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
        }
        let scope = sanitize_component(&format!(
            "{}-{}",
            metadata.os,
            metadata.environment.as_deref().unwrap_or("native")
        ));
        let lock = private_file(&root.join(format!("{scope}.lock")), false)?;
        lock.lock().context("locking host identity")?;
        let path = root.join(format!("{scope}.json"));
        let record = match File::open(&path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(4097).read_to_end(&mut bytes)?;
                if bytes.len() > 4096 {
                    bail!("host identity is too large");
                }
                let record: Record = serde_json::from_slice(&bytes)
                    .context("reading host identity; restore its saved file before hosting")?;
                let id = Uuid::parse_str(&record.id).context("invalid host identity ID")?;
                if record.version != 1
                    || id.is_nil()
                    || record.device != sanitize_component(&record.device)
                    || record.device.is_empty()
                    || !record.device.ends_with(&format!(".{}", id.simple()))
                {
                    bail!("unsupported or invalid host identity");
                }
                record
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let id = Uuid::new_v4();
                let source = [
                    metadata.environment.as_deref().unwrap_or(&metadata.os),
                    metadata.source_name.as_deref().unwrap_or(""),
                ]
                .join("-");
                let source = component(&source, 24);
                let hostname = component(&metadata.hostname, 32);
                let record = Record {
                    version: 1,
                    id: id.to_string(),
                    device: format!("{source}.{hostname}.{}", id.simple()),
                };
                persist(root, &path, &record)?;
                record
            },
            Err(error) => return Err(error).context("reading host identity"),
        };
        Ok(Self {
            root: root.to_path_buf(),
            id: record.id,
            device: record.device,
        })
    }

    /// Stable, opaque ID; survives restarts and hostname changes.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Default relay device segment; an explicit `--device` overrides it.
    pub fn device(&self) -> &str {
        &self.device
    }
}

fn component(raw: &str, limit: usize) -> String {
    sanitize_component(
        &sanitize_component(raw)
            .chars()
            .take(limit)
            .collect::<String>(),
    )
}

fn private_file(path: &Path, exclusive: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    if exclusive {
        options.create_new(true);
    } else {
        options.create(true).truncate(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .context("opening private host identity file")
}

fn persist(root: &Path, path: &Path, record: &Record) -> Result<()> {
    let pending = root.join(format!(".{}.tmp", Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = private_file(&pending, true)?;
        file.write_all(&serde_json::to_vec(record)?)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&pending, path)?;
        #[cfg(unix)]
        File::open(root)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&pending);
    }
    result.context("saving host identity")
}

#[cfg(test)]
mod tests;
