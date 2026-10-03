//! Bounded parser for OpenCode's `/api/event` server-sent events.
//!
//! The stream is volatile: events emitted while disconnected are lost, and an
//! event id is not a replay cursor. Callers resynchronize from authoritative
//! reads after any error.

use std::{collections::BTreeMap, pin::Pin, time::Duration};

use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Error, Location, Result};

/// Largest accepted single event (bytes, including unfinished lines).
pub(super) const MAX_EVENT_BYTES: usize = 8 * 1024 * 1024;
/// A healthy stream sends a heartbeat comment every ~15 s.
const IDLE_TIMEOUT: Duration = Duration::from_secs(45);

/// Native event envelope.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Event {
    /// Native event identity (not a replay cursor).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Event discriminator, e.g. `session.text.delta`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Native payload.
    #[serde(default)]
    pub data: Value,
    /// Location the event belongs to, when supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    /// Created time and future envelope fields.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Event {
    /// The session this event belongs to, if any (`global` is not a session).
    pub fn session_id(&self) -> Option<&str> {
        let id = match self.kind.as_str() {
            "form.created" | "session.form.created" => self.data["form"]["sessionID"].as_str(),
            _ => self.data["sessionID"].as_str(),
        };
        id.filter(|id| *id != "global")
    }
}

/// Pull-based event stream; ends after the first error.
pub type EventStream = Pin<Box<dyn Stream<Item = Result<Event>> + Send>>;

pub(super) fn stream(response: reqwest::Response) -> EventStream {
    let source = Box::pin(response.bytes_stream());
    Box::pin(futures::stream::unfold(
        (source, Parser::default(), std::collections::VecDeque::new(), false),
        |(mut source, mut parser, mut ready, done)| async move {
            loop {
                if let Some(event) = ready.pop_front() {
                    return Some((Ok(event), (source, parser, ready, done)));
                }
                if done {
                    return None;
                }
                let chunk = match tokio::time::timeout(IDLE_TIMEOUT, source.next()).await {
                    Ok(Some(Ok(bytes))) => bytes,
                    Ok(Some(Err(_))) => {
                        return Some((Err(Error::Transport), (source, parser, ready, true)))
                    },
                    Ok(None) | Err(_) => {
                        return Some((Err(Error::Disconnected), (source, parser, ready, true)));
                    },
                };
                match parser.feed(&chunk) {
                    Ok(events) => ready.extend(events),
                    Err(error) => return Some((Err(error), (source, parser, ready, true))),
                }
            }
        },
    ))
}

/// Incremental SSE framing: `data:` lines accumulate until a blank line.
/// Comments and other fields are ignored. Frames that are not a JSON event
/// envelope are skipped rather than failing the stream.
#[derive(Default)]
pub(super) struct Parser {
    line: Vec<u8>,
    data: Vec<u8>,
    started: bool,
    frame_bytes: usize,
}

impl Parser {
    pub(super) fn feed(&mut self, chunk: &Bytes) -> Result<Vec<Event>> {
        let mut out = Vec::new();
        let mut bytes: &[u8] = chunk;
        if !self.started {
            // Strip a UTF-8 BOM that may be split across chunks.
            self.line.extend_from_slice(bytes);
            if self.line.len() < 3 && [0xef, 0xbb, 0xbf].starts_with(&self.line) {
                return Ok(out);
            }
            let pending = std::mem::take(&mut self.line);
            self.started = true;
            let pending = pending
                .strip_prefix(&[0xef, 0xbb, 0xbf])
                .unwrap_or(&pending)
                .to_vec();
            return self.feed(&Bytes::from(pending));
        }
        while let Some(pos) = bytes.iter().position(|b| *b == b'\n') {
            let (head, rest) = bytes.split_at(pos);
            self.push(head)?;
            let mut line = std::mem::take(&mut self.line);
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.is_empty() {
                if let Some(event) = self.dispatch() {
                    out.push(event);
                }
            } else if let Some(value) = line.strip_prefix(b"data:") {
                let value = value.strip_prefix(b" ").unwrap_or(value);
                if !self.data.is_empty() {
                    self.data.push(b'\n');
                }
                self.data.extend_from_slice(value);
            }
            bytes = &rest[1..];
        }
        self.push(bytes)?;
        Ok(out)
    }

    fn push(&mut self, bytes: &[u8]) -> Result<()> {
        self.frame_bytes += bytes.len();
        if self.frame_bytes > MAX_EVENT_BYTES {
            return Err(Error::Limit);
        }
        self.line.extend_from_slice(bytes);
        Ok(())
    }

    fn dispatch(&mut self) -> Option<Event> {
        self.frame_bytes = 0;
        let data = std::mem::take(&mut self.data);
        if data.is_empty() {
            return None;
        }
        serde_json::from_slice::<Event>(&data)
            .ok()
            .filter(|event| !event.kind.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_split_frames_bom_and_comments() {
        let mut parser = Parser::default();
        let mut events = parser
            .feed(&Bytes::from_static(b"\xef\xbb"))
            .expect("bom head");
        events.extend(
            parser
                .feed(&Bytes::from_static(
                    b"\xbf: heartbeat\n\ndata: {\"type\":\"server.connected\",\"data\":{}}\n",
                ))
                .expect("chunk"),
        );
        assert!(events.is_empty());
        events.extend(
            parser
                .feed(&Bytes::from_static(b"\r\ndata: {\"type\":\"session.text.delta\",\n"))
                .expect("chunk"),
        );
        events.extend(
            parser
                .feed(&Bytes::from_static(
                    b"data: \"data\":{\"sessionID\":\"ses_1\",\"delta\":\"hi\"}}\n\n",
                ))
                .expect("chunk"),
        );
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, "server.connected");
        assert_eq!(events[1].session_id(), Some("ses_1"));
        assert_eq!(events[1].data["delta"], "hi");
    }

    #[test]
    fn oversized_frames_fail_and_garbage_is_skipped() {
        let mut parser = Parser::default();
        let events = parser
            .feed(&Bytes::from_static(b"data: not json\n\n"))
            .expect("skip");
        assert!(events.is_empty());
        let big = vec![b'a'; MAX_EVENT_BYTES + 1];
        assert_eq!(parser.feed(&Bytes::from(big)).expect_err("must fail"), Error::Limit);
    }

    #[test]
    fn global_forms_have_no_session() {
        let event: Event = serde_json::from_value(serde_json::json!({
            "type": "form.created",
            "data": {"form": {"sessionID": "global"}}
        }))
        .expect("event");
        assert_eq!(event.session_id(), None);
    }
}
