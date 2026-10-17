//! Desktop-local native history. No relay, ACP admission or runtime launch.
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::{bail, ensure, Context, Result};
use pocket_codex_host_svc::native_history::{ClaudeHistory, NativeHistory};

/// One immutable local source selection, retained across bounded page reads.
pub struct Selected {
    /// Opaque root identity; callers must not merge different selections.
    pub id: String,
    /// Human-readable source directory or database on this computer.
    pub location: String,
    /// Read-only provider implementation.
    pub reader: NativeHistory,
}

struct Cached {
    key: String,
    source: Arc<Selected>,
}
static SOURCES: Mutex<VecDeque<Cached>> = Mutex::new(VecDeque::new());

/// Resolve the local provider only. Remote views never call this function.
/// An explicit refresh discovers newly prepared readers without downloading.
pub fn source(provider: &str, expected: Option<&str>, refresh: bool) -> Result<Arc<Selected>> {
    ensure!(super::serve_acp::hosting_supported(), "native history is available on desktop only");
    ensure!(matches!(provider, "claude" | "opencode"), "unsupported native history provider");
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .context("local home is unavailable")?;
    let root = match provider {
        "claude" => std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".claude")),
        _ => opencode_path(&home)?,
    };
    ensure!(root.is_absolute(), "native history requires an absolute local path");
    let key = format!(
        "{provider}:{}:{}:{:?}:{:?}",
        root.display(),
        home.display(),
        std::env::var_os("PATH"),
        std::env::var_os("CLAUDE_CODE_EXECUTABLE")
    );
    let id = pocket_codex_core::history_sync::digest_bytes(key.as_bytes());
    ensure!(
        expected.is_none_or(|known| known == id),
        "local history source changed; refresh the view"
    );
    if !refresh {
        let sources = SOURCES.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(cached) = sources.iter().find(|entry| entry.key == key) {
            return Ok(cached.source.clone());
        }
    }
    let reader = match provider {
        "claude" => {
            let launch = super::claude_setup::environment()?
                .launch(&[])
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            let cli = launch
                .env
                .iter()
                .rev()
                .find(|(key, _)| key == "CLAUDE_CODE_EXECUTABLE")
                .map(|(_, v)| PathBuf::from(v))
                .or_else(|| std::env::var_os("CLAUDE_CODE_EXECUTABLE").map(PathBuf::from));
            let mut reader =
                ClaudeHistory::from_adapter(launch.program, &launch.source, root.clone())?;
            if let Some(cli) = cli {
                reader = reader.with_owner_probe(cli, home);
            }
            NativeHistory::Claude(Arc::new(reader))
        },
        _ => open_code(root.clone())?,
    };
    let source = Arc::new(Selected {
        id,
        location: root.to_string_lossy().into_owned(),
        reader,
    });
    let mut sources = SOURCES.lock().unwrap_or_else(|p| p.into_inner());
    sources.retain(|entry| entry.key != key);
    while sources.len() >= 2 {
        sources.pop_front();
    }
    sources.push_back(Cached {
        key,
        source: source.clone(),
    });
    Ok(source)
}

pub(super) fn open_code(path: PathBuf) -> Result<NativeHistory> {
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        Ok(NativeHistory::OpenCode(pocket_codex_host_svc::native_history::OpenCodeHistory::new(
            path,
        )?))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = path;
        bail!("OpenCode native history is available on desktop only")
    }
}

/// Stable OpenCode storage under the effective child environment. Custom
/// builds/database names must supply OPENCODE_DB; no unrelated DB is scanned.
pub(super) fn opencode_path_with(
    home: &std::path::Path,
    xdg: Option<std::ffi::OsString>,
    db: Option<std::ffi::OsString>,
) -> Result<PathBuf> {
    let data = xdg
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"))
        .join("opencode");
    let db = db
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("opencode.db"));
    if db.as_os_str() == ":memory:" {
        bail!("in-memory OpenCode history has no local database");
    }
    Ok(if db.is_absolute() { db } else { data.join(db) })
}
fn opencode_path(home: &std::path::Path) -> Result<PathBuf> {
    opencode_path_with(home, std::env::var_os("XDG_DATA_HOME"), std::env::var_os("OPENCODE_DB"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_paths_respect_explicit_roots_without_creating_them() {
        let home = PathBuf::from("/fixture-home");
        assert_eq!(
            opencode_path_with(&home, None, None).expect("default"),
            home.join(".local/share/opencode/opencode.db")
        );
        assert_eq!(
            opencode_path_with(&home, Some("/custom-data".into()), Some("custom.db".into()))
                .expect("relative"),
            PathBuf::from("/custom-data/opencode/custom.db")
        );
        assert_eq!(
            opencode_path_with(&home, None, Some("/explicit/test.db".into())).expect("absolute"),
            PathBuf::from("/explicit/test.db")
        );
        assert!(opencode_path_with(&home, None, Some(":memory:".into())).is_err());
    }

    #[test]
    #[cfg(feature = "flutter")]
    #[ignore = "requires an explicit existing OpenCode acceptance fixture"]
    fn native_opencode_fixture_uses_offline_bridge_without_admission() {
        use crate::api::bridge::native_sessions as api;
        let fixture = std::env::var("PCX_TEST_OPENCODE_DB").expect("explicit fixture path");
        assert_eq!(std::env::var("OPENCODE_DB").expect("selected local database"), fixture);
        let session = std::env::var("PCX_TEST_OPENCODE_SESSION").expect("fixture session");
        let marker = std::env::var("PCX_TEST_OPENCODE_MARKER").expect("fixture public marker");
        crate::engine::runtime::init(std::env::temp_dir()).expect("fixture runtime");
        let inventory = api::app_native_sessions("opencode".into(), None, None).expect("inventory");
        let row = inventory
            .sessions
            .iter()
            .find(|row| row.thread_id == session)
            .expect("listed session");
        assert!(!row.allows_resume && !row.requires_takeover);
        let page = api::app_native_session_page(
            "opencode".into(),
            session.clone(),
            None,
            Some(inventory.source_id.clone()),
        )
        .expect("persisted page");
        assert!(page.items.iter().any(|item| item.text.contains(&marker)));
        assert!(page
            .items
            .iter()
            .any(|item| item.item_type == "dynamicToolCall"
                && item.title == "bash"
                && item.text.contains("sleep 45")));
        assert!(page.items.iter().all(|item| item.item_type != "reasoning"));
        let revision =
            api::app_native_session_revision("opencode".into(), session, Some(inventory.source_id))
                .expect("revision");
        assert_eq!(revision.generation, page.generation);
        assert_eq!(revision.source_id, page.source_id);
        assert!(!revision.liveness.allows_resume && !revision.liveness.requires_takeover);
        assert!(api::app_native_session_page(
            "opencode".into(),
            row.thread_id.clone(),
            None,
            Some("different-root".into())
        )
        .is_err());
    }
}
