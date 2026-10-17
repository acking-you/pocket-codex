//! Publisher metadata. Relay device names are labels, not proof of locality.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

mod identity;
pub use identity::HostIdentity;

static HOST: OnceLock<HostIdentity> = OnceLock::new();

/// Initialize a hosting installation before publishing any of its services.
/// All services of this process must use the same private identity directory.
pub fn initialize(root: &std::path::Path) -> anyhow::Result<&'static HostIdentity> {
    if let Some(value) = HOST.get() {
        anyhow::ensure!(value.root == root, "host identity is already initialized elsewhere");
        return Ok(value);
    }
    let value = HostIdentity::load_or_create(root, &DeviceMetadata::current())?;
    let _ = HOST.set(value);
    let value = HOST
        .get()
        .ok_or_else(|| anyhow::anyhow!("host identity initialization failed"))?;
    anyhow::ensure!(value.root == root, "host identity is already initialized elsewhere");
    Ok(value)
}

/// Initialized hosting identity, if this process has a hosting installation.
pub fn host_identity() -> Option<&'static HostIdentity> {
    HOST.get()
}

/// The environment that actually owns a registered service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceMetadata {
    /// Metadata schema version, independent of the management protocol.
    pub protocol: u32,
    /// OS of the publisher executable (`windows`, `linux`, `macos`, etc.).
    pub os: String,
    /// Architecture of the publisher executable.
    pub arch: String,
    /// Runtime environment (`wsl` on Windows Subsystem for Linux).
    pub environment: Option<String>,
    /// Display hostname, which may be shared by independent environments.
    pub hostname: String,
    /// Stable identity of the hosting installation, absent on older hosts.
    #[serde(default)]
    pub host_id: Option<String>,
    /// OS distribution or WSL distribution name, when available.
    #[serde(default)]
    pub source_name: Option<String>,
    /// Opaque identity of this publisher process; renewed on restart.
    ///
    /// Correlates this process's service responses. Local ownership still
    /// comes from the native host registry; metadata never grants access.
    pub instance_id: String,
}

impl DeviceMetadata {
    /// Metadata shared by every service owned by this process.
    pub fn current() -> Self {
        static VALUE: OnceLock<DeviceMetadata> = OnceLock::new();
        let mut value = VALUE
            .get_or_init(|| {
                let os = std::env::consts::OS;
                let environment = environment(
                    os,
                    &std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default(),
                );
                let release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
                let distro = std::env::var("WSL_DISTRO_NAME").ok();
                Self {
                    protocol: 1,
                    os: std::env::consts::OS.into(),
                    arch: std::env::consts::ARCH.into(),
                    source_name: source_name(
                        os,
                        environment.as_deref(),
                        distro.as_deref(),
                        &release,
                    ),
                    environment,
                    hostname: sysinfo::System::host_name().unwrap_or_default(),
                    host_id: None,
                    instance_id: uuid::Uuid::new_v4().to_string(),
                }
            })
            .clone();
        value.host_id = HOST.get().map(|identity| identity.id().to_string());
        value
    }
}

fn source_name(
    os: &str,
    environment: Option<&str>,
    distro: Option<&str>,
    release: &str,
) -> Option<String> {
    if os != "linux" {
        return None;
    }
    if environment == Some("wsl") {
        if let Some(name) = distro.filter(|name| !name.trim().is_empty()) {
            return Some(name.trim().chars().take(120).collect());
        }
    }
    release
        .lines()
        .find_map(|line| {
            line.strip_prefix("PRETTY_NAME=").map(|name| {
                name.trim()
                    .trim_matches(['"', '\''])
                    .chars()
                    .take(120)
                    .collect()
            })
        })
        .filter(|name: &String| !name.is_empty())
}

fn environment(os: &str, kernel_release: &str) -> Option<String> {
    (os == "linux" && kernel_release.to_ascii_lowercase().contains("microsoft"))
        .then(|| "wsl".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_and_wsl_are_distinct_even_with_the_same_hostname() {
        assert_eq!(environment("windows", "Microsoft Windows"), None);
        assert_eq!(environment("linux", "6.6.87.2-microsoft-standard-WSL2"), Some("wsl".into()));
        assert_eq!(environment("linux", "5.15.153.1-Microsoft"), Some("wsl".into()));
        assert_eq!(environment("linux", "6.8.0-generic"), None);
        assert_eq!(environment("macos", ""), None);
        assert_eq!(
            source_name("linux", Some("wsl"), Some("Ubuntu-24.04"), "PRETTY_NAME=Ubuntu"),
            Some("Ubuntu-24.04".into())
        );
        assert_eq!(
            source_name("linux", Some("wsl"), Some("Debian"), "PRETTY_NAME=Debian"),
            Some("Debian".into())
        );
        assert_eq!(
            source_name("linux", None, Some("ignored"), "PRETTY_NAME=\"Debian GNU/Linux 12\"\n"),
            Some("Debian GNU/Linux 12".into())
        );
        assert_eq!(source_name("windows", None, Some("ignored"), "PRETTY_NAME=Linux"), None);
    }

    #[test]
    fn services_share_one_process_identity_and_ignore_future_fields() {
        let first = DeviceMetadata::current();
        assert_eq!(first, DeviceMetadata::current());
        assert_eq!(first.os, std::env::consts::OS);
        assert!(uuid::Uuid::parse_str(&first.instance_id).is_ok());
        let mut json = serde_json::to_value(&first).expect("metadata");
        json["future"] = true.into();
        assert_eq!(serde_json::from_value::<DeviceMetadata>(json).expect("future peer"), first);
    }
}
