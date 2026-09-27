use std::{collections::BTreeMap, time::Duration};

use eventsource_stream::Eventsource;
use futures::StreamExt;
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::{
    EventStream, GatewayInfo, NativeEvent, NativeMessage, NativePage, NativePermission,
    NativeQuestion,
};
use crate::opencode::{
    client::validate_id, Error, OpenCodeClient, PermissionReply, PromptInput, Result, Session,
};

const PREFIX: &str = "pocket/opencode/v2";

/// Memory-only connection to an explicitly versioned, scope-limited Pocket
/// gateway.
#[derive(Clone, Debug)]
pub struct Remote {
    client: OpenCodeClient,
    pub(super) info: GatewayInfo,
}

impl Remote {
    pub(super) async fn probe(client: OpenCodeClient) -> Result<Option<Self>> {
        let info: GatewayInfo = match client.get(&format!("{PREFIX}/info"), &[]).await {
            Ok(info) => info,
            Err(Error::Rejected(404) | Error::Protocol) => return Ok(None),
            Err(error) => return Err(error),
        };
        if info.gateway_protocol != 2
            || !matches!(info.upstream_protocol.as_str(), "v1" | "v2")
            || info.version.is_empty()
            || info.version.len() > 128
        {
            return Err(Error::Protocol);
        }
        Ok(Some(Self {
            client,
            info,
        }))
    }

    pub(super) fn directory(&self) -> &str {
        self.client.directory()
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> Result<T> {
        self.client.get(&format!("{PREFIX}/{path}"), query).await
    }

    async fn post<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T> {
        let response = self
            .client
            .request(Method::POST, &format!("{PREFIX}/{path}"), &[])?
            .json(&body)
            .send()
            .await
            .map_err(|_| Error::SubmissionUnknown)?;
        OpenCodeClient::read_json(response)
            .await
            .map(|(value, _)| value)
            .map_err(|error| match error {
                Error::Rejected(_) => error,
                _ => Error::SubmissionUnknown,
            })
    }

    async fn mutate(&self, path: &str, body: Value) -> Result<()> {
        let accepted: bool = self.post(path, body).await?;
        if !accepted {
            return Err(Error::SubmissionUnknown);
        }
        Ok(())
    }

    pub(super) async fn sessions(&self, search: Option<&str>) -> Result<Vec<Session>> {
        if search.is_some_and(|value| value.len() > 4096) {
            return Err(Error::InvalidInput);
        }
        let query = search
            .map(|value| vec![("search", value)])
            .unwrap_or_default();
        let sessions: Vec<Session> = self.get("session", &query).await?;
        if sessions.len() > 100 {
            return Err(Error::Limit);
        }
        Ok(sessions)
    }

    pub(super) async fn session(&self, id: &str) -> Result<Session> {
        validate_id(id)?;
        let session: Session = self.get(&format!("session/{id}"), &[]).await?;
        if session.id != id {
            return Err(Error::Protocol);
        }
        Ok(session)
    }

    pub(super) async fn history(
        &self,
        id: &str,
        limit: u32,
        before: Option<&str>,
    ) -> Result<NativePage> {
        validate_id(id)?;
        if !(1..=100).contains(&limit)
            || before.is_some_and(|value| value.is_empty() || value.len() > 16_384)
        {
            return Err(Error::InvalidInput);
        }
        let limit_text = limit.to_string();
        let mut query = vec![("limit", limit_text.as_str())];
        if let Some(before) = before {
            query.push(("before", before));
        }
        let page: NativePage = self.get(&format!("session/{id}/message"), &query).await?;
        if page.messages.len() > limit as usize
            || page
                .next_cursor
                .as_ref()
                .is_some_and(|value| value.is_empty() || value.len() > 16_384)
        {
            return Err(Error::Protocol);
        }
        for message in &page.messages {
            self.validate_message(message, id)?;
        }
        Ok(page)
    }

    fn validate_message(&self, message: &NativeMessage, id: &str) -> Result<()> {
        validate_id(message.id())?;
        match message {
            NativeMessage::V1(message)
                if self.info.upstream_protocol == "v1"
                    && message.info.session_id == id
                    && message.parts.iter().all(|part| {
                        part.session_id == id && part.message_id == message.info.id
                    }) =>
            {
                Ok(())
            },
            NativeMessage::V2(message)
                if self.info.upstream_protocol == "v2" && !message.kind.is_empty() =>
            {
                Ok(())
            },
            _ => Err(Error::Protocol),
        }
    }

    pub(super) async fn message(&self, id: &str, message_id: &str) -> Result<NativeMessage> {
        validate_id(id)?;
        validate_id(message_id)?;
        let message: NativeMessage = self
            .get(&format!("session/{id}/message/{message_id}"), &[])
            .await?;
        self.validate_message(&message, id)?;
        if message.id() != message_id {
            return Err(Error::Protocol);
        }
        Ok(message)
    }

    pub(super) async fn create(&self, title: Option<&str>) -> Result<Session> {
        if title.is_some_and(|title| title.len() > 4096) {
            return Err(Error::InvalidInput);
        }
        self.post("session", json!({"title":title})).await
    }

    pub(super) async fn prompt(&self, id: &str, input: &PromptInput) -> Result<()> {
        validate_id(id)?;
        self.mutate(
            &format!("session/{id}/prompt"),
            serde_json::to_value(input).map_err(|_| Error::InvalidInput)?,
        )
        .await
    }

    pub(super) async fn status(&self) -> Result<BTreeMap<String, Value>> {
        let statuses: BTreeMap<String, Value> = self.get("session/status", &[]).await?;
        if statuses.len() > 1024 {
            return Err(Error::Limit);
        }
        if statuses
            .values()
            .any(|status| !matches!(status["type"].as_str(), Some("idle" | "busy" | "retry")))
        {
            return Err(Error::Protocol);
        }
        Ok(statuses)
    }

    pub(super) async fn permissions(&self) -> Result<Vec<NativePermission>> {
        let pending: Vec<NativePermission> = self.get("permission", &[]).await?;
        if pending.len() > 1024 {
            return Err(Error::Limit);
        }
        Ok(pending)
    }

    pub(super) async fn reply_permission(
        &self,
        id: &str,
        reply: PermissionReply,
        message: Option<&str>,
    ) -> Result<()> {
        validate_id(id)?;
        self.mutate(&format!("permission/{id}/reply"), json!({"reply":reply,"message":message}))
            .await
    }

    pub(super) async fn questions(&self) -> Result<Vec<NativeQuestion>> {
        let pending: Vec<NativeQuestion> = self.get("question", &[]).await?;
        if pending.len() > 1024 {
            return Err(Error::Limit);
        }
        Ok(pending)
    }

    pub(super) async fn reply_question(&self, id: &str, answers: Vec<Vec<String>>) -> Result<()> {
        validate_id(id)?;
        self.mutate(&format!("question/{id}/reply"), json!({"answers":answers}))
            .await
    }

    pub(super) async fn reply_form(&self, id: &str, answers: Value) -> Result<()> {
        validate_id(id)?;
        self.mutate(&format!("form/{id}/reply"), json!({"answer":answers}))
            .await
    }

    pub(super) async fn reject_question(&self, id: &str) -> Result<()> {
        validate_id(id)?;
        self.mutate(&format!("question/{id}/reject"), json!({}))
            .await
    }

    pub(super) async fn abort(&self, id: &str) -> Result<()> {
        validate_id(id)?;
        self.mutate(&format!("session/{id}/abort"), json!({})).await
    }

    pub(super) async fn events(&self) -> Result<EventStream> {
        let response = tokio::time::timeout(
            Duration::from_secs(30),
            self.client
                .raw_request(Method::GET, &format!("{PREFIX}/event"), &[])?
                .send(),
        )
        .await
        .map_err(|_| Error::Disconnected)?
        .map_err(|_| Error::Transport)?;
        if !response.status().is_success() {
            return Err(Error::Rejected(response.status().as_u16()));
        }
        if !response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .is_some_and(|s| s.starts_with("text/event-stream"))
        {
            return Err(Error::Protocol);
        }
        let bytes = Box::pin(response.bytes_stream());
        let chunks = futures::stream::unfold(
            (bytes, super::super::sse::FrameBudget::default(), false),
            |(mut bytes, mut budget, done)| async move {
                if done {
                    return None;
                }
                let next = match tokio::time::timeout(Duration::from_secs(45), bytes.next()).await {
                    Ok(Some(Ok(bytes))) => budget.accept(bytes),
                    Ok(None) => return None,
                    _ => Err(Error::Disconnected),
                };
                let done = next.is_err();
                Some((next, (bytes, budget, done)))
            },
        );
        Ok(Box::pin(chunks.eventsource().map(|frame| {
            let frame = frame.map_err(|_| Error::Disconnected)?;
            serde_json::from_str::<NativeEvent>(&frame.data).map_err(|_| Error::Protocol)
        })))
    }
}
