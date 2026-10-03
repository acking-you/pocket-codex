//! `agents.toml` (TRD §5.1): read with defaults, save preserving keys this
//! version does not know, write `0600` atomically.

use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr},
    path::Path,
};

use pocket_codex_core::acp::pcx::{AcpSettingsView, AgentFlagView, CustomAgentDef, GatewayView};
use toml::{Table, Value};
use url::Url;

use super::{
    super::{auth::GatewayAuth, error::AcpError},
    catalog::Catalog,
    store::{write_private, Layout},
};

/// Agent / custom id pattern `^[a-z][a-z0-9-]{0,63}$`.
pub fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    id.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A gateway configured in `agents.toml`.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct GatewaySettings {
    /// Gateway method id.
    pub method_id: Option<String>,
    /// Base URL.
    pub base_url: String,
    /// Secret token.
    pub token: Option<String>,
    /// Provider name.
    pub provider_name: Option<String>,
    /// Extra headers.
    pub extra_headers: BTreeMap<String, String>,
}

impl std::fmt::Debug for GatewaySettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatewaySettings")
            .field("method_id", &self.method_id)
            .field("base_url", &self.base_url)
            .field("has_token", &self.token.is_some())
            .finish()
    }
}

/// Whether a gateway URL is allowed; `Ok(true)` means plain http on a
/// loopback or private network (the UI warns).
pub fn check_gateway_url(url: &str) -> Result<bool, AcpError> {
    let parsed =
        Url::parse(url).map_err(|e| AcpError::InvalidParams(format!("gateway URL: {e}")))?;
    match parsed.scheme() {
        "https" => Ok(false),
        "http" => {
            let private = match parsed.host() {
                Some(url::Host::Domain(d)) => d == "localhost",
                Some(url::Host::Ipv4(ip)) => is_private_v4(ip),
                Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
                None => false,
            };
            if private {
                Ok(true)
            } else {
                Err(AcpError::InvalidParams(
                    "gateway URLs must use https outside loopback and private networks".into(),
                ))
            }
        },
        _ => Err(AcpError::InvalidParams("gateway URLs must be http(s)".into())),
    }
}

fn is_private_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    a == 127 || a == 10 || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168)
}

/// `agents.toml`, holding the whole document so unknown keys survive.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    doc: Table,
}

impl Settings {
    /// Read `agents.toml` (missing → defaults).
    pub fn load(layout: &Layout) -> Result<Self, AcpError> {
        match std::fs::read_to_string(layout.settings()) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Parse settings text.
    pub fn parse(text: &str) -> Result<Self, AcpError> {
        let doc: Table = text
            .parse()
            .map_err(|e| AcpError::InvalidParams(format!("agents.toml: {e}")))?;
        Ok(Self {
            doc,
        })
    }

    /// Write `agents.toml` (0600, atomic).
    pub fn save(&self, layout: &Layout) -> Result<(), AcpError> {
        let mut doc = self.doc.clone();
        doc.insert("schema".into(), Value::Integer(1));
        let text = toml::to_string_pretty(&doc).map_err(|e| AcpError::Internal(e.to_string()))?;
        write_private(&layout.settings(), text.as_bytes())
    }

    fn agent(&self, id: &str) -> Option<&Table> {
        self.doc.get("agents")?.as_table()?.get(id)?.as_table()
    }

    fn agent_mut(&mut self, id: &str) -> &mut Table {
        let agents = self
            .doc
            .entry("agents")
            .or_insert_with(|| Value::Table(Table::new()));
        if !agents.is_table() {
            *agents = Value::Table(Table::new());
        }
        let Value::Table(agents) = agents else { unreachable!("just made a table") };
        let entry = agents
            .entry(id)
            .or_insert_with(|| Value::Table(Table::new()));
        if !entry.is_table() {
            *entry = Value::Table(Table::new());
        }
        match entry {
            Value::Table(t) => t,
            _ => unreachable!("just made a table"),
        }
    }

    /// Remote management switch (default on, D14).
    pub fn remote_management(&self) -> bool {
        self.doc
            .get("remote_management")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    }

    /// Set the remote management switch.
    pub fn set_remote_management(&mut self, on: bool) {
        self.doc
            .insert("remote_management".into(), Value::Boolean(on));
    }

    /// npm registry mirror.
    pub fn npm_registry(&self) -> Option<String> {
        self.doc
            .get("npm_registry")
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    /// Set or clear the npm registry mirror (must be https).
    pub fn set_npm_registry(&mut self, registry: Option<&str>) -> Result<(), AcpError> {
        match registry.map(str::trim).filter(|r| !r.is_empty()) {
            None => {
                self.doc.remove("npm_registry");
            },
            Some(r) => {
                let url = Url::parse(r).map_err(|e| AcpError::InvalidParams(e.to_string()))?;
                if url.scheme() != "https" {
                    return Err(AcpError::InvalidParams("the npm registry must use https".into()));
                }
                self.doc
                    .insert("npm_registry".into(), Value::String(r.to_string()));
            },
        }
        Ok(())
    }

    /// `[agents.<id>] <key>` string.
    pub fn agent_str(&self, id: &str, key: &str) -> Option<String> {
        self.agent(id)?
            .get(key)?
            .as_str()
            .map(str::to_string)
            .filter(|s| !s.is_empty())
    }

    /// Set or clear `[agents.<id>] <key>`.
    pub fn set_agent_str(&mut self, id: &str, key: &str, value: Option<&str>) {
        let table = self.agent_mut(id);
        match value.map(str::trim).filter(|v| !v.is_empty()) {
            Some(v) => {
                table.insert(key.into(), Value::String(v.to_string()));
            },
            None => {
                table.remove(key);
            },
        }
    }

    /// `[agents.<id>] <key>` boolean.
    pub fn agent_bool(&self, id: &str, key: &str) -> Option<bool> {
        self.agent(id)?.get(key)?.as_bool()
    }

    /// Set `[agents.<id>] <key>` boolean.
    pub fn set_agent_bool(&mut self, id: &str, key: &str, value: bool) {
        self.agent_mut(id).insert(key.into(), Value::Boolean(value));
    }

    /// Ids with an `[agents.<id>]` table.
    pub fn agent_ids(&self) -> Vec<String> {
        self.doc
            .get("agents")
            .and_then(Value::as_table)
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// `[agents.<id>.gateway]`.
    pub fn gateway(&self, id: &str) -> Option<GatewaySettings> {
        let table = self.agent(id)?.get("gateway")?.as_table()?;
        let text = |k: &str| table.get(k).and_then(Value::as_str).map(str::to_string);
        let base_url = text("base_url")?;
        let extra_headers = table
            .get("extra_headers")
            .and_then(Value::as_table)
            .map(|t| {
                t.iter()
                    .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        Some(GatewaySettings {
            method_id: text("method_id"),
            base_url,
            token: text("token").filter(|t| !t.is_empty()),
            provider_name: text("provider_name"),
            extra_headers,
        })
    }

    /// Replace or remove `[agents.<id>.gateway]`.
    pub fn set_gateway(&mut self, id: &str, gateway: Option<&GatewaySettings>) {
        let table = self.agent_mut(id);
        let Some(g) = gateway else {
            table.remove("gateway");
            return;
        };
        let mut out = Table::new();
        out.insert("base_url".into(), Value::String(g.base_url.clone()));
        if let Some(token) = &g.token {
            out.insert("token".into(), Value::String(token.clone()));
        }
        if let Some(method) = &g.method_id {
            out.insert("method_id".into(), Value::String(method.clone()));
        }
        if let Some(name) = &g.provider_name {
            out.insert("provider_name".into(), Value::String(name.clone()));
        }
        if !g.extra_headers.is_empty() {
            let headers: Table = g
                .extra_headers
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect();
            out.insert("extra_headers".into(), Value::Table(headers));
        }
        table.insert("gateway".into(), Value::Table(out));
    }

    /// The hub-side gateway login of `id`, when a URL and token are set.
    pub fn gateway_auth(&self, id: &str) -> Option<GatewayAuth> {
        let g = self.gateway(id)?;
        let token = g.token.clone()?;
        let mut headers = g.extra_headers.clone();
        headers.insert("Authorization".into(), format!("Bearer {token}"));
        Some(GatewayAuth {
            method_id: g.method_id,
            base_url: g.base_url,
            headers,
            provider_name: g.provider_name,
        })
    }

    /// D19 data mode of `family` (`auto` by default).
    pub fn data_mode(&self, family: &str) -> String {
        self.doc
            .get("data")
            .and_then(Value::as_table)
            .and_then(|t| t.get(family))
            .and_then(Value::as_str)
            .filter(|m| matches!(*m, "auto" | "shared" | "isolated"))
            .unwrap_or("auto")
            .to_string()
    }

    /// Set the D19 data mode of `family`.
    pub fn set_data_mode(&mut self, family: &str, mode: &str) -> Result<(), AcpError> {
        if !matches!(mode, "auto" | "shared" | "isolated") {
            return Err(AcpError::InvalidParams(format!("unknown data mode `{mode}`")));
        }
        let data = self
            .doc
            .entry("data")
            .or_insert_with(|| Value::Table(Table::new()));
        if let Value::Table(t) = data {
            t.insert(family.into(), Value::String(mode.into()));
        }
        Ok(())
    }

    /// `[[custom]]` agents.
    pub fn custom(&self) -> Vec<CustomAgentDef> {
        let Some(list) = self.doc.get("custom").and_then(Value::as_array) else {
            return Vec::new();
        };
        list.iter()
            .filter_map(Value::as_table)
            .filter_map(|t| {
                let text = |k: &str| t.get(k).and_then(Value::as_str).map(str::to_string);
                Some(CustomAgentDef {
                    id: text("id")?,
                    name: text("name").unwrap_or_default(),
                    command: text("command")?,
                    args: t
                        .get("args")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default(),
                    env: t
                        .get("env")
                        .and_then(Value::as_table)
                        .map(|e| {
                            e.iter()
                                .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                                .collect()
                        })
                        .unwrap_or_default(),
                })
            })
            .collect()
    }

    /// Insert or replace a custom agent.
    pub fn put_custom(&mut self, def: &CustomAgentDef) {
        let mut entry = Table::new();
        entry.insert("id".into(), Value::String(def.id.clone()));
        entry.insert("name".into(), Value::String(def.name.clone()));
        entry.insert("command".into(), Value::String(def.command.clone()));
        entry.insert(
            "args".into(),
            Value::Array(def.args.iter().cloned().map(Value::String).collect()),
        );
        entry.insert(
            "env".into(),
            Value::Table(
                def.env
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                    .collect(),
            ),
        );
        let list = self
            .doc
            .entry("custom")
            .or_insert_with(|| Value::Array(Vec::new()));
        if !list.is_array() {
            *list = Value::Array(Vec::new());
        }
        if let Value::Array(items) = list {
            let same = |v: &Value| v.get("id").and_then(Value::as_str) == Some(def.id.as_str());
            match items.iter_mut().find(|v| same(v)) {
                Some(existing) => *existing = Value::Table(entry),
                None => items.push(Value::Table(entry)),
            }
        }
    }

    /// Remove a custom agent; returns whether it existed.
    pub fn delete_custom(&mut self, id: &str) -> bool {
        let Some(Value::Array(items)) = self.doc.get_mut("custom") else { return false };
        let before = items.len();
        items.retain(|v| v.get("id").and_then(Value::as_str) != Some(id));
        before != items.len()
    }
}

/// Validate a custom agent against the catalog.
pub fn validate_custom(def: &CustomAgentDef, catalog: &Catalog) -> Result<(), AcpError> {
    if !valid_id(&def.id) {
        return Err(AcpError::InvalidParams(format!("invalid agent id `{}`", def.id)));
    }
    if catalog.agents.iter().any(|a| a.id == def.id) {
        return Err(AcpError::InvalidParams(format!("`{}` is a catalog agent id", def.id)));
    }
    let command = Path::new(&def.command);
    if !command.is_absolute() || !command.is_file() {
        return Err(AcpError::InvalidParams(
            "the command must be an existing absolute path".into(),
        ));
    }
    Ok(())
}

/// The view of `settings` for the bridge facade (tokens never included).
pub fn view(settings: &Settings, catalog: &Catalog) -> AcpSettingsView {
    let engine_override = catalog.agents.iter().find_map(|a| {
        a.engine_override
            .as_ref()
            .map(|o| (a.id.clone(), o.setting.clone()))
    });
    let external = catalog.agents.iter().find_map(|a| {
        a.external_engine
            .as_ref()
            .map(|e| (a.id.clone(), e.setting.clone()))
    });
    let mut families: Vec<String> = catalog
        .agents
        .iter()
        .flat_map(|a| a.releases.iter().filter_map(|r| r.data_family.clone()))
        .collect();
    families.sort();
    families.dedup();
    let flags = catalog
        .agents
        .iter()
        .flat_map(|agent| {
            let release = agent.releases.first();
            release
                .into_iter()
                .flat_map(|r| r.conditional_args.iter())
                .map(|c| AgentFlagView {
                    agent_id: agent.id.clone(),
                    setting: c.setting.clone(),
                    label_key: c.label_key.clone(),
                    confirm_key: c.confirm_key.clone(),
                    value: settings
                        .agent_bool(&agent.id, &c.setting)
                        .unwrap_or(c.default),
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let mut ids: Vec<String> = settings.agent_ids();
    ids.sort();
    AcpSettingsView {
        remote_management: settings.remote_management(),
        npm_registry: settings.npm_registry(),
        claude_engine_path: engine_override.and_then(|(id, key)| settings.agent_str(&id, &key)),
        codex_binary: external.and_then(|(id, key)| settings.agent_str(&id, &key)),
        binary_overrides: ids
            .iter()
            .filter_map(|id| settings.agent_str(id, "binary").map(|b| (id.clone(), b)))
            .collect(),
        opencode_data: families
            .iter()
            .map(|f| (f.clone(), settings.data_mode(f)))
            .collect(),
        gateways: ids
            .iter()
            .filter_map(|id| {
                settings.gateway(id).map(|g| GatewayView {
                    agent_id: id.clone(),
                    method_id: g.method_id,
                    base_url: g.base_url,
                    token: None,
                    has_token: g.token.is_some(),
                    provider_name: g.provider_name,
                    extra_headers: g.extra_headers.into_iter().collect(),
                    clear: false,
                })
            })
            .collect(),
        flags,
    }
}

/// Apply `view` to `settings`; returns the names of changed items (for the
/// audit log, never values).
pub fn apply(
    settings: &mut Settings,
    view: &AcpSettingsView,
    catalog: &Catalog,
) -> Result<Vec<(String, String)>, AcpError> {
    let before = settings.clone();
    let mut changed: Vec<(String, String)> = Vec::new();
    if view.remote_management != settings.remote_management() {
        settings.set_remote_management(view.remote_management);
        changed.push((String::new(), "remote_management".into()));
    }
    if view.npm_registry != settings.npm_registry() {
        settings.set_npm_registry(view.npm_registry.as_deref())?;
        changed.push((String::new(), "npm_registry".into()));
    }
    let check_path = |p: &Option<String>| -> Result<(), AcpError> {
        match p.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            Some(p) if !Path::new(p).is_absolute() => {
                Err(AcpError::InvalidParams(format!("`{p}` is not an absolute path")))
            },
            _ => Ok(()),
        }
    };
    if let Some(agent) = catalog.agents.iter().find(|a| a.engine_override.is_some()) {
        let key = agent
            .engine_override
            .as_ref()
            .map(|o| o.setting.clone())
            .unwrap_or_default();
        check_path(&view.claude_engine_path)?;
        if settings.agent_str(&agent.id, &key) != view.claude_engine_path {
            settings.set_agent_str(&agent.id, &key, view.claude_engine_path.as_deref());
            changed.push((agent.id.clone(), key));
        }
    }
    if let Some(agent) = catalog.agents.iter().find(|a| a.external_engine.is_some()) {
        let key = agent
            .external_engine
            .as_ref()
            .map(|e| e.setting.clone())
            .unwrap_or_default();
        check_path(&view.codex_binary)?;
        if settings.agent_str(&agent.id, &key) != view.codex_binary {
            settings.set_agent_str(&agent.id, &key, view.codex_binary.as_deref());
            changed.push((agent.id.clone(), key));
        }
    }
    for id in settings.agent_ids() {
        let wanted = view
            .binary_overrides
            .iter()
            .find(|(a, _)| *a == id)
            .map(|(_, b)| b.clone());
        if settings.agent_str(&id, "binary") != wanted && wanted.is_none() {
            settings.set_agent_str(&id, "binary", None);
            changed.push((id.clone(), "binary".into()));
        }
    }
    for (id, binary) in &view.binary_overrides {
        check_path(&Some(binary.clone()))?;
        if settings.agent_str(id, "binary").as_deref() != Some(binary.as_str()) {
            settings.set_agent_str(id, "binary", Some(binary));
            changed.push((id.clone(), "binary".into()));
        }
    }
    for (family, mode) in &view.opencode_data {
        if settings.data_mode(family) != *mode {
            settings.set_data_mode(family, mode)?;
            changed.push((String::new(), format!("data.{family}")));
        }
    }
    for g in &view.gateways {
        if g.clear {
            if settings.gateway(&g.agent_id).is_some() {
                settings.set_gateway(&g.agent_id, None);
                changed.push((g.agent_id.clone(), "gateway".into()));
            }
            continue;
        }
        check_gateway_url(&g.base_url)?;
        let previous = settings.gateway(&g.agent_id);
        let token = match g.token.as_deref() {
            None => previous.as_ref().and_then(|p| p.token.clone()),
            Some("") => None,
            Some(t) => Some(t.to_string()),
        };
        let next = GatewaySettings {
            method_id: g.method_id.clone().filter(|m| !m.is_empty()),
            base_url: g.base_url.trim().to_string(),
            token,
            provider_name: g.provider_name.clone().filter(|p| !p.is_empty()),
            extra_headers: g.extra_headers.iter().cloned().collect(),
        };
        if previous.as_ref() != Some(&next) {
            settings.set_gateway(&g.agent_id, Some(&next));
            changed.push((g.agent_id.clone(), "gateway".into()));
        }
    }
    for flag in &view.flags {
        let known = catalog.agents.iter().any(|a| {
            a.id == flag.agent_id
                && a.releases
                    .iter()
                    .any(|r| r.conditional_args.iter().any(|c| c.setting == flag.setting))
        });
        if !known {
            return Err(AcpError::InvalidParams(format!("unknown setting {}", flag.setting)));
        }
        let current = settings.agent_bool(&flag.agent_id, &flag.setting);
        if current != Some(flag.value) {
            let default = catalog
                .agents
                .iter()
                .find(|a| a.id == flag.agent_id)
                .and_then(|a| {
                    a.releases
                        .iter()
                        .flat_map(|r| r.conditional_args.iter())
                        .find(|c| c.setting == flag.setting)
                })
                .map(|c| c.default);
            if current.is_some() || default != Some(flag.value) {
                settings.set_agent_bool(&flag.agent_id, &flag.setting, flag.value);
                changed.push((flag.agent_id.clone(), flag.setting.clone()));
            }
        }
    }
    if changed.is_empty() {
        *settings = before;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_follow_the_pattern() {
        assert!(valid_id("gemini"));
        assert!(valid_id("a-1"));
        assert!(!valid_id("1a"));
        assert!(!valid_id("Gemini"));
        assert!(!valid_id(""));
        assert!(!valid_id(&"a".repeat(65)));
    }
}
