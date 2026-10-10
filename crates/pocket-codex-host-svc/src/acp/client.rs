//! Controller-side client of the `/acp/v1` gateway (loopback when this
//! process hosts the agent, otherwise the local end of a relay tunnel).

use std::{collections::VecDeque, pin::Pin, time::Duration};

use bytes::Bytes;
use futures::{Stream, StreamExt};
use reqwest::{Method, RequestBuilder};
use serde_json::{json, Value};
use url::Url;

use super::host::Identity;

/// `body` plus the identity fields a mutation carries (see the gateway
/// docs): the host incarnation and generation the caller acted on.
fn with_identity(mut body: Value, expected: Option<&Identity>) -> Value {
    if let Some(expected) = expected {
        body["hostId"] = json!(expected.host_id);
        body["generation"] = json!(expected.generation);
    }
    body
}

/// Largest JSON response accepted.
pub const MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
/// Largest single event accepted.
pub const MAX_EVENT_BYTES: usize = 16 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(150);
/// The gateway sends a keep-alive comment every 15 s.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Why a gateway call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClientError {
    /// The host could not be reached or the connection broke.
    #[error("the agent host could not be reached")]
    Transport,
    /// The host answered with an error.
    #[error("{message}")]
    Host {
        /// HTTP status.
        status: u16,
        /// Stable error code (see `HostError::code`).
        code: String,
        /// Human-readable message.
        message: String,
    },
    /// The response was not what the gateway sends.
    #[error("unexpected response from the agent host")]
    Protocol,
    /// The response exceeded the size limit.
    #[error("the agent host response is too large")]
    Limit,
}

/// Gateway events, as JSON bodies; ends after the first error.
pub type EventStream = Pin<Box<dyn Stream<Item = Result<Value, ClientError>> + Send>>;

/// A client fixed to one gateway origin.
#[derive(Clone)]
pub struct GatewayClient {
    http: reqwest::Client,
    stream_http: reqwest::Client,
    origin: Url,
}

impl std::fmt::Debug for GatewayClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("GatewayClient")
    }
}

async fn read_body(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>, ClientError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ClientError::Transport)? {
        if body.len() + chunk.len() > limit {
            return Err(ClientError::Limit);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

impl GatewayClient {
    /// A client for `origin` (`http://host:port/`). No network access.
    pub fn new(origin: &str) -> Result<Self, ClientError> {
        let origin = Url::parse(origin).map_err(|_| ClientError::Protocol)?;
        if origin.scheme() != "http" || origin.host().is_none() || origin.path() != "/" {
            return Err(ClientError::Protocol);
        }
        let build = |timeout: Option<Duration>| {
            let mut builder = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .connect_timeout(Duration::from_secs(10));
            if let Some(timeout) = timeout {
                builder = builder.timeout(timeout);
            }
            builder.build().map_err(|_| ClientError::Transport)
        };
        Ok(Self {
            http: build(Some(REQUEST_TIMEOUT))?,
            stream_http: build(None)?,
            origin,
        })
    }

    fn url(&self, path: &str) -> Result<Url, ClientError> {
        self.origin.join(path).map_err(|_| ClientError::Protocol)
    }

    async fn send(&self, request: RequestBuilder) -> Result<Value, ClientError> {
        let response = request.send().await.map_err(|_| ClientError::Transport)?;
        let status = response.status();
        let body = read_body(response, MAX_RESPONSE_BYTES).await?;
        if status.is_success() {
            return serde_json::from_slice(&body).map_err(|_| ClientError::Protocol);
        }
        let error: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        Err(ClientError::Host {
            status: status.as_u16(),
            code: error["code"].as_str().unwrap_or("error").to_string(),
            message: error["message"].as_str().map_or_else(
                || format!("the agent host answered HTTP {}", status.as_u16()),
                str::to_string,
            ),
        })
    }

    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, ClientError> {
        self.send(self.http.request(Method::GET, self.url(path)?).query(query))
            .await
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value, ClientError> {
        self.send(self.http.request(Method::POST, self.url(path)?).json(&body))
            .await
    }

    /// `GET info`.
    pub async fn info(&self) -> Result<Value, ClientError> {
        self.get("acp/v1/info", &[]).await
    }

    /// `GET snapshot`.
    pub async fn snapshot(&self) -> Result<Value, ClientError> {
        self.get("acp/v1/snapshot", &[]).await
    }

    /// `GET sessions`.
    pub async fn sessions(&self, cursor: Option<&str>) -> Result<Value, ClientError> {
        let mut query = Vec::new();
        if let Some(cursor) = cursor {
            query.push(("cursor", cursor));
        }
        self.get("acp/v1/sessions", &query).await
    }

    /// `POST sessions/new`, refused when the host is not `expected`.
    pub async fn new_session(
        &self,
        cwd: &str,
        expected: Option<&Identity>,
    ) -> Result<Value, ClientError> {
        self.post("acp/v1/sessions/new", with_identity(json!({"cwd": cwd}), expected))
            .await
    }

    /// `POST sessions/open`, refused when the host is not `expected`.
    pub async fn open_session(
        &self,
        session: &str,
        expected: Option<&Identity>,
    ) -> Result<Value, ClientError> {
        self.post("acp/v1/sessions/open", with_identity(json!({"sessionId": session}), expected))
            .await
    }

    /// `GET sessions/history`.
    pub async fn history(
        &self,
        session: &str,
        before: Option<&str>,
        turn: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Value, ClientError> {
        let limit = limit.map(|l| l.to_string());
        let mut query = vec![("sessionId", session)];
        if let Some(before) = before {
            query.push(("before", before));
        }
        if let Some(turn) = turn {
            query.push(("turn", turn));
        }
        if let Some(limit) = limit.as_deref() {
            query.push(("limit", limit));
        }
        self.get("acp/v1/sessions/history", &query).await
    }

    /// `POST sessions/prompt`; returns the admitted host turn id. A host
    /// that is not `expected` (replaced, or another generation) refuses it.
    pub async fn prompt(
        &self,
        session: &str,
        text: &str,
        images: &[String],
        expected: Option<&Identity>,
    ) -> Result<String, ClientError> {
        let answer = self
            .post(
                "acp/v1/sessions/prompt",
                with_identity(
                    json!({"sessionId": session, "text": text, "images": images}),
                    expected,
                ),
            )
            .await?;
        answer["turnId"]
            .as_str()
            .map(str::to_string)
            .ok_or(ClientError::Protocol)
    }

    /// `POST sessions/cancel`.
    /// Only `turn` (on the `expected` host and generation, when given) is
    /// cancelled; `false` when that turn is not running any more.
    pub async fn cancel(
        &self,
        session: &str,
        turn: Option<&str>,
        expected: Option<&Identity>,
    ) -> Result<bool, ClientError> {
        self.post(
            "acp/v1/sessions/cancel",
            with_identity(json!({"sessionId": session, "turnId": turn}), expected),
        )
        .await
        .map(|answer| answer["cancelled"] == true)
    }

    /// `POST sessions/config`, refused when the host is not `expected`.
    pub async fn set_config(
        &self,
        session: &str,
        config: &str,
        value: &str,
        expected: Option<&Identity>,
    ) -> Result<Value, ClientError> {
        self.post(
            "acp/v1/sessions/config",
            with_identity(
                json!({"sessionId": session, "configId": config, "value": value}),
                expected,
            ),
        )
        .await
    }

    /// `POST sessions/mode`, refused when the host is not `expected`.
    pub async fn set_mode(
        &self,
        session: &str,
        mode: &str,
        expected: Option<&Identity>,
    ) -> Result<Value, ClientError> {
        self.post(
            "acp/v1/sessions/mode",
            with_identity(json!({"sessionId": session, "modeId": mode}), expected),
        )
        .await
    }

    /// `POST permissions/answer`.
    pub async fn answer(&self, handle: &str, option_id: &str) -> Result<(), ClientError> {
        self.post("acp/v1/permissions/answer", json!({"handle": handle, "optionId": option_id}))
            .await
            .map(|_| ())
    }

    /// `GET events` strictly after `after` in `generation`.
    /// `host` is the incarnation the watermark came from (`info.hostId`); a
    /// replaced host answers with a `reset` event.
    pub async fn events(
        &self,
        host: Option<&str>,
        generation: u64,
        after: u64,
    ) -> Result<EventStream, ClientError> {
        let (generation, after) = (generation.to_string(), after.to_string());
        let mut query = vec![("generation", generation.as_str()), ("after", after.as_str())];
        if let Some(host) = host {
            query.push(("host", host));
        }
        let response = self
            .stream_http
            .get(self.url("acp/v1/events")?)
            .query(&query)
            .send()
            .await
            .map_err(|_| ClientError::Transport)?;
        if !response.status().is_success() {
            return Err(ClientError::Host {
                status: response.status().as_u16(),
                code: "stream_refused".into(),
                message: "the agent host refused the event stream".into(),
            });
        }
        Ok(stream(response))
    }
}

fn stream(response: reqwest::Response) -> EventStream {
    let source = Box::pin(response.bytes_stream());
    Box::pin(futures::stream::unfold(
        (source, Parser::default(), VecDeque::new(), false),
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
                    Ok(Some(Err(_))) | Ok(None) | Err(_) => {
                        return Some((Err(ClientError::Transport), (source, parser, ready, true)));
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

/// Incremental SSE framing: `data:` lines accumulate until a blank line;
/// `id:` lines, comments and other fields are ignored (the sequence number
/// is also inside every body).
#[derive(Default)]
struct Parser {
    line: Vec<u8>,
    data: Vec<u8>,
}

impl Parser {
    fn feed(&mut self, chunk: &Bytes) -> Result<Vec<Value>, ClientError> {
        let mut out = Vec::new();
        let mut bytes: &[u8] = chunk;
        while let Some(pos) = bytes.iter().position(|b| *b == b'\n') {
            let (head, rest) = bytes.split_at(pos);
            self.extend(head)?;
            let mut line = std::mem::take(&mut self.line);
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.is_empty() {
                let data = std::mem::take(&mut self.data);
                if let Ok(value) = serde_json::from_slice::<Value>(&data) {
                    out.push(value);
                }
            } else if let Some(value) = line.strip_prefix(b"data:") {
                let value = value.strip_prefix(b" ").unwrap_or(value);
                if !self.data.is_empty() {
                    self.data.push(b'\n');
                }
                if self.data.len() + value.len() > MAX_EVENT_BYTES {
                    return Err(ClientError::Limit);
                }
                self.data.extend_from_slice(value);
            }
            bytes = &rest[1..];
        }
        self.extend(bytes)?;
        Ok(out)
    }

    fn extend(&mut self, bytes: &[u8]) -> Result<(), ClientError> {
        if self.line.len() + bytes.len() > MAX_EVENT_BYTES {
            return Err(ClientError::Limit);
        }
        self.line.extend_from_slice(bytes);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_frames_split_across_chunks() {
        let mut parser = Parser::default();
        let mut events = parser
            .feed(&Bytes::from_static(b": keep\n\nid: 3\ndata: {\"seq\":3,"))
            .expect("chunk");
        assert!(events.is_empty());
        events.extend(
            parser
                .feed(&Bytes::from_static(b"\"type\":\"update\"}\r\n\r\n"))
                .expect("chunk"),
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["seq"], 3);
    }

    #[test]
    fn origins_must_be_plain_http_roots() {
        assert!(GatewayClient::new("http://127.0.0.1:9/").is_ok());
        assert!(GatewayClient::new("http://127.0.0.1:9/x").is_err());
        assert!(GatewayClient::new("file:///tmp").is_err());
    }
}
