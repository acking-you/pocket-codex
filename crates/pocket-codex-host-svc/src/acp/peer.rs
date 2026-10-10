//! A bidirectional JSON-RPC peer over an NDJSON byte stream.
//!
//! One reader task decodes frames and never waits on anything downstream:
//! responses settle the matching outbound request, and inbound requests and
//! notifications are handed to a synchronous [`Inbound`] handler that must
//! return immediately (answers are sent later through [`Peer::respond`]).
//! One writer task owns the output stream; it drains the bounded queue and
//! closes the stream when the peer shuts down.
//!
//! Outbound request ids are numbers allocated here. Inbound request ids live
//! in a separate namespace: an agent request with id `1` never settles our
//! request `1`, and a response carrying the string `"1"` never settles it
//! either.
//!
//! # Delivery
//!
//! A frame sent with a deadline ([`Peer::send_request`], [`Peer::notify`],
//! [`Peer::respond`]) returns only once the writer has written *and
//! flushed* it, and one deadline covers waiting for queue room and that
//! write. Its outcome is always one of:
//!
//! - delivered;
//! - never queued (the deadline passed while waiting for room): nothing of it
//!   reached the agent, and the connection stays open;
//! - the connection is closed — the deadline passed after the frame was queued,
//!   so it may be partly written. The connection is closed at once (the writer
//!   is stopped, nothing more is written), so such bytes can never reach a
//!   later turn.
//!
//! A request's answer is awaited afterwards, within what is left of its
//! deadline (or indefinitely without one). Closing the connection wakes
//! every waiter at once.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Arc, Mutex, MutexGuard, Weak,
    },
    time::Duration,
};

use serde_json::Value;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    sync::{mpsc, oneshot, watch, Notify, OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
    time::Instant,
};

use super::jsonrpc::{self, Frame, RequestId, RpcError};

/// Largest accepted or emitted frame.
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
/// Largest number of bytes waiting in the outbound queue.
pub const MAX_QUEUED_BYTES: usize = 32 * 1024 * 1024;
/// Largest number of frames waiting in the outbound queue.
pub const MAX_QUEUED_FRAMES: usize = 512;
/// Largest number of our requests awaiting an answer.
pub const MAX_PENDING_REQUESTS: usize = 64;
/// Malformed frames tolerated before the connection is closed.
const MAX_MALFORMED: u32 = 64;
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);

/// Receives the agent's requests and notifications on the reader task.
///
/// Implementations must not block or await: record the request and answer it
/// later through [`Peer::respond`] / [`Peer::respond_now`].
pub trait Inbound: Send + Sync + 'static {
    /// A notification from the agent.
    fn notification(&self, peer: &Peer, method: &str, params: Option<Value>);
    /// A request from the agent, to be answered with the same `id`.
    fn request(&self, peer: &Peer, id: RequestId, method: &str, params: Option<Value>);
}

/// Why an outbound call failed.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CallError {
    /// The agent answered with a JSON-RPC error.
    #[error("agent error {}: {}", .0.code, .0.message)]
    Rpc(RpcError),
    /// The connection closed before an answer arrived.
    #[error("agent connection closed")]
    Closed,
    /// No answer within the deadline.
    #[error("agent did not answer in time")]
    Timeout,
    /// Too many requests or queued bytes.
    #[error("agent connection is busy")]
    Busy,
    /// The frame exceeds [`MAX_FRAME_BYTES`].
    #[error("message exceeds the size limit")]
    TooLarge,
}

/// Why the connection closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    /// The agent closed its output.
    Eof,
    /// Reading the agent's output failed.
    ReadError,
    /// Writing to the agent failed.
    WriteError,
    /// The agent violated framing or flooded us.
    Protocol,
    /// The agent stopped reading its input: a frame was not delivered in
    /// time (see the module docs).
    Stalled,
    /// We shut the connection down.
    Shutdown,
}

struct Outgoing {
    line: Vec<u8>,
    _permit: OwnedSemaphorePermit,
    /// Told once the frame was written and flushed.
    written: Option<oneshot::Sender<()>>,
    _deadline: Option<FrameDeadline>,
}

type Waiter = oneshot::Sender<Result<Value, CallError>>;

struct Shared {
    sender: Mutex<Option<mpsc::Sender<Outgoing>>>,
    budget: Arc<Semaphore>,
    pending: Mutex<HashMap<i64, Waiter>>,
    next_id: AtomicI64,
    next_frame: AtomicU64,
    deadlines: Mutex<HashMap<u64, Instant>>,
    deadline_changed: Notify,
    closed: watch::Sender<Option<CloseReason>>,
    writer: Mutex<Option<JoinHandle<()>>>,
    reader: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// A handle to one connection. Cheap to clone.
#[derive(Clone)]
pub struct Peer {
    shared: Arc<Shared>,
}

impl Peer {
    /// Start the reader and writer tasks on the current Tokio runtime.
    pub fn spawn<R, W>(reader: R, writer: W, inbound: Arc<dyn Inbound>) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (tx, rx) = mpsc::channel(MAX_QUEUED_FRAMES);
        let (closed, _) = watch::channel(None);
        let peer = Self {
            shared: Arc::new(Shared {
                sender: Mutex::new(Some(tx)),
                budget: Arc::new(Semaphore::new(MAX_QUEUED_BYTES)),
                pending: Mutex::new(HashMap::new()),
                next_id: AtomicI64::new(1),
                next_frame: AtomicU64::new(1),
                deadlines: Mutex::new(HashMap::new()),
                deadline_changed: Notify::new(),
                closed,
                writer: Mutex::new(None),
                reader: Mutex::new(None),
            }),
        };
        tokio::spawn(watch_deliveries(peer.clone()));
        let writer_task = tokio::spawn(write_loop(writer, rx, peer.clone()));
        let reader_task = tokio::spawn(read_loop(reader, peer.clone(), inbound));
        *lock(&peer.shared.writer) = Some(writer_task);
        *lock(&peer.shared.reader) = Some(reader_task);
        peer
    }

    /// Whether the connection is closed (or closing).
    pub fn is_closed(&self) -> bool {
        self.shared.closed.borrow().is_some()
    }

    /// Wait until the connection closes and report why.
    pub async fn closed(&self) -> CloseReason {
        let mut rx = self.shared.closed.subscribe();
        loop {
            if let Some(reason) = *rx.borrow_and_update() {
                return reason;
            }
            if rx.changed().await.is_err() {
                return CloseReason::Shutdown;
            }
        }
    }

    /// Mark the connection closed: fail every pending request and let the
    /// writer drain and close the output. Idempotent; the first reason wins.
    fn mark_closed(&self, reason: CloseReason) {
        // The flag is set before draining, and `request` checks it while
        // holding the pending lock, so no request can be stranded.
        self.shared.closed.send_if_modified(|current| {
            if current.is_none() {
                *current = Some(reason);
                true
            } else {
                false
            }
        });
        let drained: Vec<Waiter> = lock(&self.shared.pending)
            .drain()
            .map(|(_, waiter)| waiter)
            .collect();
        for waiter in drained {
            let _ = waiter.send(Err(CallError::Closed));
        }
        lock(&self.shared.sender).take();
        // Wake everyone waiting for queue room.
        self.shared.budget.close();
        if reason == CloseReason::Stalled {
            // A frame may be partly written: nothing more may follow it.
            if let Some(writer) = lock(&self.shared.writer).take() {
                writer.abort();
            }
        }
    }

    /// Close the connection because the caller could not complete a
    /// control exchange in time (see the module docs): what it consumed
    /// can no longer be answered, so the agent must not keep waiting on a
    /// live connection.
    pub fn abandon(&self) {
        self.mark_closed(CloseReason::Stalled);
    }

    pub(super) fn abandon_on_drop(&self) -> IncompleteControl {
        IncompleteControl(Some(self.clone()))
    }

    fn frame_deadline(&self, deadline: Instant) -> FrameDeadline {
        let id = self.shared.next_frame.fetch_add(1, Ordering::Relaxed);
        lock(&self.shared.deadlines).insert(id, deadline);
        self.shared.deadline_changed.notify_one();
        FrameDeadline {
            shared: Arc::downgrade(&self.shared),
            id,
        }
    }

    /// Queue `line` and wait until it was written, both until `deadline`
    /// (forever with `None`) — see the module docs for the outcomes.
    async fn deliver(&self, line: Vec<u8>, deadline: Option<Instant>) -> Result<(), CallError> {
        let size = permits_for(&line)?;
        let sender = lock(&self.shared.sender).clone().ok_or(CallError::Closed)?;
        let budget = self.shared.budget.clone();
        let (written, was_written) = oneshot::channel();
        // Cancel-safe: dropped before `send` completes, nothing is queued.
        let admit = async move {
            let permit = budget
                .acquire_many_owned(size)
                .await
                .map_err(|_| CallError::Closed)?;
            let slot = sender
                .reserve_owned()
                .await
                .map_err(|_| CallError::Closed)?;
            slot.send(Outgoing {
                line,
                _permit: permit,
                written: Some(written),
                _deadline: deadline.map(|deadline| self.frame_deadline(deadline)),
            });
            Ok::<(), CallError>(())
        };
        tokio::select! {
            biased;
            _ = self.closed() => return Err(CallError::Closed),
            admitted = admit => admitted?,
            () = until(deadline) => return Err(CallError::Timeout),
        }
        let mut incomplete = self.abandon_on_drop();
        let outcome = tokio::select! {
            biased;
            written = was_written => written.map_err(|_| CallError::Closed),
            _ = self.closed() => Err(CallError::Closed),
            () = until(deadline) => Err(CallError::Timeout),
        };
        let outcome = if outcome == Err(CallError::Closed)
            && deadline.is_some_and(|at| at <= Instant::now())
            && *self.shared.closed.borrow() == Some(CloseReason::Stalled)
        {
            Err(CallError::Timeout)
        } else {
            outcome
        };
        if outcome == Err(CallError::Timeout) {
            self.mark_closed(CloseReason::Stalled);
        }
        incomplete.disarm();
        outcome
    }

    fn try_enqueue(&self, line: Vec<u8>, deadline: Instant) -> Result<(), CallError> {
        let size = permits_for(&line)?;
        let sender = lock(&self.shared.sender).clone().ok_or(CallError::Closed)?;
        let permit = self
            .shared
            .budget
            .clone()
            .try_acquire_many_owned(size)
            .map_err(|error| match error {
                tokio::sync::TryAcquireError::Closed => CallError::Closed,
                tokio::sync::TryAcquireError::NoPermits => CallError::Busy,
            })?;
        sender
            .try_send(Outgoing {
                line,
                _permit: permit,
                written: None,
                _deadline: Some(self.frame_deadline(deadline)),
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => CallError::Busy,
                mpsc::error::TrySendError::Closed(_) => CallError::Closed,
            })
    }

    /// Send a request and wait for its answer. `timeout` bounds the whole
    /// call — queue room, the write and the answer; `None` waits until the
    /// answer arrives or the connection closes.
    pub async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Option<Duration>,
    ) -> Result<Value, CallError> {
        let deadline = timeout.map(|limit| Instant::now() + limit);
        self.send_request(method, params, deadline)
            .await?
            .wait_until(deadline)
            .await
    }

    /// Deliver a request (see the module docs: queue room and the write,
    /// until `deadline`) and return a handle for its answer. When this
    /// returns, the agent has the whole request, ahead of anything sent
    /// later.
    pub async fn send_request(
        &self,
        method: &str,
        params: Value,
        deadline: Option<Instant>,
    ) -> Result<Call, CallError> {
        let (tx, rx) = oneshot::channel();
        let id = {
            let mut pending = lock(&self.shared.pending);
            if self.is_closed() {
                return Err(CallError::Closed);
            }
            if pending.len() >= MAX_PENDING_REQUESTS {
                return Err(CallError::Busy);
            }
            let id = self.shared.next_id.fetch_add(1, Ordering::Relaxed);
            pending.insert(id, tx);
            id
        };
        // Removes the waiter on every early exit, including cancellation of
        // the caller, so a late answer is ignored rather than misrouted.
        let guard = PendingGuard {
            shared: self.shared.clone(),
            id,
        };
        let line = Frame::Request {
            id: RequestId::Number(id),
            method: method.to_string(),
            params: Some(params),
        }
        .to_line();
        self.deliver(line, deadline).await?;
        Ok(Call {
            rx,
            _guard: guard,
        })
    }

    /// Deliver a notification by `deadline` (see the module docs).
    pub async fn notify(
        &self,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<(), CallError> {
        let line = Frame::Notification {
            method: method.to_string(),
            params: Some(params),
        }
        .to_line();
        self.deliver(line, Some(deadline)).await
    }

    /// Deliver the answer to an inbound request by `deadline` (see the
    /// module docs).
    pub async fn respond(
        &self,
        id: RequestId,
        result: Result<Value, RpcError>,
        deadline: Instant,
    ) -> Result<(), CallError> {
        self.deliver(
            Frame::Response {
                id,
                result,
            }
            .to_line(),
            Some(deadline),
        )
        .await
    }

    /// Answer an inbound request without waiting. When the queue is full the
    /// connection is closed instead: an unanswered request would leave the
    /// agent waiting forever.
    pub fn respond_now(&self, id: RequestId, result: Result<Value, RpcError>) {
        self.respond_now_until(id, result, Instant::now() + RESPONSE_TIMEOUT);
    }

    /// Queue an automatic response with a deadline owned by the peer, even
    /// when no caller remains to wait for its delivery.
    pub(super) fn respond_now_until(
        &self,
        id: RequestId,
        result: Result<Value, RpcError>,
        deadline: Instant,
    ) {
        match self.try_enqueue(
            Frame::Response {
                id,
                result,
            }
            .to_line(),
            deadline,
        ) {
            Ok(()) | Err(CallError::Closed) => {},
            Err(error) => {
                tracing::warn!(%error, "ACP output queue overflow; closing the agent connection");
                self.mark_closed(CloseReason::Protocol);
            },
        }
    }

    /// Close the connection: fail pending requests, let the writer flush the
    /// queue and close the agent's input, and wait up to `grace` for it.
    pub async fn shutdown(&self, grace: Duration) {
        self.mark_closed(CloseReason::Shutdown);
        let writer = lock(&self.shared.writer).take();
        if let Some(mut writer) = writer {
            if tokio::time::timeout(grace, &mut writer).await.is_err() {
                writer.abort();
            }
        }
    }

    /// Wait up to `limit` for the agent to close its output (normally
    /// because it exited after its input closed). `true` when it did.
    pub async fn wait_output_closed(&self, limit: Duration) -> bool {
        let reader = lock(&self.shared.reader).take();
        let Some(mut reader) = reader else { return true };
        if tokio::time::timeout(limit, &mut reader).await.is_ok() {
            return true;
        }
        *lock(&self.shared.reader) = Some(reader);
        false
    }

    /// Stop reading. Called by the process owner after the agent was reaped,
    /// in case a descendant still holds the output open.
    pub fn abort_reader(&self) {
        if let Some(reader) = lock(&self.shared.reader).take() {
            reader.abort();
        }
    }
}

/// A consumed control cannot be left unresolved by a dropped caller future.
pub(super) struct IncompleteControl(Option<Peer>);

impl IncompleteControl {
    pub(super) fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for IncompleteControl {
    fn drop(&mut self) {
        if let Some(peer) = self.0.take() {
            peer.abandon();
        }
    }
}

// At most one entry per queued/writing frame. One watchdog covers every
// deadline, including a short control budget behind a long prompt write.
struct FrameDeadline {
    shared: Weak<Shared>,
    id: u64,
}
impl Drop for FrameDeadline {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.upgrade() {
            lock(&shared.deadlines).remove(&self.id);
            shared.deadline_changed.notify_one();
        }
    }
}

async fn watch_deliveries(peer: Peer) {
    loop {
        let changed = peer.shared.deadline_changed.notified();
        let deadline = {
            let deadlines = lock(&peer.shared.deadlines);
            let next = deadlines.values().copied().min();
            if next.is_some_and(|at| at <= Instant::now()) {
                peer.abandon();
                return;
            }
            next
        };
        tokio::select! {
            biased;
            _ = peer.closed() => return,
            _ = changed => {},
            () = until(deadline) => {},
        }
    }
}

fn permits_for(line: &[u8]) -> Result<u32, CallError> {
    if line.len() > MAX_FRAME_BYTES {
        return Err(CallError::TooLarge);
    }
    u32::try_from(line.len()).map_err(|_| CallError::TooLarge)
}

/// A request already handed to the writer, awaiting its answer.
pub struct Call {
    rx: oneshot::Receiver<Result<Value, CallError>>,
    _guard: PendingGuard,
}

impl Call {
    /// Wait for the answer; `None` waits until it arrives or the connection
    /// closes.
    pub async fn wait(self, timeout: Option<Duration>) -> Result<Value, CallError> {
        self.wait_until(timeout.map(|limit| Instant::now() + limit))
            .await
    }

    /// Wait for the answer until `deadline` (`None`: until it arrives or the
    /// connection closes).
    pub async fn wait_until(self, deadline: Option<Instant>) -> Result<Value, CallError> {
        let Self {
            rx,
            _guard,
        } = self;
        let answer = match deadline {
            Some(deadline) => tokio::time::timeout_at(deadline, rx)
                .await
                .map_err(|_| CallError::Timeout)?,
            None => rx.await,
        };
        answer.unwrap_or(Err(CallError::Closed))
    }
}

/// Resolves at `deadline`, or never.
async fn until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

struct PendingGuard {
    shared: Arc<Shared>,
    id: i64,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        lock(&self.shared.pending).remove(&self.id);
    }
}

async fn write_loop<W>(mut writer: W, mut rx: mpsc::Receiver<Outgoing>, peer: Peer)
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    while let Some(out) = rx.recv().await {
        let written = async {
            writer.write_all(&out.line).await?;
            writer.flush().await
        }
        .await;
        if written.is_err() {
            peer.mark_closed(CloseReason::WriteError);
            break;
        }
        drop(out._deadline);
        if let Some(done) = out.written {
            let _ = done.send(());
        }
    }
    let _ = writer.shutdown().await;
}

enum LineError {
    TooLong,
    Io,
}

/// Read one `\n`-terminated line into `buf` without ever holding more than
/// [`MAX_FRAME_BYTES`]. `Ok(false)` at a clean end of stream.
async fn read_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
) -> Result<bool, LineError> {
    buf.clear();
    loop {
        let (done, used) = {
            let available = reader.fill_buf().await.map_err(|_| LineError::Io)?;
            if available.is_empty() {
                return Ok(!buf.is_empty());
            }
            match available.iter().position(|byte| *byte == b'\n') {
                Some(end) => {
                    if buf.len() + end > MAX_FRAME_BYTES {
                        return Err(LineError::TooLong);
                    }
                    buf.extend_from_slice(&available[..end]);
                    (true, end + 1)
                },
                None => {
                    if buf.len() + available.len() > MAX_FRAME_BYTES {
                        return Err(LineError::TooLong);
                    }
                    buf.extend_from_slice(available);
                    (false, available.len())
                },
            }
        };
        reader.consume(used);
        if done {
            return Ok(true);
        }
    }
}

async fn read_loop<R>(reader: R, peer: Peer, inbound: Arc<dyn Inbound>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut reader = BufReader::with_capacity(64 * 1024, reader);
    let mut buf = Vec::new();
    let mut malformed = 0u32;
    loop {
        match read_line(&mut reader, &mut buf).await {
            Ok(true) => {},
            Ok(false) => {
                peer.mark_closed(CloseReason::Eof);
                return;
            },
            Err(LineError::TooLong) => {
                tracing::warn!("ACP agent sent an oversized frame; closing the connection");
                peer.mark_closed(CloseReason::Protocol);
                return;
            },
            Err(LineError::Io) => {
                peer.mark_closed(CloseReason::ReadError);
                return;
            },
        }
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
        if buf.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        match jsonrpc::parse(&buf) {
            Ok(Frame::Response {
                id,
                result,
            }) => settle(&peer, id, result),
            Ok(Frame::Request {
                id,
                method,
                params,
            }) => {
                inbound.request(&peer, id, &method, params);
            },
            Ok(Frame::Notification {
                method,
                params,
            }) => {
                inbound.notification(&peer, &method, params);
            },
            Err(error) => {
                malformed += 1;
                peer.respond_now(
                    RequestId::Null,
                    Err(RpcError::new(error.code(), error.to_string())),
                );
                if malformed > MAX_MALFORMED {
                    tracing::warn!(
                        "ACP agent keeps sending malformed frames; closing the connection"
                    );
                    peer.mark_closed(CloseReason::Protocol);
                    return;
                }
            },
        }
    }
}

fn settle(peer: &Peer, id: RequestId, result: Result<Value, RpcError>) {
    let RequestId::Number(number) = id else {
        tracing::debug!("ignoring an ACP response with a non-numeric id");
        return;
    };
    let waiter = lock(&peer.shared.pending).remove(&number);
    match waiter {
        Some(waiter) => {
            let _ = waiter.send(result.map_err(CallError::Rpc));
        },
        None => tracing::debug!(id = number, "ignoring an unmatched ACP response"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use serde_json::json;
    use tokio::io::{duplex, split, AsyncBufReadExt, AsyncWriteExt, BufReader as TokioBufReader};

    use super::*;

    #[derive(Default)]
    struct Recorder {
        requests: StdMutex<Vec<(RequestId, String)>>,
        notes: StdMutex<Vec<String>>,
    }

    impl Inbound for Recorder {
        fn notification(&self, _peer: &Peer, method: &str, _params: Option<Value>) {
            lock(&self.notes).push(method.to_string());
        }

        fn request(&self, peer: &Peer, id: RequestId, method: &str, _params: Option<Value>) {
            lock(&self.requests).push((id.clone(), method.to_string()));
            peer.respond_now(id, Ok(json!({"handled": method})));
        }
    }

    struct Agent {
        lines: tokio::io::Lines<TokioBufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
        out: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    }

    impl Agent {
        async fn next(&mut self) -> Value {
            let line = self.lines.next_line().await.expect("read").expect("line");
            serde_json::from_str(&line).expect("json")
        }

        async fn send(&mut self, value: Value) {
            let mut line = serde_json::to_vec(&value).expect("encode");
            line.push(b'\n');
            self.out.write_all(&line).await.expect("write");
        }
    }

    fn pair() -> (Peer, Agent, Arc<Recorder>) {
        let (host, agent) = duplex(1 << 20);
        let (host_read, host_write) = split(host);
        let (agent_read, agent_write) = split(agent);
        let recorder = Arc::new(Recorder::default());
        let peer = Peer::spawn(host_read, host_write, recorder.clone());
        let agent = Agent {
            lines: TokioBufReader::new(agent_read).lines(),
            out: agent_write,
        };
        (peer, agent, recorder)
    }

    #[tokio::test]
    async fn concurrent_requests_settle_by_id_and_namespaces_stay_apart() {
        let (peer, mut agent, recorder) = pair();
        let first = tokio::spawn({
            let peer = peer.clone();
            async move { peer.request("a", json!({}), None).await }
        });
        let one = agent.next().await;
        let second = tokio::spawn({
            let peer = peer.clone();
            async move { peer.request("b", json!({}), None).await }
        });
        let two = agent.next().await;
        // The agent reuses our id for its own request, and answers with a
        // string id first: neither may settle our numeric request.
        agent
            .send(json!({"jsonrpc": "2.0", "id": one["id"], "method": "x/y", "params": {}}))
            .await;
        let handled = agent.next().await;
        assert_eq!(handled["id"], one["id"]);
        assert_eq!(handled["result"]["handled"], "x/y");
        agent
            .send(json!({"jsonrpc": "2.0", "id": one["id"].to_string(), "result": "wrong"}))
            .await;
        agent
            .send(json!({"jsonrpc": "2.0", "id": two["id"], "result": "second"}))
            .await;
        agent
            .send(json!({"jsonrpc": "2.0", "id": one["id"], "error": {"code": -32000, "message": "auth"}}))
            .await;
        assert_eq!(second.await.expect("join"), Ok(json!("second")));
        match first.await.expect("join") {
            Err(CallError::Rpc(error)) => assert_eq!(error.code, RpcError::AUTH_REQUIRED),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(lock(&recorder.requests).len(), 1);
    }

    #[tokio::test]
    async fn eof_fails_pending_requests() {
        let (peer, agent, _) = pair();
        let call = tokio::spawn({
            let peer = peer.clone();
            async move { peer.request("a", json!({}), None).await }
        });
        drop(agent);
        assert_eq!(call.await.expect("join"), Err(CallError::Closed));
        assert_eq!(peer.closed().await, CloseReason::Eof);
        assert_eq!(peer.request("b", json!({}), None).await, Err(CallError::Closed));
    }

    #[tokio::test]
    async fn timeouts_forget_the_request_and_ignore_late_answers() {
        let (peer, mut agent, _) = pair();
        let late = peer
            .request("slow", json!({}), Some(Duration::from_millis(20)))
            .await;
        assert_eq!(late, Err(CallError::Timeout));
        let sent = agent.next().await;
        agent
            .send(json!({"jsonrpc": "2.0", "id": sent["id"], "result": 1}))
            .await;
        let next = tokio::spawn({
            let peer = peer.clone();
            async move { peer.request("next", json!({}), None).await }
        });
        let request = agent.next().await;
        assert_ne!(request["id"], sent["id"]);
        agent
            .send(json!({"jsonrpc": "2.0", "id": request["id"], "result": 2}))
            .await;
        assert_eq!(next.await.expect("join"), Ok(json!(2)));
    }

    #[tokio::test]
    async fn malformed_frames_get_an_error_and_notifications_are_delivered() {
        let (peer, mut agent, recorder) = pair();
        agent.out.write_all(b"{not json\n").await.expect("write");
        let error = agent.next().await;
        assert_eq!(error["id"], Value::Null);
        assert_eq!(error["error"]["code"], RpcError::PARSE);
        agent
            .send(json!({"jsonrpc": "2.0", "method": "session/update", "params": {}}))
            .await;
        agent
            .send(json!({"jsonrpc": "2.0", "id": null, "method": "null/id", "params": {}}))
            .await;
        let answered = agent.next().await;
        assert_eq!(answered["id"], Value::Null);
        assert_eq!(answered["result"]["handled"], "null/id");
        assert_eq!(*lock(&recorder.notes), vec!["session/update".to_string()]);
        assert!(!peer.is_closed());
    }

    #[tokio::test]
    async fn oversized_frames_close_the_connection() {
        let (peer, mut agent, _) = pair();
        let chunk = vec![b'a'; 1024 * 1024];
        for _ in 0..9 {
            if agent.out.write_all(&chunk).await.is_err() {
                break;
            }
        }
        assert_eq!(peer.closed().await, CloseReason::Protocol);
    }

    /// An agent that stays alive but never reads its input, until the queue
    /// is exhausted. A frame whose deadline passes while it waits for room
    /// was never queued, so the connection stays open; closing releases
    /// waiters that have no deadline.
    #[tokio::test]
    async fn deadlines_cover_an_exhausted_queue_and_closing_releases_every_waiter() {
        let (host, agent) = duplex(64);
        let (host_read, host_write) = split(host);
        let peer = Peer::spawn(host_read, host_write, Arc::new(Recorder::default()));
        let big = json!("x".repeat(MAX_FRAME_BYTES - 1024));
        let mut fills = Vec::new();
        for _ in 0..MAX_QUEUED_BYTES / (MAX_FRAME_BYTES - 1024) {
            fills.push(tokio::spawn({
                let (peer, big) = (peer.clone(), big.clone());
                async move { peer.send_request("fill", big, None).await.map(|_| ()) }
            }));
        }
        let frame = MAX_FRAME_BYTES - 1024;
        tokio::time::timeout(Duration::from_secs(5), async {
            while peer.shared.budget.available_permits() >= frame {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the queue fills");
        let started = Instant::now();
        let late = peer
            .request("session/load", big.clone(), Some(Duration::from_millis(200)))
            .await;
        assert_eq!(late, Err(CallError::Timeout), "the deadline covers queue admission");
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(!peer.is_closed(), "nothing of it was queued, so nothing is half-sent");
        let cancel = peer
            .notify("session/cancel", big.clone(), Instant::now() + Duration::from_millis(100))
            .await;
        assert_eq!(cancel, Err(CallError::Timeout), "control traffic fails boundedly");
        assert!(!peer.is_closed());
        let stuck = tokio::spawn({
            let peer = peer.clone();
            let big = big.clone();
            async move { peer.request("session/prompt", big, None).await }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!stuck.is_finished(), "a request without a deadline waits for room");
        peer.shutdown(Duration::from_millis(50)).await;
        let released = tokio::time::timeout(Duration::from_secs(5), stuck)
            .await
            .expect("closing wakes the waiter")
            .expect("join");
        assert_eq!(released, Err(CallError::Closed));
        for fill in fills {
            let filled = tokio::time::timeout(Duration::from_secs(5), fill)
                .await
                .expect("closing wakes the queued frames")
                .expect("join");
            assert_eq!(filled, Err(CallError::Closed));
        }
        drop(agent);
    }

    /// The writer is blocked in the middle of a frame while the queue still
    /// has room. Queue admission alone is not delivery: the deadline also
    /// covers the write, and a frame that may be partly written closes the
    /// connection at once, so its rest never reaches a later exchange.
    #[tokio::test]
    async fn a_frame_not_written_in_time_closes_the_connection() {
        let (host, mut agent) = duplex(64);
        let (host_read, host_write) = split(host);
        let peer = Peer::spawn(host_read, host_write, Arc::new(Recorder::default()));
        let started = Instant::now();
        let prompt = peer
            .send_request(
                "session/prompt",
                json!("p".repeat(64 * 1024)),
                Some(Instant::now() + Duration::from_millis(200)),
            )
            .await
            .map(|_| ());
        assert_eq!(prompt, Err(CallError::Timeout));
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(peer.closed().await, CloseReason::Stalled);
        assert_eq!(
            peer.notify("session/cancel", json!({}), Instant::now() + Duration::from_secs(1))
                .await,
            Err(CallError::Closed),
            "nothing is written after a frame that may be partial"
        );
        // Whatever reached the agent ends without the rest of the frame.
        let mut seen = 0;
        let mut buf = vec![0u8; 4096];
        while let Ok(Ok(read)) = tokio::time::timeout(
            Duration::from_millis(300),
            tokio::io::AsyncReadExt::read(&mut agent, &mut buf),
        )
        .await
        {
            if read == 0 {
                break;
            }
            seen += read;
        }
        assert!(seen < 64 * 1024, "the frame was never completed");
    }

    /// A frame the agent reads is delivered: the call returns after the
    /// write, and the request is then answered as usual.
    #[tokio::test]
    async fn delivered_frames_return_after_the_write() {
        let (peer, mut agent, _) = pair();
        let deadline = Instant::now() + Duration::from_secs(5);
        peer.notify("session/cancel", json!({"sessionId": "s"}), deadline)
            .await
            .expect("delivered");
        assert_eq!(agent.next().await["method"], "session/cancel");
        let call = peer
            .send_request("session/prompt", json!({}), Some(deadline))
            .await
            .expect("delivered");
        let sent = agent.next().await;
        agent
            .send(json!({"jsonrpc": "2.0", "id": sent["id"], "result": {"stopReason": "end_turn"}}))
            .await;
        assert_eq!(call.wait(None).await, Ok(json!({"stopReason": "end_turn"})));
        assert!(!peer.is_closed());
    }

    #[tokio::test]
    async fn dropped_partial_write_abandons_the_peer() {
        let (host, mut agent) = duplex(64);
        let (reader, writer) = split(host);
        let peer = Peer::spawn(reader, writer, Arc::new(Recorder::default()));
        let mut response = Box::pin(peer.respond(
            RequestId::Number(1),
            Ok(json!("x".repeat(8192))),
            Instant::now() + Duration::from_secs(10),
        ));
        assert!(tokio::time::timeout(Duration::from_millis(20), &mut response)
            .await
            .is_err());
        let mut prefix = [0; 1];
        tokio::io::AsyncReadExt::read_exact(&mut agent, &mut prefix)
            .await
            .expect("partly written");
        drop(response);
        assert_eq!(peer.closed().await, CloseReason::Stalled);
    }

    #[tokio::test]
    async fn automatic_reply_deadline_covers_waiting_behind_an_unbounded_write() {
        let (host, _agent) = duplex(64);
        let (reader, writer) = split(host);
        let peer = Peer::spawn(reader, writer, Arc::new(Recorder::default()));
        let mut prompt =
            Box::pin(peer.send_request("session/prompt", json!("x".repeat(8192)), None));
        assert!(tokio::time::timeout(Duration::from_millis(20), &mut prompt)
            .await
            .is_err());
        peer.respond_now_until(
            RequestId::Number(1),
            Ok(json!({})),
            Instant::now() + Duration::from_millis(30),
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), peer.closed())
                .await
                .expect("owned deadline"),
            CloseReason::Stalled
        );
        assert!(prompt.await.is_err());
    }

    #[tokio::test]
    async fn shutdown_flushes_queued_answers_then_closes_the_output() {
        let (peer, mut agent, _) = pair();
        peer.respond_now(RequestId::Str("q".into()), Ok(json!("done")));
        peer.shutdown(Duration::from_secs(1)).await;
        let answer = agent.next().await;
        assert_eq!(answer["id"], "q");
        assert!(agent.lines.next_line().await.expect("read").is_none());
        assert_eq!(peer.request("late", json!({}), None).await, Err(CallError::Closed));
    }
}
