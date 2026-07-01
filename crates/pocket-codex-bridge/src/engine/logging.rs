//! In-process log capture for the UI's real-time log viewer.
//!
//! Installs a `tracing` layer as the process-global subscriber. Every event is
//! formatted into a [`LogLine`], kept in a bounded ring buffer (so a viewer
//! opened later still sees recent history), and broadcast to any live
//! subscribers. The bridge exposes this to Dart as a snapshot + a live stream
//! (see `api::bridge::log_snapshot` / `log_events`).
//!
//! codex, when hosted in-process, calls `tracing_subscriber` `try_init()` too —
//! but ours runs first (from `init_bridge`), so its call is a no-op and codex's
//! events flow through this layer as well.

use std::{
    collections::VecDeque,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use once_cell::sync::OnceCell;
use tokio::sync::broadcast;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::{layer::Context, prelude::*, registry::LookupSpan, EnvFilter, Layer};

/// One captured log event, mirrored to Dart as `LogLineDto`.
#[derive(Clone, Debug)]
pub struct LogLine {
    /// `TRACE` / `DEBUG` / `INFO` / `WARN` / `ERROR`.
    pub level: String,
    /// Event target (crate / module path).
    pub target: String,
    /// The rendered message (plus any structured fields).
    pub message: String,
    /// Capture time, unix milliseconds.
    pub timestamp_ms: i64,
}

/// How many recent lines to retain for late-opening viewers.
const RING_CAPACITY: usize = 2000;
/// Broadcast backlog before slow subscribers start lagging (dropped lines are
/// reported to Dart as a gap rather than blocking the logger).
const CHANNEL_CAPACITY: usize = 1024;

static CHANNEL: OnceCell<broadcast::Sender<LogLine>> = OnceCell::new();
static RING: OnceCell<Mutex<VecDeque<LogLine>>> = OnceCell::new();

/// Install the capture layer as the global subscriber. Idempotent — safe to
/// call once at boot; a second call (or codex's own `try_init`) is a no-op.
pub fn init() {
    CHANNEL.get_or_init(|| broadcast::channel(CHANNEL_CAPACITY).0);
    RING.get_or_init(|| Mutex::new(VecDeque::with_capacity(RING_CAPACITY)));
    // Honour RUST_LOG when set, else a useful default: everything at info, our
    // own bridge at debug. The viewer's own filtering happens in Dart.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,pocket_codex_bridge=debug"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(CaptureLayer)
        .try_init();
}

/// A snapshot of the retained ring buffer (oldest first), for a viewer that
/// just opened.
pub fn snapshot() -> Vec<LogLine> {
    RING.get()
        .map(|r| {
            r.lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// A live receiver for events captured after this call. `None` before [`init`].
pub fn subscribe() -> Option<broadcast::Receiver<LogLine>> {
    CHANNEL.get().map(broadcast::Sender::subscribe)
}

fn emit(line: LogLine) {
    if let Some(ring) = RING.get() {
        let mut r = ring.lock().unwrap_or_else(|e| e.into_inner());
        if r.len() >= RING_CAPACITY {
            r.pop_front();
        }
        r.push_back(line.clone());
    }
    if let Some(tx) = CHANNEL.get() {
        // Err just means no viewers are open — the ring already retained it.
        let _ = tx.send(line);
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A `tracing` layer that funnels every (filtered) event into [`emit`].
struct CaptureLayer;

impl<S> Layer<S> for CaptureLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let level = match *meta.level() {
            Level::TRACE => "TRACE",
            Level::DEBUG => "DEBUG",
            Level::INFO => "INFO",
            Level::WARN => "WARN",
            Level::ERROR => "ERROR",
        };
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        emit(LogLine {
            level: level.to_string(),
            target: meta.target().to_string(),
            message: visitor.message,
            timestamp_ms: now_ms(),
        });
    }
}

/// Renders an event's `message` plus any structured fields into one line.
#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl MessageVisitor {
    fn append(&mut self, text: String) {
        if self.message.is_empty() {
            self.message = text;
        } else {
            self.message.push_str(", ");
            self.message.push_str(&text);
        }
    }
}

impl tracing::field::Visit for MessageVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.append(value.to_string());
        } else {
            self.append(format!("{}={value}", field.name()));
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.append(format!("{value:?}"));
        } else {
            self.append(format!("{}={value:?}", field.name()));
        }
    }
}
