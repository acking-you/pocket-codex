//! The subset of the ACP v1 schema this client interprets.
//!
//! Only fields that drive behaviour are typed; everything else is carried as
//! raw JSON so unknown fields and future variants pass through untouched.
//! Capabilities are read conservatively: an optional feature counts as
//! supported only when the agent advertises it explicitly.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The only protocol version this client implements.
pub const PROTOCOL_VERSION: u64 = 1;

/// The `initialize` request this client sends.
///
/// No filesystem, terminal, authentication-terminal, elicitation or session
/// extension capability is advertised: none is implemented on the host, so
/// a compliant agent never routes those requests here.
pub fn initialize_params() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "clientCapabilities": {
            "fs": {"readTextFile": false, "writeTextFile": false},
            "terminal": false,
        },
        "clientInfo": {
            "name": "pocket-codex",
            "title": "Pocket-Codex",
            "version": env!("CARGO_PKG_VERSION"),
        },
    })
}

/// Optional agent features, as negotiated by `initialize`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    /// `session/load` (history replay) — `agentCapabilities.loadSession`.
    pub load_session: bool,
    /// `session/list` — `sessionCapabilities.list`.
    pub list_sessions: bool,
    /// `session/resume` (no replay) — `sessionCapabilities.resume`.
    pub resume_session: bool,
    /// `session/close` — `sessionCapabilities.close`.
    pub close_session: bool,
    /// Image prompt content — `promptCapabilities.image`.
    pub image: bool,
    /// Audio prompt content — `promptCapabilities.audio`.
    pub audio: bool,
    /// Embedded resource prompt content — `promptCapabilities.embeddedContext`.
    pub embedded_context: bool,
}

/// Agent self-description from `agentInfo`, when supplied.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    /// Programmatic name.
    pub name: Option<String>,
    /// Display title.
    pub title: Option<String>,
    /// Version string.
    pub version: Option<String>,
}

/// One advertised authentication method (display only; this client never
/// calls `authenticate`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthMethod {
    /// Method id.
    pub id: String,
    /// Display name.
    pub name: String,
}

/// The outcome of `initialize`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Negotiated {
    /// Negotiated protocol version.
    pub protocol_version: u64,
    /// Optional features.
    pub capabilities: AgentCapabilities,
    /// Agent self-description.
    pub agent: AgentInfo,
    /// Advertised authentication methods.
    pub auth_methods: Vec<AuthMethod>,
}

/// Why `initialize` could not be accepted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NegotiationError {
    /// The response was not an `InitializeResponse`.
    #[error("the agent's initialize response is malformed")]
    Malformed,
    /// The agent only speaks another protocol version.
    #[error("the agent speaks ACP version {0}; this app supports version 1")]
    UnsupportedVersion(u64),
}

fn non_null_object(value: &Value) -> bool {
    value.is_object()
}

fn short(value: &Value, limit: usize) -> Option<String> {
    value
        .as_str()
        .map(|text| text.chars().take(limit).collect::<String>())
        .filter(|text| !text.trim().is_empty())
}

/// Interpret an `initialize` result.
pub fn negotiate(result: &Value) -> Result<Negotiated, NegotiationError> {
    let version = result["protocolVersion"]
        .as_u64()
        .ok_or(NegotiationError::Malformed)?;
    if version != PROTOCOL_VERSION {
        return Err(NegotiationError::UnsupportedVersion(version));
    }
    let agent = &result["agentCapabilities"];
    let session = &agent["sessionCapabilities"];
    let prompt = &agent["promptCapabilities"];
    let capabilities = AgentCapabilities {
        load_session: agent["loadSession"] == true,
        list_sessions: non_null_object(&session["list"]),
        resume_session: non_null_object(&session["resume"]),
        close_session: non_null_object(&session["close"]),
        image: prompt["image"] == true,
        audio: prompt["audio"] == true,
        embedded_context: prompt["embeddedContext"] == true,
    };
    let info = &result["agentInfo"];
    let auth_methods = result["authMethods"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|method| {
            Some(AuthMethod {
                id: short(&method["id"], 256)?,
                name: short(&method["name"], 256).unwrap_or_default(),
            })
        })
        .take(16)
        .collect();
    Ok(Negotiated {
        protocol_version: version,
        capabilities,
        agent: AgentInfo {
            name: short(&info["name"], 256),
            title: short(&info["title"], 256),
            version: short(&info["version"], 128),
        },
        auth_methods,
    })
}

/// The kind hint of a permission option.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    /// Opaque option id, returned verbatim when chosen.
    pub option_id: String,
    /// Agent-provided label.
    pub name: String,
    /// `allow_once` / `allow_always` / `reject_once` / `reject_always`, or an
    /// unknown future value (kept verbatim; the UI treats it neutrally).
    pub kind: String,
}

/// Most options accepted on one permission request.
pub const MAX_PERMISSION_OPTIONS: usize = 16;

/// Parse a `session/request_permission` params object into its session id,
/// tool call and options. `None` when malformed.
pub fn permission_request(params: &Value) -> Option<(String, Value, Vec<PermissionOption>)> {
    let session = params["sessionId"].as_str()?.to_string();
    let tool_call = params.get("toolCall").filter(|v| v.is_object())?.clone();
    let raw = params["options"].as_array()?;
    if raw.is_empty() || raw.len() > MAX_PERMISSION_OPTIONS {
        return None;
    }
    let mut options = Vec::with_capacity(raw.len());
    for option in raw {
        let option_id = option["optionId"].as_str()?.to_string();
        if option_id.len() > 1024
            || options
                .iter()
                .any(|o: &PermissionOption| o.option_id == option_id)
        {
            return None;
        }
        options.push(PermissionOption {
            option_id,
            name: option["name"].as_str()?.chars().take(512).collect(),
            kind: option["kind"].as_str()?.chars().take(64).collect(),
        });
    }
    Some((session, tool_call, options))
}

/// Session state an agent reports on `session/new|load|resume`, and in
/// `config_option_update` / `current_mode_update`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSettings {
    /// Single-select configuration options (models, modes, thought levels,
    /// …), verbatim. Boolean options are dropped: this client does not
    /// advertise boolean support, so a compliant agent never sends them.
    pub config_options: Vec<Value>,
    /// Legacy mode state (`availableModes` / `currentModeId`), verbatim.
    pub modes: Option<Value>,
}

/// Keep only well-formed select options.
pub fn select_options(raw: &Value) -> Vec<Value> {
    raw.as_array()
        .into_iter()
        .flatten()
        .filter(|option| {
            option["id"].is_string()
                && option["name"].is_string()
                && option["type"] == "select"
                && option["currentValue"].is_string()
                && option["options"].is_array()
        })
        .take(32)
        .cloned()
        .collect()
}

impl SessionSettings {
    /// From a session setup response.
    pub fn from_response(result: &Value) -> Self {
        let modes = result
            .get("modes")
            .filter(|modes| {
                modes["availableModes"].is_array() && modes["currentModeId"].is_string()
            })
            .cloned();
        Self {
            config_options: select_options(&result["configOptions"]),
            modes,
        }
    }

    /// Whether `value` is one of the advertised values of select `config`.
    pub fn allows_config_value(&self, config: &str, value: &str) -> bool {
        self.config_options
            .iter()
            .find(|option| option["id"] == config)
            .is_some_and(|option| {
                option["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|entry| {
                        // Ungrouped entries carry `value`; grouped ones nest them.
                        entry["value"] == value
                            || entry["options"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .any(|nested| nested["value"] == value)
                    })
            })
    }

    /// Whether `mode` is an advertised legacy mode.
    pub fn allows_mode(&self, mode: &str) -> bool {
        self.modes.as_ref().is_some_and(|modes| {
            modes["availableModes"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|entry| entry["id"] == mode)
        })
    }

    /// Apply a `current_mode_update`.
    pub fn set_current_mode(&mut self, mode: &str) {
        if let Some(modes) = self.modes.as_mut() {
            modes["currentModeId"] = Value::String(mode.to_string());
        }
    }
}

/// How a prompt turn ended. Serialized as `{"kind": "stopped",
/// "stopReason": …}`, `{"kind": "failed", "message": …}`,
/// `{"kind": "agentExited"}` or `{"kind": "cancelled"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TurnOutcome {
    /// The agent answered `session/prompt` with a stop reason (verbatim, so
    /// an unknown future reason survives).
    Stopped {
        /// The stop reason.
        stop_reason: String,
    },
    /// The agent answered with an error.
    Failed {
        /// Short error message.
        message: String,
    },
    /// The agent process ended before answering.
    AgentExited,
    /// Cancelled before the prompt was sent: the agent never started it.
    Cancelled,
}

impl TurnOutcome {
    /// From a `session/prompt` result.
    pub fn from_prompt_result(result: &Value) -> Self {
        match result["stopReason"].as_str() {
            Some(reason) => Self::Stopped {
                stop_reason: reason.chars().take(64).collect(),
            },
            None => Self::Failed {
                message: "the agent's prompt response has no stop reason".into(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_require_explicit_advertisement() {
        let bare = negotiate(&json!({"protocolVersion": 1})).expect("minimal");
        assert_eq!(bare.capabilities, AgentCapabilities::default());
        let full = negotiate(&json!({
            "protocolVersion": 1,
            "agentCapabilities": {
                "loadSession": true,
                "promptCapabilities": {"image": true, "audio": false},
                "sessionCapabilities": {"list": {}, "resume": {}, "close": null},
            },
            "agentInfo": {"name": "beta", "version": "2.0"},
            "authMethods": [{"id": "login", "name": "Log in"}],
        }))
        .expect("full");
        assert!(full.capabilities.load_session);
        assert!(full.capabilities.list_sessions);
        assert!(full.capabilities.resume_session);
        assert!(!full.capabilities.close_session);
        assert!(full.capabilities.image);
        assert_eq!(full.agent.version.as_deref(), Some("2.0"));
        assert_eq!(full.auth_methods.len(), 1);
    }

    #[test]
    fn other_versions_are_refused() {
        assert_eq!(
            negotiate(&json!({"protocolVersion": 2})),
            Err(NegotiationError::UnsupportedVersion(2))
        );
        assert_eq!(negotiate(&json!({})), Err(NegotiationError::Malformed));
    }

    #[test]
    fn the_client_advertises_no_host_file_or_terminal_access() {
        let params = initialize_params();
        assert_eq!(params["clientCapabilities"]["fs"]["readTextFile"], false);
        assert_eq!(params["clientCapabilities"]["fs"]["writeTextFile"], false);
        assert_eq!(params["clientCapabilities"]["terminal"], false);
        assert!(params["clientCapabilities"].get("elicitation").is_none());
        assert!(params["clientCapabilities"].get("session").is_none());
    }

    #[test]
    fn permission_options_keep_opaque_ids_and_reject_duplicates() {
        let (session, _, options) = permission_request(&json!({
            "sessionId": "s",
            "toolCall": {"toolCallId": "t"},
            "options": [
                {"optionId": "opt:1 ✓", "name": "Allow", "kind": "allow_once"},
                {"optionId": "x", "name": "Future", "kind": "ask_later"},
            ],
        }))
        .expect("valid");
        assert_eq!(session, "s");
        assert_eq!(options[0].option_id, "opt:1 ✓");
        assert_eq!(options[1].kind, "ask_later");
        assert!(permission_request(&json!({
            "sessionId": "s", "toolCall": {},
            "options": [
                {"optionId": "a", "name": "A", "kind": "allow_once"},
                {"optionId": "a", "name": "B", "kind": "reject_once"},
            ],
        }))
        .is_none());
        assert!(
            permission_request(&json!({"sessionId": "s", "toolCall": {}, "options": []})).is_none()
        );
    }

    #[test]
    fn settings_validate_values_and_drop_booleans() {
        let mut settings = SessionSettings::from_response(&json!({
            "configOptions": [
                {"id": "model", "name": "Model", "type": "select", "currentValue": "a",
                 "options": [{"value": "a", "name": "A"}, {"group": "g", "name": "G", "options": [{"value": "b", "name": "B"}]}]},
                {"id": "fast", "name": "Fast", "type": "boolean", "currentValue": false},
            ],
            "modes": {"currentModeId": "ask", "availableModes": [{"id": "ask", "name": "Ask"}, {"id": "code", "name": "Code"}]},
        }));
        assert_eq!(settings.config_options.len(), 1);
        assert!(settings.allows_config_value("model", "b"));
        assert!(!settings.allows_config_value("model", "z"));
        assert!(!settings.allows_config_value("fast", "true"));
        assert!(settings.allows_mode("code"));
        settings.set_current_mode("code");
        assert_eq!(settings.modes.expect("modes")["currentModeId"], "code");
    }

    #[test]
    fn outcomes_use_the_gateway_wire_shape() {
        let stopped = TurnOutcome::Stopped {
            stop_reason: "end_turn".into(),
        };
        assert_eq!(
            serde_json::to_value(&stopped).expect("encode"),
            json!({"kind": "stopped", "stopReason": "end_turn"})
        );
        assert_eq!(
            serde_json::to_value(TurnOutcome::AgentExited).expect("encode"),
            json!({"kind": "agentExited"})
        );
    }

    #[test]
    fn unknown_stop_reasons_survive() {
        assert_eq!(
            TurnOutcome::from_prompt_result(&json!({"stopReason": "paused_for_review"})),
            TurnOutcome::Stopped {
                stop_reason: "paused_for_review".into()
            }
        );
    }
}
