//! Managed directory layout and `installed.json` (TRD §4.3.3, §5.2).

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};

use super::super::error::AcpError;

/// Serializes read-modify-write of `installed.json` within the process.
static INSTALLED_LOCK: Mutex<()> = Mutex::new(());

/// Private Node runtime record.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InstalledNode {
    /// Version without `v`.
    pub version: String,
    /// Runtime root, relative to the managed directory.
    pub path: String,
}

/// One installed agent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InstalledAgent {
    /// Installed release version.
    pub version: String,
    /// `npm` | `archive`
    pub kind: String,
    /// Install directory, relative to the managed directory.
    pub path: String,
    /// `catalog` | `registry`
    pub source: String,
    /// Integrity of the lockfile or archive.
    pub integrity: Option<String>,
    /// `agentInfo.version` seen during validation.
    pub agent_version: Option<String>,
    /// RFC 3339 install time.
    pub installed_at: String,
    /// Registry installs: npm entry relative to the prefix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// Registry installs: executable relative to the install directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmd: Option<String>,
    /// Registry installs: launch arguments.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

/// `installed.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Installed {
    /// Format version (1).
    pub schema: u32,
    /// Private Node runtime.
    pub node: Option<InstalledNode>,
    /// Agent id → record.
    pub agents: BTreeMap<String, InstalledAgent>,
    /// Directories (relative) to delete once unused.
    pub gc_pending: Vec<String>,
}

impl Default for Installed {
    fn default() -> Self {
        Self {
            schema: 1,
            node: None,
            agents: BTreeMap::new(),
            gc_pending: Vec::new(),
        }
    }
}

/// Paths below the managed directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The managed directory (`<state_dir>/acp`).
    pub root: PathBuf,
}

impl Layout {
    /// Layout rooted at `root`.
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// `agents.toml`
    pub fn settings(&self) -> PathBuf {
        self.root.join("agents.toml")
    }

    /// `installed.json`
    pub fn installed(&self) -> PathBuf {
        self.root.join("installed.json")
    }

    /// `node/v<version>`
    pub fn node_dir(&self, version: &str) -> PathBuf {
        self.root.join("node").join(format!("v{version}"))
    }

    /// `agents/<id>/<version>`
    pub fn agent_dir(&self, id: &str, version: &str) -> PathBuf {
        self.root.join("agents").join(id).join(version)
    }

    /// A fresh `<dir>.staging-<rand>` next to `dir`.
    pub fn staging(dir: &Path) -> PathBuf {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        dir.with_file_name(format!("{name}.staging-{}", uuid::Uuid::new_v4().simple()))
    }

    /// `tmp/`
    pub fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// `npm-cache/`
    pub fn npm_cache(&self) -> PathBuf {
        self.root.join("npm-cache")
    }

    /// `npmrc` (empty)
    pub fn npmrc(&self) -> PathBuf {
        self.root.join("npmrc")
    }

    /// `data/<family>/`
    pub fn data(&self, family: &str) -> PathBuf {
        self.root.join("data").join(family)
    }

    /// `registry-cache.json`
    pub fn registry_cache(&self) -> PathBuf {
        self.root.join("registry-cache.json")
    }

    /// `audit.log`
    pub fn audit_log(&self) -> PathBuf {
        self.root.join("audit.log")
    }

    /// Path relative to the root, with `/` separators.
    pub fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Absolute path of a relative record path.
    pub fn absolute(&self, relative: &str) -> PathBuf {
        relative
            .split('/')
            .fold(self.root.clone(), |acc, part| acc.join(part))
    }
}

/// Create `dir` (and parents); `0700` on Unix.
pub fn ensure_private_dir(dir: &Path) -> Result<(), AcpError> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Write `bytes` to `path` atomically (temp file + rename), `0600` on Unix.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<(), AcpError> {
    let parent = path
        .parent()
        .ok_or_else(|| AcpError::Io("no parent directory".into()))?;
    std::fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = parent.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        use std::io::Write as _;
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result.map_err(AcpError::from)
}

/// Read `installed.json` (missing or unreadable → empty).
pub fn read_installed(layout: &Layout) -> Installed {
    std::fs::read(layout.installed())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Read-modify-write `installed.json` under a process-wide lock.
pub fn update_installed<T>(
    layout: &Layout,
    edit: impl FnOnce(&mut Installed) -> T,
) -> Result<T, AcpError> {
    let _guard = INSTALLED_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut installed = read_installed(layout);
    let result = edit(&mut installed);
    let bytes =
        serde_json::to_vec_pretty(&installed).map_err(|e| AcpError::Internal(e.to_string()))?;
    write_private(&layout.installed(), &bytes)?;
    Ok(result)
}

/// RFC 3339 now.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
