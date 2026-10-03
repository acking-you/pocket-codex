//! `npm ci` into a staging prefix with the private Node (TRD §4.3.7, T8).

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use async_trait::async_trait;
use serde_json::json;
use tokio::io::AsyncReadExt;

use super::{super::error::AcpError, platform::Platform, store::Layout};

/// Default registry.
pub const DEFAULT_REGISTRY: &str = "https://registry.npmjs.org/";
/// `npm ci` time limit.
pub const NPM_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const LOG_TAIL: usize = 64 * 1024;

/// One npm invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NpmArgs {
    /// Private node.
    pub node: PathBuf,
    /// `npm-cli.js`.
    pub npm_cli: PathBuf,
    /// Working directory / prefix.
    pub prefix: PathBuf,
    /// Arguments after `npm-cli.js`.
    pub args: Vec<String>,
    /// Extra environment.
    pub env: BTreeMap<String, String>,
    /// Time limit.
    pub timeout: Duration,
}

/// Runs npm; replaced in tests.
#[async_trait]
pub trait NpmRunner: Send + Sync {
    /// Run `node npm-cli.js <args>`.
    async fn ci(&self, args: &NpmArgs) -> Result<(), AcpError>;
}

/// Runs the private npm as a process.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessNpm;

fn tail(bytes: &[u8]) -> String {
    let start = bytes.len().saturating_sub(LOG_TAIL);
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

#[async_trait]
impl NpmRunner for ProcessNpm {
    async fn ci(&self, args: &NpmArgs) -> Result<(), AcpError> {
        let mut command = tokio::process::Command::new(&args.node);
        command
            .arg(&args.npm_cli)
            .args(&args.args)
            .envs(&args.env)
            .current_dir(&args.prefix)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .map_err(|e| AcpError::NpmFailed(format!("starting npm: {e}")))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        async fn read_all(pipe: Option<impl tokio::io::AsyncRead + Unpin>) -> Vec<u8> {
            let mut out = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = AsyncReadExt::read_to_end(&mut pipe, &mut out).await;
            }
            out
        }
        let run = async { tokio::join!(read_all(stdout), read_all(stderr), child.wait()) };
        let (out, err, status) = tokio::time::timeout(args.timeout, run)
            .await
            .map_err(|_| AcpError::NpmFailed("npm timed out".into()))?;
        let status = status.map_err(|e| AcpError::NpmFailed(e.to_string()))?;
        if status.success() {
            Ok(())
        } else {
            Err(AcpError::NpmFailed(format!(
                "npm exited with {status}\n{}\n{}",
                tail(&out),
                tail(&err)
            )))
        }
    }
}

/// Write the staging `package.json` and `package-lock.json`.
pub fn write_package(
    dir: &Path,
    id: &str,
    package: &str,
    version: &str,
    lock: &str,
) -> Result<(), AcpError> {
    std::fs::create_dir_all(dir)?;
    let manifest = json!({
        "name": format!("pcx-{id}"),
        "private": true,
        "dependencies": { package: version }
    });
    std::fs::write(dir.join("package.json"), manifest.to_string())?;
    std::fs::write(dir.join("package-lock.json"), lock)?;
    Ok(())
}

/// `npm ci` arguments for `platform`.
pub fn ci_args(
    prefix: &Path,
    platform: &Platform,
    omit: &[String],
    registry: &str,
    layout: &Layout,
) -> Vec<String> {
    let mut args = vec![
        "ci".to_string(),
        "--prefix".into(),
        prefix.to_string_lossy().into_owned(),
        "--ignore-scripts".into(),
    ];
    args.extend(shared_args(platform, omit, registry, layout));
    args
}

/// Arguments shared by `npm ci` and the registry `npm install`.
pub fn shared_args(
    platform: &Platform,
    omit: &[String],
    registry: &str,
    layout: &Layout,
) -> Vec<String> {
    let mut args = vec![
        "--no-audit".to_string(),
        "--no-fund".into(),
        "--no-update-notifier".into(),
        format!("--os={}", platform.npm_os),
        format!("--cpu={}", platform.npm_cpu),
    ];
    if let Some(libc) = platform.libc {
        args.push(format!("--libc={libc}"));
    }
    for omit in omit {
        args.push(format!("--omit={omit}"));
    }
    args.push(format!("--registry={registry}"));
    args.push(format!("--cache={}", layout.npm_cache().to_string_lossy()));
    args.push(format!("--userconfig={}", layout.npmrc().to_string_lossy()));
    args
}

/// Environment for npm: the private node first on `PATH`.
pub fn npm_env(node: &Path) -> BTreeMap<String, String> {
    let dir = node.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut paths = vec![dir];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    let path = std::env::join_paths(paths)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    BTreeMap::from([
        ("PATH".to_string(), path),
        ("npm_config_update_notifier".to_string(), "false".to_string()),
    ])
}
