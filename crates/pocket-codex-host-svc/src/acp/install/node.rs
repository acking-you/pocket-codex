//! The private Node runtime (TRD §4.3.6, D13).

use std::{path::PathBuf, time::Duration};

use super::{
    super::error::AcpError,
    archive, fetch,
    resolve::InstallContext,
    store::{read_installed, update_installed, InstalledNode, Layout},
};

/// The first run of a fresh Node can be slow: Rosetta translated the x86_64
/// build in about 15 s on an Apple Silicon Mac, and endpoint scanners check
/// new executables too.
const VERSION_TIMEOUT: Duration = Duration::from_secs(60);

/// Private node executable and npm entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeRuntime {
    /// `node` / `node.exe`.
    pub node: PathBuf,
    /// `npm-cli.js`.
    pub npm_cli: PathBuf,
}

/// Paths inside a Node runtime root.
pub fn runtime_at(root: &std::path::Path) -> NodeRuntime {
    if cfg!(windows) {
        NodeRuntime {
            node: root.join("node.exe"),
            npm_cli: root
                .join("node_modules")
                .join("npm")
                .join("bin")
                .join("npm-cli.js"),
        }
    } else {
        NodeRuntime {
            node: root.join("bin").join("node"),
            npm_cli: root
                .join("lib")
                .join("node_modules")
                .join("npm")
                .join("bin")
                .join("npm-cli.js"),
        }
    }
}

/// The installed runtime, if it matches the catalog version.
pub fn installed_runtime(ctx: &InstallContext) -> Option<NodeRuntime> {
    let pin = ctx.catalog.node.as_ref()?;
    let layout = Layout::new(&ctx.root);
    let record = read_installed(&layout).node?;
    if record.version != pin.version {
        return None;
    }
    let runtime = runtime_at(&layout.absolute(&record.path));
    runtime.node.is_file().then_some(runtime)
}

/// Install the pinned Node once; `progress(state, bytes, total)`.
pub async fn ensure_node(
    ctx: &InstallContext,
    progress: &(dyn Fn(&'static str, u64, Option<u64>) + Send + Sync),
) -> Result<NodeRuntime, AcpError> {
    if let Some(runtime) = installed_runtime(ctx) {
        return Ok(runtime);
    }
    let pin =
        ctx.catalog.node.clone().ok_or_else(|| {
            AcpError::UnsupportedPlatform("this build has no Node runtime".into())
        })?;
    let platform = ctx.platform.clone()?;
    let target = pin.targets.get(platform.key).cloned().ok_or_else(|| {
        AcpError::UnsupportedPlatform(format!("no Node runtime for {}", platform.key))
    })?;
    let layout = Layout::new(&ctx.root);
    let suffix = if target.url.ends_with(".zip") { "zip" } else { "tar.gz" };
    let download = layout.tmp().join(format!(
        "node-{}-{}.{suffix}",
        pin.version,
        uuid::Uuid::new_v4().simple()
    ));
    progress("downloading", 0, None);
    fetch::download(&target.url, &target.integrity, &download, &ctx.fetch, &|b, t| {
        progress("downloading", b, t)
    })
    .await?;
    progress("extracting", 0, None);
    let final_dir = layout.node_dir(&pin.version);
    let staging = Layout::staging(&final_dir);
    let extracted = {
        let (download, staging) = (download.clone(), staging.clone());
        tokio::task::spawn_blocking(move || archive::extract(&download, &staging))
            .await
            .map_err(|e| AcpError::Internal(e.to_string()))?
    };
    let _ = std::fs::remove_file(&download);
    if let Err(e) = extracted {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    let root_rel = target.root.clone().unwrap_or_default();
    let runtime = runtime_at(&staging.join(&root_rel));
    let expected = format!("v{}", pin.version);
    let version = node_version(&runtime.node, VERSION_TIMEOUT).await;
    if version.as_deref() != Ok(expected.as_str()) {
        let _ = std::fs::remove_dir_all(&staging);
        let reason = match version {
            Ok(found) => format!("reports {found}"),
            Err(reason) => reason,
        };
        return Err(AcpError::ValidationFailed(format!(
            "the Node runtime {reason}, expected {expected}"
        )));
    }
    if final_dir.exists() {
        let _ = std::fs::remove_dir_all(&final_dir);
    }
    std::fs::rename(&staging, &final_dir)?;
    let root = final_dir.join(&root_rel);
    let record = InstalledNode {
        version: pin.version.clone(),
        path: layout.relative(&root),
    };
    update_installed(&layout, |installed| installed.node = Some(record))?;
    Ok(runtime_at(&root))
}

async fn node_version(node: &std::path::Path, limit: Duration) -> Result<String, String> {
    let run = tokio::process::Command::new(node)
        .arg("--version")
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(limit, run)
        .await
        .map_err(|_| format!("did not answer --version within {limit:?}"))?
        .map_err(|e| format!("could not start ({e})"))?;
    if !output.status.success() {
        return Err(format!("exited with {} on --version", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(all(test, unix))]
mod tests {
    use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};

    use super::node_version;

    fn script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    #[tokio::test]
    async fn node_version_explains_each_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let limit = Duration::from_secs(5);
        let ok = script(dir.path(), "ok", "echo v24.21.0");
        assert_eq!(node_version(&ok, limit).await.as_deref(), Ok("v24.21.0"));

        let failing = script(dir.path(), "failing", "exit 3");
        let err = node_version(&failing, limit)
            .await
            .expect_err("non-zero exit");
        assert!(err.starts_with("exited with"), "{err}");

        let missing = dir.path().join("absent");
        let err = node_version(&missing, limit)
            .await
            .expect_err("missing binary");
        assert!(err.starts_with("could not start"), "{err}");

        let slow = script(dir.path(), "slow", "exec sleep 5");
        let err = node_version(&slow, Duration::from_millis(200))
            .await
            .expect_err("timeout");
        assert_eq!(err, "did not answer --version within 200ms");
    }
}
