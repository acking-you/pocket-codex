//! In-process byte accounting validated under the cache's cross-process lock.
//! The lock-file revision invalidates other writers' counters before mutation;
//! directory changes also detect crash leftovers and external additions.

use std::{
    collections::VecDeque,
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

use anyhow::Result;

struct Record {
    root: PathBuf,
    revision: u64,
    modified: Option<SystemTime>,
    used: u64,
}

fn records() -> &'static Mutex<VecDeque<Record>> {
    static RECORDS: OnceLock<Mutex<VecDeque<Record>>> = OnceLock::new();
    RECORDS.get_or_init(Default::default)
}

pub(super) struct Guard {
    file: File,
    root: PathBuf,
    revision: u64,
    pub(super) used: Option<u64>,
}

impl Guard {
    pub(super) fn new(mut file: File, root: &Path) -> Result<Self> {
        file.lock()?;
        let mut bytes = [0; 8];
        let revision =
            if file.read_exact(&mut bytes).is_ok() { u64::from_le_bytes(bytes) } else { 0 };
        let modified = fs::metadata(root)?.modified().ok();
        let used = records().lock().ok().and_then(|records| {
            records
                .iter()
                .find(|r| r.root == root && r.revision == revision && r.modified == modified)
                .map(|r| r.used)
        });
        Ok(Self {
            file,
            root: root.into(),
            revision,
            used,
        })
    }

    /// Invalidate before any filesystem mutation. A failed write or process
    /// exit cannot leave another process trusting the old count. No fsync
    /// is needed here: an OS restart also discards every process-local
    /// counter.
    pub(super) fn invalidate(&mut self) -> Result<()> {
        self.used = None;
        self.revision = self.revision.wrapping_add(1);
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&self.revision.to_le_bytes())?;
        if let Ok(mut records) = records().lock() {
            records.retain(|r| r.root != self.root);
        }
        Ok(())
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let (Some(used), Ok(mut records)) = (self.used, records().lock()) {
            records.retain(|r| r.root != self.root);
            records.push_back(Record {
                root: self.root.clone(),
                revision: self.revision,
                modified: fs::metadata(&self.root).and_then(|m| m.modified()).ok(),
                used,
            });
            while records.len() > 8 {
                records.pop_front();
            }
        }
        // The File releases the OS lock after publishing the process-local
        // count.
    }
}
