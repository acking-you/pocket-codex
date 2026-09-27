use serde_json::Value;

use crate::opencode::{Error, Result};

pub(super) fn validate(doc: &Value) -> Result<()> {
    if !doc["openapi"]
        .as_str()
        .is_some_and(|version| version.starts_with("3."))
        || super::client::REQUIRED_ROUTES
            .iter()
            .any(|(path, method)| !doc["paths"][*path][*method].is_object())
    {
        return Err(Error::Protocol);
    }
    let prompt = request(doc, "/api/session/{sessionID}/prompt");
    if property(doc, prompt, "text")["type"] != "string" || !required(prompt, "text") {
        return Err(Error::Protocol);
    }
    if !identity_and_history(doc) || !mutations_and_events(doc) {
        return Err(Error::Protocol);
    }
    Ok(())
}

fn identity_and_history(doc: &Value) -> bool {
    let info = response(doc, "/api/info", "get");
    let sessions = response(doc, "/api/session", "get");
    let session = property(doc, response(doc, "/api/session/{sessionID}", "get"), "data");
    let messages = response(doc, "/api/session/{sessionID}/message", "get");
    let variants = resolve(doc, &property(doc, messages, "data")["items"]);
    object(doc, info, &[
        ("version", "string"),
        ("pid", "integer"),
        ("urls", "array"),
        ("paths", "object"),
    ]) && object(doc, sessions, &[("data", "array"), ("cursor", "object")])
        && object(doc, session, &[("id", "string"), ("location", "object")])
        && object(doc, property(doc, session, "location"), &[("directory", "string")])
        && object(doc, messages, &[("data", "array"), ("cursor", "object")])
        && message_variant(doc, variants, "user", "text", "string")
        && message_variant(doc, variants, "assistant", "content", "array")
}

fn mutations_and_events(doc: &Value) -> bool {
    let admission = property(doc, response(doc, "/api/session/{sessionID}/prompt", "post"), "data");
    let interrupt = response(doc, "/api/session/{sessionID}/interrupt", "post");
    let permission = request(doc, "/api/session/{sessionID}/permission/{requestID}/reply");
    let decision = property(doc, permission, "decision");
    let form = request(doc, "/api/session/{sessionID}/form/{formID}/reply");
    let event = resolve(
        doc,
        &doc["paths"]["/api/event"]["get"]["responses"]["200"]["content"]["text/event-stream"]
            ["schema"],
    );
    let data = property(doc, event, "data");
    object(doc, admission, &[
        ("id", "string"),
        ("sessionID", "string"),
        ("type", "string"),
        ("payload", "object"),
        ("delivery", "string"),
    ]) && object(doc, interrupt, &[("interrupted", "boolean")])
        && object(doc, permission, &[("decision", "string")])
        && ["once", "always", "reject"].iter().all(|value| {
            decision["enum"]
                .as_array()
                .is_some_and(|values| values.iter().any(|candidate| candidate == value))
        })
        && object(doc, form, &[("answer", "object")])
        && object(doc, event, &[("data", "string")])
        && data["contentMediaType"] == "application/json"
}

fn message_variant(
    doc: &Value,
    variants: &Value,
    kind: &str,
    content: &str,
    content_type: &str,
) -> bool {
    variants["anyOf"].as_array().is_some_and(|variants| {
        variants.iter().any(|variant| {
            let variant = resolve(doc, variant);
            let kind_schema = property(doc, variant, "type");
            kind_schema["enum"]
                .as_array()
                .is_some_and(|values| values.iter().any(|value| value == kind))
                && object(doc, variant, &[
                    ("id", "string"),
                    ("type", "string"),
                    (content, content_type),
                ])
        })
    })
}

fn object(doc: &Value, schema: &Value, fields: &[(&str, &str)]) -> bool {
    let schema = resolve(doc, schema);
    schema["type"] == "object"
        && fields
            .iter()
            .all(|(key, kind)| required(schema, key) && property(doc, schema, key)["type"] == *kind)
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

fn required(schema: &Value, key: &str) -> bool {
    schema["required"]
        .as_array()
        .is_some_and(|fields| fields.iter().any(|field| field == key))
}

fn resolve<'a>(doc: &'a Value, mut schema: &'a Value) -> &'a Value {
    for _ in 0..8 {
        let Some(reference) = schema["$ref"].as_str() else {
            return schema;
        };
        let Some(pointer) = reference.strip_prefix('#') else {
            return &Value::Null;
        };
        if !pointer.starts_with("/components/schemas/") {
            return &Value::Null;
        }
        let Some(next) = doc.pointer(pointer) else {
            return &Value::Null;
        };
        schema = next;
    }
    &Value::Null
}
