//! Reuse lifecycle scans of immutable prefixes while a rollout is appended.

use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Instant, SystemTime},
};

use super::{classify_lines, read_tail, turn_state_of_line, Result, TurnState, MAX_TAIL_BYTES};

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
}

impl Stamp {
    fn read(path: &Path) -> Result<Self> {
        let metadata = std::fs::metadata(path)?;
        Ok(Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            created: metadata.created().ok(),
        })
    }
}

#[derive(Clone)]
struct Entry {
    stamp: Stamp,
    state: TurnState,
    used: Instant,
}

fn cache() -> &'static Mutex<HashMap<PathBuf, Entry>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Entry>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

pub(super) fn classify(path: &Path) -> Result<TurnState> {
    let stamp = Stamp::read(path)?;
    let previous = cache().lock().ok().and_then(|mut entries| {
        let entry = entries.get_mut(path)?;
        entry.used = Instant::now();
        Some(entry.clone())
    });
    if let Some(entry) = &previous {
        if stamp.modified.is_some() && stamp == entry.stamp {
            return Ok(entry.state.clone());
        }
    }
    let state = if let Some(entry) = previous.filter(|entry| {
        stamp.len > entry.stamp.len
            && stamp.created.is_some()
            && stamp.created == entry.stamp.created
    }) {
        scan_appended(path, entry.stamp.len, entry.state)?
    } else {
        let (tail, truncated) = read_tail(path, MAX_TAIL_BYTES)?;
        let state = classify_lines(tail.lines().skip(usize::from(truncated)));
        if truncated && state == TurnState::Empty {
            scan_appended(path, 0, TurnState::Empty)?
        } else {
            state
        }
    };
    // A partial final line must be re-read after the writer completes it.
    // Do not cache a scan that raced an append, truncate, or file replacement.
    if Stamp::read(path)? == stamp && ends_at_line_boundary(path, stamp.len)? {
        if let Ok(mut entries) = cache().lock() {
            entries.insert(path.into(), Entry {
                stamp,
                state: state.clone(),
                used: Instant::now(),
            });
            if entries.len() > 128 {
                if let Some(oldest) = entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.used)
                    .map(|(path, _)| path.clone())
                {
                    entries.remove(&oldest);
                }
            }
        }
    }
    Ok(state)
}

fn ends_at_line_boundary(path: &Path, len: u64) -> Result<bool> {
    if len == 0 {
        return Ok(true);
    }
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(len - 1))?;
    let mut last = [0];
    file.read_exact(&mut last)?;
    Ok(last[0] == b'\n')
}

fn scan_appended(path: &Path, offset: u64, mut state: TurnState) -> Result<TurnState> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        if let Some(next) = turn_state_of_line(&String::from_utf8_lossy(&line)) {
            state = next;
        }
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    const START: &str = "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\"}}\n";
    const DONE: &str = "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\"}}\n";

    #[test]
    fn long_turn_reuses_its_prefix_and_observes_appends_and_truncation() {
        let mut file = tempfile::NamedTempFile::new().expect("file");
        writeln!(file, "{START}{}", "x".repeat(MAX_TAIL_BYTES as usize + 64)).expect("write");
        assert_eq!(classify(file.path()).expect("initial"), TurnState::Incomplete);
        assert_eq!(classify(file.path()).expect("cached"), TurnState::Incomplete);
        file.write_all(DONE.as_bytes()).expect("append");
        assert_eq!(classify(file.path()).expect("appended"), TurnState::Completed);
        std::fs::write(file.path(), START).expect("truncate");
        assert_eq!(classify(file.path()).expect("truncated"), TurnState::Incomplete);
    }

    #[test]
    fn incomplete_json_is_not_skipped_after_the_next_append() {
        let mut file = tempfile::NamedTempFile::new().expect("file");
        file.write_all(START.as_bytes()).expect("start");
        assert_eq!(classify(file.path()).expect("cached start"), TurnState::Incomplete);
        let split = DONE.len() / 2;
        file.write_all(&DONE.as_bytes()[..split]).expect("partial");
        assert_eq!(classify(file.path()).expect("partial read"), TurnState::Incomplete);
        file.write_all(&DONE.as_bytes()[split..]).expect("complete");
        assert_eq!(classify(file.path()).expect("completed read"), TurnState::Completed);
    }
}
