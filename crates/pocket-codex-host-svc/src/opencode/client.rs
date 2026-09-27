use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    time::Duration,
};

use reqwest::{Client, Method};
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

use super::{
    Capabilities, Error, Health, Message, MessagePage, PermissionReply, PermissionRequest,
    PromptInput, PromptPart, QuestionRequest, Result, Session,
};

const MAX_JSON_BYTES: usize = 64 * 1024 * 1024;

pub(super) const REQUIRED_ROUTES: &[(&str, &str)] = &[
    ("/global/health", "get"),
    ("/session", "get"),
    ("/session", "post"),
    ("/session/status", "get"),
    ("/session/{sessionID}", "get"),
    ("/session/{sessionID}/message", "get"),
    ("/session/{sessionID}/message/{messageID}", "get"),
    ("/session/{sessionID}/prompt_async", "post"),
    ("/session/{sessionID}/abort", "post"),
    ("/event", "get"),
    ("/permission", "get"),
    ("/permission/{requestID}/reply", "post"),
    ("/question", "get"),
    ("/question/{requestID}/reply", "post"),
    ("/question/{requestID}/reject", "post"),
];

/// Memory-only HTTP Basic credentials. Debug output never exposes either field.
#[derive(Clone)]
pub struct BasicCredentials {
    username: String,
    password: String,
}

impl BasicCredentials {
    /// Construct credentials supplied explicitly by the user.
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }

    pub(super) fn apply(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request.basic_auth(&self.username, Some(&self.password))
    }
}

impl fmt::Debug for BasicCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("BasicCredentials([redacted])")
    }
}

/// A cloneable client fixed to one origin and project directory.
#[derive(Clone)]
pub struct OpenCodeClient {
    http: Client,
    origin: Url,
    directory: String,
    credentials: Option<BasicCredentials>,
}

impl fmt::Debug for OpenCodeClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OpenCodeClient([redacted])")
    }
}

impl OpenCodeClient {
    /// Create a client without reading credentials or connecting to the server.
    pub fn new(
        base_url: &str,
        directory: &str,
        credentials: Option<BasicCredentials>,
    ) -> Result<Self> {
        let origin = Url::parse(base_url).map_err(|_| Error::InvalidInput)?;
        let loopback = match origin.host() {
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            Some(url::Host::Domain(host)) => host == "localhost",
            None => false,
        };
        if !matches!(origin.scheme(), "http" | "https")
            || origin.host().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.query().is_some()
            || origin.fragment().is_some()
            || origin.path() != "/"
            || directory.trim().is_empty()
            || directory.chars().any(char::is_control)
            || credentials.as_ref().is_some_and(|c| {
                c.username.contains(':') || c.username.chars().any(char::is_control)
            })
            || (credentials.is_some() && origin.scheme() != "https" && !loopback)
        {
            return Err(Error::InvalidInput);
        }
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

    /// Selected directory, never including transport credentials.
    pub fn directory(&self) -> &str {
        &self.directory
    }

    /// Check health without reading a model provider's authentication state.
    pub async fn health(&self) -> Result<Health> {
        let health: Health = self.get("global/health", &[]).await?;
        if !health.healthy || health.version.is_empty() || health.version.len() > 128 {
            return Err(Error::Protocol);
        }
        Ok(health)
    }

    /// Check the actual documented methods, independently of a healthy socket.
    pub async fn capabilities(&self) -> Result<Capabilities> {
        let health = self.health().await?;
        let doc: Value = self.get("doc", &[]).await?;
        if !doc["openapi"]
            .as_str()
            .is_some_and(|version| version.starts_with("3."))
            || REQUIRED_ROUTES
                .iter()
                .any(|(path, method)| !doc["paths"][*path][*method].is_object())
        {
            return Err(Error::Protocol);
        }
        Ok(Capabilities {
            tested_version: health.version == "1.18.32",
            version: health.version,
        })
    }

    /// List at most the 100 most recently updated sessions, optionally by
    /// title.
    pub async fn sessions(&self, search: Option<&str>) -> Result<Vec<Session>> {
        if search.is_some_and(|value| value.len() > 4096) {
            return Err(Error::InvalidInput);
        }
        let mut query = vec![("limit", "100")];
        if let Some(search) = search {
            query.push(("search", search));
        }
        let sessions: Vec<Session> = self.get("session", &query).await?;
        if sessions.len() > 100 {
            return Err(Error::Limit);
        }
        Ok(sessions
            .into_iter()
            .filter(|session| self.in_scope(session))
            .collect())
    }

    fn in_scope(&self, session: &Session) -> bool {
        if session.workspace_id.is_some() {
            return false;
        }
        if session.directory == self.directory {
            return true;
        }
        match (std::fs::canonicalize(&session.directory), std::fs::canonicalize(&self.directory)) {
            (Ok(session_directory), Ok(selected_directory)) => {
                session_directory == selected_directory
            },
            _ => false,
        }
    }

    /// Read one session only when it belongs to the selected local workspace.
    pub async fn session(&self, session_id: &str) -> Result<Session> {
        validate_id(session_id)?;
        let session: Session = self.get(&format!("session/{session_id}"), &[]).await?;
        if session.id != session_id {
            return Err(Error::Protocol);
        }
        if !self.in_scope(&session) {
            return Err(Error::Scope);
        }
        Ok(session)
    }

    /// Read a bounded tail or earlier page without following upstream Link
    /// URLs.
    pub async fn history(
        &self,
        session_id: &str,
        limit: u32,
        before: Option<&str>,
    ) -> Result<MessagePage> {
        if !(1..=100).contains(&limit) || before.is_some_and(|cursor| cursor.len() > 16_384) {
            return Err(Error::InvalidInput);
        }
        self.session(session_id).await?;
        let limit = limit.to_string();
        let mut query = vec![("limit", limit.as_str())];
        if let Some(before) = before {
            query.push(("before", before));
        }
        let (messages, headers): (Vec<Message>, _) = self
            .get_response(&format!("session/{session_id}/message"), &query)
            .await?;
        if messages.len() > limit.parse::<usize>().map_err(|_| Error::InvalidInput)? {
            return Err(Error::Protocol);
        }
        for message in &messages {
            if message.info.session_id != session_id
                || message
                    .parts
                    .iter()
                    .any(|part| part.session_id != session_id || part.message_id != message.info.id)
            {
                return Err(Error::Protocol);
            }
        }
        let next_cursor = headers
            .get("x-next-cursor")
            .map(|value| {
                value
                    .to_str()
                    .map(str::to_owned)
                    .map_err(|_| Error::Protocol)
            })
            .transpose()?;
        if next_cursor
            .as_ref()
            .is_some_and(|cursor| cursor.is_empty() || cursor.len() > 16_384)
        {
            return Err(Error::Protocol);
        }
        Ok(MessagePage {
            messages,
            next_cursor,
        })
    }

    /// Submit once. Success means accepted, not generation complete; errors are
    /// never retried.
    pub async fn prompt(&self, session_id: &str, input: &PromptInput) -> Result<()> {
        if input.parts.is_empty() || input.parts.len() > 32 {
            return Err(Error::InvalidInput);
        }
        for part in &input.parts {
            let PromptPart::Text {
                text,
            } = part;
            if text.trim().is_empty() || text.len() > 1024 * 1024 {
                return Err(Error::InvalidInput);
            }
        }
        if let Some(id) = &input.message_id {
            validate_id(id)?;
        }
        self.session(session_id).await?;
        let response = self
            .request(Method::POST, &format!("session/{session_id}/prompt_async"), &[])?
            .json(input)
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

    /// Read only current permissions belonging to the selected directory.
    pub async fn permissions(&self) -> Result<Vec<PermissionRequest>> {
        let pending: Vec<PermissionRequest> = self.get("permission", &[]).await?;
        if pending.len() > 1024 {
            return Err(Error::Limit);
        }
        let mut result = Vec::new();
        for request in pending {
            if self.session_allowed(&request.session_id).await? {
                result.push(request);
            }
        }
        Ok(result)
    }

    /// Reply once after checking that the request remains pending in this
    /// project.
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
        if !self
            .permissions()
            .await?
            .iter()
            .any(|request| request.id == request_id)
        {
            return Err(Error::Rejected(404));
        }
        let mut body = serde_json::json!({"reply": reply});
        if let Some(message) = message {
            body["message"] = Value::String(message.into());
        }
        self.post_true(&format!("permission/{request_id}/reply"), &body)
            .await
    }

    /// Read current questions in the selected directory.
    pub async fn questions(&self) -> Result<Vec<QuestionRequest>> {
        let pending: Vec<QuestionRequest> = self.get("question", &[]).await?;
        if pending.len() > 1024 {
            return Err(Error::Limit);
        }
        let mut result = Vec::new();
        for request in pending {
            if self.session_allowed(&request.session_id).await? {
                result.push(request);
            }
        }
        Ok(result)
    }

    /// Submit answers in the current upstream question order, without automatic
    /// retry.
    pub async fn reply_question(&self, request_id: &str, answers: Vec<Vec<String>>) -> Result<()> {
        validate_id(request_id)?;
        let request = self
            .questions()
            .await?
            .into_iter()
            .find(|request| request.id == request_id)
            .ok_or(Error::Rejected(404))?;
        if answers.len() != request.questions.len() {
            return Err(Error::InvalidInput);
        }
        for (answer, question) in answers.iter().zip(request.questions) {
            if answer.is_empty()
                || (!question.multiple && answer.len() != 1)
                || answer.len() > 100
                || answer.iter().any(|value| {
                    value.len() > 64 * 1024
                        || (!question.custom
                            && !question.options.iter().any(|option| option.label == *value))
                })
            {
                return Err(Error::InvalidInput);
            }
        }
        self.post_true(
            &format!("question/{request_id}/reply"),
            &serde_json::json!({"answers": answers}),
        )
        .await
    }

    /// Explicitly reject a currently pending question in the selected
    /// directory.
    pub async fn reject_question(&self, request_id: &str) -> Result<()> {
        validate_id(request_id)?;
        if !self
            .questions()
            .await?
            .iter()
            .any(|request| request.id == request_id)
        {
            return Err(Error::Rejected(404));
        }
        self.post_true(&format!("question/{request_id}/reject"), &serde_json::json!({}))
            .await
    }

    /// Read active statuses only for sessions in this project. Idle entries may
    /// be absent.
    pub async fn status(&self) -> Result<BTreeMap<String, Value>> {
        let statuses: BTreeMap<String, Value> = self.get("session/status", &[]).await?;
        if statuses.len() > 1024 {
            return Err(Error::Limit);
        }
        let mut result = BTreeMap::new();
        for (id, status) in statuses {
            if !matches!(status["type"].as_str(), Some("idle" | "busy" | "retry")) {
                return Err(Error::Protocol);
            }
            if self.session_allowed(&id).await? {
                result.insert(id, status);
            }
        }
        Ok(result)
    }

    /// Cancel only this session's execution; never disposes an instance or a
    /// process.
    pub async fn abort(&self, session_id: &str) -> Result<()> {
        self.session(session_id).await?;
        self.post_true(&format!("session/{session_id}/abort"), &serde_json::json!({}))
            .await
    }

    /// Create an empty session in this project without invoking a model.
    pub async fn create(&self, title: Option<&str>) -> Result<Session> {
        if title.is_some_and(|value| value.len() > 4096) {
            return Err(Error::InvalidInput);
        }
        let body = title
            .map_or_else(|| serde_json::json!({}), |title| serde_json::json!({"title": title}));
        let response = self
            .request(Method::POST, "session", &[])?
            .json(&body)
            .send()
            .await
            .map_err(|_| Error::SubmissionUnknown)?;
        let (session, _): (Session, _) =
            Self::read_json(response)
                .await
                .map_err(|error| match error {
                    Error::Rejected(_) => error,
                    _ => Error::SubmissionUnknown,
                })?;
        if !self.in_scope(&session) {
            return Err(Error::Scope);
        }
        Ok(session)
    }

    /// Read a single authoritative message for snapshot/delta reconciliation.
    pub async fn message(&self, session_id: &str, message_id: &str) -> Result<Message> {
        validate_id(message_id)?;
        self.session(session_id).await?;
        let message: Message = self
            .get(&format!("session/{session_id}/message/{message_id}"), &[])
            .await?;
        if message.info.id != message_id
            || message.info.session_id != session_id
            || message
                .parts
                .iter()
                .any(|part| part.session_id != session_id || part.message_id != message_id)
        {
            return Err(Error::Protocol);
        }
        Ok(message)
    }

    /// Open a scoped, bounded SSE stream. Dropping it closes only this HTTP
    /// connection.
    pub async fn events(&self) -> Result<super::OpenCodeEventStream> {
        let response = tokio::time::timeout(
            Duration::from_secs(30),
            self.raw_request(Method::GET, "event", &[])?.send(),
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
            .is_some_and(|value| value.split(';').next() == Some("text/event-stream"))
        {
            return Err(Error::Protocol);
        }
        Ok(super::sse::stream(response, self.clone()))
    }

    pub(super) async fn event_allowed(
        &self,
        event: &super::OpenCodeEvent,
        scope_cache: &mut HashMap<String, bool>,
    ) -> Result<bool> {
        if matches!(
            event.kind.as_str(),
            "server.connected" | "server.heartbeat" | "server.instance.disposed"
        ) {
            return Ok(true);
        }
        if matches!(event.kind.as_str(), "session.created" | "session.updated" | "session.deleted")
        {
            let session: Session = serde_json::from_value(event.properties["info"].clone())
                .map_err(|_| Error::Protocol)?;
            let allowed = self.in_scope(&session);
            cache_session_scope(scope_cache, &session.id, allowed);
            return Ok(allowed);
        }
        let id = event.properties["sessionID"]
            .as_str()
            .or_else(|| event.properties["info"]["sessionID"].as_str())
            .or_else(|| event.properties["part"]["sessionID"].as_str());
        match id {
            Some(id) => {
                if let Some(allowed) = scope_cache.get(id) {
                    return Ok(*allowed);
                }
                let allowed = self.session_allowed(id).await?;
                cache_session_scope(scope_cache, id, allowed);
                Ok(allowed)
            },
            None => Ok(false),
        }
    }

    async fn session_allowed(&self, id: &str) -> Result<bool> {
        match self.session(id).await {
            Ok(_) => Ok(true),
            Err(Error::Scope | Error::Rejected(404)) => Ok(false),
            Err(error) => Err(error),
        }
    }

    async fn post_true(&self, path: &str, body: &Value) -> Result<()> {
        let response = self
            .request(Method::POST, path, &[])?
            .json(body)
            .send()
            .await
            .map_err(|_| Error::SubmissionUnknown)?;
        let result: bool = Self::read_json(response)
            .await
            .map_err(|error| match error {
                Error::Rejected(_) => error,
                _ => Error::SubmissionUnknown,
            })?
            .0;
        if !result {
            return Err(Error::Protocol);
        }
        Ok(())
    }

    pub(super) async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T> {
        self.get_response(path, query).await.map(|(value, _)| value)
    }

    pub(super) async fn get_response<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<(T, reqwest::header::HeaderMap)> {
        let response = self
            .request(Method::GET, path, query)?
            .send()
            .await
            .map_err(|_| Error::Transport)?;
        Self::read_json(response).await
    }

    pub(super) async fn read_json<T: DeserializeOwned>(
        mut response: reqwest::Response,
    ) -> Result<(T, reqwest::header::HeaderMap)> {
        if !response.status().is_success() {
            return Err(Error::Rejected(response.status().as_u16()));
        }
        let headers = response.headers().clone();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Transport)? {
            if bytes.len().saturating_add(chunk.len()) > MAX_JSON_BYTES {
                return Err(Error::Limit);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes)
            .map(|value| (value, headers))
            .map_err(|_| Error::Protocol)
    }

    pub(super) fn request(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<reqwest::RequestBuilder> {
        Ok(self
            .raw_request(method, path, query)?
            .timeout(Duration::from_secs(30)))
    }

    pub(super) fn raw_request(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<reqwest::RequestBuilder> {
        let mut url = self.origin.join(path).map_err(|_| Error::InvalidInput)?;
        url.query_pairs_mut()
            .append_pair("directory", &self.directory)
            .extend_pairs(query.iter().copied());
        let mut request = self.http.request(method, url);
        if let Some(credentials) = &self.credentials {
            request = request.basic_auth(&credentials.username, Some(&credentials.password));
        }
        Ok(request)
    }
}

const MAX_EVENT_SCOPE_CACHE: usize = 256;

fn cache_session_scope(scope_cache: &mut HashMap<String, bool>, id: &str, allowed: bool) {
    if scope_cache.len() >= MAX_EVENT_SCOPE_CACHE && !scope_cache.contains_key(id) {
        scope_cache.clear();
    }
    scope_cache.insert(id.to_owned(), allowed);
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
