//! Native foreground hosting shared by CLI and desktop management.

use std::{
    fs::File,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use host_profiles::files::{write_private, Committed};
use pocket_codex_core::process::process_identity;
use pocket_codex_pb::{RelaySession, Transport};
use serde::{Deserialize, Serialize};

use super::{host_lifecycle, host_profiles, management, runtime, serve};

/// Startup configuration for one native managed-host supervisor.
pub struct Options {
    /// Directory containing the existing CLI account/relay `config.toml`.
    pub config_dir: PathBuf,
    /// Private daemon state, independent of desktop restore preferences.
    pub state_dir: PathBuf,
    /// Stable CLI installation identity, shared with its standalone workers.
    pub identity_dir: PathBuf,
    /// Device identity used by every service and the management entry.
    pub device: String,
    /// Initial Codex instance, used only without a saved lifecycle choice.
    pub name: String,
    /// External Codex executable for the initial instance.
    pub binary: Option<String>,
    /// Initial Codex app-server's loopback port.
    pub port: u16,
    /// Initial instance's upstream proxy.
    pub proxy: Option<String>,
    /// Explicit self-host relay; otherwise use the saved transport and renewal.
    pub relay: Option<RelaySession>,
}

/// Safe local status for one hosted service.
#[derive(Serialize, Deserialize)]
pub struct ServiceStatus {
    /// Instance name.
    pub name: String,
    /// Owned agent PID, if available.
    pub pid: Option<u32>,
    /// Agent is responding.
    pub alive: bool,
    /// Loopback endpoint.
    pub endpoint: String,
    /// Relay service key.
    pub key: String,
}

/// Private, identity-checked daemon status; contains no launch secrets.
#[derive(Serialize, Deserialize)]
pub struct Status {
    /// Supervisor PID.
    pub pid: u32,
    /// Supervisor process start time, for rejecting reused PIDs.
    pub started: u64,
    /// Management permission and relay reachability.
    pub management: management::LocalStatus,
    /// Services owned by this supervisor.
    pub services: Vec<ServiceStatus>,
}

/// One foreground owner of all services created through its management entry.
pub struct Manager {
    state_dir: PathBuf,
    started: u64,
    stopped: bool,
    _lock: File,
}

fn lock(state: &Path) -> Result<File> {
    std::fs::create_dir_all(state).context("creating managed-host state")?;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        std::fs::set_permissions(state, std::fs::Permissions::from_mode(0o700))?;
        options.mode(0o600);
    }
    let file = options.open(state.join("owner.lock"))?;
    file.try_lock()
        .context("another managed-host supervisor owns this state directory")?;
    Ok(file)
}

impl Manager {
    /// Restore saved services and enable the existing device-management
    /// protocol. Call on a blocking thread; shutdown owns only this
    /// process's engines.
    pub fn start(options: Options) -> Result<Self> {
        let name = super::host_names::canonical(&options.name)?;
        let lock = lock(&options.state_dir)?;
        let identity =
            process_identity(std::process::id()).context("reading supervisor identity")?;
        runtime::configure_host(runtime::HostContext {
            device: pocket_codex_core::service::sanitize_component(&options.device),
            profiles: options.state_dir.join("hosting"),
            identity_dir: options.identity_dir,
            transport: options.relay.map(|session| Transport {
                session,
                namespace: None,
            }),
        })?;
        let _ = rustls::crypto::ring::default_provider().install_default();
        runtime::init(options.config_dir)?;
        let mut manager = Self {
            state_dir: options.state_dir,
            started: identity.started,
            stopped: false,
            _lock: lock,
        };
        let restored = (|| -> Result<()> {
            let entries = host_lifecycle::entries()?;
            for (name, entry) in &entries {
                if entry.runtime.is_some() {
                    host_lifecycle::restore(name, Some(entry.revision), None)
                        .with_context(|| format!("restoring managed service `{name}`"))?;
                }
            }
            if !entries.contains_key(&name) {
                host_lifecycle::restore(
                    &name,
                    None,
                    Some(host_profiles::Runtime::Codex {
                        binary: options.binary,
                        port: options.port,
                        proxy: options.proxy,
                    }),
                )?;
            }
            management::set_enabled(true)?;
            manager.write_status()?;
            Ok(())
        })();
        if let Err(error) = restored {
            if let Err(stop) = manager.stop() {
                tracing::error!(%stop, "managed-host startup cleanup failed");
            }
            return Err(error);
        }
        Ok(manager)
    }

    /// Refresh the private status file without exporting credentials or argv.
    pub fn write_status(&self) -> Result<Status> {
        let status = Status {
            pid: std::process::id(),
            started: self.started,
            management: management::local_status()?,
            services: serve::serve_status()
                .into_iter()
                .map(|host| ServiceStatus {
                    name: host.name,
                    pid: host.pid,
                    alive: host.alive,
                    endpoint: host.app_listen_addr,
                    key: host.app_service_key,
                })
                .collect(),
        };
        if write_private(&self.state_dir.join("status.json"), &serde_json::to_vec(&status)?)?
            != Committed::Durable
        {
            bail!("managed-host status could not be durably saved");
        }
        Ok(status)
    }

    /// Withdraw management and stop owned services without changing restore
    /// choices.
    pub fn stop(&mut self) -> Result<()> {
        if !self.stopped {
            serve::serve_stop_all()?;
            self.stopped = true;
            match std::fs::remove_file(self.state_dir.join("status.json")) {
                Ok(()) => {},
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                Err(error) => return Err(error).context("removing managed-host status"),
            }
        }
        Ok(())
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            tracing::error!(%error, "managed-host shutdown failed");
        }
    }
}

/// Read fresh status only when the original supervisor still owns its PID.
pub fn read_status(state_dir: &Path) -> Result<Option<Status>> {
    let path = state_dir.join("status.json");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("reading managed-host status"),
    };
    if std::fs::metadata(&path)?
        .modified()?
        .elapsed()
        .map_or(true, |age| age > std::time::Duration::from_secs(15))
    {
        return Ok(None);
    }
    let status: Status = serde_json::from_slice(&bytes).context("reading managed-host status")?;
    let current = std::env::current_exe()?.canonicalize()?;
    Ok(process_identity(status.pid)
        .filter(|owner| owner.started == status.started && owner.executable == current)
        .map(|_| status))
}
