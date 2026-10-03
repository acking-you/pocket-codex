//! Audit log of management actions (TRD §4.3.12): JSONL in `acp/audit.log`,
//! `0600`, rotated past 1 MiB keeping `.1`–`.3`. Never records environment
//! values, tokens or paths other than agent ids.

use std::{io::Write as _, path::PathBuf};

use serde::Serialize;

use super::store::{now_rfc3339, Layout};

/// Rotation threshold.
pub const ROTATE_BYTES: u64 = 1024 * 1024;
/// Rotated files kept.
const KEEP: u32 = 3;

/// One audit record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    /// RFC 3339 time.
    pub ts: String,
    /// `install` | `uninstall` | `host` | `settings` | `custom_agent`
    pub action: String,
    /// Agent concerned (empty for hub-wide settings).
    pub agent_id: String,
    /// Version, when relevant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Requested by a remote controller.
    pub remote: bool,
    /// `ok` | `error`
    pub result: String,
    /// `acp.<code>` on error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Which setting changed (names only, never values).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
}

impl AuditEntry {
    /// An entry stamped now.
    pub fn new(action: &str, agent_id: &str, remote: bool) -> Self {
        Self {
            ts: now_rfc3339(),
            action: action.into(),
            agent_id: agent_id.into(),
            version: None,
            remote,
            result: "ok".into(),
            code: None,
            item: None,
        }
    }

    /// Mark the outcome of `result`.
    pub fn outcome<T>(mut self, result: &Result<T, super::super::error::AcpError>) -> Self {
        if let Err(e) = result {
            self.result = "error".into();
            self.code = Some(e.code().into());
        }
        self
    }
}

fn rotated(path: &std::path::Path, n: u32) -> PathBuf {
    PathBuf::from(format!("{}.{n}", path.display()))
}

/// Append `entry`; failures are logged, never propagated.
pub fn record(layout: &Layout, entry: &AuditEntry) {
    let path = layout.audit_log();
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > ROTATE_BYTES) {
        let _ = std::fs::remove_file(rotated(&path, KEEP));
        for n in (1..KEEP).rev() {
            let _ = std::fs::rename(rotated(&path, n), rotated(&path, n + 1));
        }
        let _ = std::fs::rename(&path, rotated(&path, 1));
    }
    let Ok(mut line) = serde_json::to_vec(entry) else { return };
    line.push(b'\n');
    let _ = std::fs::create_dir_all(&layout.root);
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(&path) {
        Ok(mut file) => {
            let _ = file.write_all(&line);
        },
        Err(e) => tracing::warn!("cannot write the ACP audit log: {e}"),
    }
}
