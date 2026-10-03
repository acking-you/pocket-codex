//! `fs/read_text_file` and `fs/write_text_file`, confined to the session's
//! working directory (TRD §4.2.8, D11).

use std::path::{Path, PathBuf};

use pocket_codex_core::acp::{ReadTextFileRequest, ReadTextFileResponse, WriteTextFileRequest};
use serde_json::{json, Value};
use tracing::info;

use super::error::AcpError;

/// Largest file `fs/read_text_file` reads.
pub const MAX_READ_BYTES: u64 = 16 * 1024 * 1024;
/// Largest content `fs/write_text_file` writes.
pub const MAX_WRITE_BYTES: usize = 32 * 1024 * 1024;

const OUTSIDE: &str = "path is outside the session directory";

fn canonical_root(cwd: &str) -> Result<PathBuf, AcpError> {
    std::fs::canonicalize(cwd)
        .map_err(|e| AcpError::NotFound(format!("session directory {cwd}: {e}")))
}

fn absolute(path: &str) -> Result<PathBuf, AcpError> {
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(AcpError::InvalidParams("path must be absolute".into()));
    }
    Ok(path)
}

fn not_found_or_io(path: &Path, error: std::io::Error) -> AcpError {
    if error.kind() == std::io::ErrorKind::NotFound {
        AcpError::NotFound(format!("{} does not exist", path.display()))
    } else {
        AcpError::Io(format!("{}: {error}", path.display()))
    }
}

/// Read a text file below `cwd`.
pub fn read_text_file(cwd: &str, request: &ReadTextFileRequest) -> Result<Value, AcpError> {
    let root = canonical_root(cwd)?;
    let path = absolute(&request.path)?;
    let real = std::fs::canonicalize(&path).map_err(|e| not_found_or_io(&path, e))?;
    if !real.starts_with(&root) {
        return Err(AcpError::InvalidParams(OUTSIDE.into()));
    }
    let meta = std::fs::metadata(&real).map_err(|e| not_found_or_io(&real, e))?;
    if meta.len() > MAX_READ_BYTES {
        return Err(AcpError::InvalidParams(format!("file is larger than {MAX_READ_BYTES} bytes")));
    }
    let bytes = std::fs::read(&real).map_err(|e| not_found_or_io(&real, e))?;
    let text = String::from_utf8_lossy(&bytes);
    let content = match (request.line, request.limit) {
        (None, None) => text.into_owned(),
        (line, limit) => {
            let skip = line.unwrap_or(1).saturating_sub(1) as usize;
            let take = limit.map_or(usize::MAX, |l| l as usize);
            text.split_inclusive('\n').skip(skip).take(take).collect()
        },
    };
    serde_json::to_value(ReadTextFileResponse {
        content,
        meta: None,
    })
    .map_err(|e| AcpError::Internal(e.to_string()))
}

/// Write a text file below `cwd` (atomic rename; keeps Unix permissions).
pub fn write_text_file(cwd: &str, request: &WriteTextFileRequest) -> Result<Value, AcpError> {
    let root = canonical_root(cwd)?;
    let path = absolute(&request.path)?;
    if request.content.len() > MAX_WRITE_BYTES {
        return Err(AcpError::InvalidParams(format!(
            "content is larger than {MAX_WRITE_BYTES} bytes"
        )));
    }
    let parent = path
        .parent()
        .ok_or_else(|| AcpError::InvalidParams(OUTSIDE.into()))?;
    let real_parent = std::fs::canonicalize(parent).map_err(|e| not_found_or_io(parent, e))?;
    if !real_parent.starts_with(&root) {
        return Err(AcpError::InvalidParams(OUTSIDE.into()));
    }
    let name = path
        .file_name()
        .ok_or_else(|| AcpError::InvalidParams("no file name".into()))?;
    let target = real_parent.join(name);
    let existing = std::fs::symlink_metadata(&target).ok();
    if existing
        .as_ref()
        .is_some_and(|m| m.file_type().is_symlink())
    {
        return Err(AcpError::InvalidParams("refusing to write through a symbolic link".into()));
    }
    let temp = real_parent.join(format!(
        ".{}.pcx-{}.tmp",
        name.to_string_lossy(),
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::write(&temp, request.content.as_bytes()).map_err(|e| not_found_or_io(&temp, e))?;
    #[cfg(unix)]
    if let Some(meta) = &existing {
        let _ = std::fs::set_permissions(&temp, meta.permissions());
    }
    if let Err(e) = std::fs::rename(&temp, &target) {
        let _ = std::fs::remove_file(&temp);
        return Err(not_found_or_io(&target, e));
    }
    info!(path = %target.display(), bytes = request.content.len(), "acp fs write");
    Ok(json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(cwd: &Path, path: &Path) -> Result<Value, AcpError> {
        read_text_file(&cwd.to_string_lossy(), &ReadTextFileRequest {
            session_id: "s".into(),
            path: path.to_string_lossy().into_owned(),
            ..ReadTextFileRequest::default()
        })
    }

    #[test]
    fn line_and_limit_select_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "one\ntwo\nthree\n").expect("write");
        let value = read_text_file(&dir.path().to_string_lossy(), &ReadTextFileRequest {
            session_id: "s".into(),
            path: file.to_string_lossy().into_owned(),
            line: Some(2),
            limit: Some(1),
            meta: None,
        })
        .expect("read");
        assert_eq!(value["content"], "two\n");
        assert_eq!(read(dir.path(), &file).expect("whole")["content"], "one\ntwo\nthree\n");
        let missing = read(dir.path(), &dir.path().join("nope")).expect_err("missing");
        assert_eq!(missing.code(), "acp.not_found");
    }
}
