//! NDJSON JSON-RPC peer talking to one agent process over stdio.
//!
//! Inbound requests and notifications are handed to a synchronous handler
//! from the read loop, before the next line is read. A response therefore
//! never overtakes the updates the agent wrote before it: when
//! `session/prompt` returns, every `session/update` of that turn has already
//! been folded.

use std::{
    collections::{HashMap, VecDeque},
    io::Write as _,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use pocket_codex_core::acp::{
    pcx::notifications,
    rpc::{self, RequestId, RpcError, RpcMessage},
};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use tracing::{debug, warn};

use super::{error::AcpError, launch::AgentIo};

/// Maximum size of one line from the agent.
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
/// Size of the in-memory stderr ring.
pub const STDERR_TAIL_BYTES: usize = 64 * 1024;
/// Stderr logs rotate past this size at start.
const LOG_ROTATE_BYTES: u64 = 1024 * 1024;

/// A request or notification from the agent.
#[derive(Clone, Debug, PartialEq)]
pub enum Inbound {
    /// A request expecting [`AgentPeer::respond`].
    Request {
        /// Agent request id.
        id: RequestId,
        /// Method.
        method: String,
        /// Params.
        params: Value,
    },
    /// A notification.
    Notification {
        /// Method.
        method: String,
        /// Params.
        params: Value,
    },
}

/// Why the read loop ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerExit {
    /// The agent closed stdout.
    Eof,
    /// Reading failed.
    Io(String),
}

/// Receives every inbound message, in order, on the read loop.
pub type InboundHandler = Arc<dyn Fn(Inbound) + Send + Sync>;

type Waiter = oneshot::Sender<Result<Value, RpcError>>;

/// One connected agent.
pub struct AgentPeer {
    writer: Mutex<Option<mpsc::UnboundedSender<Vec<u8>>>>,
    pending: Mutex<HashMap<i64, Waiter>>,
    next_id: AtomicI64,
    stderr: Arc<Mutex<VecDeque<u8>>>,
}

impl AgentPeer {
    /// Start the read, write and stderr tasks. The returned handle completes
    /// when the agent's stdout ends.
    pub fn start(
        io: AgentIo,
        log_file: Option<PathBuf>,
        handler: InboundHandler,
    ) -> (Arc<Self>, JoinHandle<PeerExit>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let peer = Arc::new(Self {
            writer: Mutex::new(Some(tx)),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicI64::new(1),
            stderr: Arc::new(Mutex::new(VecDeque::new())),
        });
        tokio::spawn(write_loop(io.writer, rx));
        if let Some(stderr) = io.stderr {
            tokio::spawn(stderr_loop(stderr, peer.stderr.clone(), log_file));
        }
        let reader = peer.clone();
        let join = tokio::spawn(async move {
            let exit = reader.read_loop(io.reader, handler).await;
            reader.fail_all();
            exit
        });
        (peer, join)
    }

    /// Send a request and wait for its response.
    pub async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Option<Duration>,
    ) -> Result<Value, AcpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        lock(&self.pending).insert(id, tx);
        let message = RpcMessage::Request {
            id: RequestId::Number(id),
            method: method.to_string(),
            params,
        };
        if let Err(e) = self.send(&message) {
            lock(&self.pending).remove(&id);
            return Err(e);
        }
        let outcome = match timeout {
            Some(limit) => match tokio::time::timeout(limit, rx).await {
                Ok(outcome) => outcome,
                Err(_) => {
                    lock(&self.pending).remove(&id);
                    return Err(AcpError::Timeout(format!("{method} timed out")));
                },
            },
            None => rx.await,
        };
        match outcome {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => Err(AcpError::from_agent(error)),
            Err(_) => Err(AcpError::AgentUnavailable("the agent process exited".into())),
        }
    }

    /// Send a notification (queued for the write task).
    pub fn notify_now(&self, method: &str, params: Value) -> Result<(), AcpError> {
        self.send(&RpcMessage::Notification {
            method: method.to_string(),
            params,
        })
    }

    /// Answer an agent request (queued for the write task).
    pub fn respond_now(
        &self,
        id: RequestId,
        result: Result<Value, RpcError>,
    ) -> Result<(), AcpError> {
        self.send(&RpcMessage::Response {
            id,
            result,
        })
    }

    /// Lossy UTF-8 of the last `STDERR_TAIL_BYTES` of stderr.
    pub fn stderr_tail(&self) -> String {
        let ring = lock(&self.stderr);
        let (a, b) = ring.as_slices();
        let mut bytes = a.to_vec();
        bytes.extend_from_slice(b);
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// Last `limit` bytes of the stderr tail.
    pub fn stderr_tail_bytes(&self, limit: usize) -> String {
        let tail = self.stderr_tail();
        let mut start = tail.len().saturating_sub(limit);
        while !tail.is_char_boundary(start) {
            start += 1;
        }
        tail[start..].to_string()
    }

    /// Close the agent's stdin; pending requests fail when stdout ends.
    pub fn close(&self) {
        lock(&self.writer).take();
    }

    fn send(&self, message: &RpcMessage) -> Result<(), AcpError> {
        let line = rpc::encode_line(message);
        let guard = lock(&self.writer);
        let Some(tx) = guard.as_ref() else {
            return Err(AcpError::AgentUnavailable("the agent connection is closed".into()));
        };
        tx.send(line)
            .map_err(|_| AcpError::AgentUnavailable("the agent process exited".into()))
    }

    fn fail_all(&self) {
        lock(&self.pending).clear();
        self.close();
    }

    async fn read_loop(
        &self,
        reader: Box<dyn AsyncRead + Send + Unpin>,
        handler: InboundHandler,
    ) -> PeerExit {
        let mut reader = BufReader::with_capacity(64 * 1024, reader);
        let mut line: Vec<u8> = Vec::new();
        let mut oversized = false;
        loop {
            let buf = match reader.fill_buf().await {
                Ok(buf) => buf,
                Err(e) => return PeerExit::Io(e.to_string()),
            };
            if buf.is_empty() {
                return PeerExit::Eof;
            }
            let (chunk, complete) = match buf.iter().position(|b| *b == b'\n') {
                Some(pos) => (&buf[..pos], Some(pos + 1)),
                None => (buf, None),
            };
            if !oversized {
                if line.len() + chunk.len() > MAX_LINE_BYTES {
                    oversized = true;
                    line = Vec::new();
                } else {
                    line.extend_from_slice(chunk);
                }
            }
            let consumed = complete.unwrap_or(buf.len());
            reader.consume(consumed);
            if complete.is_none() {
                continue;
            }
            if oversized {
                warn!("dropped an agent message larger than {MAX_LINE_BYTES} bytes");
                oversized = false;
                handler(Inbound::Notification {
                    method: notifications::INTERNAL_OVERSIZED.to_string(),
                    params: Value::Null,
                });
                continue;
            }
            let text = std::mem::take(&mut line);
            if text.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            self.dispatch(&text, &handler);
        }
    }

    fn dispatch(&self, line: &[u8], handler: &InboundHandler) {
        match rpc::decode(line) {
            Ok(RpcMessage::Request {
                id,
                method,
                params,
            }) => handler(Inbound::Request {
                id,
                method,
                params,
            }),
            Ok(RpcMessage::Notification {
                method,
                params,
            }) => handler(Inbound::Notification {
                method,
                params,
            }),
            Ok(RpcMessage::Response {
                id,
                result,
            }) => {
                let RequestId::Number(n) = id else {
                    debug!("ignoring a response with a non-numeric id");
                    return;
                };
                if let Some(waiter) = lock(&self.pending).remove(&n) {
                    let _ = waiter.send(result);
                }
            },
            Err(e) => debug!("ignoring an invalid line from the agent: {}", e.message),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

async fn write_loop(
    mut writer: Box<dyn tokio::io::AsyncWrite + Send + Unpin>,
    mut rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    while let Some(line) = rx.recv().await {
        if writer.write_all(&line).await.is_err() || writer.flush().await.is_err() {
            break;
        }
    }
    let _ = writer.shutdown().await;
}

async fn stderr_loop(
    mut stderr: Box<dyn AsyncRead + Send + Unpin>,
    ring: Arc<Mutex<VecDeque<u8>>>,
    log_file: Option<PathBuf>,
) {
    let mut log = log_file.as_deref().and_then(open_log);
    let mut buf = vec![0u8; 8 * 1024];
    loop {
        let n = match stderr.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        {
            let mut ring = lock(&ring);
            ring.extend(&buf[..n]);
            while ring.len() > STDERR_TAIL_BYTES {
                ring.pop_front();
            }
        }
        if let Some(file) = log.as_mut() {
            if file.write_all(&buf[..n]).is_err() {
                log = None;
            }
        }
    }
}

/// Open the stderr log (0600), rotating `.1` / `.2` when it is over 1 MiB.
fn open_log(path: &Path) -> Option<std::fs::File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > LOG_ROTATE_BYTES) {
        let rotated = |n: u32| PathBuf::from(format!("{}.{n}", path.display()));
        let _ = std::fs::rename(rotated(1), rotated(2));
        let _ = std::fs::rename(path, rotated(1));
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).ok()
}
