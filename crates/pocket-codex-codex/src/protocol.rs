//! JSON-RPC 2.0 envelopes spoken by `codex app-server`.
//!
//! The on-the-wire format is JSON-RPC 2.0 framed as JSON Lines (one
//! object per `\n`-terminated line) on stdio / unix sockets, and as
//! WebSocket text frames over `ws://`. The `jsonrpc` field is *not*
//! required on the wire — Codex omits it — but we tolerate either.
//!
//! Today these types are intentionally schema-less: `params`, `result`
//! and `error.data` use [`serde_json::Value`]. Once the upstream
//! `codex-app-server-protocol` crate stabilises we can introduce
//! strongly-typed variants here.

use serde::{Deserialize, Serialize};

/// Request id used to correlate requests with responses.
///
/// JSON-RPC allows either a string or a number; Pocket-Codex always
/// emits strings (UUIDs) but we accept both inbound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    /// String id, e.g. a UUID.
    String(String),
    /// Numeric id.
    Number(i64),
}

/// Top-level frame on the wire. Tagged externally as one of
/// `request` / `response` / `error` / `notification` based on the
/// presence of `id`/`method`/`result`/`error` fields.
///
/// The variant order matters because we use `serde(untagged)`: more
/// specific shapes (those that *require* the `id` field) come first
/// so the lighter [`Notification`] does not greedily swallow
/// requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Message {
    /// Successful response (has `id` + `result`).
    Response(Response),
    /// Error response (has `id` + `error`).
    Error(ErrorResponse),
    /// Request (has `id` + `method`).
    Request(Request),
    /// Notification (no `id`).
    Notification(Notification),
}

/// A request awaiting a response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// Optional `"jsonrpc": "2.0"` marker; Codex omits it but we keep
    /// the field for round-tripping with strict peers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jsonrpc: Option<String>,

    /// Correlation id.
    pub id: RequestId,

    /// Method name (e.g. `"initialize"`, `"thread/start"`).
    pub method: String,

    /// Method-specific parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// A fire-and-forget notification (no `id`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Optional `"jsonrpc": "2.0"` marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jsonrpc: Option<String>,

    /// Method name (e.g. `"item/started"`).
    pub method: String,

    /// Method-specific parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// A successful response to a request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// Optional `"jsonrpc": "2.0"` marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jsonrpc: Option<String>,

    /// Correlation id matching the originating request.
    pub id: RequestId,

    /// Method-specific result payload.
    pub result: serde_json::Value,
}

/// An error response for a request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// Optional `"jsonrpc": "2.0"` marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jsonrpc: Option<String>,

    /// Correlation id matching the originating request.
    pub id: RequestId,

    /// Error payload.
    pub error: ErrorPayload,
}

/// JSON-RPC error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPayload {
    /// Numeric error code (`-32001` is "server overloaded; retry").
    pub code: i64,
    /// Human-readable description.
    pub message: String,
    /// Optional structured data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_initialize_request() {
        let raw = r#"{"id":"1","method":"initialize","params":{"client":"pocket-codex"}}"#;
        let msg: Message = serde_json::from_str(raw).expect("parse");
        match msg {
            Message::Request(req) => {
                assert_eq!(req.method, "initialize");
                assert_eq!(req.id, RequestId::String("1".into()));
            },
            other => panic!("expected request, got {other:?}"),
        }
    }

    #[test]
    fn parse_notification() {
        let raw = r#"{"method":"item/started","params":{}}"#;
        let msg: Message = serde_json::from_str(raw).expect("parse");
        assert!(matches!(msg, Message::Notification(_)));
    }

    #[test]
    fn request_omits_params_when_none() {
        // No-params methods (e.g. `account/rateLimits/read`) must serialize with
        // no `params` key at all — an empty `{}` is rejected as invalid params.
        let req = Request {
            jsonrpc: None,
            id: RequestId::String("1".into()),
            method: "account/rateLimits/read".into(),
            params: None,
        };
        let v = serde_json::to_value(&req).expect("serialize");
        assert!(v.get("params").is_none(), "must omit params: {v}");
        assert_eq!(v["method"], "account/rateLimits/read");
    }
}
