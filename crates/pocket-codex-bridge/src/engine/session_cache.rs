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

#[path = "session_cache_accounting.rs"]
mod accounting;
use accounting::Guard;

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
    Ok(DiskCache::for_app(runtime::support_dir()?))
}

/// Change capacity under the same lock used by cache writers, then evict before
/// returning. Handles retained by in-flight requests read this updated setting.
pub fn set_limit(support_dir: &Path, limit_mb: u32) -> Result<()> {
    ensure!(limit_mb <= 64_000, "cache limit must be between 0 and 64000 MB");
    let cache = DiskCache::for_app(support_dir.into());
    let mut lock = cache.lock()?;
    let mut cfg = config::load_config(support_dir)?;
    cfg.history_cache.disk_limit_mb = limit_mb;
    config::save_config(support_dir, &cfg)?;
    cache
        .trim_locked(&mut lock, u64::from(limit_mb) * 1_000_000, 0)
        .map(|_| ())
}

enum CacheLimit {
    #[cfg(test)]
    Fixed(u64),
    AppConfig(PathBuf),
}

/// One cache directory shared by all controller hosts and sessions.
pub struct DiskCache {
    root: PathBuf,
    limit: CacheLimit,
}

impl DiskCache {
    /// Construct a cache with an exact byte budget; no filesystem side effects.
    #[cfg(test)]
    pub(super) fn new(root: PathBuf, limit: u64) -> Self {
        Self {
            root,
            limit: CacheLimit::Fixed(limit),
        }
    }

    fn for_app(support_dir: PathBuf) -> Self {
        Self {
            root: support_dir.join("session-cache-v1"),
            limit: CacheLimit::AppConfig(support_dir),
        }
    }

    fn limit_locked(&self) -> Result<u64> {
        match &self.limit {
            #[cfg(test)]
            CacheLimit::Fixed(limit) => Ok(*limit),
            CacheLimit::AppConfig(dir) => {
                Ok(u64::from(config::load_config(dir)?.history_cache.disk_limit_mb) * 1_000_000)
            },
        }
    }

    fn lock(&self) -> Result<Guard> {
        fs::create_dir_all(&self.root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))?;
        }
        let file = private_file(&self.root.join("cache.lock"), false)?;
        Guard::new(file, &self.root)
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
        if !self.root.exists() {
            return Ok(None);
        }
        let mut lock = self.lock()?;
        let limit = self.limit_locked()?;
        if limit == 0 {
            return Ok(None);
        }
        let path = self.path(owner, session, key);
        let read = || -> Result<Vec<u8>> {
            let file = File::open(&path)?;
            ensure!(file.metadata()?.len() <= MAX_ENTRY_BYTES.min(limit), "cache entry too large");
            let mut reader = BufReader::new(file);
            let header = read_header(&mut reader)?;
            ensure!(
                header.owner == owner && header.session == digest_bytes(session.as_bytes()),
                "cache namespace mismatch"
            );
            let mut data = Vec::new();
            reader.read_to_end(&mut data)?;
            ensure!(digest_bytes(&data) == header.digest, "cache digest mismatch");
            Ok(data)
        };
        match read() {
            Ok(data) => {
                // Windows needs write-attributes access for set_modified.
                // LRU maintenance failure does not invalidate verified content.
                if let Err(error) = OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .and_then(|file| file.set_modified(SystemTime::now()))
                {
                    tracing::debug!(%error, "cache access timestamp unavailable");
                }
                Ok(Some(data))
            },
            Err(_) => {
                lock.invalidate()?;
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
        if data.len() as u64 > MAX_ENTRY_BYTES {
            return Ok(false);
        }
        let mut lock = self.lock()?;
        let limit = self.limit_locked()?;
        if limit == 0 {
            return Ok(false);
        }
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
        if bytes > limit || bytes > MAX_ENTRY_BYTES {
            return Ok(false);
        }
        let path = self.path(owner, session, key);
        // Reserve for both old and staged versions before writing: temporary
        // file lengths count toward the same quota, including crash leftovers.
        let used = self.trim_locked(&mut lock, limit, bytes)?;
        let old_bytes = fs::metadata(&path).map_or(0, |m| m.len());
        lock.invalidate()?;
        let temporary = path.with_extension("tmp");
        let mut file = private_file(&temporary, true)?;
        file.write_all(&encoded)?;
        file.write_all(data)?;
        file.sync_all()?;
        fs::rename(&temporary, &path)?;
        #[cfg(unix)]
        File::open(&self.root)?.sync_all()?;
        lock.used = Some(used.saturating_sub(old_bytes) + bytes);
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

    /// Remove one display checkpoint after publishing a newer snapshot.
    pub(super) fn remove(&self, owner: &str, session: &str, key: &str) -> Result<()> {
        let mut lock = self.lock()?;
        lock.invalidate()?;
        match fs::remove_file(self.path(owner, session, key)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// Whether a display entry exists, without parsing a potentially large
    /// view. Concurrent quota eviction can still turn a subsequent read
    /// into a miss.
    pub(super) fn contains(&self, owner: &str, session: &str, key: &str) -> bool {
        self.path(owner, session, key).is_file()
    }

    /// Invalidate one source session after a provider generation change.
    pub fn invalidate_session(&self, owner: &str, session: &str) -> Result<()> {
        if !self.root.exists() {
            return Ok(());
        }
        let mut lock = self.lock()?;
        let session = digest_bytes(session.as_bytes());
        lock.invalidate()?;
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
        let mut lock = self.lock()?;
        // Explicit diagnostics reconcile external edits too, including in-place
        // corruption that did not change the directory's modification time.
        lock.used = None;
        self.trim_locked(&mut lock, self.limit_locked()?, 0)
    }

    fn trim_locked(&self, lock: &mut Guard, limit: u64, reserve: u64) -> Result<u64> {
        if let Some(used) = lock.used {
            if used.saturating_add(reserve) <= limit {
                return Ok(used);
            }
        }
        let mut entries = Vec::new();
        let mut used = 0;
        for entry in fs::read_dir(&self.root)?.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|s| s == "tmp") {
                lock.invalidate()?;
                fs::remove_file(path)?;
                continue;
            }
            if path.extension().is_some_and(|s| s == "entry") {
                let metadata = entry.metadata()?;
                used += metadata.len();
                entries.push((path, metadata));
            }
        }
        if used.saturating_add(reserve) > limit {
            let focused = focus().lock().ok().and_then(|f| f.clone());
            let mut eviction: Vec<_> = entries
                .into_iter()
                .map(|(path, metadata)| {
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
                    (priority, metadata.modified().ok(), metadata.len(), path)
                })
                .collect();
            eviction.sort_by_key(|a| (a.0, a.1));
            lock.invalidate()?;
            for (_, _, size, path) in eviction {
                if used.saturating_add(reserve) <= limit {
                    break;
                }
                fs::remove_file(path)?;
                used = used.saturating_sub(size);
            }
        }
        lock.used = Some(used);
        Ok(used)
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
    fn stale_accounting_is_invalidated_by_another_writer_and_crash_leftovers() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let cache = DiskCache::new(dir.path().into(), 5000);
        cache.write("h", "t", "one", &vec![1; 1200], false)?;
        // Simulate a separate process: change the lock revision without updating
        // this process's counter, then publish a file while holding the OS lock.
        let mut lock = private_file(&dir.path().join("cache.lock"), false)?;
        lock.lock()?;
        use std::io::{Seek, SeekFrom};
        lock.seek(SeekFrom::Start(0))?;
        lock.write_all(&99_u64.to_le_bytes())?;
        // Growing an existing file leaves directory mtime unchanged, so only
        // the revision can invalidate this process's old byte count.
        OpenOptions::new()
            .append(true)
            .open(cache.path("h", "t", "one"))?
            .write_all(&vec![1; 2000])?;
        drop(lock);
        cache.write("h", "t", "two", &vec![2; 2300], true)?;
        let actual: u64 = fs::read_dir(dir.path())?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "entry"))
            .map(|entry| entry.metadata().expect("entry metadata").len())
            .sum();
        assert!(actual <= 5000, "revision must prevent stale under-accounting");
        fs::write(dir.path().join("orphan.tmp"), vec![1; 300])?;
        cache.write("h", "t", "two", &vec![2; 2300], true)?;
        assert!(!dir.path().join("orphan.tmp").exists());
        assert!(cache.usage()? <= 5000);
        assert_eq!(cache.read("h", "t", "two")?, Some(vec![2; 2300]));
        Ok(())
    }

    #[test]
    #[ignore = "manual filesystem benchmark"]
    fn benchmark_cache_rewrites() -> Result<()> {
        for count in [100, 1000] {
            let dir = tempfile::tempdir()?;
            let cache = DiskCache::new(dir.path().into(), 64 * 1024 * 1024);
            let data = vec![b'x'; 1024];
            for i in 0..count {
                cache.write("host", "thread", &i.to_string(), &data, false)?;
            }
            let started = std::time::Instant::now();
            for _ in 0..50 {
                cache.write("host", "thread", "0", &data, true)?;
            }
            eprintln!(
                "cache_rewrite entries={count} mean_us={}",
                started.elapsed().as_micros() / 50
            );
        }
        Ok(())
    }

    #[test]
    fn hits_refresh_timestamps_without_rewriting_the_entry() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let cache = DiskCache::new(dir.path().into(), 4000);
        cache.write("host", "thread", "tail", b"cached text", false)?;
        let path = cache.path("host", "thread", "tail");
        let before = fs::read(&path)?;
        let old = UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        OpenOptions::new()
            .write(true)
            .open(&path)?
            .set_modified(old)?;
        assert_eq!(
            cache.read("host", "thread", "tail")?.as_deref(),
            Some(b"cached text".as_slice())
        );
        assert!(fs::metadata(&path)?.modified()? > old);
        assert_eq!(fs::read(&path)?, before);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn failed_timestamp_updates_keep_verified_content() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let cache = DiskCache::new(dir.path().into(), 4000);
        cache.write("host", "thread", "tail", b"readable", false)?;
        let path = cache.path("host", "thread", "tail");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o400))?;
        assert_eq!(cache.read("host", "thread", "tail")?.as_deref(), Some(b"readable".as_slice()));
        assert!(path.exists());
        Ok(())
    }

    #[test]
    fn retained_handles_observe_disabled_and_reduced_app_quotas() -> Result<()> {
        let support = tempfile::tempdir()?;
        set_limit(support.path(), 4)?;
        let in_flight = DiskCache::for_app(support.path().into());
        in_flight.write("host", "thread", "old", &vec![0; 2_000_000], false)?;
        set_limit(support.path(), 0)?;
        assert!(!in_flight.write("host", "thread", "late", b"late reply", true)?);
        assert_eq!(in_flight.usage()?, 0);
        set_limit(support.path(), 1)?;
        assert!(!in_flight.write("host", "thread", "large", &vec![0; 2_000_000], true)?);
        assert!(in_flight.write("host", "thread", "one", &vec![1; 600_000], false)?);
        assert!(in_flight.write("host", "thread", "two", &vec![2; 600_000], true)?);
        assert!(in_flight.usage()? <= 1_000_000);
        assert!(in_flight.read("host", "thread", "one")?.is_none());
        Ok(())
    }

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
