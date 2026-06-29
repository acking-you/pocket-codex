//! Host-side persistence for per-thread session config (model, reasoning
//! effort, permission mode, plan mode) — requirement **#2**.
//!
//! A single JSON file on the host, guarded by an async mutex and written
//! atomically (temp file + rename). See `DESIGN.md` for why a JSON map is used
//! rather than an embedded SQL engine (this crate is linked in-process by the
//! mobile bridge; the data is a small, low-write per-thread map). It survives
//! restarts and is the single source of truth shared across every device that
//! reaches this host.

use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{anyhow, Context, Result};
use pocket_codex_codex::rollout;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// The default config-store path: a file under `CODEX_HOME`, so per-thread
/// config lives alongside the sessions it annotates and is shared by every host
/// on this machine (app or CLI, any service name) — they share one
/// `CODEX_HOME`, so they share one config map.
pub fn default_db_path() -> Result<PathBuf> {
    let home =
        rollout::codex_home().map_err(|e| anyhow!("resolving CODEX_HOME for config store: {e}"))?;
    Ok(home.join("pocket-codex-threads.json"))
}

/// Persisted per-thread session config. Every field is optional: an unset field
/// means "no stored preference", so the client falls back to its own default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadConfig {
    /// Selected model id, when the user pinned one for this thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Reasoning-effort tag (`minimal`/`low`/`medium`/`high`), when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// Permission / approval mode tag, when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    /// Whether plan mode is on for this thread, when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_mode: Option<bool>,
}

/// A JSON-file store mapping thread id → [`ThreadConfig`].
pub struct ConfigStore {
    path: PathBuf,
    inner: Mutex<BTreeMap<String, ThreadConfig>>,
}

impl ConfigStore {
    /// Open the store at `path`, loading existing entries. A missing file
    /// starts empty; an unreadable/corrupt file also starts empty (logged)
    /// rather than wedging hosting — config is convenience state, not a
    /// hard dependency.
    pub async fn open(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("creating thread-config dir {}", parent.display()))?;
        }
        let inner = match tokio::fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                tracing::warn!(
                    error = %e,
                    path = %path.display(),
                    "thread-config store unreadable; starting empty"
                );
                BTreeMap::new()
            }),
            Err(_) => BTreeMap::new(),
        };
        Ok(Self {
            path,
            inner: Mutex::new(inner),
        })
    }

    /// The stored config for `thread_id`, or a default (all-unset) when absent.
    pub async fn get(&self, thread_id: &str) -> ThreadConfig {
        self.inner
            .lock()
            .await
            .get(thread_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Upsert `thread_id`'s config and persist the whole map atomically (temp +
    /// rename). The in-memory lock serializes writes within this process;
    /// before writing it re-reads the on-disk map so a concurrent write
    /// from another host process sharing this `CODEX_HOME` (an app host + a
    /// CLI host on one machine) is merged rather than clobbered — each
    /// write lands on top of the freshest on-disk state. (A true
    /// sub-write-window cross-process race is not fully prevented without
    /// an OS file lock; it is an accepted edge for this convenience state.
    /// No fsync is issued, so an unclean shutdown may lose the last write —
    /// but the rename keeps the file from ever being partial.)
    pub async fn put(&self, thread_id: &str, config: ThreadConfig) -> Result<()> {
        let mut guard = self.inner.lock().await;
        // Re-read the on-disk map under the lock so another process's entries
        // survive — apply our change on top of the freshest state, not a stale
        // in-memory snapshot.
        if let Ok(bytes) = tokio::fs::read(&self.path).await {
            if let Ok(on_disk) = serde_json::from_slice::<BTreeMap<String, ThreadConfig>>(&bytes) {
                *guard = on_disk;
            }
        }
        guard.insert(thread_id.to_string(), config);
        let bytes =
            serde_json::to_vec_pretty(&*guard).context("serializing thread-config store")?;
        // A per-process temp name so two host processes writing concurrently
        // can't corrupt each other's temp file before the atomic rename.
        let tmp = self
            .path
            .with_extension(format!("{}.tmp", std::process::id()));
        tokio::fs::write(&tmp, &bytes)
            .await
            .with_context(|| format!("writing thread-config temp {}", tmp.display()))?;
        tokio::fs::rename(&tmp, &self.path)
            .await
            .with_context(|| format!("replacing thread-config store {}", self.path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_then_get_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("threads.json");
        let store = ConfigStore::open(path.clone()).await.expect("open");

        assert_eq!(store.get("t1").await, ThreadConfig::default());

        let cfg = ThreadConfig {
            model: Some("gpt-5.5".to_string()),
            reasoning_effort: Some("high".to_string()),
            permission_mode: Some("auto".to_string()),
            plan_mode: Some(true),
        };
        store.put("t1", cfg.clone()).await.expect("put");
        assert_eq!(store.get("t1").await, cfg);
    }

    #[tokio::test]
    async fn persists_across_reopen() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("threads.json");
        {
            let store = ConfigStore::open(path.clone()).await.expect("open");
            store
                .put("t2", ThreadConfig {
                    model: Some("gpt-5.5".to_string()),
                    ..Default::default()
                })
                .await
                .expect("put");
        }
        let reopened = ConfigStore::open(path).await.expect("reopen");
        assert_eq!(reopened.get("t2").await.model.as_deref(), Some("gpt-5.5"));
    }

    #[tokio::test]
    async fn corrupt_file_starts_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("threads.json");
        tokio::fs::write(&path, b"{ not json").await.expect("seed");
        let store = ConfigStore::open(path)
            .await
            .expect("open tolerates corruption");
        assert_eq!(store.get("anything").await, ThreadConfig::default());
    }
}
