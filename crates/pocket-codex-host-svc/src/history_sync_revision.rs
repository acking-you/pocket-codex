//! Persist small append-scan checkpoints, never a second transcript copy.

use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
};

use anyhow::{ensure, Result};
use pocket_codex_core::history_sync::digest_bytes;
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
struct Index {
    identity: String,
    offset: u64,
    length: u64,
    modified: String,
    generation: String,
}

pub(super) fn generation(session: &str) -> Result<String> {
    let path = super::sessions::rollout_path(session)?;
    let directory = pocket_codex_core::paths::state_dir()?.join("history-source-index-v1");
    scan(&path, &directory)
}

pub(super) fn scan(path: &Path, directory: &Path) -> Result<String> {
    fs::create_dir_all(directory)?;
    let lock = private_file(&directory.join("index.lock"), false)?;
    lock.lock()?;
    let file = File::open(path)?;
    let stat = file.metadata()?;
    scan_snapshot(path, directory, file, stat)
}

// The caller holds index.lock. Stat and the file handle describe the prefix
// this scan may consume, even if the writer appends before parsing begins.
fn scan_snapshot(path: &Path, directory: &Path, file: File, stat: Metadata) -> Result<String> {
    let key = digest_bytes(path.as_os_str().as_encoded_bytes());
    let index_path = directory.join(format!("{key}.json"));
    let mut index: Index = fs::read(&index_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let mut reader = BufReader::new(file);
    let identity = source_identity(&mut reader, &stat)?;
    let modified = format!("{:?}", stat.modified().ok());
    if index.identity == identity && index.length == stat.len() && index.modified == modified {
        return Ok(index.generation);
    }
    if index.identity != identity
        || stat.len() < index.length
        || (stat.len() == index.length && index.modified != modified)
    {
        index = Index {
            identity: identity.clone(),
            generation: digest_bytes(format!("{identity}:{modified}").as_bytes()),
            ..Index::default()
        };
    }
    reader.seek(SeekFrom::Start(index.offset))?;
    // Deserialize only the discriminator fields. Ignored JSON values are
    // streamed past, so even a multi-megabyte compaction record is recognized
    // without retaining its replacement transcript in memory.
    let start = index.offset;
    let mut records = serde_json::Deserializer::from_reader(
        reader.by_ref().take(stat.len().saturating_sub(start)),
    )
    .into_iter::<RevisionMarker>();
    while let Some(record) = records.next() {
        let marker = match record {
            Ok(marker) => marker,
            Err(error) if error.is_eof() => break,
            Err(error) => return Err(error.into()),
        };
        index.offset = start + records.byte_offset() as u64;
        if marker.destructive() {
            index.generation = digest_bytes(
                format!("{}:{}:{}", index.generation, index.offset, marker.kind).as_bytes(),
            );
        }
    }
    // Reopen the path to catch replacement of the file behind our old handle.
    // Ordinary appends preserve the verified prefix; truncation, replacement,
    // or an in-place rewrite at the same length must never publish it.
    let mut current = BufReader::new(File::open(path)?);
    let after = current.get_ref().metadata()?;
    ensure!(
        after.len() >= stat.len()
            && (after.len() > stat.len() || after.modified().ok() == stat.modified().ok())
            && source_identity(&mut current, &after)? == identity,
        "history changed while scanning; retry the window"
    );
    index.length = stat.len();
    index.modified = modified;
    let temporary = directory.join(format!("{key}.tmp"));
    let mut file = private_file(&temporary, true)?;
    file.write_all(&serde_json::to_vec(&index)?)?;
    file.sync_all()?;
    fs::rename(temporary, &index_path)?;
    let mut entries = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|s| s == "json"))
        .collect::<Vec<_>>();
    if entries.len() > 256 {
        entries.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
        for entry in entries.iter().take(entries.len() - 256) {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(index.generation)
}

fn source_identity(reader: &mut BufReader<File>, stat: &Metadata) -> Result<String> {
    let mut header = Vec::new();
    reader
        .by_ref()
        .take(stat.len().min(8192))
        .read_until(b'\n', &mut header)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(digest_bytes(
            format!("{}:{}:{}", stat.dev(), stat.ino(), digest_bytes(&header)).as_bytes(),
        ))
    }
    #[cfg(not(unix))]
    {
        Ok(digest_bytes(format!("{:?}:{}", stat.created().ok(), digest_bytes(&header)).as_bytes()))
    }
}

#[derive(Default, Deserialize)]
struct RevisionMarker {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    payload: Option<RevisionKind>,
}

#[derive(Deserialize)]
struct RevisionKind {
    #[serde(rename = "type", default)]
    kind: String,
}

impl RevisionMarker {
    fn destructive(&self) -> bool {
        self.kind == "compacted"
            || (self.kind == "event_msg"
                && self.payload.as_ref().is_some_and(|payload| {
                    matches!(payload.kind.as_str(), "thread_rolled_back" | "context_compacted")
                }))
    }
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

    fn scan_after_change(
        path: &Path,
        directory: &Path,
        change: impl FnOnce() -> Result<()>,
    ) -> Result<String> {
        fs::create_dir_all(directory)?;
        let lock = private_file(&directory.join("index.lock"), false)?;
        lock.lock()?;
        let file = File::open(path)?;
        let stat = file.metadata()?;
        change()?;
        scan_snapshot(path, directory, file, stat)
    }

    fn saved_index(path: &Path, directory: &Path) -> Result<Index> {
        let key = digest_bytes(path.as_os_str().as_encoded_bytes());
        Ok(serde_json::from_slice(&fs::read(directory.join(format!("{key}.json")))?)?)
    }

    #[test]
    fn initial_scan_persists_its_prefix_while_the_writer_keeps_appending() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("rollout.jsonl");
        let directory = temp.path().join("index");
        fs::write(&path, b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"session\"}}\n")?;
        let append = || -> Result<()> {
            writeln!(
                OpenOptions::new().append(true).open(&path)?,
                "{}",
                serde_json::json!({
                    "type": "response_item", "payload": {"type": "message", "text": "x".repeat(1_000_000)}
                })
            )?;
            Ok(())
        };
        let mut generation = None;
        let mut offset = 0;
        for _ in 0..3 {
            let bound = fs::metadata(&path)?.len();
            let current = scan_after_change(&path, &directory, append)?;
            let index = saved_index(&path, &directory)?;
            assert_eq!(index.length, bound);
            assert!(index.offset > offset && index.offset <= bound);
            if let Some(previous) = &generation {
                assert_eq!(&current, previous);
            }
            generation = Some(current);
            offset = index.offset;
        }
        assert_eq!(Some(scan(&path, &directory)?), generation);
        Ok(())
    }

    #[test]
    fn partial_record_and_new_rollback_wait_for_the_next_bounded_scan() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("rollout.jsonl");
        let directory = temp.path().join("index");
        fs::write(&path, b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"session\"}}\n")?;
        let complete = fs::metadata(&path)?.len();
        OpenOptions::new()
            .append(true)
            .open(&path)?
            .write_all(b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"thread_rolled_back\"")?;
        let first = scan_after_change(&path, &directory, || {
            OpenOptions::new()
                .append(true)
                .open(&path)?
                .write_all(b"}}\n")?;
            Ok(())
        })?;
        let index = saved_index(&path, &directory)?;
        assert!(index.offset <= complete);
        assert!(index.offset < index.length);
        let next = scan(&path, &directory)?;
        assert_ne!(first, next);
        assert_eq!(scan(&path, &directory)?, next);
        Ok(())
    }

    #[test]
    fn changed_source_cannot_publish_an_old_snapshot() -> Result<()> {
        for truncate in [false, true] {
            let temp = tempfile::tempdir()?;
            let path = temp.path().join("rollout.jsonl");
            let directory = temp.path().join("index");
            fs::write(&path, b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"original\"}}\n")?;
            let error = scan_after_change(&path, &directory, || {
                if truncate {
                    fs::write(&path, b"{}\n")?;
                } else {
                    fs::rename(&path, temp.path().join("old.jsonl"))?;
                    fs::write(
                        &path,
                        b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"replaced\"}}\n",
                    )?;
                }
                Ok(())
            })
            .expect_err("replacement must reject the snapshot");
            assert!(error.to_string().contains("history changed"));
            assert!(saved_index(&path, &directory).is_err());
        }
        Ok(())
    }

    #[test]
    fn append_restart_rollback_and_replacement_have_correct_generations() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let rollout = temp.path().join("rollout.jsonl");
        let directory = temp.path().join("index");
        fs::write(&rollout, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"session\"}}\n")?;
        let initial = scan(&rollout, &directory)?;
        let mut writer = OpenOptions::new().append(true).open(&rollout)?;
        writeln!(
            writer,
            "{}",
            serde_json::json!({"type": "response_item", "payload": {
                "type": "message", "text": "x".repeat(1_000_000)
            }})
        )?;
        assert_eq!(scan(&rollout, &directory)?, initial);
        assert_eq!(scan(&rollout, &directory)?, initial);
        writer
            .write_all(b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"thread_rolled_back\"")?;
        assert_eq!(scan(&rollout, &directory)?, initial);
        writer.write_all(b"}}\n")?;
        let rollback = scan(&rollout, &directory)?;
        assert_ne!(rollback, initial);
        assert_eq!(scan(&rollout, &directory)?, rollback);
        writeln!(
            writer,
            "{}",
            serde_json::json!({"type": "compacted", "payload": {
                "message": "summary".repeat(20_000), "replacement_history": []
            }})
        )?;
        let compacted = scan(&rollout, &directory)?;
        assert_ne!(compacted, rollback);
        assert_eq!(scan(&rollout, &directory)?, compacted);
        fs::write(&rollout, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"replacement\"}}\n")?;
        assert_ne!(scan(&rollout, &directory)?, rollback);
        Ok(())
    }
}
