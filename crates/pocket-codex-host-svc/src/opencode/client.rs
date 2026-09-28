//! HTTP client for the OpenCode v2 `/api/*` surface.
//!
//! Used against the real server (with Basic credentials, on the host) and
//! against the Pocket gateway or a relay tunnel to it (without credentials, on
//! a controller). Every call is bounded in time and size; mutations whose
//! outcome is unobservable surface [`Error::SubmissionUnknown`] instead of
//! being retried.

use std::{collections::BTreeSet, fmt, time::Duration};

use reqwest::{Method, RequestBuilder};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use url::Url;

use super::{
    protocol::{Data, Page},
    validate_id, validate_origin, BasicCredentials, Error, FileDiff, Form, MessagePage, ModelRef,
    Permission, PermissionReply, PromptAcceptance, Result, ServerInfo, Session, SessionPage,
};

const MAX_JSON_BYTES: usize = 64 * 1024 * 1024;
const MAX_CURSOR: usize = 16 * 1024;

/// Filters for `GET /api/session`.
#[derive(Clone, Debug, Default)]
pub struct SessionQuery {
    /// Only sessions whose location is this directory.
    pub directory: Option<String>,
    /// Only root sessions (no parent).
    pub roots_only: bool,
    /// Only children of this session.
    pub parent_id: Option<String>,
    /// Page size, 1..=200.
    pub limit: u32,
    /// Opaque continuation cursor from a previous page.
    pub cursor: Option<String>,
}

/// Body of `POST /api/session/{id}/prompt`.
#[derive(Clone, Debug, Default)]
pub struct PromptRequest {
    /// Prompt text.
    pub text: String,
    /// File attachments as URIs (`file://…` or `data:…`), with display names.
    pub files: Vec<(String, Option<String>)>,
    /// `steer` joins the running execution; otherwise the prompt is queued.
    pub steer: bool,
}

/// Cloneable client fixed to one origin.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    origin: Url,
    credentials: Option<BasicCredentials>,
}

impl fmt::Debug for Client {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("opencode::Client([redacted])")
    }
}

impl Client {
    /// Configure a client without touching the network. Credentials are only
    /// accepted for loopback or HTTPS origins.
    pub fn new(origin: &str, credentials: Option<BasicCredentials>) -> Result<Self> {
        let origin = validate_origin(origin, false)?;
        if credentials.is_some()
            && origin.scheme() != "https"
            && validate_origin(origin.as_str(), true).is_err()
        {
            return Err(Error::InvalidInput);
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| Error::Transport)?;
        Ok(Self {
            http,
            origin,
            credentials,
        })
    }

    /// The origin this client talks to.
    pub fn origin(&self) -> &Url {
        &self.origin
    }

    /// `GET /api/info`.
    pub async fn info(&self) -> Result<ServerInfo> {
        self.get("api/info", &[]).await
    }

    /// Read the server identity and check its API against the contract this
    /// build relies on. Returns the identity plus whether its version is the
    /// verified one.
    pub async fn connect(&self) -> Result<(ServerInfo, bool)> {
        let info = self.info().await?;
        let doc: Value = self.get("openapi.json", &[]).await?;
        let report = super::contract::check(&doc);
        if !report.missing.is_empty() {
            return Err(Error::Incompatible {
                version: info.version.clone(),
                missing: report.missing.join(", "),
            });
        }
        let verified = info.version == super::VERIFIED_VERSION;
        Ok((info, verified))
    }

    /// One page of sessions, newest first.
    pub async fn sessions(&self, query: &SessionQuery) -> Result<SessionPage> {
        let limit = query.limit.clamp(1, 200).to_string();
        let mut params: Vec<(&str, &str)> = vec![("limit", &limit)];
        match &query.cursor {
            Some(cursor) => {
                check_cursor(cursor)?;
                params.push(("cursor", cursor));
            },
            None => params.push(("order", "desc")),
        }
        if let Some(directory) = &query.directory {
            params.push(("directory", directory));
        }
        if query.roots_only {
            params.push(("parentID", "null"));
        } else if let Some(parent) = &query.parent_id {
            validate_id(parent)?;
            params.push(("parentID", parent));
        }
        let page: Page<Session> = self.get("api/session", &params).await?;
        for session in &page.data {
            validate_id(&session.id).map_err(|_| Error::Protocol)?;
        }
        let full = page.data.len() >= query.limit.clamp(1, 200) as usize;
        Ok(SessionPage {
            next_cursor: page
                .cursor
                .next
                .filter(|next| full && next.len() <= MAX_CURSOR),
            sessions: page.data,
        })
    }

    /// `GET /api/session/{id}`.
    pub async fn session(&self, session_id: &str) -> Result<Session> {
        validate_id(session_id)?;
        let response: Data<Session> = self.get(&format!("api/session/{session_id}"), &[]).await?;
        if response.data.id != session_id {
            return Err(Error::Protocol);
        }
        Ok(response.data)
    }

    /// A chronological window of messages ending at the newest one (no cursor)
    /// or just before the window `cursor` came from. `kind` filters by message
    /// type (e.g. `user`).
    pub async fn messages(
        &self,
        session_id: &str,
        limit: u32,
        cursor: Option<&str>,
        kind: Option<&str>,
    ) -> Result<MessagePage> {
        validate_id(session_id)?;
        if !(1..=200).contains(&limit) {
            return Err(Error::InvalidInput);
        }
        let limit_text = limit.to_string();
        let mut params: Vec<(&str, &str)> = vec![("limit", &limit_text)];
        match cursor {
            Some(cursor) => {
                check_cursor(cursor)?;
                params.push(("cursor", cursor));
            },
            None => params.push(("order", "desc")),
        }
        if let Some(kind) = kind {
            params.push(("type", kind));
        }
        let mut page: Page<super::Message> = self
            .get(&format!("api/session/{session_id}/message"), &params)
            .await?;
        if page.data.len() > limit as usize {
            return Err(Error::Protocol);
        }
        for message in &page.data {
            validate_id(&message.id).map_err(|_| Error::Protocol)?;
        }
        // A short page is the oldest one, whatever cursor the server attaches.
        let older = page
            .cursor
            .next
            .filter(|next| page.data.len() == limit as usize && next.len() <= MAX_CURSOR);
        page.data.reverse();
        Ok(MessagePage {
            messages: page.data,
            older_cursor: older,
        })
    }

    /// Ids of sessions that are currently executing.
    pub async fn active(&self) -> Result<BTreeSet<String>> {
        let response: Data<serde_json::Map<String, Value>> =
            self.get("api/session/active", &[]).await?;
        if response.data.len() > 4096 {
            return Err(Error::Limit);
        }
        Ok(response
            .data
            .into_iter()
            .filter(|(_, status)| status["type"] == "running")
            .map(|(id, _)| id)
            .collect())
    }

    /// Create an empty session in `directory`. No permission rules are sent,
    /// so the server's own configuration applies.
    pub async fn create_session(&self, directory: &str, title: Option<&str>) -> Result<Session> {
        if directory.trim().is_empty() || directory.chars().any(char::is_control) {
            return Err(Error::InvalidInput);
        }
        let mut body = json!({"location": {"directory": directory}});
        if let Some(title) = title {
            body["title"] = title.into();
        }
        let response: Data<Session> = self.mutate_json(Method::POST, "api/session", &body).await?;
        validate_id(&response.data.id).map_err(|_| Error::SubmissionUnknown)?;
        Ok(response.data)
    }

    /// Admit one prompt exactly once.
    pub async fn prompt(
        &self,
        session_id: &str,
        prompt: &PromptRequest,
    ) -> Result<PromptAcceptance> {
        validate_id(session_id)?;
        if prompt.text.trim().is_empty() && prompt.files.is_empty() {
            return Err(Error::InvalidInput);
        }
        let mut body = json!({
            "text": prompt.text,
            "delivery": if prompt.steer { "steer" } else { "queue" },
        });
        if !prompt.files.is_empty() {
            body["files"] = prompt
                .files
                .iter()
                .map(|(uri, name)| match name {
                    Some(name) => json!({"uri": uri, "name": name}),
                    None => json!({"uri": uri}),
                })
                .collect();
        }
        let response: Data<PromptAcceptance> = self
            .mutate_json(Method::POST, &format!("api/session/{session_id}/prompt"), &body)
            .await?;
        let accepted = response.data;
        if validate_id(&accepted.id).is_err()
            || accepted.session_id != session_id
            || accepted.kind != "user"
        {
            return Err(Error::SubmissionUnknown);
        }
        Ok(accepted)
    }

    /// Interrupt the session's foreground execution (never the process).
    pub async fn interrupt(&self, session_id: &str) -> Result<bool> {
        validate_id(session_id)?;
        #[derive(serde::Deserialize)]
        struct Interrupted {
            interrupted: bool,
        }
        let result: Interrupted = self
            .mutate_json(Method::POST, &format!("api/session/{session_id}/interrupt"), &json!({}))
            .await?;
        Ok(result.interrupted)
    }

    /// Rename a session.
    pub async fn rename(&self, session_id: &str, title: &str) -> Result<()> {
        validate_id(session_id)?;
        self.mutate_empty(
            Method::PATCH,
            &format!("api/session/{session_id}"),
            &json!({"title": title}),
        )
        .await
    }

    /// Queue a context compaction.
    pub async fn compact(&self, session_id: &str) -> Result<()> {
        validate_id(session_id)?;
        let _: Value = self
            .mutate_json(Method::POST, &format!("api/session/{session_id}/compact"), &json!({}))
            .await?;
        Ok(())
    }

    /// Select the session's model for subsequent executions.
    pub async fn set_model(&self, session_id: &str, model: &ModelRef) -> Result<()> {
        validate_id(session_id)?;
        self.mutate_empty(
            Method::POST,
            &format!("api/session/{session_id}/model"),
            &json!({"model": model}),
        )
        .await
    }

    /// Select the session's primary agent (e.g. `build` / `plan`).
    pub async fn set_agent(&self, session_id: &str, agent: &str) -> Result<()> {
        validate_id(session_id)?;
        self.mutate_empty(
            Method::POST,
            &format!("api/session/{session_id}/agent"),
            &json!({"agent": agent}),
        )
        .await
    }

    /// Pending permission requests of one session.
    pub async fn permissions(&self, session_id: &str) -> Result<Vec<Permission>> {
        validate_id(session_id)?;
        let response: Data<Vec<Permission>> = self
            .get(&format!("api/session/{session_id}/permission"), &[])
            .await?;
        Ok(response.data)
    }

    /// Reply to a pending permission request.
    pub async fn reply_permission(
        &self,
        session_id: &str,
        request_id: &str,
        reply: PermissionReply,
    ) -> Result<()> {
        validate_id(session_id)?;
        validate_id(request_id)?;
        self.mutate_empty(
            Method::POST,
            &format!("api/session/{session_id}/permission/{request_id}/reply"),
            &json!({"decision": reply}),
        )
        .await
    }

    /// Pending forms of one session.
    pub async fn forms(&self, session_id: &str) -> Result<Vec<Form>> {
        validate_id(session_id)?;
        let response: Data<Vec<Form>> = self
            .get(&format!("api/session/{session_id}/form"), &[])
            .await?;
        Ok(response.data)
    }

    /// Answer a pending form after validating the answer against its fields.
    pub async fn reply_form(&self, session_id: &str, form_id: &str, answer: Value) -> Result<()> {
        validate_id(form_id)?;
        let form = self
            .forms(session_id)
            .await?
            .into_iter()
            .find(|form| form.id == form_id)
            .ok_or(Error::Rejected(404))?;
        super::forms::validate_answer(&form, &answer)?;
        self.mutate_empty(
            Method::POST,
            &format!("api/session/{session_id}/form/{form_id}/reply"),
            &json!({"answer": answer}),
        )
        .await
    }

    /// Cancel a pending form.
    pub async fn cancel_form(&self, session_id: &str, form_id: &str) -> Result<()> {
        validate_id(session_id)?;
        validate_id(form_id)?;
        let response = self
            .request(Method::DELETE, &format!("api/session/{session_id}/form/{form_id}"), &[])?
            .send()
            .await
            .map_err(|_| Error::SubmissionUnknown)?;
        if !response.status().is_success() {
            return Err(Error::Rejected(response.status().as_u16()));
        }
        Ok(())
    }

    /// Replace a session's own permission rules (`[{action, resource,
    /// effect}]`). Pocket never sends rules for user sessions; this exists for
    /// tools and verification on sessions they created.
    pub async fn set_session_permissions(&self, session_id: &str, rules: Value) -> Result<()> {
        validate_id(session_id)?;
        self.mutate_empty(
            Method::PATCH,
            &format!("api/session/{session_id}"),
            &json!({"permissions": rules}),
        )
        .await
    }

    /// Raise a permission request on a session, as a tool would. The server
    /// may hold the call open until the request is answered.
    pub async fn request_permission(
        &self,
        session_id: &str,
        action: &str,
        resources: &[&str],
    ) -> Result<Value> {
        validate_id(session_id)?;
        let body = json!({"action": action, "resources": resources});
        let response: Data<Value> = self
            .mutate_json(Method::POST, &format!("api/session/{session_id}/permission"), &body)
            .await?;
        Ok(response.data)
    }

    /// Open a form (question) on a session, as a tool would.
    pub async fn create_form(&self, session_id: &str, title: &str, fields: Value) -> Result<Form> {
        validate_id(session_id)?;
        let body = json!({"title": title, "fields": fields});
        let response: Data<Form> = self
            .mutate_json(Method::POST, &format!("api/session/{session_id}/form"), &body)
            .await?;
        Ok(response.data)
    }

    /// Models known to the server (native `Model.Info` objects).
    pub async fn models(&self) -> Result<Vec<Value>> {
        let response: Data<Vec<Value>> = self.get("api/model", &[]).await?;
        Ok(response.data)
    }

    /// Agents known to the server (native `Agent.Info` objects).
    pub async fn agents(&self) -> Result<Vec<Value>> {
        let response: Data<Vec<Value>> = self.get("api/agent", &[]).await?;
        Ok(response.data)
    }

    /// Working-tree changes of the repository at `directory`.
    pub async fn vcs_diff(&self, directory: &str) -> Result<Vec<FileDiff>> {
        let response: Data<Vec<FileDiff>> = self
            .get("api/vcs/diff", &[("mode", "working"), ("location[directory]", directory)])
            .await?;
        Ok(response.data)
    }

    /// Subscribe to the server's volatile event stream.
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
        if !content_type_is(&response, "text/event-stream") {
            return Err(Error::Protocol);
        }
        Ok(super::sse::stream(response))
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
        read_json(response).await.map_err(|error| match error {
            Error::Rejected(_) => error,
            _ => Error::SubmissionUnknown,
        })
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> Result<T> {
        let response = self
            .request(Method::GET, path, query)?
            .send()
            .await
            .map_err(|_| Error::Transport)?;
        read_json(response).await
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
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query.iter().copied());
        }
        let request = self
            .http
            .request(method, url)
            .header("accept", "application/json");
        Ok(match &self.credentials {
            Some(credentials) => credentials.apply(request),
            None => request,
        })
    }
}

fn check_cursor(cursor: &str) -> Result<()> {
    if cursor.is_empty() || cursor.len() > MAX_CURSOR {
        return Err(Error::InvalidInput);
    }
    Ok(())
}

fn content_type_is(response: &reqwest::Response, mime: &str) -> bool {
    response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case(mime))
}

async fn read_json<T: DeserializeOwned>(mut response: reqwest::Response) -> Result<T> {
    if !response.status().is_success() {
        return Err(Error::Rejected(response.status().as_u16()));
    }
    if !content_type_is(&response, "application/json") {
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
