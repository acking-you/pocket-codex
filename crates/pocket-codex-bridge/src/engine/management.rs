//! Device management is a separate relay service. Its lifetime does not depend
//! on any engine. Only controllers in the already-authorized relay namespace
//! can reach it; remote launch requests cannot introduce an executable or argv.

use std::collections::BTreeSet;

use anyhow::{anyhow, Result};
use pocket_codex_core::provider::Secret;
use pocket_codex_host_svc::acp::AgentSpec;
use serde::{Deserialize, Serialize};

use super::{
    host_apply, host_lifecycle,
    host_profiles::{self, Change, KeyChange, ProfileInput, Runtime, RuntimeChange},
    host_start, runtime, serve,
};

#[path = "management_http.rs"]
mod http;
#[path = "management_server.rs"]
mod server;
pub(super) use http::router;
use http::{AdmissionToken, Fault};
pub use server::{enable_after_hosting, initialize, local_status, set_enabled, stop, LocalStatus};

/// The remote launch choices. Saved binaries belong to this computer; request
/// payloads never carry executable paths, arguments or custom agent programs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum RuntimeChoice {
    /// Use this computer's configured or detected Codex.
    Codex {
        /// App-server port, zero for automatic.
        #[serde(default)]
        port: u16,
        /// Optional validated upstream HTTP proxy.
        proxy: Option<String>,
        /// Retain a host-only proxy (including masked credentials) while
        /// editing another runtime field.
        #[serde(default, rename = "keepProxy")]
        keep_proxy: bool,
    },
    /// Use this computer's detected OpenCode.
    Opencode {},
    /// Use the built-in Claude preset, after explicit adapter preparation.
    Claude {},
    /// Reuse this name's previously saved runtime unchanged.
    Saved {},
}

/// Write-only change to a stored provider key.
#[derive(Serialize, Deserialize)]
#[serde(tag = "action", content = "value", rename_all = "lowercase")]
pub enum KeyInput {
    /// Preserve the current key.
    Keep,
    /// Remove the current key.
    Remove,
    /// Replace it; never returned by a management response.
    Replace(String),
}

/// Config editor submission, with optimistic concurrency for saves.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveRequest {
    /// Model provider configuration.
    pub input: ProfileInput,
    /// Explicit handling of the write-only key.
    pub key: KeyInput,
    /// The saved revision the controller reviewed.
    pub expected_revision: u64,
    /// Absent retains the saved runtime.
    pub runtime: Option<RuntimeChoice>,
    /// Keep a masked URL unchanged rather than saving its display placeholder.
    #[serde(default)]
    pub keep_base_url: bool,
}

/// Create one host. Existing names are refused by the shared lifecycle lease.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartRequest {
    /// Device-local instance name.
    pub name: String,
    /// Host-side runtime selection.
    pub runtime: RuntimeChoice,
    /// Model provider configuration.
    pub input: ProfileInput,
    /// Write-only key handling.
    pub key: KeyInput,
}

/// Apply exactly the saved revision to the observed running host.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyRequest {
    /// Observed running identity.
    pub expected_incarnation: String,
    /// Observed saved revision.
    pub expected_revision: u64,
}

/// Stop exactly the observed running host.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StopRequest {
    /// Observed running identity; a replacement is never stopped.
    pub expected_incarnation: String,
}

/// Saved configuration and current launch (provider credentials omitted).
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostView {
    /// Instance name.
    pub name: String,
    /// Sanitized saved provider and runtime configuration.
    pub config: host_profiles::HostProfile,
    /// The actual running incarnation.
    pub active: Option<host_apply::Active>,
    /// Runtime status and all tunnel details, when running.
    pub status: Option<serve::ServeStatus>,
    /// Automatic restart is recorded for this name.
    pub restore: bool,
    /// Restart persistence can fail independently of the running process.
    pub restore_problem: Option<String>,
    /// The displayed provider URL is masked; unchanged saves must retain it.
    pub base_url_masked: bool,
}

/// One computer, including stopped remotely managed services.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    /// Version of this management protocol.
    pub protocol: u32,
    /// Application build version.
    pub app_version: String,
    /// Host OS, never the controller's OS.
    pub os: String,
    /// Publisher identity and environment; absent on older hosts.
    #[serde(default)]
    pub metadata: Option<pocket_codex_core::device::DeviceMetadata>,
    /// Relay device identity.
    pub device: String,
    /// Managed services owned by this app only.
    pub hosts: Vec<HostView>,
    /// Safe diagnostic when automatic restore state is unavailable.
    pub restore_problem: Option<String>,
}

fn resolve_runtime(name: &str, choice: RuntimeChoice) -> Result<Runtime> {
    match choice {
        RuntimeChoice::Codex {
            port,
            proxy,
            keep_proxy,
        } => {
            let proxy = if keep_proxy {
                match host_profiles::get(name)?.runtime {
                    Some(Runtime::Codex {
                        proxy, ..
                    }) => proxy,
                    _ => return Err(Fault::InvalidConfiguration.into()),
                }
            } else {
                proxy
            };
            if let Some(proxy) = &proxy {
                pocket_codex_api_proxy::validate_proxy(proxy)?;
            }
            Ok(Runtime::Codex {
                binary: None,
                port,
                proxy,
            })
        },
        RuntimeChoice::Opencode {} => Ok(Runtime::OpenCode {
            binary: None,
            separate_history: separate_history(
                host_profiles::get(name)?.runtime.as_ref(),
                super::serve_opencode::shared_history_host().as_deref(),
                name,
            ),
        }),
        RuntimeChoice::Claude {} => {
            if !super::claude_setup::status()?.ready {
                return Err(Fault::PreparationRequired.into());
            }
            Ok(Runtime::Acp(AgentSpec {
                profile_id: "claude".into(),
                display_name: "Claude".into(),
                program: "claude-agent-acp".into(),
                args: Vec::new(),
            }))
        },
        RuntimeChoice::Saved {} => host_profiles::get(name)?
            .runtime
            .ok_or_else(|| anyhow!("no saved runtime for this host")),
    }
}

/// Where a remotely chosen OpenCode host keeps its sessions: as this name
/// saved, else in a database of its own while `shared_holder` (another host)
/// serves OpenCode's default one, which one host serves at a time.
fn separate_history(saved: Option<&Runtime>, shared_holder: Option<&str>, name: &str) -> bool {
    match saved {
        Some(Runtime::OpenCode {
            separate_history, ..
        }) => *separate_history,
        _ => shared_holder.is_some_and(|holder| !super::host_names::same(holder, name)),
    }
}

fn change(input: ProfileInput, key: KeyInput) -> Change {
    Change {
        input,
        key: match key {
            KeyInput::Keep => KeyChange::Keep,
            KeyInput::Remove => KeyChange::Remove,
            KeyInput::Replace(value) => KeyChange::Replace(Secret::new(value)),
        },
        runtime: RuntimeChange::Keep,
    }
}

/// Read one name, including a stopped host with saved configuration.
pub fn host(name: &str) -> Result<HostView> {
    let name = super::host_names::canonical(name)?;
    let mut config = display_profile(host_profiles::get(&name));
    let mut active = host_apply::active(&name);
    let mut status = serve::serve_status()
        .into_iter()
        .find(|status| super::host_names::same(&status.name, &name));
    let restore_problem = host_lifecycle::problem(Some(&name))?;
    let restore = restore_problem.is_none()
        && host_lifecycle::entries()
            .unwrap_or_default()
            .get(&name)
            .is_some_and(|e| e.runtime.is_some());
    let base_url_masked = mask_url(&mut config.base_url);
    if let Some(runtime) = &mut config.runtime {
        mask_runtime(runtime);
    }
    if let Some(active) = &mut active {
        mask_runtime(&mut active.runtime);
    }
    if let Some(status) = &mut status {
        status.codex_binary = None;
        mask_url(&mut status.proxy);
        if status.agent_error.is_some() {
            status.agent_error =
                Some("The agent needs attention; check the host diagnostics.".into());
        }
        if let Some(opencode) = &mut status.opencode {
            if opencode.problem.is_some() {
                opencode.problem =
                    Some("OpenCode needs attention; check the host diagnostics.".into());
            }
        }
    }
    for (name, value) in &mut config.claude_env {
        if name.ends_with("_URL") && mask_url(value) {
            // Null already means "keep this stored override". Never send a
            // shortened URL that an unchanged editor could save destructively.
            *value = None;
        }
    }
    Ok(HostView {
        name,
        config,
        active,
        status,
        restore,
        restore_problem,
        base_url_masked,
    })
}

fn display_profile(result: Result<host_profiles::HostProfile>) -> host_profiles::HostProfile {
    let mut config = result.unwrap_or_else(|error| {
        tracing::warn!(%error, "saved host configuration could not be read");
        host_profiles::HostProfile {
            source: host_profiles::Source::Inherited,
            protocol: None,
            base_url: None,
            model: None,
            key_set: false,
            claude_env: Default::default(),
            runtime: None,
            revision: 0,
            problem: Some(String::new()),
        }
    });
    if config.problem.is_some() {
        config.problem =
            Some("Saved configuration is unavailable or invalid; review it on the host.".into());
    }
    config
}

fn mask_url(value: &mut Option<String>) -> bool {
    let Some(raw) = value else { return false };
    let Ok(mut url) = reqwest::Url::parse(raw) else {
        *value = None;
        return true;
    };
    let masked = !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some();
    if masked {
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.set_query(None);
        url.set_fragment(None);
        *raw = url.into();
    }
    masked
}

fn mask_runtime(runtime: &mut Runtime) {
    match runtime {
        Runtime::Codex {
            binary,
            proxy,
            ..
        } => {
            *binary = None;
            mask_url(proxy);
        },
        // Remote clients select a saved launch without receiving its command.
        Runtime::Acp(spec) => {
            spec.program.clear();
            spec.args.clear();
        },
        Runtime::OpenCode {
            binary, ..
        } => *binary = None,
    }
}

/// Snapshot this app's managed hosts without probing or adopting other apps.
pub fn device() -> Result<DeviceView> {
    let names: BTreeSet<_> = serve::serve_status()
        .into_iter()
        .filter_map(|s| super::host_names::canonical(&s.name).ok())
        .chain(host_lifecycle::entries().unwrap_or_default().into_keys())
        .collect();
    Ok(DeviceView {
        protocol: 1,
        app_version: env!("CARGO_PKG_VERSION").into(),
        os: std::env::consts::OS.into(),
        metadata: Some(pocket_codex_core::device::DeviceMetadata::current()),
        device: runtime::device_id(),
        hosts: names.iter().map(|name| host(name)).collect::<Result<_>>()?,
        restore_problem: host_lifecycle::problem(None)?,
    })
}

/// Save a remote edit without restarting the host.
fn save(name: &str, mut request: SaveRequest, token: AdmissionToken) -> Result<HostView> {
    let saved = host_profiles::get(name)?;
    if request.keep_base_url {
        request.input.base_url = saved.base_url.clone();
    }
    let mut change = change(request.input, request.key);
    let desired = match request.runtime {
        Some(choice) => resolve_runtime(name, choice)?,
        None => saved
            .runtime
            .clone()
            .ok_or_else(|| anyhow!("choose a runtime"))?,
    };
    change.runtime = RuntimeChange::Set(desired.clone());
    let (_, staged) = host_profiles::stage_checked_in(
        &host_profiles::root()?,
        name,
        &change,
        Some(request.expected_revision),
        desired.engine(),
        &|| token.check(),
    )?;
    staged.finish();
    host(name)
}

/// Start a host from the restricted runtime choices.
fn start(request: StartRequest, token: AdmissionToken) -> Result<HostView> {
    start_revision(request, 0, token)
}

/// Starting an existing stopped service reads private fields on the host,
/// rather than resubmitting a redacted editor snapshot.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartSavedRequest {
    /// The saved revision the controller reviewed.
    pub expected_revision: u64,
}

fn start_saved(name: &str, request: StartSavedRequest, token: AdmissionToken) -> Result<HostView> {
    let saved = host_profiles::get(name)?;
    if saved.revision != request.expected_revision {
        return Err(host_profiles::RevisionConflict {
            current: saved.revision,
        }
        .into());
    }
    if saved.problem.is_some() || saved.runtime.is_none() {
        return Err(Fault::InvalidConfiguration.into());
    }
    start_revision(
        StartRequest {
            name: name.into(),
            runtime: RuntimeChoice::Saved {},
            key: KeyInput::Keep,
            input: ProfileInput {
                source: saved.source,
                protocol: saved.protocol,
                base_url: saved.base_url,
                model: saved.model,
                claude_env: None,
            },
        },
        request.expected_revision,
        token,
    )
}

fn start_revision(
    mut request: StartRequest,
    expected: u64,
    token: AdmissionToken,
) -> Result<HostView> {
    request.name =
        super::host_names::canonical(&request.name).map_err(|_| Fault::InvalidConfiguration)?;
    if request.input.source == host_profiles::Source::Official {
        runtime::runtime()
            .block_on(pocket_codex_api_proxy::check_auth())
            .map_err(|_| Fault::AuthenticationRequired)?;
    }
    let runtime = resolve_runtime(&request.name, request.runtime)?;
    host_start::start_configured_checked(
        Some(&request.name),
        runtime,
        change(request.input, request.key),
        Some(expected),
        &|| {
            token.check()?;
            if host_apply::active(&request.name).is_some() {
                return Err(Fault::AlreadyRunning.into());
            }
            Ok(())
        },
    )?;
    host(&request.name)
}

/// Apply a reviewed revision and incarnation using the ordinary lifecycle
/// lease.
fn apply(
    name: &str,
    request: ApplyRequest,
    token: AdmissionToken,
) -> Result<host_apply::ApplyReport> {
    if request.expected_incarnation.is_empty() {
        return Err(Fault::IncarnationChanged.into());
    }
    let mut report = host_apply::apply_checked(
        name,
        &host_apply::Expected {
            incarnation: Some(request.expected_incarnation.clone()),
            revision: Some(request.expected_revision),
        },
        &|| {
            token.check()?;
            require_incarnation(name, &request.expected_incarnation)
        },
    )?;
    // Provider/child output can include URL credentials not covered by API-key
    // redaction. The host keeps diagnostics; remote UI gets the actual outcome.
    if report.error.is_some() {
        report.error = Some("The new configuration could not start on the host.".into());
    }
    if report.restore_error.is_some() {
        report.restore_error =
            Some("The previous configuration could not restart. Check the host.".into());
    }
    Ok(report)
}

/// Stop only the reviewed incarnation and suppress its next automatic restore.
fn stop_host(name: &str, request: StopRequest, token: AdmissionToken) -> Result<()> {
    if request.expected_incarnation.is_empty() {
        return Err(Fault::IncarnationChanged.into());
    }
    serve::serve_stop_remembered_checked(name, Some(&request.expected_incarnation), &|| {
        token.check()?;
        require_incarnation(name, &request.expected_incarnation)
    })
}

fn require_incarnation(name: &str, expected: &str) -> Result<()> {
    match host_apply::active(name) {
        None => Err(Fault::HostStopped.into()),
        Some(active) if active.incarnation != expected => Err(Fault::IncarnationChanged.into()),
        Some(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreadable_profiles_leave_runtime_status_available_without_leaking_errors() {
        let root = tempfile::tempdir().expect("temp");
        std::fs::write(root.path().join("profiles.json"), b"broken secret")
            .expect("corrupt profile");
        let profile = display_profile(host_profiles::get_in(root.path(), "running"));
        assert!(profile.problem.is_some());
        assert!(profile.runtime.is_none());
        assert!(!serde_json::to_string(&profile)
            .expect("JSON")
            .contains("secret"));
        assert_eq!(
            std::fs::read(root.path().join("profiles.json")).expect("unchanged"),
            b"broken secret"
        );
    }

    #[test]
    fn credential_urls_and_local_command_arguments_are_not_returned() {
        let mut url = Some("https://user:secret@example.org/v1?key=secret".into());
        assert!(mask_url(&mut url));
        assert_eq!(url.as_deref(), Some("https://example.org/v1"));
        let mut invalid = Some("https://secret@[invalid".into());
        assert!(mask_url(&mut invalid));
        assert!(invalid.is_none());
        for mut runtime in [
            Runtime::Acp(AgentSpec {
                profile_id: "custom".into(),
                display_name: "Custom".into(),
                program: "/private/secret/agent".into(),
                args: vec!["--token=secret".into()],
            }),
            Runtime::Codex {
                binary: Some("/private/secret/codex".into()),
                port: 1234,
                proxy: Some("https://secret@example.org".into()),
            },
            Runtime::OpenCode {
                binary: Some("/private/secret/opencode".into()),
                separate_history: false,
            },
        ] {
            mask_runtime(&mut runtime);
            assert!(!serde_json::to_string(&runtime)
                .expect("JSON")
                .contains("secret"));
        }
    }

    #[test]
    fn a_remote_opencode_host_keeps_its_history_or_avoids_a_shared_one() {
        let shared = Runtime::OpenCode {
            binary: None,
            separate_history: false,
        };
        let own = Runtime::OpenCode {
            binary: None,
            separate_history: true,
        };
        // A new name beside a host on OpenCode's default database gets its own.
        assert!(separate_history(None, Some("opencode"), "work"));
        assert!(!separate_history(None, None, "work"));
        // The holder itself, under any spelling, is not "another" host.
        assert!(!separate_history(None, Some("my-host"), "My Host"));
        // An edit keeps what the name saved.
        assert!(!separate_history(Some(&shared), Some("other"), "work"));
        assert!(separate_history(Some(&own), None, "work"));
    }

    #[test]
    fn runtime_payloads_cannot_inject_executables_or_arguments() {
        for value in [
            serde_json::json!({"kind":"opencode","binary":"/tmp/script"}),
            serde_json::json!({"kind":"claude","args":["--eval", "bad"]}),
            serde_json::json!({"kind":"codex","binary":"/tmp/script"}),
        ] {
            assert!(serde_json::from_value::<RuntimeChoice>(value).is_err());
        }
        assert!(
            serde_json::from_value::<RuntimeChoice>(serde_json::json!({"kind":"opencode"})).is_ok()
        );
    }
}
