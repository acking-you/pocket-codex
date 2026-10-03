//! Contract check of a server's `/openapi.json` against the routes and fields
//! Pocket-Codex relies on. A server passes when nothing is missing, whatever
//! its version; the missing list names exactly what an upgrade removed.

use serde_json::Value;

/// Routes (OpenAPI path template, lowercase method) the controller uses.
pub const REQUIRED_ROUTES: &[(&str, &str)] = &[
    ("/api/info", "get"),
    ("/api/session", "get"),
    ("/api/session", "post"),
    ("/api/session/active", "get"),
    ("/api/session/{sessionID}", "get"),
    ("/api/session/{sessionID}", "patch"),
    ("/api/session/{sessionID}/message", "get"),
    ("/api/session/{sessionID}/prompt", "post"),
    ("/api/session/{sessionID}/interrupt", "post"),
    ("/api/session/{sessionID}/compact", "post"),
    ("/api/session/{sessionID}/model", "post"),
    ("/api/session/{sessionID}/agent", "post"),
    ("/api/session/{sessionID}/permission", "get"),
    ("/api/session/{sessionID}/permission/{requestID}/reply", "post"),
    ("/api/session/{sessionID}/form", "get"),
    ("/api/session/{sessionID}/form/{formID}/reply", "post"),
    ("/api/session/{sessionID}/form/{formID}", "delete"),
    ("/api/model", "get"),
    ("/api/agent", "get"),
    ("/api/vcs/diff", "get"),
    ("/api/event", "get"),
];

/// Result of a contract check.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    /// Missing routes (`METHOD path`) and schema fields (`schema.field`).
    pub missing: Vec<String>,
}

/// Check `doc` (a parsed `/openapi.json`).
pub fn check(doc: &Value) -> Report {
    let mut missing = Vec::new();
    if !doc["openapi"].as_str().is_some_and(|v| v.starts_with("3.")) {
        missing.push("openapi 3.x document".to_string());
        return Report {
            missing,
        };
    }
    for (path, method) in REQUIRED_ROUTES {
        if !doc["paths"][*path][*method].is_object() {
            missing.push(format!("{} {path}", method.to_uppercase()));
        }
    }
    let prompt = request(doc, "/api/session/{sessionID}/prompt");
    for field in ["text", "files", "delivery"] {
        if property(doc, prompt, field).is_null() {
            missing.push(format!("prompt.{field}"));
        }
    }
    let decision = property(
        doc,
        request(doc, "/api/session/{sessionID}/permission/{requestID}/reply"),
        "decision",
    );
    for value in ["once", "always", "reject"] {
        if !decision["enum"]
            .as_array()
            .is_some_and(|values| values.iter().any(|candidate| candidate == value))
        {
            missing.push(format!("permission.decision.{value}"));
        }
    }
    let session = property(doc, response(doc, "/api/session/{sessionID}", "get"), "data");
    for field in ["id", "location"] {
        if property(doc, session, field).is_null() {
            missing.push(format!("session.{field}"));
        }
    }
    let messages = response(doc, "/api/session/{sessionID}/message", "get");
    if property(doc, messages, "cursor").is_null() {
        missing.push("messages.cursor".to_string());
    }
    Report {
        missing,
    }
}

fn response<'a>(doc: &'a Value, path: &str, method: &str) -> &'a Value {
    resolve(
        doc,
        &doc["paths"][path][method]["responses"]["200"]["content"]["application/json"]["schema"],
    )
}

fn request<'a>(doc: &'a Value, path: &str) -> &'a Value {
    resolve(
        doc,
        &doc["paths"][path]["post"]["requestBody"]["content"]["application/json"]["schema"],
    )
}

fn property<'a>(doc: &'a Value, schema: &'a Value, key: &str) -> &'a Value {
    resolve(doc, &resolve(doc, schema)["properties"][key])
}

fn resolve<'a>(doc: &'a Value, mut schema: &'a Value) -> &'a Value {
    for _ in 0..8 {
        let Some(reference) = schema["$ref"].as_str() else {
            return schema;
        };
        let Some(pointer) = reference.strip_prefix('#') else {
            return &Value::Null;
        };
        match doc.pointer(pointer) {
            Some(next) => schema = next,
            None => return &Value::Null,
        }
    }
    &Value::Null
}
