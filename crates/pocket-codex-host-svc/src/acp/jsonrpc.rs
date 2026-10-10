//! Strict JSON-RPC 2.0 envelopes for the Agent Client Protocol.
//!
//! These are deliberately separate from the Codex app-server envelopes in
//! `pocket_codex_codex::protocol`, which tolerate a missing `jsonrpc` marker
//! and fall back to notification shapes. ACP requires JSON-RPC 2.0, permits an
//! explicit `null` request id (distinct from an absent id, which makes a
//! notification) and signed 64-bit numeric ids, and every frame must be
//! exactly one of request, notification, success response or error response.

use serde_json::{json, Map, Value};

/// A JSON-RPC request id as ACP defines it: `null`, an `int64`, or a string.
///
/// String and numeric ids never compare equal (`7` is not `"7"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RequestId {
    /// The explicit `null` id.
    Null,
    /// A signed 64-bit numeric id.
    Number(i64),
    /// A string id.
    Str(String),
}

impl RequestId {
    /// Parse an id value; fractional, unsigned-overflowing or structured
    /// values are rejected.
    pub fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Null => Some(Self::Null),
            Value::String(text) => Some(Self::Str(text.clone())),
            Value::Number(number) => number.as_i64().map(Self::Number),
            _ => None,
        }
    }

    /// The wire value of this id.
    pub fn to_value(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Number(number) => Value::from(*number),
            Self::Str(text) => Value::String(text.clone()),
        }
    }
}

/// A JSON-RPC error object.
#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    /// Integer error code.
    pub code: i64,
    /// Short human-readable description.
    pub message: String,
    /// Optional structured details, preserved without interpretation.
    pub data: Option<Value>,
}

impl RpcError {
    /// `-32700`: invalid JSON.
    pub const PARSE: i64 = -32700;
    /// `-32600`: not a valid request object.
    pub const INVALID_REQUEST: i64 = -32600;
    /// `-32601`: method not found or not available.
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// `-32602`: invalid parameters.
    pub const INVALID_PARAMS: i64 = -32602;
    /// `-32603`: internal error.
    pub const INTERNAL: i64 = -32603;
    /// `-32800`: request cancelled.
    pub const CANCELLED: i64 = -32800;
    /// `-32000`: ACP authentication required.
    pub const AUTH_REQUIRED: i64 = -32000;

    /// An error with no data.
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    fn to_value(&self) -> Value {
        let mut object = Map::new();
        object.insert("code".into(), Value::from(self.code));
        object.insert("message".into(), Value::String(self.message.clone()));
        if let Some(data) = &self.data {
            object.insert("data".into(), data.clone());
        }
        Value::Object(object)
    }
}

/// One decoded frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    /// A request expecting a response with the same id.
    Request {
        /// Correlation id (may be `null`).
        id: RequestId,
        /// Method name.
        method: String,
        /// Structured parameters, when present.
        params: Option<Value>,
    },
    /// A notification (no id member at all).
    Notification {
        /// Method name.
        method: String,
        /// Structured parameters, when present.
        params: Option<Value>,
    },
    /// A response to an earlier request.
    Response {
        /// Id of the request being answered.
        id: RequestId,
        /// The result, or the error object.
        result: Result<Value, RpcError>,
    },
}

/// Why a line is not a valid frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// The line is not JSON.
    #[error("invalid JSON")]
    Json,
    /// The JSON is valid but not a JSON-RPC 2.0 frame.
    #[error("invalid JSON-RPC frame")]
    Invalid,
}

impl FrameError {
    /// The JSON-RPC error code that reports this failure.
    pub fn code(self) -> i64 {
        match self {
            Self::Json => RpcError::PARSE,
            Self::Invalid => RpcError::INVALID_REQUEST,
        }
    }
}

/// Decode one NDJSON line.
pub fn parse(line: &[u8]) -> Result<Frame, FrameError> {
    let value: Value = serde_json::from_slice(line).map_err(|_| FrameError::Json)?;
    let Value::Object(mut object) = value else {
        return Err(FrameError::Invalid);
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(FrameError::Invalid);
    }
    let id = match object.get("id") {
        None => None,
        Some(raw) => Some(RequestId::from_value(raw).ok_or(FrameError::Invalid)?),
    };
    let has_result = object.contains_key("result");
    let has_error = object.contains_key("error");
    if let Some(method) = object.remove("method") {
        let Value::String(method) = method else {
            return Err(FrameError::Invalid);
        };
        if has_result || has_error {
            return Err(FrameError::Invalid);
        }
        let params = match object.remove("params") {
            None => None,
            Some(params @ (Value::Object(_) | Value::Array(_))) => Some(params),
            Some(_) => return Err(FrameError::Invalid),
        };
        return Ok(match id {
            Some(id) => Frame::Request {
                id,
                method,
                params,
            },
            None => Frame::Notification {
                method,
                params,
            },
        });
    }
    let id = id.ok_or(FrameError::Invalid)?;
    let result = match (object.remove("result"), object.remove("error")) {
        (Some(result), None) => Ok(result),
        (None, Some(error)) => Err(parse_error_object(error)?),
        _ => return Err(FrameError::Invalid),
    };
    Ok(Frame::Response {
        id,
        result,
    })
}

fn parse_error_object(value: Value) -> Result<RpcError, FrameError> {
    let Value::Object(mut object) = value else {
        return Err(FrameError::Invalid);
    };
    let code = object
        .get("code")
        .and_then(Value::as_i64)
        .ok_or(FrameError::Invalid)?;
    let message = match object.remove("message") {
        Some(Value::String(message)) => message,
        _ => return Err(FrameError::Invalid),
    };
    Ok(RpcError {
        code,
        message,
        data: object.remove("data"),
    })
}

impl Frame {
    /// Encode as one NDJSON line (terminated by `\n`). `serde_json` escapes
    /// every newline inside strings, so the frame never spans lines.
    pub fn to_line(&self) -> Vec<u8> {
        let value = match self {
            Self::Request {
                id,
                method,
                params,
            } => {
                let mut frame = json!({"jsonrpc": "2.0", "id": id.to_value(), "method": method});
                if let Some(params) = params {
                    frame["params"] = params.clone();
                }
                frame
            },
            Self::Notification {
                method,
                params,
            } => {
                let mut frame = json!({"jsonrpc": "2.0", "method": method});
                if let Some(params) = params {
                    frame["params"] = params.clone();
                }
                frame
            },
            Self::Response {
                id,
                result,
            } => match result {
                Ok(result) => json!({"jsonrpc": "2.0", "id": id.to_value(), "result": result}),
                Err(error) => {
                    json!({"jsonrpc": "2.0", "id": id.to_value(), "error": error.to_value()})
                },
            },
        };
        let mut line = serde_json::to_vec(&value).unwrap_or_default();
        line.push(b'\n');
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(raw: &str) -> Result<Frame, FrameError> {
        parse(raw.as_bytes())
    }

    #[test]
    fn null_string_and_number_ids_stay_distinct() {
        let null = frame(r#"{"jsonrpc":"2.0","id":null,"method":"x"}"#).expect("null id");
        assert!(matches!(null, Frame::Request {
            id: RequestId::Null,
            ..
        }));
        let number = frame(r#"{"jsonrpc":"2.0","id":7,"method":"x"}"#).expect("number");
        let text = frame(r#"{"jsonrpc":"2.0","id":"7","method":"x"}"#).expect("string");
        let (
            Frame::Request {
                id: a, ..
            },
            Frame::Request {
                id: b, ..
            },
        ) = (number, text)
        else {
            panic!("requests expected");
        };
        assert_ne!(a, b);
        assert_eq!(a, RequestId::Number(7));
        assert_eq!(b, RequestId::Str("7".into()));
        let negative = frame(r#"{"jsonrpc":"2.0","id":-9223372036854775808,"method":"x"}"#)
            .expect("int64 minimum");
        assert!(matches!(negative, Frame::Request {
            id: RequestId::Number(i64::MIN),
            ..
        }));
    }

    #[test]
    fn absent_id_is_a_notification_not_a_null_request() {
        let note = frame(r#"{"jsonrpc":"2.0","method":"session/update","params":{}}"#)
            .expect("notification");
        assert!(matches!(note, Frame::Notification { .. }));
    }

    #[test]
    fn invalid_shapes_are_rejected() {
        for raw in [
            r#"{"id":1,"method":"x"}"#,
            r#"{"jsonrpc":"1.0","id":1,"method":"x"}"#,
            r#"{"jsonrpc":"2.0","id":1.5,"method":"x"}"#,
            r#"{"jsonrpc":"2.0","id":18446744073709551615,"method":"x"}"#,
            r#"{"jsonrpc":"2.0","id":{},"method":"x"}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"x","result":1}"#,
            r#"{"jsonrpc":"2.0","id":1,"result":1,"error":{"code":1,"message":"m"}}"#,
            r#"{"jsonrpc":"2.0","result":1}"#,
            r#"{"jsonrpc":"2.0","id":1}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":7}"#,
            r#"{"jsonrpc":"2.0","method":"x","params":"text"}"#,
            r#"{"jsonrpc":"2.0","id":1,"error":{"message":"m"}}"#,
            r#"[1,2]"#,
        ] {
            assert_eq!(frame(raw), Err(FrameError::Invalid), "{raw}");
        }
        assert_eq!(frame("{not json"), Err(FrameError::Json));
    }

    #[test]
    fn error_data_and_unknown_fields_round_trip() {
        let parsed = frame(
            r#"{"jsonrpc":"2.0","id":"a","error":{"code":-32000,"message":"auth","data":{"k":1}},"extra":true}"#,
        )
        .expect("error response");
        let Frame::Response {
            id,
            result: Err(error),
        } = &parsed
        else {
            panic!("error response expected");
        };
        assert_eq!(id, &RequestId::Str("a".into()));
        assert_eq!(error.code, RpcError::AUTH_REQUIRED);
        assert_eq!(error.data, Some(json!({"k": 1})));
        let line = parsed.to_line();
        assert_eq!(line.last(), Some(&b'\n'));
        assert_eq!(parse(&line[..line.len() - 1]).expect("re-parse"), parsed);
    }

    #[test]
    fn encoded_frames_never_contain_raw_newlines() {
        let line = Frame::Notification {
            method: "m".into(),
            params: Some(json!({"text": "a\nb"})),
        }
        .to_line();
        assert_eq!(line.iter().filter(|b| **b == b'\n').count(), 1);
    }
}
