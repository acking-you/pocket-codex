use std::{collections::BTreeMap, fmt, time::Duration};

use reqwest::{Client, Method, RequestBuilder};
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

use super::{
    protocol::{Data, Located, Page},
    Form, Location, Message, MessagePage, Permission, PromptAcceptance, ServerInfo, Session,
};
use crate::opencode::{BasicCredentials, Error, PermissionReply, Result};

const MAX_JSON_BYTES: usize = 64 * 1024 * 1024;

pub(super) const REQUIRED_ROUTES: &[(&str, &str)] = &[
    ("/api/info", "get"),
    ("/api/session", "get"),
    ("/api/session", "post"),
    ("/api/session/active", "get"),
    ("/api/session/{sessionID}", "get"),
    ("/api/session/{sessionID}/message", "get"),
    ("/api/session/{sessionID}/message/{messageID}", "get"),
    ("/api/session/{sessionID}/prompt", "post"),
    ("/api/session/{sessionID}/interrupt", "post"),
    ("/api/session/{sessionID}/permission", "get"),
    ("/api/session/{sessionID}/permission/{requestID}/reply", "post"),
    ("/api/form", "get"),
    ("/api/permission/request", "get"),
    ("/api/session/{sessionID}/form/{formID}/reply", "post"),
    ("/api/session/{sessionID}/form/{formID}", "delete"),
    ("/api/event", "get"),
];

/// Cloneable v2 client bound to one origin and one selected directory.
#[derive(Clone)]
pub struct V2Client {
    http: Client,
    origin: Url,
    directory: String,
    credentials: Option<BasicCredentials>,
}

impl fmt::Debug for V2Client {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("V2Client([redacted])")
    }
}

impl V2Client {
    /// Configure a connection without network or filesystem side effects.
    pub fn new(
        origin: &str,
        directory: &str,
        credentials: Option<BasicCredentials>,
    ) -> Result<Self> {
        // Reuse the established origin policy without exposing either secret field.
        crate::opencode::OpenCodeClient::new(origin, directory, credentials.clone())?;
        let origin = Url::parse(origin).map_err(|_| Error::InvalidInput)?;
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| Error::Transport)?;
        Ok(Self {
            http,
            origin,
            directory: directory.into(),
            credentials,
        })
    }

    /// Selected host directory, without any transport credentials.
    pub fn directory(&self) -> &str {
        &self.directory
    }

    /// Verify readiness, the exact tested version and the required native
    /// routes.
    pub async fn connect(&self) -> Result<ServerInfo> {
        let info: ServerInfo = self.get("api/info", &[]).await?;
        if info.version != "2.0.18"
            || !info.extra.get("urls").is_some_and(Value::is_array)
            || !info
                .extra
                .get("paths")
                .is_some_and(|paths| paths["tmp"].is_string())
        {
            return Err(Error::Protocol);
        }
        let doc: Value = self.get("openapi.json", &[]).await?;
        super::contract::validate(&doc)?;
        Ok(info)
    }

    /// List at most one hundred recent native sessions in the selected
    /// directory.
    pub async fn sessions(&self, search: Option<&str>) -> Result<Vec<Session>> {
        if search.is_some_and(|value| value.len() > 4096) {
            return Err(Error::InvalidInput);
        }
        let mut query = vec![("directory", self.directory()), ("limit", "100"), ("order", "desc")];
        if let Some(search) = search {
            query.push(("search", search));
        }
        let page: Page<Session> = self.get("api/session", &query).await?;
        if page.data.len() > 100 {
            return Err(Error::Limit);
        }
        for session in &page.data {
            validate_id(&session.id)?;
        }
        Ok(page
            .data
            .into_iter()
            .filter(|session| self.in_scope(&session.location))
            .collect())
    }

    /// Read current session metadata and enforce the selected location locally.
    pub async fn session(&self, session_id: &str) -> Result<Session> {
        validate_id(session_id)?;
        let response: Data<Session> = self.get(&format!("api/session/{session_id}"), &[]).await?;
        if response.data.id != session_id {
            return Err(Error::Protocol);
        }
        if !self.in_scope(&response.data.location) {
            return Err(Error::Scope);
        }
        Ok(response.data)
    }

    /// Fetch a newest-first native page and expose chronological messages.
    pub async fn history(
        &self,
        session_id: &str,
        limit: u32,
        before: Option<&str>,
    ) -> Result<MessagePage> {
        if !(1..=200).contains(&limit)
            || before.is_some_and(|cursor| cursor.is_empty() || cursor.len() > 16_384)
        {
            return Err(Error::InvalidInput);
        }
        self.session(session_id).await?;
        let limit_text = limit.to_string();
        let mut query = vec![("limit", limit_text.as_str())];
        match before {
            Some(cursor) => query.push(("cursor", cursor)),
            None => query.push(("order", "desc")),
        }
        let mut page: Page<Message> = self
            .get(&format!("api/session/{session_id}/message"), &query)
            .await?;
        if page.data.len() > limit as usize
            || page
                .cursor
                .next
                .as_ref()
                .is_some_and(|cursor| cursor.is_empty() || cursor.len() > 16_384)
        {
            return Err(Error::Protocol);
        }
        for message in &page.data {
            validate_message(message)?;
        }
        page.data.reverse();
        Ok(MessagePage {
            messages: page.data,
            next_cursor: page.cursor.next,
        })
    }

    /// Read one authoritative native message without reconstructing v1 parts.
    pub async fn message(&self, session_id: &str, message_id: &str) -> Result<Message> {
        validate_id(message_id)?;
        self.session(session_id).await?;
        let response: Data<Message> = self
            .get(&format!("api/session/{session_id}/message/{message_id}"), &[])
            .await?;
        validate_message(&response.data)?;
        if response.data.id != message_id {
            return Err(Error::Protocol);
        }
        Ok(response.data)
    }

    /// Foreground executions owned by this server, scoped to the selected
    /// directory.
    pub async fn active(&self) -> Result<BTreeMap<String, Value>> {
        let response: Data<BTreeMap<String, Value>> = self.get("api/session/active", &[]).await?;
        if response.data.len() > 1024 {
            return Err(Error::Limit);
        }
        let mut result = BTreeMap::new();
        for (id, status) in response.data {
            if status["type"] != "running" {
                return Err(Error::Protocol);
            }
            if self.session_allowed(&id).await? {
                result.insert(id, status);
            }
        }
        Ok(result)
    }

    /// Create an empty native session at the selected location without a model
    /// call.
    pub async fn create(&self, title: Option<&str>) -> Result<Session> {
        if title.is_some_and(|value| value.len() > 4096) {
            return Err(Error::InvalidInput);
        }
        let mut body = serde_json::json!({"location":{"directory":self.directory}});
        if let Some(title) = title {
            body["title"] = title.into();
        }
        let response: Data<Session> = self.mutate_json(Method::POST, "api/session", &body).await?;
        validate_id(&response.data.id).map_err(|_| Error::SubmissionUnknown)?;
        if !self.in_scope(&response.data.location) {
            return Err(Error::Scope);
        }
        Ok(response.data)
    }

    /// Admit one text prompt exactly once; callers must reconcile uncertain
    /// results.
    pub async fn prompt(
        &self,
        session_id: &str,
        text: &str,
        message_id: Option<&str>,
    ) -> Result<PromptAcceptance> {
        if text.trim().is_empty() || text.len() > 1024 * 1024 {
            return Err(Error::InvalidInput);
        }
        if let Some(id) = message_id {
            validate_id(id)?;
        }
        self.session(session_id).await?;
        let mut body = serde_json::json!({"text":text});
        if let Some(id) = message_id {
            body["id"] = id.into();
        }
        let response: Data<PromptAcceptance> = self
            .mutate_json(Method::POST, &format!("api/session/{session_id}/prompt"), &body)
            .await?;
        let accepted = response.data;
        if validate_id(&accepted.id).is_err()
            || accepted.session_id != session_id
            || accepted.kind != "user"
            || message_id.is_some_and(|id| accepted.id != id)
            || !matches!(accepted.delivery.as_str(), "queue" | "steer")
            || accepted.payload["text"] != text
        {
            return Err(Error::SubmissionUnknown);
        }
        Ok(accepted)
    }

    /// Interrupt only this server's foreground session execution, never its
    /// process.
    pub async fn abort(&self, session_id: &str) -> Result<bool> {
        self.session(session_id).await?;
        #[derive(serde::Deserialize)]
        struct Interrupted {
            interrupted: bool,
        }
        let result: Interrupted = self
            .mutate_json(
                Method::POST,
                &format!("api/session/{session_id}/interrupt"),
                &serde_json::json!({}),
            )
            .await?;
        Ok(result.interrupted)
    }

    /// Read current requests for this location and independently check session
    /// scope.
    pub async fn permissions(&self) -> Result<Vec<Permission>> {
        let response: Located<Vec<Permission>> = self
            .get("api/permission/request", &[("location[directory]", self.directory())])
            .await?;
        if !self.in_scope(&response.location) {
            return Err(Error::Scope);
        }
        if response.data.len() > 1024 {
            return Err(Error::Limit);
        }
        let mut result = Vec::new();
        for permission in response.data {
            validate_id(&permission.id)?;
            if self.session_allowed(&permission.session_id).await? {
                result.push(permission);
            }
        }
        Ok(result)
    }

    /// Reply only to an authoritative pending request, never a historical
    /// approval.
    pub async fn reply_permission(
        &self,
        request_id: &str,
        reply: PermissionReply,
        message: Option<&str>,
    ) -> Result<()> {
        validate_id(request_id)?;
        if message.is_some_and(|value| value.len() > 64 * 1024) {
            return Err(Error::InvalidInput);
        }
        let permission = self
            .permissions()
            .await?
            .into_iter()
            .find(|request| request.id == request_id)
            .ok_or(Error::Rejected(404))?;
        let mut body = serde_json::json!({"decision":reply});
        if let Some(message) = message {
            body["message"] = message.into();
        }
        self.mutate_empty(
            Method::POST,
            &format!("api/session/{}/permission/{request_id}/reply", permission.session_id),
            &body,
        )
        .await
    }

    /// Read pending native forms in this location; global and foreign owners
    /// are excluded.
    pub async fn forms(&self) -> Result<Vec<Form>> {
        let response: Located<Vec<Form>> = self
            .get("api/form", &[("location[directory]", self.directory())])
            .await?;
        if !self.in_scope(&response.location) {
            return Err(Error::Scope);
        }
        if response.data.len() > 1024 {
            return Err(Error::Limit);
        }
        let mut result = Vec::new();
        for form in response.data {
            validate_id(&form.id)?;
            if form.session_id == "global" {
                continue;
            }
            if self.session_allowed(&form.session_id).await? {
                result.push(form);
            }
        }
        Ok(result)
    }

    /// Submit an explicit typed field dictionary to a currently pending form
    /// once.
    pub async fn reply_form(&self, form_id: &str, answer: Value) -> Result<()> {
        let form = self.pending_form(form_id).await?;
        super::forms::validate_answer(&form, &answer)?;
        self.mutate_empty(
            Method::POST,
            &format!("api/session/{}/form/{form_id}/reply", form.session_id),
            &serde_json::json!({"answer":answer}),
        )
        .await
    }

    /// Explicitly cancel a pending form without inventing answers or opening
    /// links.
    pub async fn reject_form(&self, form_id: &str) -> Result<()> {
        let form = self.pending_form(form_id).await?;
        self.mutate_empty(
            Method::DELETE,
            &format!("api/session/{}/form/{form_id}", form.session_id),
            &serde_json::json!({}),
        )
        .await
    }

    /// Observe native volatile events. Dropping the stream only closes this
    /// connection.
    pub async fn events(&self) -> Result<super::EventStream> {
        let response = tokio::time::timeout(
            Duration::from_secs(30),
            self.raw_request(Method::GET, "api/event", &[])?
                .header("accept", "text/event-stream")
                .send(),
        )
        .await
        .map_err(|_| Error::Transport)?
        .map_err(|_| Error::Transport)?;
        if !response.status().is_success() {
            return Err(Error::Rejected(response.status().as_u16()));
        }
        if !response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(';')
                    .next()
                    .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/event-stream"))
            })
        {
            return Err(Error::Protocol);
        }
        Ok(super::sse::stream(response, self.clone()))
    }

    async fn pending_form(&self, form_id: &str) -> Result<Form> {
        validate_id(form_id)?;
        self.forms()
            .await?
            .into_iter()
            .find(|form| form.id == form_id)
            .ok_or(Error::Rejected(404))
    }

    async fn mutate_empty(&self, method: Method, path: &str, body: &Value) -> Result<()> {
        let response = self
            .request(method, path, &[])?
            .json(body)
            .send()
            .await
            .map_err(|_| Error::SubmissionUnknown)?;
        if !response.status().is_success() {
            return Err(Error::Rejected(response.status().as_u16()));
        }
        if response.status() != reqwest::StatusCode::NO_CONTENT {
            return Err(Error::SubmissionUnknown);
        }
        Ok(())
    }

    async fn mutate_json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: &Value,
    ) -> Result<T> {
        let response = self
            .request(method, path, &[])?
            .json(body)
            .send()
            .await
            .map_err(|_| Error::SubmissionUnknown)?;
        Self::read_json(response).await.map_err(submission_error)
    }

    pub(super) fn in_scope(&self, location: &Location) -> bool {
        // A controller's filesystem cannot authorize paths on a remote server.
        !location.extra.contains_key("workspaceID") && location.directory == self.directory
    }

    pub(super) async fn session_allowed(&self, id: &str) -> Result<bool> {
        match self.session(id).await {
            Ok(_) => Ok(true),
            Err(Error::Scope | Error::Rejected(404)) => Ok(false),
            Err(error) => Err(error),
        }
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> Result<T> {
        let response = self
            .request(Method::GET, path, query)?
            .send()
            .await
            .map_err(|_| Error::Transport)?;
        if response.status().is_success() && response.status() != reqwest::StatusCode::OK {
            return Err(Error::Protocol);
        }
        Self::read_json(response).await
    }

    fn request(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<RequestBuilder> {
        Ok(self
            .raw_request(method, path, query)?
            .timeout(Duration::from_secs(30)))
    }

    fn raw_request(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<RequestBuilder> {
        let mut url = self.origin.join(path).map_err(|_| Error::InvalidInput)?;
        if url.origin() != self.origin.origin() {
            return Err(Error::InvalidInput);
        }
        url.query_pairs_mut().extend_pairs(query.iter().copied());
        let request = self
            .http
            .request(method, url)
            .header("accept", "application/json");
        Ok(match &self.credentials {
            Some(credentials) => credentials.apply(request),
            None => request,
        })
    }

    async fn read_json<T: DeserializeOwned>(mut response: reqwest::Response) -> Result<T> {
        if !response.status().is_success() {
            return Err(Error::Rejected(response.status().as_u16()));
        }
        if !response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(';')
                    .next()
                    .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
            })
        {
            return Err(Error::Protocol);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Transport)? {
            if bytes.len().saturating_add(chunk.len()) > MAX_JSON_BYTES {
                return Err(Error::Limit);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::Protocol)
    }
}

fn validate_message(message: &Message) -> Result<()> {
    validate_id(&message.id)?;
    if message.kind.is_empty() || message.kind.len() > 128 {
        return Err(Error::Protocol);
    }
    Ok(())
}

pub(super) fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 512
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(Error::InvalidInput);
    }
    Ok(())
}

fn submission_error(error: Error) -> Error {
    match error {
        Error::Rejected(_) => error,
        _ => Error::SubmissionUnknown,
    }
}
