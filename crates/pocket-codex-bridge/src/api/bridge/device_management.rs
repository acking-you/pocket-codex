//! Device management and durable local restore choices.

use super::*;
use crate::engine::{management, management_client as remote};

/// Whether other authorized controllers can manage this computer.
pub struct DeviceManagementDto {
    /// This platform can host services.
    pub supported: bool,
    /// Local opt-in; cannot be changed remotely.
    pub enabled: bool,
    /// Relay registration is currently live.
    pub registered: bool,
    /// Stable device management key, if started.
    pub service_key: Option<String>,
    /// Safe startup or registration failure.
    pub problem: Option<String>,
}

impl From<management::LocalStatus> for DeviceManagementDto {
    fn from(value: management::LocalStatus) -> Self {
        Self {
            supported: value.supported,
            enabled: value.enabled,
            registered: value.registered,
            service_key: value.service_key,
            problem: value.problem,
        }
    }
}

/// Read this device's management setting and reachability.
pub fn device_management_status() -> Result<DeviceManagementDto> {
    Ok(management::local_status()?.into())
}

/// Enable or disable remote management on this computer only.
pub fn device_management_set_enabled(enabled: bool) -> Result<DeviceManagementDto> {
    Ok(management::set_enabled(enabled)?.into())
}

/// Authoritative override to old Flutter auto-host preferences.
pub struct HostRestoreOverrideDto {
    /// Canonical instance name.
    pub name: String,
    /// Monotonic revision for UI reconciliation.
    pub revision: u64,
    /// Desired runtime; None means suppress/forget the old restore entry.
    pub runtime: Option<HostRuntimeDto>,
}

/// Apply these records before planning automatic restores. On failure, stop
/// restoring and show the error; never restore stale controller preferences.
pub fn host_restore_overrides() -> Result<Vec<HostRestoreOverrideDto>> {
    Ok(crate::engine::host_lifecycle::entries()?
        .into_iter()
        .map(|(name, entry)| HostRestoreOverrideDto {
            name,
            revision: entry.revision,
            runtime: entry.runtime.map(HostRuntimeDto::of),
        })
        .collect())
}

/// One host owned by the selected remote computer's Pocket app.
pub struct ManagedHostDto {
    /// Canonical instance name.
    pub name: String,
    /// Saved configuration and current launch; keys are never returned.
    pub config: HostConfigDto,
    /// Live status, absent when stopped.
    pub status: Option<AppServeStatusDto>,
    /// Restored on the host's next launch.
    pub restore: bool,
    /// Warning distinct from a successful process operation.
    pub restore_problem: Option<String>,
    /// Query credentials are hidden; retain the original URL on unchanged save.
    pub base_url_masked: bool,
}

impl From<management::HostView> for ManagedHostDto {
    fn from(value: management::HostView) -> Self {
        let active = value.active.map(|active| HostActiveDto {
            incarnation: active.incarnation,
            runtime: HostRuntimeDto::of(active.runtime),
            source: active.source.as_str().into(),
            revision: active.revision,
        });
        Self {
            name: value.name,
            config: host_config::dto(value.config, active),
            status: value.status.map(Into::into),
            restore: value.restore,
            restore_problem: value.restore_problem,
            base_url_masked: value.base_url_masked,
        }
    }
}

/// Device-level management remains reachable when its host list is empty.
pub struct ManagedDeviceDto {
    /// Relay device identity.
    pub device: String,
    /// OS of the host computer.
    pub os: String,
    /// Publisher identity and environment, absent on older hosts.
    pub metadata: Option<device_metadata::DeviceMetadataDto>,
    /// Pocket app version on the host.
    pub app_version: String,
    /// Its own running and stopped services.
    pub hosts: Vec<ManagedHostDto>,
    /// Automatic restore state needs attention on this host.
    pub restore_problem: Option<String>,
}

/// Read a remote management service discovered under kind `host`.
pub fn managed_device(service_key: String) -> Result<ManagedDeviceDto> {
    let value = remote::device(&service_key)?;
    Ok(ManagedDeviceDto {
        device: value.device,
        os: value.os,
        metadata: value
            .metadata
            .filter(|metadata| metadata.protocol == 1)
            .map(Into::into),
        app_version: value.app_version,
        hosts: value.hosts.into_iter().map(Into::into).collect(),
        restore_problem: value.restore_problem,
    })
}

/// Read one remote host's configuration and live status.
pub fn managed_host(service_key: String, name: String) -> Result<ManagedHostDto> {
    Ok(remote::host(&service_key, &name)?.into())
}

/// Restricted host-side runtime selection. No path/argv can cross this API.
pub struct ManagedRuntimeDto {
    /// `codex`, `opencode`, `claude`, or `saved`.
    pub kind: String,
    /// Codex port, zero for automatic.
    pub port: u16,
    /// Codex proxy; absent inherits the host's environment.
    pub proxy: Option<String>,
    /// Keep the host's stored proxy when changing its port.
    pub keep_proxy: bool,
}

impl ManagedRuntimeDto {
    fn choice(self) -> Result<management::RuntimeChoice> {
        if self.kind != "codex" && (self.port != 0 || self.proxy.is_some() || self.keep_proxy) {
            anyhow::bail!("only Codex accepts a port or proxy");
        }
        Ok(match self.kind.as_str() {
            "codex" => management::RuntimeChoice::Codex {
                port: self.port,
                proxy: self.proxy,
                keep_proxy: self.keep_proxy,
            },
            "opencode" => management::RuntimeChoice::Opencode {},
            "claude" => management::RuntimeChoice::Claude {},
            "saved" => management::RuntimeChoice::Saved {},
            _ => anyhow::bail!("unknown runtime choice"),
        })
    }
}

fn provider(
    input: HostConfigInputDto,
    action: String,
    key: Option<String>,
) -> Result<(crate::engine::host_profiles::ProfileInput, management::KeyInput)> {
    let change = host_config::change_of(input, &action, key)?;
    let key = match change.key {
        crate::engine::host_profiles::KeyChange::Keep => management::KeyInput::Keep,
        crate::engine::host_profiles::KeyChange::Remove => management::KeyInput::Remove,
        crate::engine::host_profiles::KeyChange::Replace(value) => {
            management::KeyInput::Replace(value.expose().into())
        },
    };
    Ok((change.input, key))
}

/// Create one remote service; a lost response requires status refresh, not
/// auto-retry.
pub fn managed_host_start(
    service_key: String,
    name: String,
    runtime: ManagedRuntimeDto,
    input: HostConfigInputDto,
    key_action: String,
    key: Option<String>,
) -> Result<ManagedHostDto> {
    let (input, key) = provider(input, key_action, key)?;
    Ok(remote::start(&service_key, &management::StartRequest {
        name,
        runtime: runtime.choice()?,
        input,
        key,
    })?
    .into())
}

/// A complete remote edit of one reviewed configuration revision.
pub struct ManagedHostSaveDto {
    /// Model service and Claude overrides.
    pub input: HostConfigInputDto,
    /// `keep`, `remove`, or `replace` the write-only key.
    pub key_action: String,
    /// Replacement key, only with `replace`.
    pub key: Option<String>,
    /// Revision read before editing.
    pub expected_revision: u64,
    /// Absent retains the saved runtime, including private proxy parameters.
    pub runtime: Option<ManagedRuntimeDto>,
    /// Keep the original provider URL when its display value was masked.
    pub keep_base_url: bool,
}

/// Save with optimistic concurrency, leaving the live service unchanged.
pub fn managed_host_save(
    service_key: String,
    name: String,
    edit: ManagedHostSaveDto,
) -> Result<ManagedHostDto> {
    let (input, key) = provider(edit.input, edit.key_action, edit.key)?;
    Ok(remote::save(&service_key, &name, &management::SaveRequest {
        input,
        key,
        expected_revision: edit.expected_revision,
        runtime: edit.runtime.map(ManagedRuntimeDto::choice).transpose()?,
        keep_base_url: edit.keep_base_url,
    })?
    .into())
}

/// Apply the exact revision and running incarnation reviewed in the editor.
pub fn managed_host_apply(
    service_key: String,
    name: String,
    expected_incarnation: String,
    expected_revision: u64,
) -> Result<HostApplyDto> {
    let value = remote::apply(&service_key, &name, &management::ApplyRequest {
        expected_incarnation,
        expected_revision,
    })?;
    Ok(HostApplyDto {
        name: value.name,
        applied: value.applied,
        restored_previous: value.restored_previous,
        error: value.error,
        restore_error: value.restore_error,
        revision: value.revision,
    })
}

/// Stop the exact observed instance; the computer's management entry remains.
pub fn managed_host_stop(
    service_key: String,
    name: String,
    expected_incarnation: String,
) -> Result<()> {
    remote::stop(&service_key, &name, &management::StopRequest {
        expected_incarnation,
    })
}

/// Available runtime on a managed computer, independent of this controller OS.
pub struct ManagedRuntimeAvailabilityDto {
    /// `codex`, `opencode`, or `claude`.
    pub kind: String,
    /// Native runtime/adapter can start, subject to provider authentication.
    pub ready: bool,
    /// Detected host executable, read-only.
    pub path: Option<String>,
    /// Version when detection reports one.
    pub version: Option<String>,
    /// Inherited provider needs host sign-in; custom configuration remains
    /// possible.
    pub needs_host_login: bool,
    /// Explicit preparation of the built-in adapter is available.
    pub can_prepare: bool,
    /// Safe unavailable-state explanation.
    pub problem: Option<String>,
}

/// Discover installed runtimes; never installs or signs in automatically.
pub fn managed_runtimes(service_key: String) -> Result<Vec<ManagedRuntimeAvailabilityDto>> {
    Ok(remote::runtimes(&service_key)?
        .into_iter()
        .map(|value| ManagedRuntimeAvailabilityDto {
            kind: value.kind,
            ready: value.ready,
            path: value.path,
            version: value.version,
            needs_host_login: value.needs_host_login,
            can_prepare: value.can_prepare,
            problem: value.problem,
        })
        .collect())
}

/// Explicitly prepare the host's pinned Claude adapter, or read its progress.
/// Only set `start` after the user's Prepare action, never during discovery.
pub fn managed_claude_prepare(
    service_key: String,
    start: bool,
) -> Result<hosting::AcpClaudeSetupDto> {
    let value = remote::claude_prepare(&service_key, start)?;
    Ok(hosting::AcpClaudeSetupDto {
        state: value.state,
        stage: value.stage,
        elapsed_ms: value.elapsed_ms,
        downloaded_bytes: value.downloaded_bytes,
        download_bytes_per_second: value.download_bytes_per_second,
        error: value.error,
        run: value.run,
        started: value.started,
    })
}

/// Start a stopped service from the exact reviewed saved revision. Retains
/// host-only credentials, URL parameters, runtime paths and Claude overrides.
pub fn managed_host_start_saved(
    service_key: String,
    name: String,
    expected_revision: u64,
) -> Result<ManagedHostDto> {
    Ok(remote::start_saved(&service_key, &name, expected_revision)?.into())
}

/// Restore only the boot-time choice still current under the host lease.
/// For an old UI preference with no override, pass no revision and its local
/// runtime. A later remote stop takes precedence even while this call waits.
pub fn host_restore_service(
    name: String,
    expected_revision: Option<u64>,
    legacy_runtime: Option<HostRuntimeDto>,
) -> Result<()> {
    crate::engine::host_lifecycle::restore(
        &name,
        expected_revision,
        legacy_runtime
            .map(HostRuntimeDto::into_runtime)
            .transpose()?,
    )
}

/// Read a restart-persistence warning without hiding a successful start/apply.
/// None selects any warning on this computer; a name limits it to that host.
pub fn host_restore_problem(name: Option<String>) -> Result<Option<String>> {
    crate::engine::host_lifecycle::problem(name.as_deref())
}
