//! Private, bounded runtime snapshots for independently managed network
//! workers.

use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context, Result};
use pocket_codex_core::{process::pb_worker_start_time, state::PbSessionInfo};
use pocket_codex_pb::{TunnelDiagnostics, TunnelStatus};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct WorkerHealth {
    pub pid: u32,
    pub process_started_at: u64,
    pub sdk_version: String,
    pub status: String,
    pub diagnostics: serde_json::Value,
}

pub(crate) struct Reporter {
    path: PathBuf,
    started: u64,
}

fn health_path(session: &PbSessionInfo) -> PathBuf {
    session
        .log_file
        .with_extension(format!("{}.health.json", session.pid))
}

impl Reporter {
    pub fn new(session: &PbSessionInfo) -> Result<Self> {
        Ok(Self {
            path: health_path(session),
            started: pb_worker_start_time(session).context("identifying this network worker")?,
        })
    }

    pub fn write(&self, status: TunnelStatus, diagnostics: TunnelDiagnostics) -> Result<()> {
        let health = WorkerHealth {
            pid: std::process::id(),
            process_started_at: self.started,
            sdk_version: diagnostics.sdk_version.to_string(),
            status: match status {
                TunnelStatus::Starting => "starting",
                TunnelStatus::Connected => "connected",
                TunnelStatus::Retrying => "retrying",
                TunnelStatus::Stopped => "stopped",
                TunnelStatus::Failed(_) => "failed",
            }
            .to_string(),
            diagnostics: serde_json::to_value(diagnostics)?,
        };
        let bytes = serde_json::to_vec(&health)?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = self.path.with_extension("tmp");
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        drop(file);
        fs::rename(&temporary, &self.path)?;
        Ok(())
    }
}

impl Drop for Reporter {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(self.path.with_extension("tmp"));
    }
}

pub(crate) fn read(session: &PbSessionInfo) -> Option<WorkerHealth> {
    let mut file = fs::File::open(health_path(session)).ok()?;
    let metadata = file.metadata().ok()?;
    if metadata.len() > 65_536
        || metadata.modified().ok()?.elapsed().ok()? > Duration::from_secs(30)
    {
        return None;
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(65_537)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 65_536 {
        return None;
    }
    let health: WorkerHealth = serde_json::from_slice(&bytes).ok()?;
    (health.pid == session.pid && pb_worker_start_time(session) == Some(health.process_started_at))
        .then_some(health)
}
