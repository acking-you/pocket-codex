//! The host's record of sessions it admitted, per hosted agent *source*.
//!
//! An entry is written only after the host itself admitted a session: a
//! `session/new` with a cwd the host validated, or a confirmed
//! `session/load` / `session/resume` whose cwd came from an earlier entry
//! (which always wins) or the agent's own listing. Entries therefore carry
//! host authority: the meta service may resolve conversation file links
//! beneath their cwd. Nothing a controller sends, and nothing in agent
//! content (tool locations, `_meta`, transcript paths), ever creates or
//! rebinds an entry.
//!
//! # Source identity
//!
//! Authority belongs to the agent that was actually run, not to a label.
//! [`SourceId`] is a keyed digest of the resolved executable path and the
//! complete argument vector, so a host name or profile reused for another
//! executable or another configuration starts with no sessions and no file
//! authority. The digest key is a random per-state-directory secret (file
//! mode 0600), so the stored identity neither contains nor allows offline
//! guessing of arguments that may hold secrets.
//!
//! # Storage
//!
//! `root/<hex(name)>/<source>.json`, format version 2. The host-name
//! directory is hex encoded, so no name (including `.` or `..`) can escape
//! the state directory and distinct names never share a directory. Version
//! 1 files (`sessions.json`, bound only to a profile label) are never read.
//!
//! Every mutation and its write happen under one lock, and each write goes
//! to a temporary file of its own that is renamed over the record, so
//! concurrent admissions and title updates can neither lose each other nor
//! leave a partial file. A record that cannot be parsed is moved aside
//! (never silently treated as empty) and the store starts empty.
//!
//! # Pending writes
//!
//! The host never writes on its event path. It records what changed in a
//! [`Pending`] set — one coalesced [`Change`] per session, so a burst of
//! title updates is one write of the newest title — and a worker applies
//! the set as one batch ([`SessionStore::apply`]). An admission and its
//! title merge into one change, so their order cannot lose the title.

use std::{
    collections::HashMap,
    io::Write as _,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, MutexGuard,
    },
};

use serde::{Deserialize, Serialize};

/// Most retained entries per source.
pub const MAX_ENTRIES: usize = 500;
const FILE_VERSION: u32 = 2;
const KEY_FILE: &str = "source-key";
const SOURCE_DOMAIN: &str = "pocket-codex/acp-source/v1";

/// One admitted session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// The agent's session id.
    pub session_id: String,
    /// Absolute, canonical working directory the host admitted the session
    /// with. Never changed once recorded.
    pub cwd: String,
    /// Title, when the agent reported one.
    pub title: Option<String>,
    /// The agent's last-activity timestamp (verbatim ISO 8601), when it
    /// reported one. Never fabricated.
    pub updated_at: Option<String>,
    /// Host clock, Unix seconds, when the host last admitted the session.
    pub admitted_at: i64,
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    source: String,
    sessions: Vec<Entry>,
}

/// The identity of an agent invocation (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceId(String);

impl SourceId {
    /// Derive the identity of running `program` with `args` under the state
    /// directory `root` (creating its digest key on first use).
    pub fn derive(root: &Path, program: &Path, args: &[String]) -> std::io::Result<Self> {
        let key = source_key(root)?;
        Ok(Self::with_key(&key, program, args))
    }

    /// An identity under an explicit digest key (tests and in-memory stores).
    pub fn with_key(key: &str, program: &Path, args: &[String]) -> Self {
        let mut material = Vec::new();
        let mut field = |bytes: &[u8]| {
            material.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            material.extend_from_slice(bytes);
        };
        field(SOURCE_DOMAIN.as_bytes());
        field(key.as_bytes());
        field(program.as_os_str().as_encoded_bytes());
        field(&(args.len() as u64).to_le_bytes());
        for arg in args {
            field(arg.as_bytes());
        }
        let digest = pocket_codex_core::history_sync::digest_bytes(&material);
        Self(digest.chars().take(32).collect())
    }

    /// The identity as stored (lowercase hex, filename safe).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn source_key(root: &Path) -> std::io::Result<String> {
    let path = root.join(KEY_FILE);
    if let Ok(key) = std::fs::read_to_string(&path) {
        if key.trim().len() >= 32 {
            return Ok(key.trim().to_string());
        }
    }
    std::fs::create_dir_all(root)?;
    let key = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    match create_private(&path, key.as_bytes(), true) {
        Ok(()) => Ok(key),
        // Another host created it first: use theirs.
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing = std::fs::read_to_string(&path)?;
            let existing = existing.trim();
            if existing.len() < 32 {
                return Err(std::io::Error::other("the ACP source key is unreadable"));
            }
            Ok(existing.to_string())
        },
        Err(error) => Err(error),
    }
}

/// The persisted session record of one hosted agent source.
pub struct SessionStore {
    path: Option<PathBuf>,
    source: SourceId,
    /// Entries, and the lock that orders every mutation with its write.
    entries: Mutex<Vec<Entry>>,
    writes: AtomicU64,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// A filesystem-safe directory name for `name`.
pub fn encoded_component(name: &str) -> String {
    name.bytes().map(|byte| format!("{byte:02x}")).collect()
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

impl SessionStore {
    /// An in-memory store (tests, or when no state directory is available).
    pub fn memory(source: SourceId) -> Self {
        Self {
            path: None,
            source,
            entries: Mutex::new(Vec::new()),
            writes: AtomicU64::new(0),
        }
    }

    /// Open (or start) the record of host `name` and agent `source` under
    /// `root`.
    pub fn open(root: &Path, name: &str, source: SourceId) -> Self {
        let path = root
            .join(encoded_component(name))
            .join(format!("{}.json", source.as_str()));
        let entries = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<File>(&bytes) {
                Ok(file) if file.version == FILE_VERSION && file.source == source.as_str() => file
                    .sessions
                    .into_iter()
                    .filter(|entry| Path::new(&entry.cwd).is_absolute())
                    .take(MAX_ENTRIES)
                    .collect(),
                _ => {
                    quarantine(&path);
                    Vec::new()
                },
            },
            Err(_) => Vec::new(),
        };
        Self {
            path: Some(path),
            source,
            entries: Mutex::new(entries),
            writes: AtomicU64::new(0),
        }
    }

    /// The source this record belongs to.
    pub fn source(&self) -> &SourceId {
        &self.source
    }

    /// The entry for `session`.
    pub fn lookup(&self, session: &str) -> Option<Entry> {
        lock(&self.entries)
            .iter()
            .find(|entry| entry.session_id == session)
            .cloned()
    }

    /// All entries, newest admission first.
    pub fn entries(&self) -> Vec<Entry> {
        let mut entries = lock(&self.entries).clone();
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.admitted_at));
        entries
    }

    /// Record an admission. An existing entry keeps its cwd: an admitted
    /// session's working directory is immutable.
    pub fn record(
        &self,
        session: &str,
        cwd: &str,
        title: Option<String>,
        updated_at: Option<String>,
    ) {
        let mut entries = lock(&self.entries);
        let admitted_at = now();
        if let Some(entry) = entries.iter_mut().find(|entry| entry.session_id == session) {
            entry.admitted_at = admitted_at;
            if title.is_some() {
                entry.title = title;
            }
            if updated_at.is_some() {
                entry.updated_at = updated_at;
            }
        } else {
            entries.push(Entry {
                session_id: session.to_string(),
                cwd: cwd.to_string(),
                title,
                updated_at,
                admitted_at,
            });
            if entries.len() > MAX_ENTRIES {
                entries.sort_by_key(|entry| std::cmp::Reverse(entry.admitted_at));
                entries.truncate(MAX_ENTRIES);
            }
        }
        self.persist(&entries);
    }

    /// Update the title of a recorded session.
    pub fn set_title(&self, session: &str, title: Option<String>) {
        let mut entries = lock(&self.entries);
        let Some(entry) = entries.iter_mut().find(|entry| entry.session_id == session) else {
            return;
        };
        if entry.title == title {
            return;
        }
        entry.title = title;
        self.persist(&entries);
    }

    /// Apply a batch of coalesced changes with one write.
    pub fn apply(&self, changes: &[Change]) {
        if changes.is_empty() {
            return;
        }
        let mut entries = lock(&self.entries);
        let admitted_at = now();
        let mut changed = false;
        for change in changes {
            let existing = entries
                .iter()
                .position(|entry| entry.session_id == change.session);
            match (existing, &change.cwd) {
                (Some(index), cwd) => {
                    let entry = &mut entries[index];
                    if cwd.is_some() {
                        entry.admitted_at = admitted_at;
                        changed = true;
                    }
                    if let Some(title) = &change.title {
                        changed |= entry.title != *title;
                        entry.title.clone_from(title);
                    }
                },
                (None, Some(cwd)) => {
                    entries.push(Entry {
                        session_id: change.session.clone(),
                        cwd: cwd.clone(),
                        title: change.title.clone().flatten(),
                        updated_at: None,
                        admitted_at,
                    });
                    changed = true;
                },
                // A title for a session that was never admitted here.
                (None, None) => {},
            }
        }
        if entries.len() > MAX_ENTRIES {
            entries.sort_by_key(|entry| std::cmp::Reverse(entry.admitted_at));
            entries.truncate(MAX_ENTRIES);
        }
        if changed {
            self.persist(&entries);
        }
    }

    /// Write `sessions`. Called with the entries lock held, so writes land
    /// in mutation order.
    fn persist(&self, sessions: &[Entry]) {
        let Some(path) = &self.path else { return };
        let write = || -> std::io::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let bytes = serde_json::to_vec(&File {
                version: FILE_VERSION,
                source: self.source.as_str().to_string(),
                sessions: sessions.to_vec(),
            })
            .map_err(std::io::Error::other)?;
            // A temporary file owned by this write alone.
            let serial = self.writes.fetch_add(1, Ordering::Relaxed);
            let temporary = path.with_extension(format!("{}.{serial}.tmp", std::process::id()));
            if let Err(error) = create_private(&temporary, &bytes, false) {
                let _ = std::fs::remove_file(&temporary);
                return Err(error);
            }
            std::fs::rename(&temporary, path).inspect_err(|_| {
                let _ = std::fs::remove_file(&temporary);
            })
        };
        if let Err(error) = write() {
            tracing::warn!(%error, "saving the ACP session record failed");
        }
    }
}

/// One session's coalesced pending write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The agent's session id.
    pub session: String,
    /// An admission with this host-validated cwd (an existing entry keeps
    /// its own).
    pub cwd: Option<String>,
    /// A new title (`Some(None)` clears it); `None` leaves it unchanged.
    pub title: Option<Option<String>>,
}

/// Most sessions with a pending write. Beyond it the oldest change is
/// dropped: the record keeps at most [`MAX_ENTRIES`] entries anyway, and a
/// title is cosmetic.
pub const MAX_PENDING: usize = MAX_ENTRIES;

/// Writes not yet applied, coalesced per session (see the module docs).
/// Bounded by [`MAX_PENDING`] however fast changes arrive.
#[derive(Debug, Default)]
pub struct Pending {
    changes: HashMap<String, (u64, Change)>,
    order: u64,
}

impl Pending {
    fn entry(&mut self, session: &str) -> &mut Change {
        if !self.changes.contains_key(session) && self.changes.len() >= MAX_PENDING {
            // Prefer dropping a title over an admission.
            let victim = self
                .changes
                .iter()
                .min_by_key(|(_, (order, change))| (change.cwd.is_some(), *order))
                .map(|(session, _)| session.clone());
            if let Some(victim) = victim {
                self.changes.remove(&victim);
            }
        }
        self.order += 1;
        let order = self.order;
        &mut self
            .changes
            .entry(session.to_string())
            .or_insert_with(|| {
                (order, Change {
                    session: session.to_string(),
                    cwd: None,
                    title: None,
                })
            })
            .1
    }

    /// An admission of `session` in `cwd`. `title` follows [`Change::title`]:
    /// `Some(None)` is an explicit clear, `None` leaves the title (stored or
    /// pending) as it is.
    pub fn record(&mut self, session: &str, cwd: &str, title: Option<Option<String>>) {
        let change = self.entry(session);
        change.cwd = Some(cwd.to_string());
        if title.is_some() {
            change.title = title;
        }
    }

    /// A title change of `session`; supersedes an earlier pending one.
    pub fn title(&mut self, session: &str, title: Option<String>) {
        self.entry(session).title = Some(title);
    }

    /// Number of sessions with a pending write.
    pub fn len(&self) -> usize {
        self.changes.len()
    }

    /// Whether nothing is pending.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Take everything pending, in first-submission order.
    pub fn take(&mut self) -> Vec<Change> {
        let mut changes: Vec<(u64, Change)> = self.changes.drain().map(|(_, c)| c).collect();
        changes.sort_by_key(|(order, _)| *order);
        changes.into_iter().map(|(_, change)| change).collect()
    }
}

/// Move an unreadable record aside so it is neither trusted nor lost.
fn quarantine(path: &Path) {
    let aside = path.with_extension(format!("corrupt-{}", now()));
    match std::fs::rename(path, &aside) {
        Ok(()) => tracing::warn!(
            file = %aside.display(),
            "the ACP session record was unreadable; it was set aside and a new one started"
        ),
        Err(error) => tracing::warn!(%error, "the ACP session record is unreadable"),
    }
}

fn create_private(path: &Path, bytes: &[u8], exclusive: bool) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    if exclusive {
        options.create_new(true);
    } else {
        options.create(true).truncate(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use super::*;

    fn source(args: &[&str]) -> SourceId {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        SourceId::with_key(&"k".repeat(32), Path::new("/bin/agent"), &args)
    }

    #[test]
    fn dot_names_are_encoded_and_cannot_escape() {
        assert_eq!(encoded_component(".."), "2e2e");
        assert_eq!(encoded_component("a/b"), "612f62");
        let root = tempfile::tempdir().expect("tempdir");
        let id = source(&[]);
        let store = SessionStore::open(root.path(), "..", id.clone());
        store.record("s1", "/work", None, None);
        assert!(root
            .path()
            .join("2e2e")
            .join(format!("{}.json", id.as_str()))
            .is_file());
    }

    #[test]
    fn identity_follows_the_invocation_not_the_label() {
        let a = source(&["acp"]);
        assert_eq!(a, source(&["acp"]));
        assert_ne!(a, source(&["acp", "--model", "x"]), "arguments define the source");
        assert_ne!(a, source(&["ac", "p"]), "argument boundaries are part of it");
        let other_program =
            SourceId::with_key(&"k".repeat(32), Path::new("/bin/other"), &["acp".to_string()]);
        assert_ne!(a, other_program);
        let root = tempfile::tempdir().expect("tempdir");
        let derived = SourceId::derive(root.path(), Path::new("/bin/agent"), &[]).expect("derive");
        let again = SourceId::derive(root.path(), Path::new("/bin/agent"), &[]).expect("derive");
        assert_eq!(derived, again, "the key persists");
        let elsewhere = tempfile::tempdir().expect("tempdir");
        let foreign =
            SourceId::derive(elsewhere.path(), Path::new("/bin/agent"), &[]).expect("derive");
        assert_ne!(derived, foreign, "identities are keyed per state directory");
    }

    #[test]
    fn entries_survive_reopen_only_for_the_same_source_and_cwd_is_immutable() {
        let root = tempfile::tempdir().expect("tempdir");
        let alpha = source(&["--profile", "alpha"]);
        let store = SessionStore::open(root.path(), "work", alpha.clone());
        store.record("s1", "/w/one", Some("First".into()), None);
        store.record("s1", "/elsewhere", None, Some("2026-10-10T00:00:00Z".into()));
        let entry = store.lookup("s1").expect("entry");
        assert_eq!(entry.cwd, "/w/one");
        assert_eq!(entry.title.as_deref(), Some("First"));
        assert_eq!(SessionStore::open(root.path(), "work", alpha).lookup("s1"), Some(entry));
        let changed = source(&["--profile", "beta"]);
        assert!(SessionStore::open(root.path(), "work", changed)
            .lookup("s1")
            .is_none());
    }

    #[test]
    fn concurrent_admissions_and_titles_leave_the_newest_complete_record() {
        let root = tempfile::tempdir().expect("tempdir");
        let id = source(&[]);
        let store = Arc::new(SessionStore::open(root.path(), "busy", id.clone()));
        let threads = 8;
        let barrier = Arc::new(Barrier::new(threads));
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                let store = store.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    for n in 0..40 {
                        let session = format!("s{t}-{n}");
                        store.record(&session, "/w", None, None);
                        store.set_title(&session, Some(format!("title {t}-{n}")));
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("thread");
        }
        let reopened = SessionStore::open(root.path(), "busy", id);
        let entries = reopened.entries();
        assert_eq!(entries.len(), threads * 40, "no admission lost");
        for entry in &entries {
            let suffix = entry.session_id.trim_start_matches('s');
            assert_eq!(entry.title.as_deref(), Some(format!("title {suffix}").as_str()));
        }
        let leftovers = std::fs::read_dir(root.path().join(encoded_component("busy")))
            .expect("dir")
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0, "no temporary file is left behind");
    }

    /// A title can be reported before its admission's write is applied, and
    /// a flood of titles must stay one pending change per session.
    #[test]
    fn pending_writes_coalesce_stay_bounded_and_keep_early_titles() {
        let root = tempfile::tempdir().expect("tempdir");
        let id = source(&[]);
        let mut pending = Pending::default();
        pending.title("s", Some("early".into()));
        pending.record("s", "/w", None);
        for n in 0..10_000 {
            pending.title("t", Some(format!("v{n}")));
        }
        pending.record("t", "/w2", None);
        assert_eq!(pending.len(), 2, "one change per session");
        let store = SessionStore::open(root.path(), "titles", id.clone());
        store.apply(&pending.take());
        assert!(pending.is_empty());
        let reopened = SessionStore::open(root.path(), "titles", id);
        assert_eq!(reopened.lookup("s").and_then(|e| e.title).as_deref(), Some("early"));
        assert_eq!(reopened.lookup("t").and_then(|e| e.title).as_deref(), Some("v9999"));
        assert_eq!(reopened.lookup("t").map(|e| e.cwd).as_deref(), Some("/w2"));

        pending.record("admitted", "/k", None);
        for n in 0..MAX_PENDING * 3 {
            pending.title(&format!("x{n}"), None);
        }
        assert_eq!(pending.len(), MAX_PENDING, "bounded however fast titles arrive");
        let kept = pending.take();
        assert!(kept.iter().any(|c| c.session == "admitted"), "admissions outlive titles");
    }

    /// An admission that carries an explicit clear removes a stored title;
    /// one that carries no title leaves the stored one.
    #[test]
    fn an_admission_distinguishes_a_cleared_title_from_no_title() {
        let root = tempfile::tempdir().expect("tempdir");
        let id = source(&[]);
        let store = SessionStore::open(root.path(), "clear", id.clone());
        let mut pending = Pending::default();
        pending.record("cleared", "/w", Some(Some("old".into())));
        pending.record("kept", "/w", Some(Some("old".into())));
        store.apply(&pending.take());
        pending.record("cleared", "/w", Some(None));
        pending.record("kept", "/w", None);
        store.apply(&pending.take());
        let reopened = SessionStore::open(root.path(), "clear", id);
        assert_eq!(reopened.lookup("cleared").map(|e| e.title), Some(None));
        assert_eq!(reopened.lookup("kept").and_then(|e| e.title).as_deref(), Some("old"));
    }

    #[test]
    fn an_unreadable_record_is_set_aside_not_trusted() {
        let root = tempfile::tempdir().expect("tempdir");
        let id = source(&[]);
        let dir = root.path().join(encoded_component("broken"));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(format!("{}.json", id.as_str()));
        std::fs::write(&path, b"{\"version\":2,\"sessions\":[").expect("write");
        let store = SessionStore::open(root.path(), "broken", id);
        assert!(store.entries().is_empty());
        assert!(!path.exists(), "the damaged file was moved");
        let aside = std::fs::read_dir(&dir)
            .expect("dir")
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().contains("corrupt"));
        assert!(aside, "and kept for inspection");
    }
}
