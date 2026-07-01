//! Read codex session *rollout* files directly from `CODEX_HOME`.
//!
//! Every codex client (the CLI, the desktop app, the VS Code extension,
//! and Pocket-Codex's own spawned app-server) appends a JSON-Lines
//! transcript — a *rollout* — under `$CODEX_HOME/sessions/YYYY/MM/DD/`
//! as it runs. Because all those clients share one `CODEX_HOME`, reading
//! the rollouts lets Pocket-Codex *observe* sessions it did not create
//! (e.g. the ones the desktop app is driving) without going through any
//! app-server.
//!
//! This module is deliberately transport-free and side-effect-free: it
//! parses the on-disk JSONL and classifies a session's most recent turn
//! ([`TurnState`]). Whether a session is *owned by a live process* — and
//! therefore unsafe to resume — is a separate, liveness question
//! answered by [`crate::liveness`]; the two are combined in
//! [`crate::takeover`].
//!
//! ## Rollout wire shape
//!
//! Each line is one JSON object `{"timestamp", "type", "payload"}` where
//! `type` is the rollout-item kind and `payload` its body. The first
//! line is always `"type":"session_meta"`. Turn lifecycle is carried by
//! `"type":"event_msg"` lines whose `payload.type` is one of
//! `task_started`, `task_complete`, or `turn_aborted`.

use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use pocket_codex_core::{Error, Result};
use serde_json::Value;
use tracing::debug;

/// Only the trailing window of a rollout is scanned to classify the most
/// recent turn: the lifecycle markers (`task_started` / `task_complete` /
/// `turn_aborted`) are tiny events that land at the very end of the
/// transcript, so a bounded tail keeps scanning many sessions cheap. A
/// full-file fallback covers the rare case where the last turn's markers
/// predate this window (see [`classify_turn_state`]).
const MAX_TAIL_BYTES: u64 = 256 * 1024;

/// How many leading lines to scan for a human-readable preview.
const PREVIEW_HEAD_LINES: usize = 64;

/// Maximum length of the extracted preview string.
const PREVIEW_MAX_CHARS: usize = 160;

/// Lifecycle state of a session's most recent turn, derived purely from
/// the rollout transcript (no liveness probe).
///
/// `Incomplete` alone cannot tell *running* from *crashed* — a live
/// writer and a dead one both leave a turn without its terminal marker.
/// Disambiguating requires the liveness probe in [`crate::liveness`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnState {
    /// The transcript carries no turn lifecycle events yet (a freshly
    /// created session that has not run a turn).
    Empty,
    /// The most recent turn ended normally (`task_complete`).
    Completed,
    /// The most recent turn was aborted; carries the reason token
    /// (`interrupted` / `replaced` / `reviewEnded` / `budgetLimited` /
    /// …) as serialized on the wire.
    Aborted(String),
    /// A turn was started but has no matching `task_complete` or
    /// `turn_aborted` after it — it is either running right now or was
    /// left dangling by a crashed writer.
    Incomplete,
}

impl TurnState {
    /// Whether the most recent turn has reached a terminal state
    /// (completed or aborted), i.e. the session is *not* mid-turn from
    /// the transcript's point of view. `Empty` counts as finished (there
    /// is nothing in flight).
    pub fn is_finished(&self) -> bool {
        matches!(self, TurnState::Completed | TurnState::Aborted(_) | TurnState::Empty)
    }

    /// Stable lowercase tag for FFI / UI (`empty` / `completed` /
    /// `aborted` / `incomplete`).
    pub fn tag(&self) -> &'static str {
        match self {
            TurnState::Empty => "empty",
            TurnState::Completed => "completed",
            TurnState::Aborted(_) => "aborted",
            TurnState::Incomplete => "incomplete",
        }
    }
}

/// Session header parsed from a rollout's first `session_meta` line.
#[derive(Debug, Clone)]
pub struct SessionMeta {
    /// Thread / conversation id (equals the UUID embedded in the rollout
    /// filename).
    pub id: String,
    /// Working directory the session controls, when recorded.
    pub cwd: Option<String>,
    /// Client that created the session (`cli` / `vscode` / …), when
    /// recorded. Useful for badging "from desktop app" in the UI.
    pub source: Option<String>,
    /// The recorded `originator` string, when present.
    pub originator: Option<String>,
}

/// A session discovered on disk, with its rollout path, header metadata
/// and classified most-recent-turn state.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// Thread / conversation id.
    pub thread_id: String,
    /// Absolute path to the rollout JSONL file.
    pub rollout_path: PathBuf,
    /// Working directory the session controls, when recorded.
    pub cwd: Option<String>,
    /// Client that created the session, when recorded.
    pub source: Option<String>,
    /// Best-effort one-line preview (the first user message text).
    pub preview: String,
    /// Last-modified time of the rollout file, in unix seconds.
    pub updated_at: i64,
    /// Classified state of the most recent turn.
    pub turn_state: TurnState,
}

/// Resolve `CODEX_HOME`: the `CODEX_HOME` environment variable when set
/// and non-empty, otherwise `~/.codex`.
///
/// This mirrors how the codex binaries themselves resolve their home, so
/// the path Pocket-Codex reads matches the one the desktop app / CLI
/// write to (assuming the same environment).
pub fn codex_home() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("CODEX_HOME") {
        let path = PathBuf::from(dir);
        if !path.as_os_str().is_empty() {
            return Ok(path);
        }
    }
    let base = directories::BaseDirs::new()
        .ok_or_else(|| Error::Path("cannot determine home directory for CODEX_HOME".into()))?;
    Ok(base.home_dir().join(".codex"))
}

/// The `sessions` directory under [`codex_home`].
pub fn sessions_dir() -> Result<PathBuf> {
    Ok(codex_home()?.join("sessions"))
}

/// Enumerate every session under [`sessions_dir`], newest first.
///
/// Missing directories yield an empty list rather than an error (a fresh
/// install simply has no sessions). Individual files that fail to parse
/// are skipped (logged at debug) so one corrupt rollout cannot hide the
/// rest.
pub fn scan_sessions() -> Result<Vec<SessionInfo>> {
    let dir = sessions_dir()?;
    let mut files = Vec::new();
    collect_jsonl(&dir, &mut files)?;
    let mut out = Vec::with_capacity(files.len());
    for path in files {
        match read_session_info(&path) {
            Ok(info) => out.push(info),
            Err(err) => debug!(path = %path.display(), %err, "skipping unreadable rollout"),
        }
    }
    out.sort_by_key(|info| std::cmp::Reverse(info.updated_at));
    Ok(out)
}

/// Locate the rollout file for a given `thread_id`.
///
/// The thread id is embedded verbatim at the end of the rollout filename
/// (`rollout-<timestamp>-<thread_id>.jsonl`), so this anchors the match to that
/// exact suffix — a bare `contains` would let one id match another whose id (or
/// timestamp) merely contains it, resolving the wrong session. Returns `None`
/// when no rollout exists for the id.
pub fn rollout_path_for_thread(thread_id: &str) -> Result<Option<PathBuf>> {
    if thread_id.is_empty() {
        return Ok(None);
    }
    let suffix = format!("-{thread_id}.jsonl");
    let dir = sessions_dir()?;
    let mut files = Vec::new();
    collect_jsonl(&dir, &mut files)?;
    Ok(files.into_iter().find(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(&suffix))
    }))
}

/// Read header metadata, preview, mtime and turn state for one rollout.
pub fn read_session_info(path: &Path) -> Result<SessionInfo> {
    let head = read_head(path, PREVIEW_HEAD_LINES)?;
    let meta = head.lines().next().and_then(parse_session_meta);
    let thread_id = meta
        .as_ref()
        .map(|m| m.id.clone())
        .or_else(|| thread_id_from_path(path))
        .ok_or_else(|| Error::Config(format!("no thread id for rollout {}", path.display())))?;
    let updated_at = mtime_unix_secs(path);
    let turn_state = classify_turn_state(path)?;
    let preview = extract_preview(&head);
    Ok(SessionInfo {
        thread_id,
        rollout_path: path.to_path_buf(),
        cwd: meta.as_ref().and_then(|m| m.cwd.clone()),
        source: meta.as_ref().and_then(|m| m.source.clone()),
        preview,
        updated_at,
        turn_state,
    })
}

/// One displayable item parsed from a rollout transcript, in the same
/// `{type, title, text}` shape the live conversation renders, so the
/// read-only session viewer can reuse that rendering.
#[derive(Debug, Clone)]
pub struct TranscriptItem {
    /// Stable row id (the source line index).
    pub id: String,
    /// Item kind: `userMessage` / `agentMessage` / `reasoning` /
    /// `commandExecution`.
    pub item_type: String,
    /// One-line title (the command for tool calls; empty for messages).
    pub title: String,
    /// Body text: message markdown, reasoning summary, or command output.
    pub text: String,
    /// Image URLs attached to a user message (`data:image/...;base64,...` —
    /// codex inlines attachments into the model request, so that is what the
    /// rollout records). Empty for every other item kind.
    pub images: Vec<String>,
}

/// Parse a rollout's FULL transcript into displayable [`TranscriptItem`]s so
/// a session can be viewed read-only without resuming it. Maps codex's
/// on-disk `response_item` records:
///
/// * `message` (role `user` / `assistant`) → user / agent message
/// * `function_call` + its matching `function_call_output` (by `call_id`) → one
///   `commandExecution` item (title = command, text = output, ANSI stripped)
/// * `reasoning` with a non-empty `summary` → a reasoning item
///
/// Encrypted reasoning (no readable `summary`), lifecycle and token events
/// are skipped. Unreadable lines are skipped rather than failing the whole
/// read, so a partially-written rollout (one a live writer is appending to)
/// still renders what is parseable.
pub fn read_transcript(path: &Path) -> Result<Vec<TranscriptItem>> {
    use std::{collections::HashMap, io::BufRead};
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut out: Vec<TranscriptItem> = Vec::new();
    // Index of the `commandExecution` item awaiting its output, by call_id.
    let mut pending: HashMap<String, usize> = HashMap::new();
    for (idx, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("response_item") {
            continue;
        }
        let Some(payload) = value.get("payload") else {
            continue;
        };
        let id = format!("t{idx}");
        match payload.get("type").and_then(Value::as_str) {
            Some("message") => {
                let (text, images) = split_message_content(payload);
                // Keep image-only user messages (empty text, ≥1 image) — they
                // are real turns; only fully empty messages are noise.
                if text.trim().is_empty() && images.is_empty() {
                    continue;
                }
                let is_user = payload.get("role").and_then(Value::as_str) == Some("user");
                out.push(TranscriptItem {
                    id,
                    item_type: if is_user { "userMessage" } else { "agentMessage" }.to_string(),
                    title: String::new(),
                    text,
                    images,
                });
            },
            Some("function_call") => {
                let title = command_title(payload);
                out.push(TranscriptItem {
                    id,
                    item_type: "commandExecution".to_string(),
                    title,
                    text: String::new(),
                    images: Vec::new(),
                });
                if let Some(call_id) = payload.get("call_id").and_then(Value::as_str) {
                    pending.insert(call_id.to_string(), out.len() - 1);
                }
            },
            Some("function_call_output") => {
                let output =
                    strip_ansi(payload.get("output").and_then(Value::as_str).unwrap_or(""));
                match payload
                    .get("call_id")
                    .and_then(Value::as_str)
                    .and_then(|cid| pending.get(cid).copied())
                {
                    // Merge the output into its originating command item.
                    Some(i) => out[i].text = output,
                    None => out.push(TranscriptItem {
                        id,
                        item_type: "commandExecution".to_string(),
                        title: String::new(),
                        text: output,
                        images: Vec::new(),
                    }),
                }
            },
            Some("reasoning") => {
                let summary = reasoning_summary(payload);
                if !summary.trim().is_empty() {
                    out.push(TranscriptItem {
                        id,
                        item_type: "reasoning".to_string(),
                        title: String::new(),
                        text: summary,
                        images: Vec::new(),
                    });
                }
            },
            _ => {},
        }
    }
    Ok(out)
}

/// Split a `message` payload's `content` array into (typed text, image data
/// URLs). codex uses `input_text` / `output_text` parts for text (both carry
/// it under `text`) and `input_image` parts for inlined attachments — every
/// attachment (including `localImage` paths) is converted into a
/// `data:image/...;base64,...` URL when the model request is built, so the
/// rollout records renderable URLs.
///
/// The `<image>` / `<image name=…>` / `</image>` marker texts codex wraps
/// around each inlined image are wire framing, not something the user typed —
/// but a marker is stripped ONLY when it is actually adjacent to an
/// `input_image` part (mirroring codex's own `parse_user_message`), so a user
/// who literally typed "<image>" keeps their text.
fn split_message_content(payload: &Value) -> (String, Vec<String>) {
    let Some(parts) = payload.get("content").and_then(Value::as_array) else {
        return (String::new(), Vec::new());
    };
    let is_image = |p: Option<&Value>| {
        p.and_then(|p| p.get("type")).and_then(Value::as_str) == Some("input_image")
    };
    let mut text = String::new();
    let mut images = Vec::new();
    for (idx, part) in parts.iter().enumerate() {
        if is_image(Some(part)) {
            if let Some(url) = part.get("image_url").and_then(Value::as_str) {
                images.push(url.to_string());
            }
            continue;
        }
        let Some(t) = part.get("text").and_then(Value::as_str) else {
            continue;
        };
        let open = t == "<image>" || (t.starts_with("<image name=") && t.ends_with('>'));
        let close = t == "</image>";
        if (open && is_image(parts.get(idx + 1)))
            || (close && idx > 0 && is_image(parts.get(idx - 1)))
        {
            continue;
        }
        text.push_str(t);
    }
    (text, images)
}

/// One-line title for a `function_call`: the shell command when present,
/// else the tool name. `arguments` is a JSON *string* that must be
/// re-parsed; `command` may be a string or an argv array.
fn command_title(payload: &Value) -> String {
    let name = payload
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("tool");
    let args: Option<Value> = payload
        .get("arguments")
        .and_then(Value::as_str)
        .and_then(|s| serde_json::from_str(s).ok());
    if let Some(args) = args.as_ref() {
        match args.get("command") {
            Some(Value::String(s)) => return s.clone(),
            Some(Value::Array(parts)) => {
                let joined = parts
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ");
                if !joined.is_empty() {
                    return joined;
                }
            },
            _ => {},
        }
    }
    name.to_string()
}

/// Join a `reasoning` payload's `summary[].text`. codex stores the model's
/// raw reasoning under `encrypted_content` (unreadable); only the optional
/// `summary` is human-readable, and is often empty.
fn reasoning_summary(payload: &Value) -> String {
    payload
        .get("summary")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// Strip ANSI/VT escape sequences (CSI `ESC [ … final-byte` and a few
/// common others) from command output so it reads cleanly in the UI.
/// Avoids a regex dependency; keeps everything else verbatim.
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            // CSI: ESC [ ... <final byte 0x40..=0x7E>
            Some('[') => {
                chars.next();
                for ec in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&ec) {
                        break;
                    }
                }
            },
            // OSC: ESC ] ... terminated by BEL or ESC \
            Some(']') => {
                chars.next();
                while let Some(ec) = chars.next() {
                    if ec == '\u{7}' {
                        break;
                    }
                    if ec == '\u{1b}' {
                        chars.next(); // consume the trailing backslash
                        break;
                    }
                }
            },
            // Lone ESC or two-char sequence: drop the next char.
            _ => {
                chars.next();
            },
        }
    }
    out
}

/// Classify the most recent turn of a rollout from its tail.
///
/// Reads only the trailing [`MAX_TAIL_BYTES`]; if that window contains no
/// lifecycle markers yet the file is larger than the window (a long turn
/// whose `task_started` predates the tail), it falls back to a full
/// scan so a long-running turn is never misread as [`TurnState::Empty`].
pub fn classify_turn_state(path: &Path) -> Result<TurnState> {
    let (text, truncated) = read_tail(path, MAX_TAIL_BYTES)?;
    let skip = usize::from(truncated);
    let state = classify_lines(text.lines().skip(skip));
    if matches!(state, TurnState::Empty) && truncated {
        // The tail held no lifecycle markers but the file is larger than the
        // window, so a `task_started` predates it. Stream the whole file line
        // by line (one line in memory at a time) rather than reading it into a
        // single String, which a pathologically large rollout could OOM.
        return scan_full_turn_state(path);
    }
    Ok(state)
}

/// Stream a rollout line by line to classify its most recent turn, keeping
/// memory bounded to one line at a time. The [`classify_turn_state`] fallback
/// for files larger than the tail window.
fn scan_full_turn_state(path: &Path) -> Result<TurnState> {
    use std::io::BufRead;
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    let mut state = TurnState::Empty;
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        if let Some(s) = turn_state_of_line(&String::from_utf8_lossy(&line)) {
            state = s;
        }
    }
    Ok(state)
}

/// Classify turn state from an ordered iterator of rollout lines.
///
/// Pure and allocation-light: a cheap substring prefilter skips the
/// overwhelming majority of lines (messages, reasoning, tool output)
/// before any JSON parsing, so only the tiny lifecycle events are
/// deserialized. The classification is the state left by the *last*
/// lifecycle event in file order:
///
/// * `task_started`  ⇒ [`TurnState::Incomplete`]
/// * `task_complete` ⇒ [`TurnState::Completed`]
/// * `turn_aborted`  ⇒ [`TurnState::Aborted`]
///
/// with [`TurnState::Empty`] when none are present.
fn classify_lines<'a, I: Iterator<Item = &'a str>>(lines: I) -> TurnState {
    let mut state = TurnState::Empty;
    for line in lines {
        if let Some(s) = turn_state_of_line(line) {
            state = s;
        }
    }
    state
}

/// Classify a single rollout line as a turn lifecycle transition, or `None`
/// when it is not a `task_started` / `task_complete` / `turn_aborted`
/// `event_msg`. Shared by the tail scan and the streaming full-file fallback.
fn turn_state_of_line(line: &str) -> Option<TurnState> {
    if !(line.contains("task_started")
        || line.contains("task_complete")
        || line.contains("turn_aborted"))
    {
        return None;
    }
    let value: Value = serde_json::from_str(line).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("event_msg") {
        return None;
    }
    let payload = value.get("payload")?;
    match payload.get("type").and_then(Value::as_str) {
        Some("task_started") => Some(TurnState::Incomplete),
        Some("task_complete") => Some(TurnState::Completed),
        Some("turn_aborted") => Some(TurnState::Aborted(parse_abort_reason(payload))),
        _ => None,
    }
}

/// Extract a `turn_aborted` reason token, tolerating either a plain
/// string (`"reason":"interrupted"`) or a tagged object
/// (`"reason":{"replaced":…}`). Falls back to `"unknown"`.
fn parse_abort_reason(payload: &Value) -> String {
    match payload.get("reason") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(map)) => map
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string()),
        _ => "unknown".to_string(),
    }
}

/// Parse a rollout's first line into [`SessionMeta`]; `None` when the
/// line is not a `session_meta` record or lacks an id.
fn parse_session_meta(line: &str) -> Option<SessionMeta> {
    let value: Value = serde_json::from_str(line).ok()?;
    if value.get("type").and_then(Value::as_str)? != "session_meta" {
        return None;
    }
    let payload = value.get("payload")?;
    let id = payload.get("id").and_then(Value::as_str)?.to_string();
    Some(SessionMeta {
        id,
        cwd: payload
            .get("cwd")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        source: source_label(payload.get("source")),
        originator: payload
            .get("originator")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    })
}

/// Reduce a `source` value (a bare string, or an object with a `kind` /
/// `type` discriminant) to a short label.
fn source_label(source: Option<&Value>) -> Option<String> {
    match source? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Object(map) => map
            .get("kind")
            .or_else(|| map.get("type"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

/// Best-effort preview: the first user message text found in the head of
/// a rollout, trimmed to [`PREVIEW_MAX_CHARS`].
fn extract_preview(head: &str) -> String {
    for line in head.lines() {
        if !(line.contains("user_message")
            || line.contains("input_text")
            || line.contains("\"role\":\"user\""))
        {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(text) = user_text(&value) {
            return truncate_chars(text.trim(), PREVIEW_MAX_CHARS);
        }
    }
    String::new()
}

/// Pull user-authored text out of a rollout line, tolerating the shapes
/// codex uses (`user_message` event, `input_text` payload, or a
/// `message` response item with `role:"user"` and a `content[].text`).
fn user_text(value: &Value) -> Option<String> {
    let payload = value.get("payload")?;
    if let Some(text) = payload.get("text").and_then(Value::as_str) {
        if !text.trim().is_empty() {
            return Some(text.to_string());
        }
    }
    let is_user_message = payload.get("role").and_then(Value::as_str) == Some("user")
        || payload.get("type").and_then(Value::as_str) == Some("user_message");
    if !is_user_message {
        return None;
    }
    // `content` is either a plain string (`"content":"hi"`) or the structured
    // array of parts (`[{"type":"input_text","text":"hi"}]`). Handle both so the
    // preview isn't lost for string-content sessions. The array form goes
    // through [`split_message_content`] so an attachment's `<image>` wire
    // markers never leak into the preview.
    let content = payload.get("content")?;
    if let Some(s) = content.as_str() {
        return (!s.trim().is_empty()).then(|| s.to_string());
    }
    content.as_array()?;
    let (collected, _images) = split_message_content(payload);
    (!collected.trim().is_empty()).then_some(collected)
}

/// Truncate to at most `max` characters (char-boundary safe), appending
/// an ellipsis when shortened.
fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Derive the thread id from a rollout filename
/// (`rollout-<ts>-<thread_id>.jsonl`): the id is the trailing five
/// dash-separated groups of the stem. Returns `None` when the stem is
/// too short to contain one.
fn thread_id_from_path(path: &Path) -> Option<String> {
    let stem = path.file_stem().and_then(|s| s.to_str())?;
    let parts: Vec<&str> = stem.split('-').collect();
    if parts.len() < 5 {
        return None;
    }
    Some(parts[parts.len() - 5..].join("-"))
}

/// Last-modified time of `path` in unix seconds (0 when unavailable).
fn mtime_unix_secs(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Read the trailing `max` bytes of a file as lossy UTF-8, returning the
/// text and whether the file was larger than `max` (so the first
/// recovered line may be partial and should be dropped by the caller).
fn read_tail(path: &Path, max: u64) -> Result<(String, bool)> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let truncated = len > max;
    if truncated {
        file.seek(SeekFrom::Start(len - max))?;
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok((String::from_utf8_lossy(&bytes).into_owned(), truncated))
}

/// Read at most the first `max_lines` lines of a file as lossy UTF-8.
///
/// Reads raw bytes per line and decodes lossily rather than `lines()` (which
/// errors on the first invalid-UTF-8 byte): rollouts are written by external
/// processes, and one malformed byte must not hide the whole session.
fn read_head(path: &Path, max_lines: usize) -> Result<String> {
    use std::io::BufRead;
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    let mut out = String::new();
    let mut line = Vec::new();
    for _ in 0..max_lines {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        out.push_str(&String::from_utf8_lossy(&line));
    }
    Ok(out)
}

/// Recursively collect `*.jsonl` files beneath `dir` into `out`.
///
/// A missing `dir` is not an error (it just contributes nothing).
fn collect_jsonl(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_jsonl(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started(turn: &str) -> String {
        format!(
            r#"{{"timestamp":"t","type":"event_msg","payload":{{"type":"task_started","turn_id":"{turn}"}}}}"#
        )
    }
    fn completed(turn: &str) -> String {
        format!(
            r#"{{"timestamp":"t","type":"event_msg","payload":{{"type":"task_complete","turn_id":"{turn}","last_agent_message":"done"}}}}"#
        )
    }
    fn aborted(turn: &str, reason: &str) -> String {
        format!(
            r#"{{"timestamp":"t","type":"event_msg","payload":{{"type":"turn_aborted","turn_id":"{turn}","reason":"{reason}"}}}}"#
        )
    }
    fn noise() -> String {
        r#"{"timestamp":"t","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hi"}]}}"#.to_string()
    }

    #[test]
    fn empty_when_no_lifecycle_events() {
        assert_eq!(classify_lines([noise(), noise()].iter().map(String::as_str)), TurnState::Empty);
    }

    #[test]
    fn completed_when_last_turn_finishes() {
        let lines = [started("a"), noise(), completed("a")];
        assert_eq!(classify_lines(lines.iter().map(String::as_str)), TurnState::Completed);
    }

    #[test]
    fn incomplete_when_started_without_terminal() {
        let lines = [started("a"), completed("a"), started("b"), noise()];
        assert_eq!(classify_lines(lines.iter().map(String::as_str)), TurnState::Incomplete);
    }

    #[test]
    fn aborted_carries_reason() {
        let lines = [started("a"), aborted("a", "interrupted")];
        assert_eq!(
            classify_lines(lines.iter().map(String::as_str)),
            TurnState::Aborted("interrupted".into())
        );
    }

    #[test]
    fn last_turn_wins_over_earlier_ones() {
        // completed → aborted → completed: the final completed is the verdict.
        let lines = [completed("a"), aborted("b", "replaced"), started("c"), completed("c")];
        assert_eq!(classify_lines(lines.iter().map(String::as_str)), TurnState::Completed);
    }

    #[test]
    fn turn_state_finished_predicate() {
        assert!(TurnState::Completed.is_finished());
        assert!(TurnState::Aborted("x".into()).is_finished());
        assert!(TurnState::Empty.is_finished());
        assert!(!TurnState::Incomplete.is_finished());
    }

    #[test]
    fn parses_session_meta_fields() {
        let line = r#"{"timestamp":"t","type":"session_meta","payload":{"id":"thr-1","cwd":"/repo","source":"vscode","originator":"codex_cli"}}"#;
        let meta = parse_session_meta(line).expect("meta");
        assert_eq!(meta.id, "thr-1");
        assert_eq!(meta.cwd.as_deref(), Some("/repo"));
        assert_eq!(meta.source.as_deref(), Some("vscode"));
        assert_eq!(meta.originator.as_deref(), Some("codex_cli"));
    }

    #[test]
    fn session_meta_source_object_shape() {
        let line = r#"{"type":"session_meta","payload":{"id":"x","source":{"kind":"cli"}}}"#;
        let meta = parse_session_meta(line).expect("meta");
        assert_eq!(meta.source.as_deref(), Some("cli"));
    }

    #[test]
    fn non_meta_first_line_is_rejected() {
        let line = r#"{"type":"event_msg","payload":{"type":"task_started"}}"#;
        assert!(parse_session_meta(line).is_none());
    }

    #[test]
    fn thread_id_from_rollout_filename() {
        let path =
            Path::new("rollout-2026-06-17T06-20-52-019ed285-eb3f-7010-b4f9-2ad9389c8e99.jsonl");
        assert_eq!(
            thread_id_from_path(path).as_deref(),
            Some("019ed285-eb3f-7010-b4f9-2ad9389c8e99")
        );
    }

    #[test]
    fn extracts_first_user_preview() {
        let head = format!(
            "{}\n{}\n",
            r#"{"type":"session_meta","payload":{"id":"x"}}"#,
            r#"{"type":"event_msg","payload":{"type":"user_message","content":[{"type":"text","text":"hello there"}]}}"#
        );
        assert_eq!(extract_preview(&head), "hello there");
    }

    #[test]
    fn extracts_preview_from_plain_string_content() {
        // `content` as a plain string rather than the structured parts array.
        let head = format!(
            "{}\n{}\n",
            r#"{"type":"session_meta","payload":{"id":"x"}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":"hi from a string"}}"#
        );
        assert_eq!(extract_preview(&head), "hi from a string");
    }

    #[test]
    fn truncate_is_char_boundary_safe() {
        let s = "héllo wörld and then some more text past the limit boundary here";
        let out = truncate_chars(s, 10);
        assert_eq!(out.chars().count(), 10);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn strip_ansi_removes_escape_sequences() {
        assert_eq!(strip_ansi("\u{1b}[31;1mred\u{1b}[0m text"), "red text");
        assert_eq!(strip_ansi("plain text"), "plain text");
        // OSC sequence terminated by BEL.
        assert_eq!(strip_ansi("a\u{1b}]0;title\u{7}b"), "ab");
    }

    #[test]
    fn command_title_prefers_the_shell_command() {
        let call: Value = serde_json::from_str(
            r#"{"type":"function_call","name":"shell_command","arguments":"{\"command\":\"ls -la\"}"}"#,
        )
        .expect("parse shell_command json");
        assert_eq!(command_title(&call), "ls -la");
        // argv-array form.
        let argv: Value = serde_json::from_str(
            r#"{"name":"shell","arguments":"{\"command\":[\"echo\",\"hi\"]}"}"#,
        )
        .expect("parse shell argv json");
        assert_eq!(command_title(&argv), "echo hi");
        // falls back to the tool name when there's no command.
        let tool: Value = serde_json::from_str(r#"{"name":"apply_patch","arguments":"{}"}"#)
            .expect("parse apply_patch json");
        assert_eq!(command_title(&tool), "apply_patch");
    }

    #[test]
    fn read_transcript_maps_messages_commands_and_merges_output() {
        let lines = [
            r#"{"type":"session_meta","payload":{"id":"x"}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"function_call","name":"shell_command","arguments":"{\"command\":\"echo hi\"}","call_id":"c1"}}"#,
            r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"[32mhi[0m"}}"#,
            r#"{"type":"response_item","payload":{"type":"reasoning","summary":[],"encrypted_content":"opaque"}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}}"#,
        ];
        let path = std::env::temp_dir().join(format!(
            "pcx-transcript-{}-{}.jsonl",
            std::process::id(),
            line!()
        ));
        std::fs::write(&path, lines.join("\n")).expect("write transcript fixture");
        let items = read_transcript(&path).expect("read transcript");
        std::fs::remove_file(&path).ok();

        // session_meta + encrypted reasoning (no summary) are dropped;
        // function_call + its output collapse into one command item.
        assert_eq!(items.len(), 3, "{items:?}");
        assert_eq!(items[0].item_type, "userMessage");
        assert_eq!(items[0].text, "hi");
        assert_eq!(items[1].item_type, "commandExecution");
        assert_eq!(items[1].title, "echo hi");
        // The function_call_output collapsed into its command item (ANSI
        // stripping itself is covered by strip_ansi_removes_escape_sequences).
        assert_eq!(items[1].text, "[32mhi[0m");
        assert_eq!(items[2].item_type, "agentMessage");
        assert_eq!(items[2].text, "done");
    }

    #[test]
    fn read_transcript_extracts_user_message_images() {
        // codex inlines an attached image into the model request as an
        // `input_image` data URL wrapped in `<image>` marker texts; a
        // local-file attachment gets a `<image name=[Image #1]>` open tag.
        let lines = [
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"look: "},{"type":"input_text","text":"<image>"},{"type":"input_image","image_url":"data:image/png;base64,AAAA"},{"type":"input_text","text":"</image>"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<image name=[Image #1]>"},{"type":"input_image","image_url":"data:image/jpeg;base64,BBBB"},{"type":"input_text","text":"</image>"}]}}"#,
        ];
        let path = std::env::temp_dir().join(format!(
            "pcx-transcript-{}-{}.jsonl",
            std::process::id(),
            line!()
        ));
        std::fs::write(&path, lines.join("\n")).expect("write transcript fixture");
        let items = read_transcript(&path).expect("read transcript");
        std::fs::remove_file(&path).ok();

        assert_eq!(items.len(), 2, "{items:?}");
        // Marker texts are wire framing, not typed text — stripped.
        assert_eq!(items[0].text, "look: ");
        assert_eq!(items[0].images, vec!["data:image/png;base64,AAAA".to_string()]);
        // An image-only message (all its text was markers) is KEPT: it is a
        // real user turn, previously dropped by the empty-text skip.
        assert_eq!(items[1].item_type, "userMessage");
        assert_eq!(items[1].text, "");
        assert_eq!(items[1].images, vec!["data:image/jpeg;base64,BBBB".to_string()]);
    }

    #[test]
    fn image_markers_are_stripped_only_next_to_an_image() {
        // A user who literally typed "<image>" (no adjacent input_image) keeps
        // their text — stripping is adjacency-gated like codex's own parser.
        let payload: Value = serde_json::from_str(
            r#"{"type":"message","role":"user","content":[{"type":"input_text","text":"<image>"}]}"#,
        )
        .expect("parse literal-tag message");
        let (text, images) = split_message_content(&payload);
        assert_eq!(text, "<image>");
        assert!(images.is_empty());

        // …while a real wrapped attachment still strips its markers.
        let payload: Value = serde_json::from_str(
            r#"{"type":"message","role":"user","content":[
                {"type":"input_text","text":"<image>"},
                {"type":"input_image","image_url":"data:image/png;base64,CC"},
                {"type":"input_text","text":"</image>"}]}"#,
        )
        .expect("parse wrapped-image message");
        let (text, images) = split_message_content(&payload);
        assert_eq!(text, "");
        assert_eq!(images, vec!["data:image/png;base64,CC".to_string()]);
    }

    #[test]
    fn preview_skips_image_wire_markers() {
        // The sessions-list preview must not leak "<image></image>" framing
        // for a first message that carried an attachment.
        let line = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"look: "},{"type":"input_text","text":"<image>"},{"type":"input_image","image_url":"data:image/png;base64,AA"},{"type":"input_text","text":"</image>"}]}}"#;
        assert_eq!(extract_preview(line), "look:");
    }
}
