use std::fs::FileTimes;

use super::*;

struct LogDir(PathBuf);

impl LogDir {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("pcx-log-test-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).expect("create log test directory");
        Self(path)
    }

    fn files(&self) -> Vec<PathBuf> {
        fs::read_dir(&self.0)
            .expect("read log directory")
            .map(|entry| entry.expect("log entry").path())
            .collect()
    }
}

impl Drop for LogDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn line(text: &str) -> LogLine {
    LogLine {
        level: "INFO".into(),
        target: "test".into(),
        message: text.into(),
        timestamp_ms: 0,
    }
}

#[test]
fn running_logger_rotates_and_prunes_without_reinitializing() {
    let dir = LogDir::new();
    let now = SystemTime::now();
    let mut log = RotatingFile::new(dir.0.clone());
    log.write(&line("first hour"), now);
    let first = dir.files().pop().expect("first file");
    log.file
        .as_ref()
        .expect("first handle")
        .set_times(FileTimes::new().set_modified(now))
        .expect("timestamp first file");
    let later = now + Duration::from_secs(3600);
    log.write(&line("second hour"), later);
    log.file
        .as_ref()
        .expect("second handle")
        .set_times(FileTimes::new().set_modified(later))
        .expect("timestamp second file");
    assert_eq!(dir.files().len(), 2);
    assert!(fs::read_to_string(&first)
        .expect("read first")
        .contains("first hour"));
    assert!(!fs::read_to_string(&first)
        .expect("read first")
        .contains("second hour"));

    // The maintenance tick prunes even when no new event is written.
    log.maintain(now + FILE_RETENTION + Duration::from_secs(60));
    assert!(!first.exists());
    let remaining = dir.files();
    assert_eq!(remaining.len(), 1);
    assert!(fs::read_to_string(&remaining[0])
        .expect("read second")
        .contains("second hour"));
}

#[test]
fn pruning_keeps_other_files_and_removes_legacy_daily_logs() {
    let dir = LogDir::new();
    let old = dir.0.join("pocket-codex-2026-01-01.log");
    let unrelated = dir.0.join("notes.txt");
    fs::write(&old, "legacy").expect("write legacy log");
    fs::write(&unrelated, "keep").expect("write unrelated file");
    let now = SystemTime::now();
    let aged = now - FILE_RETENTION - Duration::from_secs(60);
    File::options()
        .write(true)
        .open(&old)
        .expect("open legacy")
        .set_times(FileTimes::new().set_modified(aged))
        .expect("age legacy");
    let mut log = RotatingFile::new(dir.0.clone());
    log.maintain(now);
    assert!(!old.exists());
    assert!(unrelated.exists());
}
