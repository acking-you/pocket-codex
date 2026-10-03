//! Authentication method classification, terminal-login commands and D20
//! gateway login (TRD §4.2.9, §4.2.12).

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use pocket_codex_core::acp::{
    pcx::{auth_kind, AuthMethodInfo},
    AuthMethod,
};
use serde_json::{json, Value};

use super::{error::AcpError, launch::LaunchSpec};

/// A command to run in a visible terminal on the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalLaunch {
    /// Program.
    pub program: PathBuf,
    /// Arguments.
    pub args: Vec<String>,
    /// Extra environment.
    pub env: BTreeMap<String, String>,
    /// Window title.
    pub title: String,
}

impl TerminalLaunch {
    /// Shell-quoted command line, shown when no terminal can be opened.
    pub fn command_line(&self) -> String {
        let mut parts: Vec<String> = self
            .env
            .iter()
            .map(|(k, v)| format!("{k}={}", shell_quote(v)))
            .collect();
        parts.push(shell_quote(&self.program.to_string_lossy()));
        parts.extend(self.args.iter().map(|a| shell_quote(a)));
        parts.join(" ")
    }
}

/// POSIX single-quote `value` when it contains anything but safe characters.
pub fn shell_quote(value: &str) -> String {
    let safe = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@,+".contains(c));
    if safe {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}

/// Opens terminals on the host desktop.
pub trait TerminalLauncher: Send + Sync {
    /// Open a visible terminal that runs `launch` and writes its exit code to
    /// `status_file`.
    fn open(&self, launch: &TerminalLaunch, status_file: &Path) -> Result<(), AcpError>;
}

/// D20. `headers` already contains `Authorization: Bearer <token>` plus extra
/// headers.
#[derive(Clone, PartialEq, Eq)]
pub struct GatewayAuth {
    /// Method to use; `None` = the first gateway method.
    pub method_id: Option<String>,
    /// Gateway base URL.
    pub base_url: String,
    /// Headers (contain the secret).
    pub headers: BTreeMap<String, String>,
    /// Provider name (codex-acp).
    pub provider_name: Option<String>,
}

impl std::fmt::Debug for GatewayAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatewayAuth")
            .field("method_id", &self.method_id)
            .field("base_url", &self.base_url)
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .field("provider_name", &self.provider_name)
            .finish()
    }
}

impl GatewayAuth {
    /// Replace every header value in `text` with `***`.
    pub fn redact(&self, text: &str) -> String {
        let mut out = text.to_string();
        for value in self.headers.values() {
            for secret in [value.as_str(), value.strip_prefix("Bearer ").unwrap_or("")] {
                if secret.len() >= 4 {
                    out = out.replace(secret, "***");
                }
            }
        }
        out
    }
}

/// Legacy `_meta["terminal-auth"]` description.
fn legacy_terminal(method: &AuthMethod) -> Option<(String, Vec<String>, Option<String>)> {
    let meta = method.meta.as_ref()?.get("terminal-auth")?;
    let command = meta.get("command")?.as_str()?.to_string();
    let args = meta
        .get("args")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let label = meta
        .get("label")
        .and_then(Value::as_str)
        .map(str::to_string);
    Some((command, args, label))
}

fn gateway_meta(method: &AuthMethod) -> Option<&Value> {
    method.meta.as_ref()?.get("gateway")
}

fn program_stem(program: &Path) -> String {
    let name = program
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.strip_suffix(".exe").unwrap_or(&name).to_string()
}

/// Gateway method ids, in agent order.
pub fn gateway_methods(methods: &[AuthMethod]) -> Vec<&AuthMethod> {
    methods
        .iter()
        .filter(|m| gateway_meta(m).is_some())
        .collect()
}

/// The gateway method a configuration selects.
pub fn selected_gateway<'a>(
    methods: &'a [AuthMethod],
    gateway: &GatewayAuth,
) -> Option<&'a AuthMethod> {
    let candidates = gateway_methods(methods);
    match &gateway.method_id {
        Some(id) => candidates.into_iter().find(|m| &m.id == id),
        None => candidates.into_iter().next(),
    }
}

/// Classify the agent's methods for controllers.
pub fn classify(
    methods: &[AuthMethod],
    spec: &LaunchSpec,
    gateway: Option<&GatewayAuth>,
) -> Vec<AuthMethodInfo> {
    let configured = gateway
        .and_then(|g| selected_gateway(methods, g))
        .map(|m| m.id.clone());
    methods
        .iter()
        .map(|method| {
            let mut info = AuthMethodInfo {
                id: method.id.clone(),
                name: method.name.clone(),
                description: method.description.clone().unwrap_or_default(),
                kind: auth_kind::AGENT.into(),
                remote: true,
                available: true,
                gateway_protocol: None,
                gateway_configured: false,
            };
            if method.kind.as_deref() == Some("terminal") {
                info.kind = auth_kind::TERMINAL.into();
                info.remote = false;
            } else if let Some((command, _, _)) = legacy_terminal(method) {
                info.kind = auth_kind::TERMINAL.into();
                info.remote = false;
                info.available = command == program_stem(&spec.program);
            } else if let Some(meta) = gateway_meta(method) {
                info.kind = auth_kind::GATEWAY.into();
                info.remote = false;
                info.gateway_protocol = meta
                    .get("protocol")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                info.gateway_configured = configured.as_deref() == Some(method.id.as_str());
            }
            info
        })
        .collect()
}

/// The terminal command for a terminal method (never includes
/// `launch_only_args`).
pub fn terminal_launch(method: &AuthMethod, spec: &LaunchSpec) -> Result<TerminalLaunch, AcpError> {
    let title = format!("{} login", spec.display_name);
    if method.kind.as_deref() == Some("terminal") {
        let mut args = spec.args.clone();
        args.extend(method.args.iter().cloned());
        let mut env = spec.env.clone();
        env.extend(method.env.clone());
        return Ok(TerminalLaunch {
            program: spec.program.clone(),
            args,
            env,
            title,
        });
    }
    let Some((command, args, label)) = legacy_terminal(method) else {
        return Err(AcpError::InvalidParams(format!("{} is not a terminal method", method.id)));
    };
    if command != program_stem(&spec.program) {
        return Err(AcpError::InvalidParams(
            method
                .description
                .clone()
                .unwrap_or_else(|| format!("run `{command}` yourself")),
        ));
    }
    let args = args
        .into_iter()
        .filter(|a| !spec.launch_only_args.contains(a))
        .collect();
    Ok(TerminalLaunch {
        program: spec.program.clone(),
        args,
        env: spec.env.clone(),
        title: label.unwrap_or(title),
    })
}

/// `authenticate` params for the gateway login.
pub fn gateway_params(method_id: &str, gateway: &GatewayAuth) -> Value {
    let mut inner = json!({ "baseUrl": gateway.base_url, "headers": gateway.headers });
    if let Some(name) = &gateway.provider_name {
        inner["providerName"] = json!(name);
    }
    json!({ "methodId": method_id, "_meta": { "gateway": inner } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_and_redaction() {
        assert_eq!(shell_quote("plain-arg"), "plain-arg");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        let gateway = GatewayAuth {
            method_id: None,
            base_url: "https://g".into(),
            headers: BTreeMap::from([("Authorization".into(), "Bearer sk-secret-1".into())]),
            provider_name: None,
        };
        assert_eq!(gateway.redact("bad key sk-secret-1"), "bad key ***");
        assert!(!format!("{gateway:?}").contains("sk-secret"));
    }
}
