//! In-process log capture for the UI's real-time log viewer.
//!
//! Installs a `tracing` layer as the process-global subscriber. Every event is
//! formatted into a [`LogLine`], kept in a bounded ring buffer (so a viewer
//! opened later still sees recent history), and broadcast to any live
//! subscribers. The bridge exposes this to Dart as one stream (see
//! `api::bridge::log_events`) that replays the retained history then streams
//! live.
//!
//! codex, when hosted in-process, calls `tracing_subscriber` `try_init()` too —
//! but ours runs first (from `init_bridge`), so its call is a no-op and codex's
//! events flow through this layer as well.

use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
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

/// How long a log file is kept before it is pruned at startup.
const FILE_RETENTION: Duration = Duration::from_secs(6 * 60 * 60);

static CHANNEL: OnceCell<broadcast::Sender<LogLine>> = OnceCell::new();
static RING: OnceCell<Mutex<VecDeque<LogLine>>> = OnceCell::new();
/// Append handle for the on-disk log, `None` when no directory was set (tests)
/// or the file could not be opened.
static FILE: OnceCell<Mutex<Option<File>>> = OnceCell::new();

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

/// Feed a log line from an EXTERNAL source into the same stream — used to tail
/// a spawned (外接) codex process's own log file, so the viewer shows its logs
/// the way the in-process (自带) one already does. The level is a best-effort
/// parse of the raw line (for coloring); the raw line itself is the message.
pub fn push_external(target: &str, raw: &str) {
    emit(LogLine {
        level: parse_level(raw).to_string(),
        target: target.to_string(),
        message: raw.to_string(),
        timestamp_ms: now_ms(),
    });
}

/// Best-effort level from a formatted log line (codex writes `<ts> LEVEL
/// target: msg`), scanning the first few tokens so a later "error" in the
/// message body doesn't mis-color the line.
fn parse_level(raw: &str) -> &'static str {
    for tok in raw.split_whitespace().take(4) {
        match tok
            .trim_matches(|c: char| !c.is_ascii_alphabetic())
            .to_ascii_uppercase()
            .as_str()
        {
            "ERROR" => return "ERROR",
            "WARN" | "WARNING" => return "WARN",
            "INFO" => return "INFO",
            "DEBUG" => return "DEBUG",
            "TRACE" => return "TRACE",
            _ => {},
        }
    }
    "INFO"
}

/// Start writing captured lines to `<dir>/logs/pocket-codex-<date>.log`, and
/// drop files older than [`FILE_RETENTION`].
///
/// Separate from [`init`] because the support directory isn't known that early.
/// Failure is silent: the in-memory viewer is the primary sink, and losing the
/// file copy must not stop the app from starting.
pub fn init_file(support_dir: &Path) {
    let dir = support_dir.join("logs");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    prune_old_logs(&dir);
    let path = dir.join(format!("pocket-codex-{}.log", today_stamp()));
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok();
    let opened = file.is_some();
    FILE.get_or_init(|| Mutex::new(file));
    if opened {
        tracing::info!(target: "pocket_codex_bridge::logging", "log file: {}", path.display());
    }
}

/// Delete log files last modified longer ago than [`FILE_RETENTION`].
fn prune_old_logs(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("pocket-codex-") || !name.ends_with(".log") {
            continue;
        }
        let aged = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > FILE_RETENTION);
        if aged {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// `YYYY-MM-DD` in UTC, for the log file name.
fn today_stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    // Civil-from-days (Howard Hinnant's algorithm), so no date dependency here.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Milliseconds since the unix epoch rendered as `HH:MM:SS.mmm` UTC.
fn clock(timestamp_ms: i64) -> String {
    let total_ms = timestamp_ms.rem_euclid(86_400_000);
    let ms = total_ms % 1000;
    let secs = total_ms / 1000;
    format!("{:02}:{:02}:{:02}.{ms:03}", secs / 3600, (secs % 3600) / 60, secs % 60)
}

fn emit(line: LogLine) {
    if let Some(ring) = RING.get() {
        let mut r = ring.lock().unwrap_or_else(|e| e.into_inner());
        if r.len() >= RING_CAPACITY {
            r.pop_front();
        }
        r.push_back(line.clone());
    }
    if let Some(file) = FILE.get() {
        let mut guard = file.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(file) = guard.as_mut() {
            let _ = writeln!(
                file,
                "{} {:5} {} {}",
                clock(line.timestamp_ms),
                line.level,
                line.target,
                line.message
            );
        }
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
