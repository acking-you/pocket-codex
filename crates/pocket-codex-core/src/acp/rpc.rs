//! JSON-RPC 2.0 framing for ACP over stdio (NDJSON) and WebSocket.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// A JSON-RPC request id. ACP ids are numbers or strings.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    /// Numeric id.
    Number(i64),
    /// String id.
    Text(String),
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{n}"),
            Self::Text(s) => f.write_str(s),
        }
    }
}

/// One decoded JSON-RPC message.
#[derive(Clone, Debug, PartialEq)]
pub enum RpcMessage {
    /// A request that expects a response.
    Request {
        /// Request id.
        id: RequestId,
        /// Method name.
        method: String,
        /// Parameters; `Value::Null` when omitted.
        params: Value,
    },
    /// A notification (no id, no response).
    Notification {
        /// Method name.
        method: String,
        /// Parameters; `Value::Null` when omitted.
        params: Value,
    },
    /// A response to an earlier request.
    Response {
        /// Id of the request being answered.
        id: RequestId,
        /// `result` or `error`.
        result: std::result::Result<Value, RpcError>,
    },
}

/// A JSON-RPC error object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[error("JSON-RPC error {code}: {message}")]
pub struct RpcError {
    /// Error code (see [`code`]).
    pub code: i64,
    /// Human-readable message.
    pub message: String,
    /// Optional structured data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    /// Build an error without data.
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
}

/// JSON-RPC and ACP error codes, plus the `_pcx` extension codes.
pub mod code {
    /// Invalid JSON.
    pub const PARSE_ERROR: i64 = -32700;
    /// Not a valid request object.
    pub const INVALID_REQUEST: i64 = -32600;
    /// Unknown method.
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// Invalid parameters.
    pub const INVALID_PARAMS: i64 = -32602;
    /// Internal error.
    pub const INTERNAL_ERROR: i64 = -32603;
    /// ACP: authentication required.
    pub const AUTH_REQUIRED: i64 = -32000;
    /// ACP: resource not found.
    pub const RESOURCE_NOT_FOUND: i64 = -32002;
    /// ACP: request cancelled.
    pub const REQUEST_CANCELLED: i64 = -32800;
    /// `_pcx`: the transcript generation changed; re-read the window.
    pub const GENERATION_CHANGED: i64 = -32010;
    /// `_pcx`: the operation can only be done on the host desktop.
    pub const HOST_ONLY: i64 = -32011;
    /// `_pcx`: the agent can neither load nor resume this session.
    pub const SESSION_NOT_LOADABLE: i64 = -32012;
    /// `_pcx`: the agent process is not ready.
    pub const AGENT_UNAVAILABLE: i64 = -32013;
    /// `_pcx`: the session is still loading.
    pub const SESSION_LOADING: i64 = -32014;
}

const PCX_PREFIX: &str = "[acp.";

/// Build a hub error whose message starts with `[acp.<pcx_code>] ` and whose
/// `data` is `{"pcxCode": "acp.<pcx_code>"}` (T16).
///
/// `pcx_code` may be given with or without the leading `acp.`.
pub fn pcx_error(code: i64, pcx_code: &str, message: impl fmt::Display) -> RpcError {
    let bare = pcx_code.strip_prefix("acp.").unwrap_or(pcx_code);
    RpcError {
        code,
        message: format!("{PCX_PREFIX}{bare}] {message}"),
        data: Some(json!({ "pcxCode": format!("acp.{bare}") })),
    }
}

/// Extract `acp.<code>` from a message produced by [`pcx_error`].
///
/// The prefix may appear after other context (for example an `anyhow` chain
/// rendered as `"turn failed: [acp.timeout] …"`); the first occurrence wins.
pub fn pcx_code_of(message: &str) -> Option<&str> {
    let start = message.find(PCX_PREFIX)? + 1;
    let rest = &message[start..];
    let end = rest.find(']')?;
    let code = &rest[..end];
    let name = code.strip_prefix("acp.")?;
    let valid = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    valid.then_some(code)
}

/// Parse one frame; accepts messages with or without `"jsonrpc": "2.0"`.
pub fn decode(bytes: &[u8]) -> std::result::Result<RpcMessage, RpcError> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|e| RpcError::new(code::PARSE_ERROR, format!("invalid JSON: {e}")))?;
    let Value::Object(mut map) = value else {
        return Err(RpcError::new(code::INVALID_REQUEST, "message is not an object"));
    };
    let id = match map.remove("id") {
        None | Some(Value::Null) => None,
        Some(raw) => Some(
            serde_json::from_value::<RequestId>(raw)
                .map_err(|_| RpcError::new(code::INVALID_REQUEST, "invalid id"))?,
        ),
    };
    if let Some(method) = map.remove("method") {
        let Value::String(method) = method else {
            return Err(RpcError::new(code::INVALID_REQUEST, "method is not a string"));
        };
        let params = map.remove("params").unwrap_or(Value::Null);
        return Ok(match id {
            Some(id) => RpcMessage::Request {
                id,
                method,
                params,
            },
            None => RpcMessage::Notification {
                method,
                params,
            },
        });
    }
    let Some(id) = id else {
        return Err(RpcError::new(code::INVALID_REQUEST, "response without id"));
    };
    if let Some(error) = map.remove("error") {
        let error: RpcError = serde_json::from_value(error)
            .map_err(|_| RpcError::new(code::INVALID_REQUEST, "invalid error object"))?;
        return Ok(RpcMessage::Response {
            id,
            result: Err(error),
        });
    }
    match map.remove("result") {
        Some(result) => Ok(RpcMessage::Response {
            id,
            result: Ok(result),
        }),
        None => Err(RpcError::new(code::INVALID_REQUEST, "response without result or error")),
    }
}

/// Serialize with `"jsonrpc": "2.0"` for a WebSocket text frame.
pub fn encode(message: &RpcMessage) -> String {
    Value::Object(to_map(message)).to_string()
}

/// `encode` plus a trailing `\n` for stdio; serde_json never emits a raw
/// newline.
pub fn encode_line(message: &RpcMessage) -> Vec<u8> {
    let mut out = encode(message).into_bytes();
    out.push(b'\n');
    out
}

fn id_value(id: &RequestId) -> Value {
    match id {
        RequestId::Number(n) => Value::from(*n),
        RequestId::Text(s) => Value::String(s.clone()),
    }
}

fn to_map(message: &RpcMessage) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("jsonrpc".into(), Value::String("2.0".into()));
    match message {
        RpcMessage::Request {
            id,
            method,
            params,
        } => {
            map.insert("id".into(), id_value(id));
            map.insert("method".into(), Value::String(method.clone()));
            if !params.is_null() {
                map.insert("params".into(), params.clone());
            }
        },
        RpcMessage::Notification {
            method,
            params,
        } => {
            map.insert("method".into(), Value::String(method.clone()));
            if !params.is_null() {
                map.insert("params".into(), params.clone());
            }
        },
        RpcMessage::Response {
            id,
            result,
        } => {
            map.insert("id".into(), id_value(id));
            match result {
                Ok(value) => {
                    map.insert("result".into(), value.clone());
                },
                Err(error) => {
                    let error = serde_json::to_value(error).unwrap_or_else(
                        |_| json!({ "code": error.code, "message": error.message }),
                    );
                    map.insert("error".into(), error);
                },
            }
        },
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_accepts_frames_without_jsonrpc_field() {
        let request = decode(br#"{"id":1,"method":"initialize","params":{"protocolVersion":1}}"#)
            .expect("request");
        assert_eq!(request, RpcMessage::Request {
            id: RequestId::Number(1),
            method: "initialize".into(),
            params: json!({"protocolVersion": 1}),
        });
        let note = decode(br#"{"jsonrpc":"2.0","method":"session/cancel"}"#).expect("note");
        assert_eq!(note, RpcMessage::Notification {
            method: "session/cancel".into(),
            params: Value::Null
        });
        let response = decode(br#"{"id":"a","result":{}}"#).expect("response");
        assert_eq!(response, RpcMessage::Response {
            id: RequestId::Text("a".into()),
            result: Ok(json!({}))
        });
        let error = decode(br#"{"id":2,"error":{"code":-32601,"message":"nope"}}"#).expect("err");
        assert_eq!(error, RpcMessage::Response {
            id: RequestId::Number(2),
            result: Err(RpcError::new(code::METHOD_NOT_FOUND, "nope")),
        });
        assert_eq!(decode(b"not json").expect_err("parse").code, code::PARSE_ERROR);
        assert_eq!(decode(b"[1]").expect_err("shape").code, code::INVALID_REQUEST);
    }

    #[test]
    fn encode_line_has_single_trailing_newline() {
        let message = RpcMessage::Notification {
            method: "session/update".into(),
            params: json!({"text": "line one\nline two"}),
        };
        let line = encode_line(&message);
        assert_eq!(line.iter().filter(|b| **b == b'\n').count(), 1);
        assert_eq!(line.last(), Some(&b'\n'));
        let back = decode(&line[..line.len() - 1]).expect("round trip");
        assert_eq!(back, message);
        assert!(encode(&message).contains("\"jsonrpc\":\"2.0\""));
    }

    #[test]
    fn pcx_error_prefix_round_trips() {
        let error = pcx_error(code::GENERATION_CHANGED, "generation_changed", "re-read");
        assert_eq!(error.message, "[acp.generation_changed] re-read");
        assert_eq!(error.data, Some(json!({"pcxCode": "acp.generation_changed"})));
        assert_eq!(pcx_code_of(&error.message), Some("acp.generation_changed"));
        let same = pcx_error(code::INTERNAL_ERROR, "acp.timeout", "slow");
        assert_eq!(same.message, "[acp.timeout] slow");
        assert_eq!(pcx_code_of("request failed: [acp.timeout] slow"), Some("acp.timeout"));
        assert_eq!(pcx_code_of("plain failure"), None);
        assert_eq!(pcx_code_of("[acp.] empty"), None);
        assert_eq!(pcx_code_of("[acp.Bad Code] x"), None);
    }
}
