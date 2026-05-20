//! Persistent runtime state for Pocket-Codex.
//!
//! The CLI writes a single `state.toml` file (located via
//! [`crate::paths::state_file`]) so that subsequent invocations can
//! attach to a running `codex app-server` and pb-mapper session
//! instead of spawning duplicates.
//!
//! All fields are optional: a fresh install starts with an empty
//! [`RuntimeState`] and individual subcommands populate the parts they
//! own.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{error::Result, paths};

/// Top-level on-disk state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RuntimeState {
    /// State of the local `codex app-server` process, if one is being
    /// supervised by Pocket-Codex.
    pub codex: Option<CodexProcessInfo>,

    /// pb-mapper sessions Pocket-Codex spawned. Indexed by their
    /// service `key` so a single host can register/subscribe several
    /// services in parallel.
    #[serde(default)]
    pub pb: Vec<PbSessionInfo>,
}

/// Recorded metadata for a supervised `codex app-server` child
/// process.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexProcessInfo {
    /// Operating-system process id of the spawned child.
    pub pid: u32,

    /// The `--listen` URL the child was started with (e.g.
    /// `ws://127.0.0.1:18080` or `unix:///path/to/socket`).
    pub listen: String,

    /// Path to the captured stdout/stderr log.
    pub log_file: PathBuf,

    /// Wall-clock time when the process was spawned (RFC 3339).
    pub started_at: String,
}

/// Role a pb-mapper session is playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PbRole {
    /// Local service is registered to a relay (publisher side).
    Register,

    /// Remote service is exposed locally (subscriber side).
    Subscribe,
}

/// Recorded metadata for a single pb-mapper session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PbSessionInfo {
    /// `register` or `subscribe`.
    pub role: PbRole,

    /// Service key the session is bound to.
    pub key: String,

    /// Local TCP/UDP address the session attaches to (the codex
    /// app-server socket for `register`, or the locally exposed port
    /// for `subscribe`).
    pub local_addr: String,

    /// Address of the upstream pb-mapper relay.
    pub relay_addr: String,

    /// pid of the worker process that owns this session.
    pub pid: u32,

    /// Path to the captured stdout/stderr log.
    pub log_file: PathBuf,

    /// Wall-clock time when the session was started (RFC 3339).
    pub started_at: String,
}

impl RuntimeState {
    /// Load runtime state from the default location.
    ///
    /// Returns a default-empty [`RuntimeState`] when the file does not
    /// exist; only true I/O / parse errors propagate.
    pub fn load() -> Result<Self> {
        Self::load_from(&paths::state_file()?)
    }

    /// Load runtime state from a specific path.
    pub fn load_from(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(raw) => Ok(toml::from_str(&raw)?),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err.into()),
        }
    }

    /// Persist runtime state to the default location, creating parent
    /// directories as needed.
    pub fn save(&self) -> Result<()> {
        self.save_to(&paths::state_file()?)
    }

    /// Persist runtime state to a specific path.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = toml::to_string_pretty(self)?;
        std::fs::write(path, raw)?;
        Ok(())
    }

    /// Look up a pb-mapper session by `(role, key)`.
    pub fn find_pb(&self, role: PbRole, key: &str) -> Option<&PbSessionInfo> {
        self.pb.iter().find(|s| s.role == role && s.key == key)
    }

    /// Replace (or append) the pb-mapper session identified by
    /// `(role, key)`.
    pub fn upsert_pb(&mut self, session: PbSessionInfo) {
        if let Some(slot) = self
            .pb
            .iter_mut()
            .find(|s| s.role == session.role && s.key == session.key)
        {
            *slot = session;
        } else {
            self.pb.push(session);
        }
    }

    /// Remove the pb-mapper session identified by `(role, key)`.
    pub fn remove_pb(&mut self, role: PbRole, key: &str) -> Option<PbSessionInfo> {
        let idx = self
            .pb
            .iter()
            .position(|s| s.role == role && s.key == key)?;
        Some(self.pb.swap_remove(idx))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn roundtrip_state_via_toml() {
        let state = RuntimeState {
            codex: Some(CodexProcessInfo {
                pid: 4242,
                listen: "ws://127.0.0.1:18080".into(),
                log_file: PathBuf::from("/tmp/codex.log"),
                started_at: "2026-05-20T12:00:00Z".into(),
            }),
            pb: vec![PbSessionInfo {
                role: PbRole::Register,
                key: "codex".into(),
                local_addr: "127.0.0.1:18080".into(),
                relay_addr: "relay.example.com:7666".into(),
                pid: 4243,
                log_file: PathBuf::from("/tmp/pb-register-codex.log"),
                started_at: "2026-05-20T12:00:01Z".into(),
            }],
        };
        let raw = toml::to_string_pretty(&state).expect("serialize");
        let parsed: RuntimeState = toml::from_str(&raw).expect("deserialize");
        assert_eq!(parsed.codex.as_ref().expect("codex present").pid, 4242);
        assert_eq!(
            parsed
                .find_pb(PbRole::Register, "codex")
                .expect("session present")
                .pid,
            4243
        );
    }
}
