//! Service identifiers used to map Pocket-Codex devices onto pb-mapper keys.
//!
//! ```text
//!                     pcx : <device> : <kind> : <name>
//!                      │      │          │        │
//!                      │      │          │        └── instance name
//!                      │      │          │            (e.g. "default", "work")
//!                      │      │          └─────────── ServiceKind::as_key_segment
//!                      │      │                       ("app" | "api")
//!                      │      └────────────────────── sanitised host id
//!                      │                              (default: hostname)
//!                      └───────────────────────────── SERVICE_KEY_PREFIX
//!
//!   examples:
//!     pcx:macbook:app:default       ← codex app-server on the laptop
//!     pcx:studio:api:work           ← Responses API proxy on the desktop
//! ```
//!
//! `sanitize_component` lower-cases the input and replaces any run of
//! non-`[a-z0-9_.]` characters with a single `-`, so user-facing names
//! like `"Bo's MacBook Pro"` collapse into `bo-s-macbook-pro`. Empty
//! results fall back to `DEFAULT_SERVICE_NAME` so the resulting key
//! is always well-formed.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// Prefix used for Pocket-Codex owned pb-mapper service keys.
pub const SERVICE_KEY_PREFIX: &str = "pcx";

/// Default service instance name used when the user does not provide one.
pub const DEFAULT_SERVICE_NAME: &str = "default";

/// A kind of Pocket-Codex service exposed through pb-mapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceKind {
    /// Codex app-server remote-control service.
    App,
    /// Responses API proxy service.
    Api,
}

impl ServiceKind {
    /// String segment used inside Pocket-Codex service keys.
    pub fn as_key_segment(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Api => "api",
        }
    }
}

impl fmt::Display for ServiceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_key_segment())
    }
}

impl FromStr for ServiceKind {
    type Err = ();

    fn from_str(raw: &str) -> std::result::Result<Self, Self::Err> {
        match raw {
            "app" => Ok(Self::App),
            "api" => Ok(Self::Api),
            _ => Err(()),
        }
    }
}

/// Device and instance identity for one relay-exposed Pocket-Codex service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceId {
    /// Device id selected by the host/user.
    pub device: String,
    /// Service kind.
    pub kind: ServiceKind,
    /// Instance name within the device and service kind.
    pub name: String,
}

impl ServiceId {
    /// Build a service id, sanitizing user-facing device/name strings into
    /// stable pb-mapper key segments.
    pub fn new(device: impl AsRef<str>, kind: ServiceKind, name: impl AsRef<str>) -> Self {
        Self {
            device: sanitize_component(device.as_ref()),
            kind,
            name: sanitize_component(name.as_ref()),
        }
    }

    /// Build the pb-mapper key for this Pocket-Codex service.
    pub fn key(&self) -> String {
        format!("{SERVICE_KEY_PREFIX}:{}:{}:{}", self.device, self.kind.as_key_segment(), self.name)
    }

    /// Parse a Pocket-Codex service key.
    ///
    /// Returns `None` for non-Pocket-Codex keys so generic pb-mapper keys can
    /// continue to coexist in the same relay.
    pub fn parse_key(key: &str) -> Option<Self> {
        let mut parts = key.split(':');
        if parts.next()? != SERVICE_KEY_PREFIX {
            return None;
        }
        let device = parts.next()?;
        let kind = parts.next()?.parse().ok()?;
        let name = parts.next()?;
        if parts.next().is_some() || device.is_empty() || name.is_empty() {
            return None;
        }
        Some(Self {
            device: device.to_string(),
            kind,
            name: name.to_string(),
        })
    }
}

/// Normalize a user/device supplied identifier into a shell-friendly key
/// segment.
pub fn sanitize_component(raw: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in raw.trim().chars().flat_map(char::to_lowercase) {
        let keep = ch.is_ascii_alphanumeric() || ch == '_' || ch == '.';
        if keep {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        DEFAULT_SERVICE_NAME.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Best-effort default device id for CLI commands.
pub fn default_device_id() -> String {
    sysinfo::System::host_name()
        .map(|name| sanitize_component(&name))
        .filter(|name| name != DEFAULT_SERVICE_NAME)
        .unwrap_or_else(|| DEFAULT_SERVICE_NAME.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_id_formats_key_with_device_kind_and_name() {
        let id = ServiceId::new("macbook", ServiceKind::App, "work");

        assert_eq!(id.key(), "pcx:macbook:app:work");
    }

    #[test]
    fn service_id_parses_pocket_codex_keys() {
        let id = ServiceId::parse_key("pcx:studio:api:default").expect("parse service id");

        assert_eq!(id.device, "studio");
        assert_eq!(id.kind, ServiceKind::Api);
        assert_eq!(id.name, "default");
    }

    #[test]
    fn service_id_rejects_non_pocket_codex_keys() {
        assert!(ServiceId::parse_key("codex").is_none());
        assert!(ServiceId::parse_key("pcx:studio:api").is_none());
    }

    #[test]
    fn sanitize_component_keeps_keys_shell_friendly() {
        assert_eq!(sanitize_component("Bo Liu's MacBook Pro"), "bo-liu-s-macbook-pro");
        assert_eq!(sanitize_component(""), "default");
    }
}
