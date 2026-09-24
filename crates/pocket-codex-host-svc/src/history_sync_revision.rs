//! Persist small append-scan checkpoints, never a second transcript copy.

use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
};

use anyhow::Result;
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
    let key = digest_bytes(path.as_os_str().as_encoded_bytes());
    let index_path = directory.join(format!("{key}.json"));
    let mut index: Index = fs::read(&index_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let mut reader = BufReader::new(File::open(path)?);
    let stat = reader.get_ref().metadata()?;
    let mut header = Vec::new();
    reader.by_ref().take(8192).read_until(b'\n', &mut header)?;
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        digest_bytes(format!("{}:{}:{}", stat.dev(), stat.ino(), digest_bytes(&header)).as_bytes())
    };
    #[cfg(not(unix))]
    let identity =
        digest_bytes(format!("{:?}:{}", stat.created().ok(), digest_bytes(&header)).as_bytes());
    let modified = format!("{:?}", stat.modified().ok());
    if index.identity == identity && index.length == stat.len() && index.modified == modified {
        return Ok(index.generation);
    }
    if index.identity != identity
        || stat.len() < index.length
        || (stat.len() == index.length && index.modified != modified)
    {
        index = Index {
            identity,
            generation: digest_bytes(format!("{}:{modified}", digest_bytes(&header)).as_bytes()),
            ..Index::default()
        };
    }
    reader.seek(SeekFrom::Start(index.offset))?;
    // Deserialize only the discriminator fields. Ignored JSON values are
    // streamed past, so even a multi-megabyte compaction record is recognized
    // without retaining its replacement transcript in memory.
    let start = index.offset;
    let mut records =
        serde_json::Deserializer::from_reader(&mut reader).into_iter::<RevisionMarker>();
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
    let after = reader.get_ref().metadata()?;
    // A raced scan is safe to return but cannot become an authoritative checkpoint.
    if after.len() == stat.len() && after.modified().ok() == stat.modified().ok() {
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
    }
    Ok(index.generation)
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
