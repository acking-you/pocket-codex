//! Explicit file-link reads and same-device checks for a remote controller.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, HeaderValue},
    response::Response,
    Json,
};
use serde::Deserialize;
use tokio::io::AsyncReadExt;
use tokio_util::io::ReaderStream;

use crate::{fs, sessions, ApiError, AppState};

/// Maximum bytes returned for a preview, including images.
pub const PREVIEW_LIMIT: u64 = 8 * 1024 * 1024;

#[derive(Deserialize)]
pub(crate) struct FileQuery {
    thread: Option<String>,
    href: String,
    #[serde(default)]
    preview: bool,
}

/// Strip editor line/column references without stripping a Windows drive.
fn without_location(href: &str) -> &str {
    let value = href.split('#').next().unwrap_or(href);
    let mut end = value.len();
    for _ in 0..2 {
        let Some(colon) = value[..end].rfind(':') else { break };
        let suffix = &value[colon + 1..end];
        if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_digit()) {
            break;
        }
        end = colon;
    }
    &value[..end]
}

/// Resolve a file URI, native absolute path or session-relative link.
pub fn resolve_link(href: &str, cwd: Option<&Path>) -> Result<PathBuf> {
    let raw = without_location(href.trim());
    if raw.is_empty() || raw.starts_with('#') || raw.starts_with("//") {
        bail!("not a file link");
    }
    let path = if raw.starts_with("file:") {
        let mut uri = url::Url::parse(raw).context("invalid file URI")?;
        if uri
            .host_str()
            .is_some_and(|host| host != "localhost" && !host.is_empty())
        {
            bail!("network file authorities are not supported");
        }
        uri.set_host(None)
            .map_err(|_| anyhow!("invalid file authority"))?;
        uri.to_file_path()
            .map_err(|_| anyhow!("invalid file path"))?
    } else {
        let windows_drive = raw.as_bytes().get(1) == Some(&b':')
            && raw.as_bytes().first().is_some_and(u8::is_ascii_alphabetic);
        if !windows_drive && url::Url::parse(raw).is_ok() {
            bail!("only file links may be read");
        }
        let decoded = percent_encoding::percent_decode_str(raw)
            .decode_utf8()
            .context("file path is not UTF-8")?;
        PathBuf::from(decoded.as_ref())
    };
    if path.as_os_str().to_string_lossy().contains('\0') {
        bail!("invalid file path");
    }
    if path.is_absolute() {
        return Ok(path);
    }
    Ok(cwd
        .context("relative file link requires a session working directory")?
        .join(path))
}

fn referenced(text: &str, href: &str) -> bool {
    text.match_indices(href).any(|(start, _)| {
        let end = start + href.len();
        let boundary = |c: char| {
            c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>' | '[' | ']' | '"' | '\'' | '`')
        };
        (start == 0 || text[..start].chars().next_back().is_some_and(boundary))
            && (end == text.len() || text[end..].chars().next().is_some_and(boundary))
    })
}

fn authorized_file(query: &FileQuery, roots: &[String]) -> Result<PathBuf> {
    let rollout = query
        .thread
        .as_deref()
        .map(sessions::rollout_path)
        .transpose()?;
    authorize_link(query, roots, rollout.as_deref())
}

fn authorize_link(query: &FileQuery, roots: &[String], rollout: Option<&Path>) -> Result<PathBuf> {
    let info = rollout
        .map(pocket_codex_codex::rollout::read_session_info)
        .transpose()?;
    let cwd = info
        .as_ref()
        .and_then(|info| info.cwd.as_deref())
        .map(Path::new);
    let path = resolve_link(&query.href, cwd)?;
    if !path.is_file() {
        bail!("path is not a file");
    }
    if fs::within_roots(&path, roots)
        || cwd.is_some_and(|cwd| fs::within_roots(&path, &[cwd.to_string_lossy().into_owned()]))
    {
        return std::fs::canonicalize(path).context("resolving linked file");
    }
    // This endpoint is invoked only after an explicit Preview/Download action.
    // Unlike automatic image thumbnails, a selected conversation link may name
    // an artifact outside project roots. Never grant unrelated file browsing.
    if let Some(rollout) = rollout {
        let items = pocket_codex_codex::rollout::read_transcript(rollout)?;
        // Markdown percent-encodes spaces and Unicode in displayed hrefs. Only
        // accept the decoded spelling when it still resolves to this same file;
        // a literal %20 filename must not inherit a space filename's grant.
        let decoded = percent_encoding::percent_decode_str(&query.href)
            .decode_utf8()
            .ok();
        let decoded = decoded.filter(|value| resolve_link(value, cwd).ok().as_ref() == Some(&path));
        let matches = |value: &str| {
            referenced(value, &query.href)
                || decoded.as_ref().is_some_and(|href| referenced(value, href))
        };
        if items.iter().any(|item| {
            matches(&item.text)
                || item.images.iter().any(|image| {
                    image == &query.href
                        || decoded.as_ref().is_some_and(|href| image == href.as_ref())
                })
        }) {
            return std::fs::canonicalize(path).context("resolving linked file");
        }
    }
    bail!("path is outside the configured project roots and session links")
}

pub(crate) async fn read(
    State(state): State<std::sync::Arc<AppState>>,
    Query(query): Query<FileQuery>,
) -> Result<Response, ApiError> {
    let roots = state.host.get().await.project_roots;
    let preview = query.preview;
    let path = tokio::task::spawn_blocking(move || authorized_file(&query, &roots))
        .await
        .context("file authorization task panicked")??;
    let file = tokio::fs::File::open(path)
        .await
        .context("opening linked file")?;
    let size = file
        .metadata()
        .await
        .context("reading file metadata")?
        .len();
    let length = if preview { size.min(PREVIEW_LIMIT) } else { size };
    let mut response = Response::new(Body::from_stream(ReaderStream::new(file.take(length))));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    response
        .headers_mut()
        .insert("x-file-size", HeaderValue::from(size));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

#[derive(Deserialize)]
pub(crate) struct ProbeQuery {
    id: String,
}

/// Directory for short-lived filesystem challenges used to confirm a local
/// host.
pub fn probe_dir() -> Result<PathBuf> {
    Ok(pocket_codex_core::paths::state_dir()?.join("local-probes"))
}

pub(crate) async fn local_probe(
    Query(query): Query<ProbeQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Only a UUID under our own probe directory can be read. The unpredictable
    // token is never supplied in the request, so a remote peer cannot echo it.
    let token = match tokio::fs::File::open(
        probe_dir()?.join(
            uuid::Uuid::parse_str(&query.id)
                .context("invalid probe ID")?
                .to_string(),
        ),
    )
    .await
    {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(37)
                .read_to_end(&mut bytes)
                .await
                .context("reading local probe")?;
            String::from_utf8(bytes)
                .ok()
                .filter(|s| uuid::Uuid::parse_str(s).is_ok())
        },
        Err(_) => None,
    };
    Ok(Json(serde_json::json!({"token": token})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_encoded_and_relative_links_with_locations() {
        let cwd = std::env::temp_dir();
        assert_eq!(
            resolve_link("notes/a%20b.md#L12", Some(&cwd)).expect("relative"),
            cwd.join("notes/a b.md")
        );
        assert_eq!(
            resolve_link("notes/a.md:12:4", Some(&cwd)).expect("line"),
            cwd.join("notes/a.md")
        );
        let uri = url::Url::from_file_path(cwd.join("a b.md")).expect("file URI");
        assert_eq!(resolve_link(uri.as_str(), None).expect("file URI"), cwd.join("a b.md"));
        for invalid in
            ["javascript:alert(1)", "https://example.com/a", "file://elsewhere/a", "#anchor"]
        {
            assert!(resolve_link(invalid, Some(&cwd)).is_err(), "{invalid}");
        }
    }

    #[test]
    fn references_require_complete_destinations() {
        assert!(referenced("[report](/tmp/a%20b.txt#L2)", "/tmp/a%20b.txt#L2"));
        assert!(!referenced("[report](/tmp/a.txt.bak)", "/tmp/a.txt"));
        assert!(!referenced("[report](/other/tmp/a.txt)", "/tmp/a.txt"));
    }
    #[test]
    fn session_links_authorize_cwd_and_exact_artifacts_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = dir.path().join("project");
        std::fs::create_dir(&cwd).expect("project");
        let inside = cwd.join("local.txt");
        let outside = dir.path().join("report space.txt");
        let unrelated = dir.path().join("secret.txt");
        for path in [&inside, &outside, &unrelated] {
            std::fs::write(path, b"hello").expect("file");
        }
        let href = url::Url::from_file_path(&outside).expect("url").to_string() + "#L2";
        let rollout = dir.path().join("rollout.jsonl");
        let header = serde_json::json!({"type":"session_meta", "payload":{"id":"test", "cwd":cwd}});
        let message = serde_json::json!({"type":"response_item", "payload":{"type":"message", "role":"assistant", "content":[{"type":"output_text", "text":format!("[report](<{}>)", href.replace("%20", " "))}]}});
        std::fs::write(&rollout, format!("{header}\n{message}\n")).expect("rollout");
        let percent_path = dir.path().join("report%20space.txt");
        std::fs::write(&percent_path, b"not referenced").expect("percent file");
        let query = |href| FileQuery {
            thread: Some("test".into()),
            href,
            preview: true,
        };
        assert_eq!(
            authorize_link(&query("local.txt:1".into()), &[], Some(&rollout)).expect("cwd"),
            inside.canonicalize().expect("canonical")
        );
        assert_eq!(
            authorize_link(&query(href.clone()), &[], Some(&rollout)).expect("artifact"),
            outside.canonicalize().expect("canonical")
        );
        let encoded = serde_json::json!({"type":"response_item", "payload":{"type":"message", "role":"assistant", "content":[{"type":"output_text", "text":format!("[encoded]({href})")}]}});
        std::fs::write(&rollout, format!("{header}\n{encoded}\n")).expect("encoded rollout");
        let percent_href = url::Url::from_file_path(percent_path)
            .expect("percent URI")
            .to_string()
            + "#L2";
        assert!(authorize_link(&query(percent_href), &[], Some(&rollout)).is_err());
        assert!(authorize_link(
            &query(unrelated.to_string_lossy().into_owned()),
            &[],
            Some(&rollout)
        )
        .is_err());
        assert!(authorize_link(&query("../secret.txt".into()), &[], Some(&rollout)).is_err());
        #[cfg(unix)]
        {
            let link = cwd.join("escape");
            std::os::unix::fs::symlink(&unrelated, &link).expect("symlink");
            assert!(authorize_link(&query("escape".into()), &[], Some(&rollout)).is_err());
        }
    }
}
