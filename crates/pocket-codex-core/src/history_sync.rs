//! Content-verified synchronization of bounded, ordered document windows.
//!
//! A provider owns document interpretation and opaque pagination tokens. The
//! protocol only understands identities, JSON documents and UTF-8 text. Clients
//! advertise fingerprints of bytes they actually retain, not an event offset
//! that might outlive an evicted page. No server-side replay log is required.

use std::collections::BTreeMap;

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Current wire version; unknown versions must fail explicitly.
pub const VERSION: u32 = 1;
/// Maximum number of documents in one window.
pub const MAX_DOCUMENTS: usize = 100;
/// Maximum number of text fingerprints per document.
pub const MAX_TEXT_FIELDS: usize = 128;

/// A provider-independent window selector. Collection names are adapter-owned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowQuery {
    /// Stable session identifier within the authenticated provider namespace.
    pub session: String,
    /// Adapter collection, such as `items`, `groups`, or `metadata`.
    pub collection: String,
    /// Optional group containing the requested documents.
    pub group: Option<String>,
    /// Opaque provider pagination token; never interpreted by synchronization.
    pub cursor: Option<String>,
    /// Maximum number of documents requested.
    pub limit: u32,
    /// Adapter-defined projection, such as a group summary.
    pub projection: Option<String>,
}

impl WindowQuery {
    /// Validate resource limits before invoking a provider.
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.session.is_empty() && self.session.len() <= 512, "invalid session id");
        ensure!(!self.collection.is_empty() && self.collection.len() <= 64, "invalid collection");
        ensure!((1..=MAX_DOCUMENTS as u32).contains(&self.limit), "invalid window limit");
        ensure!(self.cursor.as_ref().is_none_or(|v| v.len() <= 16_384), "cursor too large");
        ensure!(self.group.as_ref().is_none_or(|v| v.len() <= 512), "group too large");
        ensure!(self.projection.as_ref().is_none_or(|v| v.len() <= 64), "projection too large");
        Ok(())
    }
}

/// Authoritative bounded window returned by a history adapter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryWindow {
    /// Schema identifier for document interpretation, independent of transport.
    pub provider: String,
    /// Stable source identity; changes when the underlying history is replaced.
    pub generation: String,
    /// Small provider metadata, including pagination continuation.
    pub metadata: Value,
    /// Authoritative document order, including deletions from this window.
    pub order: Vec<String>,
    /// Documents indexed by stable provider identity.
    pub documents: BTreeMap<String, Value>,
}

impl HistoryWindow {
    /// Verify identities and bounds before trusting or persisting a window.
    pub fn validate(&self) -> Result<()> {
        ensure!(self.documents.len() <= MAX_DOCUMENTS, "too many history documents");
        ensure!(self.order.len() == self.documents.len(), "history order length mismatch");
        let ids: std::collections::BTreeSet<_> = self.order.iter().collect();
        ensure!(ids.len() == self.order.len(), "duplicate history document id");
        ensure!(ids.iter().all(|id| self.documents.contains_key(*id)), "unknown ordered document");
        Ok(())
    }

    /// Describe only this retained window; an evicted window has no manifest.
    pub fn manifest(&self) -> Result<WindowManifest> {
        self.validate()?;
        let documents = self
            .documents
            .iter()
            .map(|(id, value)| {
                let mut text = Vec::new();
                text_fingerprints(value, "", &mut text);
                Ok((id.clone(), DocumentManifest {
                    digest: json_digest(value)?,
                    text,
                }))
            })
            .collect::<Result<_>>()?;
        Ok(WindowManifest {
            provider: self.provider.clone(),
            generation: self.generation.clone(),
            metadata_digest: json_digest(&self.metadata)?,
            documents,
        })
    }
}

/// Fingerprints of a retained history window.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowManifest {
    /// Document schema expected by the client.
    pub provider: String,
    /// Source identity expected by the client.
    pub generation: String,
    /// Digest of retained pagination and other metadata.
    pub metadata_digest: String,
    /// Only documents actually retained on the client.
    pub documents: BTreeMap<String, DocumentManifest>,
}

/// A retained document and its potentially large text fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentManifest {
    /// Digest of the entire document.
    pub digest: String,
    /// UTF-8 text prefixes that can be reused when this document changes.
    pub text: Vec<TextFingerprint>,
}

/// A content-addressed UTF-8 prefix at a JSON pointer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextFingerprint {
    /// RFC 6901 JSON pointer; supports fields nested in objects and arrays.
    pub pointer: String,
    /// Byte length, rather than characters or UTF-16 code units.
    pub bytes: usize,
    /// SHA-256 digest of the retained bytes.
    pub digest: String,
}

/// Stateless window synchronization request.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncRequest {
    /// Protocol version.
    pub version: u32,
    /// Window to reconcile.
    pub query: WindowQuery,
    /// Absent on first load, corruption, or eviction.
    pub known: Option<WindowManifest>,
}

impl SyncRequest {
    /// Validate client-controlled work before reading source data.
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == VERSION, "unsupported history sync version");
        self.query.validate()?;
        if let Some(known) = &self.known {
            ensure!(known.documents.len() <= MAX_DOCUMENTS, "too many known documents");
            for doc in known.documents.values() {
                ensure!(doc.text.len() <= MAX_TEXT_FIELDS, "too many text fingerprints");
                ensure!(doc.text.iter().all(|t| t.pointer.len() <= 4096), "text pointer too long");
            }
        }
        Ok(())
    }
}

/// One text field reconstructed from an authenticated cached prefix.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextAppend {
    /// Prefix that must still be present in the cached document.
    pub base: TextFingerprint,
    /// New UTF-8 suffix; empty means reuse the entire unchanged field.
    pub suffix: String,
}

/// A replacement document with optional retained text prefixes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentDelta {
    /// Stable document id.
    pub id: String,
    /// Required cached document digest when retained prefixes are used.
    pub base_digest: Option<String>,
    /// Replacement template. Reused text fields contain null placeholders.
    pub value: Value,
    /// Text fields to reconstruct before accepting this template.
    pub append: Vec<TextAppend>,
    /// Digest of the complete reconstructed document.
    pub digest: String,
}

/// Reconciliation result. Empty changes means no document body is
/// retransmitted.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncResponse {
    /// Protocol version.
    pub version: u32,
    /// Provider document schema.
    pub provider: String,
    /// Authoritative source identity.
    pub generation: String,
    /// True when the old window cannot be used as a base.
    pub reset: bool,
    /// Omitted when identical to the retained metadata.
    pub metadata: Option<Value>,
    /// Digest verifying retained or replaced metadata.
    pub metadata_digest: String,
    /// Exact membership and order of the requested window after reconciliation.
    pub order: Vec<String>,
    /// Replacements and text appends; absent documents in `order` are removed.
    pub changes: Vec<DocumentDelta>,
}

/// Build a delta using client fingerprints without retaining a server replay
/// log.
pub fn reconcile(window: &HistoryWindow, known: Option<&WindowManifest>) -> Result<SyncResponse> {
    window.validate()?;
    let valid =
        known.filter(|k| k.provider == window.provider && k.generation == window.generation);
    let reset = known.is_some() && valid.is_none();
    let metadata_digest = json_digest(&window.metadata)?;
    let metadata = (!valid.is_some_and(|k| k.metadata_digest == metadata_digest))
        .then(|| window.metadata.clone());
    let mut changes = Vec::new();
    for (id, value) in &window.documents {
        let digest = json_digest(value)?;
        let base = valid.and_then(|k| k.documents.get(id));
        if base.is_some_and(|b| b.digest == digest) {
            continue;
        }
        let mut template = value.clone();
        let mut append = Vec::new();
        if let Some(base) = base {
            for field in &base.text {
                let Some(text) = value.pointer(&field.pointer).and_then(Value::as_str) else {
                    continue;
                };
                let Some(prefix) = text.get(..field.bytes) else { continue };
                if digest_bytes(prefix.as_bytes()) != field.digest {
                    continue;
                }
                if let Some(slot) = template.pointer_mut(&field.pointer) {
                    *slot = Value::Null;
                    append.push(TextAppend {
                        base: field.clone(),
                        suffix: text[field.bytes..].to_owned(),
                    });
                }
            }
        }
        changes.push(DocumentDelta {
            id: id.clone(),
            base_digest: base
                .filter(|_| !append.is_empty())
                .map(|b| b.digest.clone()),
            value: template,
            append,
            digest,
        });
    }
    Ok(SyncResponse {
        version: VERSION,
        provider: window.provider.clone(),
        generation: window.generation.clone(),
        reset,
        metadata,
        metadata_digest,
        order: window.order.clone(),
        changes,
    })
}

/// Verify and apply a response transactionally. The caller must persist the
/// returned window atomically; the supplied base is never modified on failure.
pub fn apply(base: Option<&HistoryWindow>, response: &SyncResponse) -> Result<HistoryWindow> {
    ensure!(response.version == VERSION, "unsupported history sync version");
    ensure!(
        response.order.len() <= MAX_DOCUMENTS && response.changes.len() <= MAX_DOCUMENTS,
        "oversized sync response"
    );
    let base = base.filter(|b| {
        !response.reset && b.provider == response.provider && b.generation == response.generation
    });
    let mut documents = base.map(|b| b.documents.clone()).unwrap_or_default();
    for change in &response.changes {
        let old = documents.get(&change.id);
        // A response may be delivered twice after a retry or a lost acknowledgement.
        if old.is_some_and(|v| json_digest(v).ok().as_ref() == Some(&change.digest)) {
            continue;
        }
        if let Some(expected) = &change.base_digest {
            ensure!(
                old.map(json_digest).transpose()?.as_ref() == Some(expected),
                "history delta base mismatch"
            );
        }
        let mut value = change.value.clone();
        for text in &change.append {
            let original = old
                .and_then(|v| v.pointer(&text.base.pointer))
                .and_then(Value::as_str)
                .context("missing cached text prefix")?;
            ensure!(
                original.len() == text.base.bytes
                    && digest_bytes(original.as_bytes()) == text.base.digest,
                "cached text prefix mismatch"
            );
            let slot = value
                .pointer_mut(&text.base.pointer)
                .context("invalid text delta pointer")?;
            *slot = Value::String(format!("{original}{}", text.suffix));
        }
        ensure!(json_digest(&value)? == change.digest, "history delta digest mismatch");
        documents.insert(change.id.clone(), value);
    }
    documents.retain(|id, _| response.order.contains(id));
    let metadata = response
        .metadata
        .clone()
        .or_else(|| base.map(|b| b.metadata.clone()))
        .context("missing history metadata base")?;
    ensure!(json_digest(&metadata)? == response.metadata_digest, "history metadata mismatch");
    let result = HistoryWindow {
        provider: response.provider.clone(),
        generation: response.generation.clone(),
        metadata,
        order: response.order.clone(),
        documents,
    };
    result.validate()?;
    if result
        .order
        .iter()
        .any(|id| !result.documents.contains_key(id))
    {
        bail!("missing history document base");
    }
    Ok(result)
}

/// SHA-256 of exact bytes, also used for collision-resistant cache namespaces.
pub fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Digest of canonical serde JSON (object keys are ordered without
/// preserve_order).
pub fn json_digest(value: &Value) -> Result<String> {
    Ok(digest_bytes(&serde_json::to_vec(value)?))
}

fn text_fingerprints(value: &Value, pointer: &str, out: &mut Vec<TextFingerprint>) {
    if out.len() >= MAX_TEXT_FIELDS {
        return;
    }
    match value {
        Value::String(s) if s.len() >= 256 => out.push(TextFingerprint {
            pointer: pointer.into(),
            bytes: s.len(),
            digest: digest_bytes(s.as_bytes()),
        }),
        Value::Array(values) => {
            for (i, v) in values.iter().enumerate() {
                text_fingerprints(v, &format!("{pointer}/{i}"), out);
            }
        },
        Value::Object(values) => {
            for (key, v) in values {
                text_fingerprints(
                    v,
                    &format!("{pointer}/{}", key.replace('~', "~0").replace('/', "~1")),
                    out,
                );
            }
        },
        _ => {},
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn window(text: &str) -> HistoryWindow {
        HistoryWindow {
            provider: "fixture/chat".into(),
            generation: "history-1".into(),
            metadata: json!({"next": "older"}),
            order: vec!["message".into()],
            documents: BTreeMap::from([(
                "message".into(),
                json!({"body": [{"text": text}], "state": "running"}),
            )]),
        }
    }

    #[test]
    fn megabyte_message_appends_only_the_new_bytes_across_restart() -> Result<()> {
        let old = window(&"a".repeat(1_000_000));
        let disk = serde_json::to_vec(&old)?;
        let retained: HistoryWindow = serde_json::from_slice(&disk)?;
        let new = window(&format!("{}{}", "a".repeat(1_000_000), "新".repeat(3333)));
        let response = reconcile(&new, Some(&retained.manifest()?))?;
        let bytes = serde_json::to_vec(&response)?.len();
        assert!(bytes < 12_000, "delta was {bytes} bytes");
        let applied = apply(Some(&retained), &response)?;
        assert_eq!(applied, new);
        assert_eq!(apply(Some(&applied), &response)?, new);
        let unchanged = reconcile(&new, Some(&new.manifest()?))?;
        assert!(unchanged.changes.is_empty() && unchanged.metadata.is_none());
        eprintln!(
            "history benchmark: snapshot={} bytes, append_delta={bytes} bytes, unchanged={} bytes",
            disk.len(),
            serde_json::to_vec(&unchanged)?.len()
        );
        Ok(())
    }

    #[test]
    fn rewrites_deletions_and_generation_changes_replace_only_the_window() -> Result<()> {
        let old = window(&"old".repeat(1000));
        let mut new = window(&"rewrite".repeat(1000));
        let response = reconcile(&new, Some(&old.manifest()?))?;
        assert!(response.changes[0].append.is_empty());
        assert_eq!(apply(Some(&old), &response)?, new);
        new.generation = "replacement".into();
        let response = reconcile(&new, Some(&old.manifest()?))?;
        assert!(response.reset);
        assert_eq!(apply(Some(&old), &response)?, new);
        new.order.clear();
        new.documents.clear();
        assert_eq!(apply(Some(&old), &reconcile(&new, Some(&old.manifest()?))?)?, new);
        Ok(())
    }

    #[test]
    fn missing_corrupt_or_out_of_order_bases_cannot_advance_progress() -> Result<()> {
        let old = window(&"a".repeat(1000));
        let new = window(&"a".repeat(2000));
        let response = reconcile(&new, Some(&old.manifest()?))?;
        assert!(apply(None, &response).is_err());
        assert!(apply(Some(&window(&"b".repeat(1000))), &response).is_err());
        assert!(apply(Some(&window(&"a".repeat(3000))), &response).is_err());
        assert_eq!(apply(None, &reconcile(&new, None)?)?, new);
        Ok(())
    }
}
