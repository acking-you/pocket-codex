//! Read-only publisher identity, shared by the local and remote device views.

use pocket_codex_core::device::DeviceMetadata;

use super::*;

/// The environment owning a service, never inferred from its device label.
pub struct DeviceMetadataDto {
    /// Publisher OS.
    pub os: String,
    /// Publisher executable architecture.
    pub arch: String,
    /// Optional runtime environment, currently `wsl`.
    pub environment: Option<String>,
    /// Display hostname.
    pub hostname: String,
    /// Stable installation identity, absent on older publishers.
    pub host_id: Option<String>,
    /// OS or WSL distribution name.
    pub source_name: Option<String>,
    /// Opaque publisher process identity, renewed on restart.
    pub instance_id: String,
}

impl From<DeviceMetadata> for DeviceMetadataDto {
    fn from(value: DeviceMetadata) -> Self {
        Self {
            os: value.os,
            arch: value.arch,
            environment: value.environment,
            hostname: value.hostname,
            host_id: value.host_id,
            source_name: value.source_name,
            instance_id: value.instance_id,
        }
    }
}

/// Identity of this app process, available before it starts hosting.
#[frb(sync)]
pub fn local_device_metadata() -> DeviceMetadataDto {
    DeviceMetadata::current().into()
}

/// Read a registered service's metadata over its existing meta/host tunnel.
/// Older hosts without the route return `None`; outages remain errors.
pub fn service_device_metadata(service_key: String) -> Result<Option<DeviceMetadataDto>> {
    crate::engine::management_client::device_metadata(&service_key)
        .map(|value| value.map(Into::into))
}
