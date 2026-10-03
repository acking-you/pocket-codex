//! What to launch ([`LaunchSpec`]) and how ([`AgentConnector`]).
//!
//! The production [`ProcessConnector`] spawns the agent as its own process
//! group (npm-based agents spawn children such as `codex app-server`) so
//! [`ChildHandle::terminate`] can end the whole tree.

use std::{collections::BTreeMap, path::PathBuf, process::Stdio, time::Duration};

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};

use super::error::AcpError;

/// How to start one agent process.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchSpec {
    /// Catalog id or custom agent id.
    pub agent_id: String,
    /// Display name.
    pub display_name: String,
    /// Absolute path.
    pub program: PathBuf,
    /// Arguments.
    pub args: Vec<String>,
    /// Added on top of the App environment.
    pub env: BTreeMap<String, String>,
    /// Removed from the App environment.
    pub env_remove: Vec<String>,
    /// Installed version.
    pub version: Option<String>,
    /// True when the catalog pins this exact version.
    pub pinned: bool,
    /// Working directory of the agent process; `None` means the user's home.
    pub cwd: Option<PathBuf>,
    /// Appended after `args` only when launching the ACP agent (e.g. D21's
    /// `--hide-claude-auth`); never passed to terminal-login commands.
    pub launch_only_args: Vec<String>,
}

/// Connected agent stdio.
pub struct AgentIo {
    /// Agent stdout.
    pub reader: Box<dyn AsyncRead + Send + Unpin>,
    /// Agent stdin.
    pub writer: Box<dyn AsyncWrite + Send + Unpin>,
    /// Agent stderr.
    pub stderr: Option<Box<dyn AsyncRead + Send + Unpin>>,
    /// The process, when there is one.
    pub child: Option<ChildHandle>,
}

/// Starts an agent and returns its stdio.
#[async_trait]
pub trait AgentConnector: Send + Sync {
    /// Start the agent described by `spec`.
    async fn connect(&self, spec: &LaunchSpec) -> Result<AgentIo, AcpError>;
}

/// Owns the `tokio::process::Child` and, on Unix, its process-group id.
pub struct ChildHandle {
    child: tokio::process::Child,
    #[cfg_attr(not(unix), allow(dead_code, reason = "process groups are Unix-only"))]
    pgid: Option<i32>,
}

impl ChildHandle {
    /// Process id.
    pub fn pid(&self) -> Option<u32> {
        self.child.id()
    }

    /// Wait `grace` for the process to exit on its own (the caller has
    /// already closed stdin), then terminate the whole process tree.
    pub async fn terminate(mut self, grace: Duration) {
        let _ = tokio::time::timeout(grace, self.child.wait()).await;
        self.kill_tree().await;
    }

    #[cfg(unix)]
    async fn kill_tree(&mut self) {
        use nix::{
            sys::signal::{killpg, Signal},
            unistd::Pid,
        };
        let Some(pgid) = self.pgid else {
            let _ = self.child.kill().await;
            return;
        };
        let group = Pid::from_raw(pgid);
        let _ = killpg(group, Signal::SIGTERM);
        let _ = tokio::time::timeout(Duration::from_secs(2), self.child.wait()).await;
        let _ = killpg(group, Signal::SIGKILL);
        let _ = self.child.wait().await;
    }

    #[cfg(not(unix))]
    async fn kill_tree(&mut self) {
        if let Some(pid) = self.child.id() {
            let _ = tokio::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
        let _ = self.child.kill().await;
    }
}

/// Spawns the agent as a local process.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessConnector;

/// The user's home directory, or the temp directory.
pub(crate) fn default_cwd() -> PathBuf {
    let home = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(home)
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

#[async_trait]
impl AgentConnector for ProcessConnector {
    async fn connect(&self, spec: &LaunchSpec) -> Result<AgentIo, AcpError> {
        let mut command = tokio::process::Command::new(&spec.program);
        command
            .args(&spec.args)
            .args(&spec.launch_only_args)
            .envs(&spec.env);
        for name in &spec.env_remove {
            command.env_remove(name);
        }
        command
            .current_dir(spec.cwd.clone().unwrap_or_else(default_cwd))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        let mut child = command.spawn().map_err(|e| AcpError::AgentStartFailed {
            message: format!("starting {}: {e}", spec.program.display()),
            stderr_tail: String::new(),
        })?;
        let missing = |what: &str| AcpError::AgentStartFailed {
            message: format!("the agent process has no {what} pipe"),
            stderr_tail: String::new(),
        };
        let reader = child.stdout.take().ok_or_else(|| missing("stdout"))?;
        let writer = child.stdin.take().ok_or_else(|| missing("stdin"))?;
        let stderr = child.stderr.take();
        let pgid = child.id().and_then(|pid| i32::try_from(pid).ok());
        Ok(AgentIo {
            reader: Box::new(reader),
            writer: Box::new(writer),
            stderr: stderr.map(|s| Box::new(s) as Box<dyn AsyncRead + Send + Unpin>),
            child: Some(ChildHandle {
                child,
                pgid,
            }),
        })
    }
}
