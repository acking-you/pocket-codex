//! Permission and elicitation requests waiting for a controller (TRD §4.2.7).
//!
//! The first valid answer wins; an invalid one is re-sent to the same
//! connection with a new id and `_meta.pcx.rejected`.

use std::collections::HashMap;

use pocket_codex_core::acp::{
    elicitation_action, rpc::RequestId, ElicitationResponse, PermissionOption, PermissionOutcome,
    RequestPermissionResponse, ToolCallUpdate,
};
use serde_json::{json, Value};

/// What the agent asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum PendingKind {
    /// `session/request_permission`.
    Permission {
        /// The tool call.
        tool_call: Box<ToolCallUpdate>,
        /// Offered options.
        options: Vec<PermissionOption>,
    },
    /// `elicitation/create` in form mode.
    Form {
        /// Message.
        message: String,
        /// Requested schema.
        schema: Value,
    },
    /// `elicitation/create` in URL mode.
    Url {
        /// Message.
        message: String,
        /// URL.
        url: String,
        /// URL-mode id.
        elicitation_id: String,
    },
}

/// One agent request waiting for a controller.
#[derive(Clone, Debug)]
pub struct Pending {
    /// "r{n}", unique within the hub.
    pub id: String,
    /// Owning session; `None` for hub-level elicitations.
    pub session_id: Option<String>,
    /// What is asked.
    pub kind: PendingKind,
    /// Agent request id.
    pub agent_request: RequestId,
    /// Agent process run that sent it.
    pub run_id: u64,
    /// Agent method.
    pub method: String,
    /// Agent params, forwarded verbatim.
    pub params: Value,
    /// Connection → request id used for that connection.
    pub sent_to: HashMap<u64, RequestId>,
}

impl Pending {
    /// Elicitation mode (`form` / `url`), `None` for permissions.
    pub fn mode(&self) -> Option<&'static str> {
        match self.kind {
            PendingKind::Permission {
                ..
            } => None,
            PendingKind::Form {
                ..
            } => Some("form"),
            PendingKind::Url {
                ..
            } => Some("url"),
        }
    }

    /// Params sent to a controller: the agent's params plus
    /// `_meta.pcx.requestId` (and `rejected` when re-sent).
    pub fn controller_params(&self, rejected: Option<&str>) -> Value {
        let mut params = match &self.params {
            Value::Object(map) => Value::Object(map.clone()),
            _ => json!({}),
        };
        let mut pcx = json!({ "requestId": self.id });
        if let Some(reason) = rejected {
            pcx["rejected"] = json!(reason);
        }
        match params.get_mut("_meta") {
            Some(Value::Object(meta)) => {
                meta.insert("pcx".into(), pcx);
            },
            _ => params["_meta"] = json!({ "pcx": pcx }),
        }
        params
    }

    /// The answer sent to the agent when the turn is cancelled.
    pub fn cancel_response(&self) -> Value {
        match self.kind {
            PendingKind::Permission {
                ..
            } => json!({"outcome": {"outcome": "cancelled"}}),
            PendingKind::Form {
                ..
            }
            | PendingKind::Url {
                ..
            } => json!({"action": "cancel"}),
        }
    }

    /// Validate a controller answer; returns the response for the agent or
    /// the rejection reason.
    pub fn validate(&self, answer: &Value) -> Result<Value, String> {
        match &self.kind {
            PendingKind::Permission {
                options, ..
            } => {
                let response: RequestPermissionResponse = serde_json::from_value(answer.clone())
                    .map_err(|e| format!("invalid permission outcome: {e}"))?;
                if let PermissionOutcome::Selected {
                    option_id,
                } = &response.outcome
                {
                    if !options.iter().any(|o| &o.option_id == option_id) {
                        return Err(format!("unknown option `{option_id}`"));
                    }
                }
                serde_json::to_value(&response).map_err(|e| e.to_string())
            },
            PendingKind::Form {
                schema, ..
            } => {
                let response = elicitation(answer)?;
                if response.action == elicitation_action::ACCEPT {
                    let allowed = schema.get("properties").and_then(Value::as_object);
                    if let Some(content) = response.content.as_ref().and_then(Value::as_object) {
                        for key in content.keys() {
                            if !allowed.is_some_and(|props| props.contains_key(key)) {
                                return Err(format!("field `{key}` is not in the form schema"));
                            }
                        }
                    }
                }
                serde_json::to_value(&response).map_err(|e| e.to_string())
            },
            PendingKind::Url {
                ..
            } => {
                let mut response = elicitation(answer)?;
                response.content = None;
                serde_json::to_value(&response).map_err(|e| e.to_string())
            },
        }
    }
}

fn elicitation(answer: &Value) -> Result<ElicitationResponse, String> {
    let response: ElicitationResponse = serde_json::from_value(answer.clone())
        .map_err(|e| format!("invalid elicitation answer: {e}"))?;
    match response.action.as_str() {
        elicitation_action::ACCEPT | elicitation_action::DECLINE | elicitation_action::CANCEL => {
            Ok(response)
        },
        other => Err(format!("unknown action `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(kind: PendingKind) -> Pending {
        Pending {
            id: "r1".into(),
            session_id: Some("s".into()),
            kind,
            agent_request: RequestId::Number(7),
            run_id: 1,
            method: "x".into(),
            params: json!({"sessionId": "s", "_meta": {"other": 1}}),
            sent_to: HashMap::new(),
        }
    }

    #[test]
    fn permission_answers_are_validated() {
        let p = pending(PendingKind::Permission {
            tool_call: Box::default(),
            options: vec![PermissionOption {
                option_id: "allow".into(),
                name: "Allow".into(),
                kind: "allow_once".into(),
                meta: None,
            }],
        });
        assert!(p
            .validate(&json!({"outcome": {"outcome": "selected", "optionId": "allow"}}))
            .is_ok());
        assert!(p
            .validate(&json!({"outcome": {"outcome": "selected", "optionId": "nope"}}))
            .is_err());
        assert!(p
            .validate(&json!({"outcome": {"outcome": "cancelled"}}))
            .is_ok());
        assert!(p.validate(&json!({"bogus": true})).is_err());
        let params = p.controller_params(Some("bad"));
        assert_eq!(params["_meta"]["pcx"], json!({"requestId": "r1", "rejected": "bad"}));
        assert_eq!(params["_meta"]["other"], json!(1));
    }

    #[test]
    fn form_answers_stay_within_schema() {
        let p = pending(PendingKind::Form {
            message: "m".into(),
            schema: json!({"type": "object", "properties": {"name": {"type": "string"}}}),
        });
        assert!(p
            .validate(&json!({"action": "accept", "content": {"name": "x"}}))
            .is_ok());
        assert!(p
            .validate(&json!({"action": "accept", "content": {"other": "x"}}))
            .is_err());
        assert!(p.validate(&json!({"action": "decline"})).is_ok());
        assert!(p.validate(&json!({"action": "maybe"})).is_err());
        assert_eq!(p.cancel_response(), json!({"action": "cancel"}));
    }
}
