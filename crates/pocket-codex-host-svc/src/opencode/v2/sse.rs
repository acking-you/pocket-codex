use std::{
    collections::{BTreeMap, HashMap},
    pin::Pin,
    time::Duration,
};

use eventsource_stream::{EventStreamError, Eventsource};
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Location, V2Client};
use crate::opencode::{sse::FrameBudget, Error, Result};

/// Native v2 envelope. An event ID is not an SSE replay cursor.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Event {
    /// Native event identity, never used as Last-Event-ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Exact native event discriminator.
    #[serde(rename = "type")]
    pub kind: String,
    /// Native payload, deliberately not renamed to v1 properties.
    pub data: Value,
    /// Server location associated with this event, when supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    /// Native created, durable, metadata and future envelope fields.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Pull-based scoped stream without automatic replay or unbounded buffering.
pub type EventStream = Pin<Box<dyn Stream<Item = Result<Event>> + Send>>;

pub(super) fn stream(response: reqwest::Response, client: V2Client) -> EventStream {
    let bytes = futures::stream::unfold(
        (Box::pin(response.bytes_stream()), FrameBudget::default(), false),
        |(mut source, mut budget, done)| async move {
            if done {
                return None;
            }
            // Heartbeat comments count as transport activity even though the parser
            // discards them.
            let chunk = match tokio::time::timeout(Duration::from_secs(45), source.next()).await {
                Ok(Some(Ok(bytes))) => budget.accept(bytes),
                Ok(Some(Err(_))) => Err(Error::Transport),
                Ok(None) => return None,
                Err(_) => Err(Error::Disconnected),
            };
            let done = chunk.is_err();
            Some((chunk, (source, budget, done)))
        },
    );
    let frames = Box::pin(bytes.eventsource());
    Box::pin(futures::stream::unfold(
        (frames, client, HashMap::new(), false),
        |(mut frames, client, mut scope, done)| async move {
            if done {
                return None;
            }
            loop {
                let parsed = match frames.next().await {
                    Some(Ok(frame)) => {
                        serde_json::from_str::<Event>(&frame.data).map_err(|_| Error::Protocol)
                    },
                    Some(Err(EventStreamError::Transport(error))) => Err(error),
                    Some(Err(_)) => Err(Error::Protocol),
                    None => Err(Error::Disconnected),
                };
                let event = match parsed {
                    Ok(event) if event.data.is_object() && !event.kind.is_empty() => event,
                    Ok(_) => return Some((Err(Error::Protocol), (frames, client, scope, true))),
                    Err(error) => return Some((Err(error), (frames, client, scope, true))),
                };
                match event_allowed(&client, &event, &mut scope).await {
                    Ok(true) => return Some((Ok(event), (frames, client, scope, false))),
                    Ok(false) => {},
                    Err(error) => return Some((Err(error), (frames, client, scope, true))),
                }
            }
        },
    ))
}

async fn event_allowed(
    client: &V2Client,
    event: &Event,
    scope: &mut HashMap<String, bool>,
) -> Result<bool> {
    if event.kind == "server.connected" {
        return Ok(event.data.as_object().is_some_and(|data| data.is_empty())
            && event
                .location
                .as_ref()
                .is_none_or(|location| client.in_scope(location)));
    }
    if !known_session_event(&event.kind) {
        return Ok(false);
    }
    let id = if event.kind == "form.created" {
        event.data["form"]["sessionID"].as_str()
    } else {
        event.data["sessionID"].as_str()
    };
    let Some(id) = id else {
        return Err(Error::Protocol);
    };
    if id == "global" {
        return Ok(false);
    }
    super::client::validate_id(id)?;
    if matches!(event.kind.as_str(), "session.created" | "session.moved") {
        let location: Location =
            serde_json::from_value(event.data["location"].clone()).map_err(|_| Error::Protocol)?;
        let allowed = client.in_scope(&location);
        cache(scope, id, allowed);
        return Ok(allowed
            && event
                .location
                .as_ref()
                .is_none_or(|location| client.in_scope(location)));
    }
    if event
        .location
        .as_ref()
        .is_some_and(|location| !client.in_scope(location))
    {
        return Ok(false);
    }
    if let Some(allowed) = scope.get(id) {
        return Ok(*allowed);
    }
    let allowed = client.session_allowed(id).await?;
    cache(scope, id, allowed);
    Ok(allowed)
}

fn cache(scope: &mut HashMap<String, bool>, id: &str, allowed: bool) {
    if scope.len() >= 256 && !scope.contains_key(id) {
        scope.clear();
    }
    scope.insert(id.to_owned(), allowed);
}

fn known_session_event(kind: &str) -> bool {
    matches!(
        kind,
        "session.created"
            | "session.agent.selected"
            | "session.model.selected"
            | "session.moved"
            | "session.renamed"
            | "session.metadata.updated"
            | "session.permissions"
            | "session.viewed"
            | "session.message.content.updated"
            | "session.usage.recorded"
            | "session.usage.updated"
            | "session.deleted"
            | "session.forked"
            | "session.inbox.delivered"
            | "session.inbox.enqueued"
            | "session.inbox.cancelled"
            | "session.inbox.delivery.changed"
            | "session.execution.started"
            | "session.execution.succeeded"
            | "session.execution.failed"
            | "session.execution.interrupted"
            | "session.instructions.updated"
            | "session.synthetic"
            | "session.skill.activated"
            | "session.shell.started"
            | "session.shell.ended"
            | "session.step.started"
            | "session.step.streamed"
            | "session.step.ended"
            | "session.step.failed"
            | "session.text.started"
            | "session.text.delta"
            | "session.text.ended"
            | "session.reasoning.started"
            | "session.reasoning.delta"
            | "session.reasoning.ended"
            | "session.tool.input.started"
            | "session.tool.input.delta"
            | "session.tool.input.ended"
            | "session.tool.called"
            | "session.tool.progress"
            | "session.tool.success"
            | "session.tool.failed"
            | "session.retry.scheduled"
            | "session.compaction.started"
            | "session.compaction.delta"
            | "session.compaction.ended"
            | "session.compaction.failed"
            | "session.revert.staged"
            | "session.revert.cleared"
            | "session.revert.committed"
            | "permission.asked"
            | "permission.replied"
            | "form.created"
            | "form.replied"
            | "form.cancelled"
    )
}
