//! Contract tests: the local ACP types against the pinned official schema
//! (`tests/fixtures/acp/schema-v1.23.0.json`).
//!
//! Every hand-written sample under `fixtures/acp/messages/` is decoded,
//! deserialized into the matching local type, serialized again and then
//! checked structurally against its `$defs` entry: required fields exist,
//! every emitted property is declared (or is `_meta`), and `const` / `enum`
//! constrained values are legal.

use std::{collections::BTreeMap, fs, path::PathBuf};

use pocket_codex_core::acp::{
    rpc::{self, RpcMessage},
    types::{self, methods},
    update::SessionNotification,
    PROTOCOL_VERSION,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{Map, Value};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/acp")
}

fn read_json(path: PathBuf) -> Value {
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

struct Schema {
    defs: Map<String, Value>,
}

impl Schema {
    fn load() -> Self {
        let root = read_json(fixtures().join("schema-v1.23.0.json"));
        let defs = root
            .get("$defs")
            .and_then(Value::as_object)
            .cloned()
            .expect("$defs");
        Self {
            defs,
        }
    }

    fn def(&self, name: &str) -> &Value {
        self.defs
            .get(name)
            .unwrap_or_else(|| panic!("no $defs/{name}"))
    }

    fn resolve<'a>(&'a self, schema: &'a Value) -> &'a Value {
        match schema.get("$ref").and_then(Value::as_str) {
            Some(r) => {
                let name = r.rsplit('/').next().unwrap_or(r);
                self.resolve(self.def(name))
            },
            None => schema,
        }
    }

    /// Every schema declared for each property name, walking `allOf`,
    /// `anyOf` and `oneOf`; `None` when the tree declares no properties at
    /// all (a free-form object).
    fn properties<'a>(&'a self, schema: &'a Value) -> Option<BTreeMap<String, Vec<&'a Value>>> {
        let mut out: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
        let mut any = false;
        self.collect(schema, &mut out, &mut any);
        any.then_some(out)
    }

    fn collect<'a>(
        &'a self,
        schema: &'a Value,
        out: &mut BTreeMap<String, Vec<&'a Value>>,
        any: &mut bool,
    ) {
        let schema = self.resolve(schema);
        if let Some(props) = schema.get("properties").and_then(Value::as_object) {
            *any = true;
            for (name, prop) in props {
                out.entry(name.clone()).or_default().push(prop);
            }
        }
        for key in ["allOf", "anyOf", "oneOf"] {
            for sub in schema
                .get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                self.collect(sub, out, any);
            }
        }
    }

    /// `required` of the schema itself and of its `allOf` members.
    fn required<'a>(&'a self, schema: &'a Value, out: &mut Vec<&'a str>) {
        let schema = self.resolve(schema);
        for name in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(name) = name.as_str() {
                out.push(name);
            }
        }
        for sub in schema
            .get("allOf")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            self.required(sub, out);
        }
    }

    fn check(&self, schema: &Value, value: &Value, path: &str) -> Result<(), String> {
        let schema = self.resolve(schema);
        match value {
            Value::Object(map) => self.check_object(schema, map, path),
            Value::Array(items) => {
                let item_schemas = self.item_schemas(schema);
                for (i, item) in items.iter().enumerate() {
                    let at = format!("{path}[{i}]");
                    if !item_schemas.is_empty() {
                        self.any_of(&item_schemas, item, &at)?;
                    }
                }
                Ok(())
            },
            scalar => self.check_scalar(schema, scalar, path),
        }
    }

    fn item_schemas<'a>(&'a self, schema: &'a Value) -> Vec<&'a Value> {
        let schema = self.resolve(schema);
        let mut out = Vec::new();
        if let Some(items) = schema.get("items") {
            out.push(items);
        }
        for key in ["allOf", "anyOf", "oneOf"] {
            for sub in schema
                .get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                out.extend(self.item_schemas(sub));
            }
        }
        out
    }

    fn any_of(&self, schemas: &[&Value], value: &Value, path: &str) -> Result<(), String> {
        let mut errors = Vec::new();
        for schema in schemas {
            match self.check(schema, value, path) {
                Ok(()) => return Ok(()),
                Err(e) => errors.push(e),
            }
        }
        Err(errors.join(" | "))
    }

    fn check_object(
        &self,
        schema: &Value,
        map: &Map<String, Value>,
        path: &str,
    ) -> Result<(), String> {
        let mut required = Vec::new();
        self.required(schema, &mut required);
        for name in required {
            if !map.contains_key(name) {
                return Err(format!("{path}: missing required `{name}`"));
            }
        }
        let Some(props) = self.properties(schema) else { return Ok(()) };
        for (key, value) in map {
            if key == "_meta" {
                continue;
            }
            let at = format!("{path}.{key}");
            let Some(schemas) = props.get(key) else {
                return Err(format!("{at}: property not in schema"));
            };
            self.any_of(schemas, value, &at)?;
        }
        Ok(())
    }

    fn check_scalar(&self, schema: &Value, value: &Value, path: &str) -> Result<(), String> {
        if let Some(expected) = schema.get("const") {
            if expected != value {
                return Err(format!("{path}: {value} != const {expected}"));
            }
        }
        if let Some(options) = schema.get("enum").and_then(Value::as_array) {
            if !options.contains(value) {
                return Err(format!("{path}: {value} not in enum"));
            }
        }
        for sub in schema
            .get("allOf")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            self.check(sub, value, path)?;
        }
        for key in ["anyOf", "oneOf"] {
            if let Some(subs) = schema.get(key).and_then(Value::as_array) {
                let subs: Vec<&Value> = subs.iter().collect();
                self.any_of(&subs, value, path)?;
            }
        }
        Ok(())
    }
}

/// Deserialize into `T`, serialize back and require an identical value.
fn round_trip<T: DeserializeOwned + Serialize>(value: &Value) -> Result<Value, String> {
    let typed: T =
        serde_json::from_value(value.clone()).map_err(|e| format!("deserialize: {e}"))?;
    serde_json::to_value(&typed).map_err(|e| format!("serialize: {e}"))
}

fn local_round_trip(def: &str, payload: &Value) -> Result<Value, String> {
    use types::*;
    match def {
        "InitializeRequest" => round_trip::<InitializeRequest>(payload),
        "InitializeResponse" => round_trip::<InitializeResponse>(payload),
        "AuthenticateRequest" => round_trip::<AuthenticateRequest>(payload),
        "NewSessionRequest" => round_trip::<NewSessionRequest>(payload),
        "LoadSessionRequest" | "ResumeSessionRequest" => round_trip::<LoadSessionRequest>(payload),
        "NewSessionResponse" | "LoadSessionResponse" | "ResumeSessionResponse" => {
            round_trip::<SessionSetup>(payload)
        },
        "CloseSessionRequest" => round_trip::<CloseSessionRequest>(payload),
        "ListSessionsRequest" => round_trip::<ListSessionsRequest>(payload),
        "ListSessionsResponse" => round_trip::<ListSessionsResponse>(payload),
        "PromptRequest" => round_trip::<PromptRequest>(payload),
        "PromptResponse" => round_trip::<PromptResponse>(payload),
        "CancelNotification" => round_trip::<CancelNotification>(payload),
        "SetSessionConfigOptionRequest" => round_trip::<SetConfigOptionRequest>(payload),
        "SetSessionConfigOptionResponse" => round_trip::<SetConfigOptionResponse>(payload),
        "SetSessionModeRequest" => round_trip::<SetSessionModeRequest>(payload),
        "SessionNotification" => round_trip::<SessionNotification>(payload),
        "RequestPermissionRequest" => round_trip::<RequestPermissionRequest>(payload),
        "RequestPermissionResponse" => round_trip::<RequestPermissionResponse>(payload),
        "CreateElicitationRequest" => round_trip::<ElicitationRequest>(payload),
        "CreateElicitationResponse" => round_trip::<ElicitationResponse>(payload),
        "CompleteElicitationNotification" => round_trip::<ElicitationComplete>(payload),
        "ReadTextFileRequest" => round_trip::<ReadTextFileRequest>(payload),
        "ReadTextFileResponse" => round_trip::<ReadTextFileResponse>(payload),
        "WriteTextFileRequest" => round_trip::<WriteTextFileRequest>(payload),
        // Responses that carry only `_meta`.
        "AuthenticateResponse"
        | "CloseSessionResponse"
        | "SetSessionModeResponse"
        | "WriteTextFileResponse" => round_trip::<CapabilityMarker>(payload),
        other => Err(format!("no local type mapped for {other}")),
    }
}

#[test]
fn every_used_method_exists_in_meta() {
    let meta = read_json(fixtures().join("meta-v1.23.0.json"));
    let mut known: Vec<&str> = Vec::new();
    for group in ["agentMethods", "clientMethods", "protocolMethods"] {
        let map = meta
            .get(group)
            .and_then(Value::as_object)
            .expect("method group");
        known.extend(map.values().filter_map(Value::as_str));
    }
    for method in [
        methods::INITIALIZE,
        methods::AUTHENTICATE,
        methods::SESSION_NEW,
        methods::SESSION_LOAD,
        methods::SESSION_RESUME,
        methods::SESSION_CLOSE,
        methods::SESSION_LIST,
        methods::SESSION_PROMPT,
        methods::SESSION_CANCEL,
        methods::SESSION_SET_CONFIG_OPTION,
        methods::SESSION_SET_MODE,
        methods::SESSION_UPDATE,
        methods::SESSION_REQUEST_PERMISSION,
        methods::ELICITATION_CREATE,
        methods::ELICITATION_COMPLETE,
        methods::FS_READ_TEXT_FILE,
        methods::FS_WRITE_TEXT_FILE,
        methods::CANCEL_REQUEST,
    ] {
        assert!(known.contains(&method), "{method} missing from meta-v1.23.0.json");
    }
    assert_eq!(meta.get("version").and_then(Value::as_u64), Some(u64::from(PROTOCOL_VERSION)));
}

#[test]
fn message_samples_round_trip_and_match_schema() {
    let schema = Schema::load();
    let mut dir: Vec<_> = fs::read_dir(fixtures().join("messages"))
        .expect("messages dir")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    dir.sort();
    assert!(dir.len() >= 40, "expected the full sample set, found {}", dir.len());
    let mut failures = Vec::new();
    for path in dir {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let sample = read_json(path);
        let def = sample
            .get("def")
            .and_then(Value::as_str)
            .expect("def")
            .to_string();
        let frame = sample.get("frame").expect("frame").to_string();
        let message = rpc::decode(frame.as_bytes()).expect("decode frame");
        let payload = match &message {
            RpcMessage::Request {
                params, ..
            }
            | RpcMessage::Notification {
                params, ..
            } => params.clone(),
            RpcMessage::Response {
                result: Ok(result), ..
            } => result.clone(),
            RpcMessage::Response {
                result: Err(e), ..
            } => panic!("{name}: error sample {e}"),
        };
        let emitted = match local_round_trip(&def, &payload) {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            },
        };
        if emitted != payload {
            failures
                .push(format!("{name}: round trip changed\n  in:  {payload}\n  out: {emitted}"));
        }
        if let Err(e) = schema.check(schema.def(&def), &emitted, &def) {
            failures.push(format!("{name}: {e}"));
        }
        // The re-encoded frame decodes to the same message.
        let again = rpc::decode(rpc::encode(&message).as_bytes()).expect("re-decode");
        assert_eq!(again, message, "{name}");
    }
    assert!(failures.is_empty(), "contract failures:\n{}", failures.join("\n"));
}

#[test]
fn validator_rejects_undeclared_properties_and_bad_enums() {
    let schema = Schema::load();
    let bad_prop = serde_json::json!({"stopReason": "end_turn", "extra": 1});
    assert!(schema
        .check(schema.def("PromptResponse"), &bad_prop, "p")
        .is_err());
    let bad_enum = serde_json::json!({"stopReason": "bored"});
    assert!(schema
        .check(schema.def("PromptResponse"), &bad_enum, "p")
        .is_err());
    let missing = serde_json::json!({});
    assert!(schema
        .check(schema.def("PromptResponse"), &missing, "p")
        .is_err());
    let good = serde_json::json!({"stopReason": "cancelled", "_meta": {"x": 1}});
    assert!(schema
        .check(schema.def("PromptResponse"), &good, "p")
        .is_ok());
}

#[test]
fn opencode_2_0_18_initialize_deserializes() {
    let value = read_json(fixtures().join("opencode-2.0.18-initialize.json"));
    let init: types::InitializeResponse = serde_json::from_value(value).expect("initialize");
    assert_eq!(init.protocol_version, 1);
    let caps = &init.agent_capabilities;
    assert!(caps.load_session);
    assert!(caps.prompt_capabilities.image && caps.prompt_capabilities.embedded_context);
    assert!(!caps.prompt_capabilities.audio);
    let session = &caps.session_capabilities;
    assert!(session.list.is_some() && session.resume.is_some() && session.close.is_some());
    assert_eq!(init.auth_methods.len(), 1);
    assert_eq!(init.auth_methods[0].id, "opencode-login");
    assert_eq!(init.auth_methods[0].kind, None);
    assert_eq!(init.agent_info.and_then(|i| i.version).as_deref(), Some("2.0.18"));
}

#[test]
fn replay_fixtures_fold_into_transcripts() {
    use pocket_codex_core::acp::Transcript;
    for (name, turns) in [("claude", 2), ("codex", 1), ("opencode", 1)] {
        let text = fs::read_to_string(fixtures().join(format!("replays/{name}.jsonl")))
            .expect("replay fixture");
        let mut transcript = Transcript::new("e.0");
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let note: SessionNotification = serde_json::from_str(line).expect("notification");
            transcript.apply(&note.update, None);
        }
        assert_eq!(transcript.current_turn(), turns, "{name}");
        assert!(!transcript.items().is_empty(), "{name}");
        assert!(transcript.items().iter().all(|i| i.turn >= 1), "{name}");
    }
}
