//! Bounded, disposable controller-side disk cache. One atomic file contains
//! both data and its synchronization manifest; eviction never leaves a cursor
//! claiming bytes that are no longer present. No SQLite/mobile runtime added.

use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{ensure, Context, Result};
use pocket_codex_core::{config::Mode, history_sync::digest_bytes};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::{config, runtime};

/// Maximum single cached allocation. Oversized pages remain usable online.
pub const MAX_ENTRY_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Header {
    owner: String,
    session: String,
    priority: u8,
    hot_until: u64,
    digest: String,
}

fn focus() -> &'static Mutex<Option<(String, String)>> {
    static FOCUS: OnceLock<Mutex<Option<(String, String)>>> = OnceLock::new();
    FOCUS.get_or_init(Default::default)
}

/// Pin the current reading session ahead of background prefetch. The cache is
/// still bounded when every retained window belongs to the current session.
pub fn set_focus(service: &str, session: Option<&str>) -> Result<()> {
    let owner = namespace(service)?;
    let mut focus = focus()
        .lock()
        .map_err(|_| anyhow::anyhow!("cache focus poisoned"))?;
    if let Some(session) = session {
        *focus = Some((owner, digest_bytes(session.as_bytes())));
    } else if focus.as_ref().is_some_and(|(current, _)| current == &owner) {
        *focus = None;
    }
    Ok(())
}

/// Whether foreground reading already owns this session's synchronization.
pub fn is_focused(service: &str, session: &str) -> bool {
    let Ok(owner) = namespace(service) else { return false };
    focus().lock().ok().is_some_and(|focus| {
        focus
            .as_ref()
            .is_some_and(|(o, s)| o == &owner && s == &digest_bytes(session.as_bytes()))
    })
}

/// Namespace includes account/backend or relay/key identity, never port numbers
/// or expiring tokens. Secrets are hashed and never persisted in cache headers.
pub fn namespace(service: &str) -> Result<String> {
    let cfg = config::load_config(&runtime::support_dir()?)?;
    let identity = match cfg.account_mode() {
        Mode::Account => format!(
            "account\0{}\0{}",
            cfg.account.backend.as_deref().unwrap_or_default(),
            cfg.account.account_id.as_deref().unwrap_or_default()
        ),
        _ => format!(
            "relay\0{}\0{}",
            cfg.relay().unwrap_or_default(),
            cfg.relay_key().unwrap_or_default()
        ),
    };
    Ok(digest_bytes(format!("{identity}\0{service}").as_bytes()))
}

/// Open the application-wide disk budget without making a network request.
pub fn application_cache() -> Result<DiskCache> {
    let dir = runtime::support_dir()?;
    let cfg = config::load_config(&dir)?;
    Ok(DiskCache::new(
        dir.join("session-cache-v1"),
        u64::from(cfg.history_cache.disk_limit_mb) * 1_000_000,
    ))
}

/// One cache directory shared by all controller hosts and sessions.
pub struct DiskCache {
    root: PathBuf,
    limit: u64,
}

impl DiskCache {
    /// Construct a cache with an exact byte budget; no filesystem side effects.
    pub fn new(root: PathBuf, limit: u64) -> Self {
        Self {
            root,
            limit,
        }
    }

    fn lock(&self) -> Result<File> {
        fs::create_dir_all(&self.root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))?;
        }
        let file = private_file(&self.root.join("cache.lock"), false)?;
        file.lock()?;
        Ok(file)
    }

    fn path(&self, owner: &str, session: &str, key: &str) -> PathBuf {
        self.root.join(format!(
            "{}.entry",
            digest_bytes(format!("{owner}\0{session}\0{key}").as_bytes())
        ))
    }

    /// Read a verified entry and update its access time. Corrupt or oversized
    /// cache entries are misses; source data is never modified.
    pub fn read(&self, owner: &str, session: &str, key: &str) -> Result<Option<Vec<u8>>> {
        if self.limit == 0 || !self.root.exists() {
            return Ok(None);
        }
        let _lock = self.lock()?;
        let path = self.path(owner, session, key);
        let read = || -> Result<Vec<u8>> {
            let file = File::open(&path)?;
            ensure!(
                file.metadata()?.len() <= MAX_ENTRY_BYTES.min(self.limit),
                "cache entry too large"
            );
            let mut reader = BufReader::new(file);
            let header = read_header(&mut reader)?;
            ensure!(
                header.owner == owner && header.session == digest_bytes(session.as_bytes()),
                "cache namespace mismatch"
            );
            let mut data = Vec::new();
            reader.read_to_end(&mut data)?;
            ensure!(digest_bytes(&data) == header.digest, "cache digest mismatch");
            reader.get_ref().set_modified(SystemTime::now())?;
            Ok(data)
        };
        match read() {
            Ok(data) => Ok(Some(data)),
            Err(_) => {
                let _ = fs::remove_file(path);
                Ok(None)
            },
        }
    }

    /// Deserialize one verified cached value. Invalid schemas are cache misses.
    pub fn read_json<T: DeserializeOwned>(
        &self,
        owner: &str,
        session: &str,
        key: &str,
    ) -> Result<Option<T>> {
        Ok(self
            .read(owner, session, key)?
            .and_then(|bytes| serde_json::from_slice(&bytes).ok()))
    }

    /// Reserve space, evict cold windows, then atomically publish data. Returns
    /// false when an individual value cannot fit; online use remains possible.
    pub fn write(
        &self,
        owner: &str,
        session: &str,
        key: &str,
        data: &[u8],
        running: bool,
    ) -> Result<bool> {
        if self.limit == 0 || data.len() as u64 > MAX_ENTRY_BYTES {
            return Ok(false);
        }
        let _lock = self.lock()?;
        let header = Header {
            owner: owner.into(),
            session: digest_bytes(session.as_bytes()),
            priority: if running { 2 } else { 1 },
            hot_until: now() + 30,
            digest: digest_bytes(data),
        };
        let mut encoded = serde_json::to_vec(&header)?;
        encoded.push(b'\n');
        let bytes = (encoded.len() + data.len()) as u64;
        if bytes > self.limit || bytes > MAX_ENTRY_BYTES {
            return Ok(false);
        }
        let path = self.path(owner, session, key);
        // Reserve for both old and staged versions before writing: temporary
        // file lengths count toward the same quota, including crash leftovers.
        self.trim_locked(bytes)?;
        let temporary = path.with_extension("tmp");
        let mut file = private_file(&temporary, true)?;
        file.write_all(&encoded)?;
        file.write_all(data)?;
        file.sync_all()?;
        fs::rename(&temporary, &path)?;
        #[cfg(unix)]
        File::open(&self.root)?.sync_all()?;
        Ok(true)
    }

    /// Store a complete value and its cursor/version in the same atomic entry.
    pub fn write_json<T: Serialize>(
        &self,
        owner: &str,
        session: &str,
        key: &str,
        value: &T,
        running: bool,
    ) -> Result<bool> {
        self.write(owner, session, key, &serde_json::to_vec(value)?, running)
    }

    /// Invalidate one source session after a provider generation change.
    pub fn invalidate_session(&self, owner: &str, session: &str) -> Result<()> {
        if !self.root.exists() {
            return Ok(());
        }
        let _lock = self.lock()?;
        let session = digest_bytes(session.as_bytes());
        for entry in fs::read_dir(&self.root)?.filter_map(Result::ok) {
            if entry.path().extension().is_none_or(|s| s != "entry") {
                continue;
            }
            let header = File::open(entry.path())
                .ok()
                .and_then(|file| read_header(&mut BufReader::new(file)).ok());
            if header.is_some_and(|h| h.owner == owner && h.session == session) {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }

    /// Apply a changed quota immediately and return cache file bytes.
    /// Filesystem allocation units and directory metadata are outside this
    /// byte count.
    pub fn usage(&self) -> Result<u64> {
        if !self.root.exists() {
            return Ok(0);
        }
        let _lock = self.lock()?;
        self.trim_locked(0)?;
        Ok(fs::read_dir(&self.root)?
            .filter_map(Result::ok)
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum())
    }

    fn trim_locked(&self, reserve: u64) -> Result<()> {
        let focused = focus().lock().ok().and_then(|f| f.clone());
        let mut entries = Vec::new();
        let mut used = 0;
        for entry in fs::read_dir(&self.root)?.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|s| s == "tmp") {
                fs::remove_file(path)?;
                continue;
            }
            if path.extension().is_none_or(|s| s != "entry") {
                continue;
            }
            let metadata = entry.metadata()?;
            let header = File::open(&path)
                .ok()
                .and_then(|file| read_header(&mut BufReader::new(file)).ok());
            let priority = header.as_ref().map_or(0, |h| {
                if focused
                    .as_ref()
                    .is_some_and(|(o, s)| o == &h.owner && s == &h.session)
                {
                    3
                } else if h.hot_until >= now() {
                    h.priority
                } else {
                    1
                }
            });
            used += metadata.len();
            entries.push((priority, metadata.modified().ok(), metadata.len(), path));
        }
        entries.sort_by_key(|a| (a.0, a.1));
        for (_, _, size, path) in entries {
            if used.saturating_add(reserve) <= self.limit {
                break;
            }
            fs::remove_file(path)?;
            used = used.saturating_sub(size);
        }
        Ok(())
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |v| v.as_secs())
}

fn read_header(reader: &mut BufReader<File>) -> Result<Header> {
    let mut line = String::new();
    reader.by_ref().take(4096).read_line(&mut line)?;
    ensure!(line.ends_with('\n'), "invalid cache header");
    serde_json::from_str(&line).context("cache header")
}

fn private_file(path: &Path, truncate: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options
        .create(true)
        .read(true)
        .write(true)
        .truncate(truncate);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_eviction_namespaces_and_corruption_are_cache_misses() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let cache = DiskCache::new(dir.path().into(), 4000);
        cache.write("host-a", "thread", "older", &vec![1; 1200], false)?;
        cache.write("host-b", "thread", "tail", &vec![2; 1200], true)?;
        let restarted = DiskCache::new(dir.path().into(), 4000);
        assert_eq!(restarted.read("host-b", "thread", "tail")?, Some(vec![2; 1200]));
        restarted.write("host-a", "thread", "latest", &vec![3; 1200], true)?;
        assert!(restarted.read("host-a", "thread", "older")?.is_none());
        assert!(restarted.read("host-b", "thread", "tail")?.is_some());
        assert!(restarted.read("host-a", "thread", "tail")?.is_none());
        assert!(restarted.usage()? <= 4000);
        fs::write(restarted.path("host-a", "thread", "latest"), "torn write")?;
        assert!(restarted.read("host-a", "thread", "latest")?.is_none());
        Ok(())
    }

    #[test]
    fn staging_bytes_orphans_and_smaller_quota_are_accounted_for() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let cache = DiskCache::new(dir.path().into(), 3000);
        cache.write("host", "thread", "tail", &vec![1; 1000], true)?;
        fs::write(dir.path().join("crashed.tmp"), vec![0; 500])?;
        cache.write("host", "thread", "tail", &vec![2; 2000], true)?;
        assert!(!dir.path().join("crashed.tmp").exists());
        assert!(cache.usage()? <= 3000);
        assert!(!cache.write("host", "thread", "huge", &vec![0; 4000], true)?);
        assert_eq!(DiskCache::new(dir.path().into(), 0).usage()?, 0);
        Ok(())
    }
}
