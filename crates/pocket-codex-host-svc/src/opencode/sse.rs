use std::{collections::HashMap, pin::Pin, time::Duration};

use bytes::Bytes;
use eventsource_stream::{EventStreamError, Eventsource};
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Error, OpenCodeClient, Result};

const MAX_EVENT_BYTES: usize = 8 * 1024 * 1024;

/// Native event payload. Its ID is not an SSE replay cursor.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OpenCodeEvent {
    /// Optional native event identity; never sent as Last-Event-ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Upstream event name, including future scoped event kinds.
    #[serde(rename = "type")]
    pub kind: String,
    /// Native properties retained for provider-specific reconciliation.
    pub properties: Value,
}

/// Pull-based event stream with no unbounded internal queue or automatic
/// replay.
pub type OpenCodeEventStream = Pin<Box<dyn Stream<Item = Result<OpenCodeEvent>> + Send>>;

pub(super) fn stream(response: reqwest::Response, client: OpenCodeClient) -> OpenCodeEventStream {
    let mut guard = FrameBudget::default();
    let bytes = response
        .bytes_stream()
        .map(move |chunk| guard.accept(chunk.map_err(|_| Error::Transport)?));
    let events = Box::pin(bytes.eventsource());
    Box::pin(futures::stream::unfold(
        (events, client, false, HashMap::new()),
        |(mut events, client, done, mut scope_cache)| async move {
            if done {
                return None;
            }
            loop {
                let parsed =
                    match tokio::time::timeout(Duration::from_secs(45), events.next()).await {
                        Ok(Some(Ok(frame))) => serde_json::from_str::<OpenCodeEvent>(&frame.data)
                            .map_err(|_| Error::Protocol),
                        Ok(Some(Err(EventStreamError::Transport(error)))) => Err(error),
                        Ok(Some(Err(_))) => Err(Error::Protocol),
                        _ => Err(Error::Disconnected),
                    };
                let event = match parsed {
                    Ok(event) if event.properties.is_object() && !event.kind.is_empty() => event,
                    Ok(_) => {
                        return Some((Err(Error::Protocol), (events, client, true, scope_cache)))
                    },
                    Err(error) => return Some((Err(error), (events, client, true, scope_cache))),
                };
                match client.event_allowed(&event, &mut scope_cache).await {
                    Ok(true) => return Some((Ok(event), (events, client, false, scope_cache))),
                    Ok(false) => {},
                    Err(error) => return Some((Err(error), (events, client, true, scope_cache))),
                }
            }
        },
    ))
}

// Limit bytes before they enter the standard parser, including unfinished
// lines.
#[derive(Default)]
pub(super) struct FrameBudget {
    started: bool,
    prefix: Vec<u8>,
    frame: usize,
    line: usize,
    cr: bool,
}

impl FrameBudget {
    pub(super) fn accept(&mut self, mut bytes: Bytes) -> Result<Bytes> {
        if !self.started {
            self.prefix.extend_from_slice(&bytes);
            if self.prefix.len() < 3 {
                return Ok(Bytes::new());
            }
            let mut prefix = std::mem::take(&mut self.prefix);
            // eventsource-stream 0.2.3 slices a BOM at byte 1; strip it safely first.
            if prefix.starts_with(&[0xef, 0xbb, 0xbf]) {
                prefix.drain(..3);
            }
            bytes = Bytes::from(prefix);
            self.started = true;
        }
        for byte in &bytes {
            self.frame += 1;
            if self.frame > MAX_EVENT_BYTES {
                return Err(Error::Limit);
            }
            if self.cr && *byte == b'\n' {
                self.cr = false;
                continue;
            }
            if matches!(byte, b'\r' | b'\n') {
                if self.line == 0 {
                    self.frame = 0;
                }
                self.line = 0;
                self.cr = *byte == b'\r';
            } else {
                self.line += 1;
                self.cr = false;
            }
        }
        Ok(bytes)
    }
}
