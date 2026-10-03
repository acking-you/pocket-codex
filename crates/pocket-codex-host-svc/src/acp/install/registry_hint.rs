//! New-version hints from the ACP registry (TRD §4.3.10). The registry is
//! unsigned, so its versions are only offered as an advanced, host-only
//! install marked "not verified by Pocket-Codex".

use std::{collections::BTreeMap, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    super::error::AcpError,
    catalog::{CatalogAgent, Release, ReleaseKind},
    platform::Platform,
    resolve::InstallContext,
    store::{write_private, Layout},
};

/// Registry index.
pub const REGISTRY_URL: &str =
    "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
/// Refresh at most this often.
const REFRESH_EVERY: i64 = 6 * 3600;
const MAX_BODY: usize = 4 * 1024 * 1024;

/// One registry agent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RegistryEntry {
    /// Registry id.
    pub id: String,
    /// Latest version.
    pub version: String,
    /// `{binary?, npx?, uvx?}`.
    pub distribution: Value,
}

/// `registry-cache.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RegistryCache {
    /// Unix seconds of the fetch.
    pub fetched_at: i64,
    /// Entries.
    pub agents: Vec<RegistryEntry>,
}

/// The cached registry, if any.
pub fn cached(layout: &Layout) -> Option<RegistryCache> {
    let bytes = std::fs::read(layout.registry_cache()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Refresh the cache when older than 6 hours.
pub async fn refresh(ctx: &InstallContext) -> Result<(), AcpError> {
    let layout = ctx.layout();
    let now = chrono::Utc::now().timestamp();
    if cached(&layout).is_some_and(|c| now - c.fetched_at < REFRESH_EVERY) {
        return Ok(());
    }
    let url = url::Url::parse(REGISTRY_URL).map_err(|e| AcpError::Internal(e.to_string()))?;
    if !ctx.fetch.allows(&url) {
        return Err(AcpError::DownloadFailed("the registry host is not allowed".into()));
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| AcpError::DownloadFailed(e.to_string()))?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| AcpError::DownloadFailed(e.to_string()))?;
    if !response.status().is_success() {
        return Err(AcpError::DownloadFailed(format!("registry: HTTP {}", response.status())));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| AcpError::DownloadFailed(e.to_string()))?
    {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_BODY {
            return Err(AcpError::DownloadFailed("the registry index is too large".into()));
        }
    }
    let index: Value =
        serde_json::from_slice(&body).map_err(|e| AcpError::DownloadFailed(e.to_string()))?;
    let agents = index
        .get("agents")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|a| {
                    Some(RegistryEntry {
                        id: a.get("id")?.as_str()?.to_string(),
                        version: a.get("version")?.as_str()?.to_string(),
                        distribution: a.get("distribution").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let cache = RegistryCache {
        fetched_at: now,
        agents,
    };
    let bytes = serde_json::to_vec(&cache).map_err(|e| AcpError::Internal(e.to_string()))?;
    write_private(&layout.registry_cache(), &bytes)
}

/// A registry version newer than the catalog pin.
pub fn newer_version(agent: &CatalogAgent, cache: Option<&RegistryCache>) -> Option<String> {
    let registry_id = agent.registry_id.as_ref()?;
    let entry = cache?.agents.iter().find(|e| &e.id == registry_id)?;
    let latest = semver::Version::parse(&entry.version).ok()?;
    let pinned = agent
        .releases
        .iter()
        .filter_map(|r| semver::Version::parse(&r.version).ok())
        .max()?;
    (latest > pinned).then(|| entry.version.clone())
}

/// What a registry install fetches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryPlan {
    /// Version.
    pub version: String,
    /// `(url, SRI, cmd)` of a binary target with a sha256.
    pub archive: Option<(String, String, String)>,
    /// npm package (no lockfile; the UI confirms first).
    pub npm_package: Option<String>,
    /// Launch arguments.
    pub args: Vec<String>,
}

fn hex_to_sri(hex: &str) -> Option<String> {
    use base64::Engine as _;
    if hex.len() != 64 {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..32)
        .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
        .collect();
    Some(format!("sha256-{}", base64::engine::general_purpose::STANDARD.encode(bytes?)))
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Plan installing `version` of `agent` from the cached registry.
pub fn install_plan(
    ctx: &InstallContext,
    agent: &CatalogAgent,
    version: &str,
) -> Result<RegistryPlan, AcpError> {
    let not_pinned =
        || AcpError::VersionNotPinned(format!("{} {version} cannot be installed", agent.name));
    let registry_id = agent.registry_id.as_ref().ok_or_else(not_pinned)?;
    let cache = cached(&ctx.layout()).ok_or_else(not_pinned)?;
    let entry = cache
        .agents
        .iter()
        .find(|e| &e.id == registry_id && e.version == version)
        .ok_or_else(not_pinned)?;
    let platform = ctx.platform.clone()?;
    let binary = entry
        .distribution
        .get("binary")
        .and_then(|b| b.get(platform.key));
    if let Some(target) = binary {
        let url = target
            .get("archive")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let supported = [".zip", ".tar.gz", ".tgz"]
            .iter()
            .any(|s| url.to_lowercase().ends_with(s));
        let sri = target
            .get("sha256")
            .and_then(Value::as_str)
            .and_then(hex_to_sri);
        let cmd = target
            .get("cmd")
            .and_then(Value::as_str)
            .map(|c| c.trim_start_matches("./").to_string());
        if let (true, Some(sri), Some(cmd)) = (supported, sri, cmd) {
            return Ok(RegistryPlan {
                version: version.into(),
                archive: Some((url.to_string(), sri, cmd)),
                npm_package: None,
                args: strings(target.get("args")),
            });
        }
    }
    if let Some(npx) = entry.distribution.get("npx") {
        let package = npx
            .get("package")
            .and_then(Value::as_str)
            .ok_or_else(not_pinned)?;
        let package = match package.rsplit_once('@') {
            Some((name, _)) if !name.is_empty() => name.to_string(),
            _ => package.to_string(),
        };
        return Ok(RegistryPlan {
            version: version.into(),
            archive: None,
            npm_package: Some(package),
            args: strings(npx.get("args")),
        });
    }
    Err(not_pinned())
}

/// Entry script of an npm registry install (`node_modules/<pkg>`'s `bin`,
/// resolved at launch from `package.json`; the default is `dist/index.js`).
pub fn npm_entry(package: &str) -> String {
    format!("node_modules/{package}/dist/index.js")
}

/// The synthesized release of a registry install.
pub fn release_of(agent: &CatalogAgent, plan: &RegistryPlan, platform: &Platform) -> Release {
    let template = agent.releases.first();
    let kind = match (&plan.archive, &plan.npm_package) {
        (Some((url, integrity, cmd)), _) => ReleaseKind::Archive {
            targets: BTreeMap::from([(platform.key.to_string(), super::catalog::ArchiveTarget {
                url: url.clone(),
                integrity: integrity.clone(),
                root: None,
                cmd: Some(cmd.clone()),
            })]),
        },
        (None, Some(package)) => ReleaseKind::Npm {
            package: package.clone(),
            entry: npm_entry(package),
            lock: String::new(),
            omit: Vec::new(),
            platforms: Vec::new(),
        },
        _ => ReleaseKind::Archive {
            targets: BTreeMap::new(),
        },
    };
    Release {
        version: plan.version.clone(),
        engine_range: None,
        kind,
        args: plan.args.clone(),
        env: template.map(|r| r.env.clone()).unwrap_or_default(),
        random_env: template.map(|r| r.random_env.clone()).unwrap_or_default(),
        data_family: template.and_then(|r| r.data_family.clone()),
        conditional_args: template
            .map(|r| r.conditional_args.clone())
            .unwrap_or_default(),
    }
}
