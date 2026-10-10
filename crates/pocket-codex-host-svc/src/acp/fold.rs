//! Folding ACP `session/update` notifications into a bounded transcript.
//!
//! The host folds every update it receives (live and `session/load` replay)
//! so a controller that connects later can read the conversation; the
//! controller folds the same live updates with this same code (see
//! [`super::replica`]) to produce item events, so live and reloaded views
//! agree on item ids and text.
//!
//! Folding follows the schema's replacement rules: a `tool_call_update`
//! replaces the `content` / `locations` collections it supplies, a `plan`
//! replaces the whole plan, and message chunks append to the message they
//! continue.
//!
//! # Bounds
//!
//! Every retained byte is charged to the transcript — text, titles, ids,
//! images, tool input, diffs, locations, plans and a fixed per-item
//! overhead — and the limits hold even inside one long turn:
//!
//! - an item keeps at most [`MAX_ITEM_TEXT`] of text, [`MAX_ITEM_IMAGES`]
//!   images within [`MAX_ITEM_IMAGE_BYTES`], and tool input up to
//!   [`MAX_RAW_INPUT`]. Text past its limit ends with a visible marker and sets
//!   `text_full` (later text is dropped); anything else dropped is named in
//!   `omitted` (`images`, `diffs`, `input`, …) so the UI can say so — losing an
//!   image never stops the text that follows it. Both set `truncated`;
//! - past [`MAX_TRANSCRIPT_BYTES`] / [`MAX_ITEMS`] / [`MAX_TURNS`] whole oldest
//!   turns are evicted first, then the oldest items of the remaining turn
//!   (never its prompt or its newest item), counted in `omitted`.
//!
//! Any eviction marks the transcript `truncated`: it never pretends to be
//! complete. Item ids come from a per-turn counter that is never reused, so
//! they stay stable across eviction and recovery.
//!
//! # Protocol ids
//!
//! Tool call and message ids are the agent's and are compared exactly. An
//! id longer than [`MAX_PROTOCOL_ID`] is kept as a fixed-size key — a
//! prefix plus its SHA-256 — that is longer than any id kept verbatim, so
//! distinct ids never alias and one id always maps to the same key.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::schema::{select_options, TurnOutcome};

/// Largest text retained per item (a truncation marker follows when cut).
pub const MAX_ITEM_TEXT: usize = 512 * 1024;
/// Largest retained transcript, counting every retained byte.
pub const MAX_TRANSCRIPT_BYTES: usize = 8 * 1024 * 1024;
/// Most retained items.
pub const MAX_ITEMS: usize = 4000;
/// Most retained turns.
pub const MAX_TURNS: usize = 500;
/// Largest total size of the inline images kept on one item.
pub const MAX_ITEM_IMAGE_BYTES: usize = 2 * 1024 * 1024;
/// Most inline images kept on one item.
pub const MAX_ITEM_IMAGES: usize = 8;
/// Largest retained tool input, serialized.
pub const MAX_RAW_INPUT: usize = 64 * 1024;
const MAX_DIFFS: usize = 64;
const MAX_LOCATIONS: usize = 64;
const MAX_LOCATION: usize = 4096;
/// Bookkeeping charged per retained item on top of its payload.
const ITEM_OVERHEAD: usize = 128;
const TRUNCATED: &str = "\n[… truncated]";
/// Longest agent id kept verbatim (see the module docs).
pub const MAX_PROTOCOL_ID: usize = 256;
/// Bytes of an oversized id kept before its digest.
const ID_PREFIX: usize = 192;

/// Omission reason: images over the item's image budget.
pub const OMITTED_IMAGES: &str = "images";
/// Omission reason: tool diffs over their budget.
pub const OMITTED_DIFFS: &str = "diffs";
/// Omission reason: tool input over [`MAX_RAW_INPUT`].
pub const OMITTED_INPUT: &str = "input";
/// Omission reason: tool locations over their limit.
pub const OMITTED_LOCATIONS: &str = "locations";
/// Omission reason: tool content entries over their limit.
pub const OMITTED_CONTENT: &str = "content";
/// Omission reason: plan entries over their limit.
pub const OMITTED_PLAN: &str = "plan";

/// The key an agent id is kept under: itself when short, otherwise a
/// collision-resistant fixed-size form that no verbatim id can equal.
pub fn protocol_key(raw: &str) -> String {
    if raw.len() <= MAX_PROTOCOL_ID {
        return raw.to_string();
    }
    let mut end = ID_PREFIX;
    while !raw.is_char_boundary(end) {
        end -= 1;
    }
    let digest = pocket_codex_core::history_sync::digest_bytes(raw.as_bytes());
    format!("{}#sha256:{digest}", &raw[..end])
}

/// What a transcript item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// The user's prompt.
    UserMessage,
    /// Agent prose.
    AgentMessage,
    /// Agent reasoning.
    Thought,
    /// A tool call.
    ToolCall,
    /// The agent's plan.
    Plan,
}

/// A file modification reported in tool content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    /// File path as reported by the agent (display only).
    pub path: String,
    /// Previous content; `None` for a new file.
    pub old_text: Option<String>,
    /// New content.
    pub new_text: String,
}

/// Folded state of one tool call.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    /// The agent's tool call id.
    pub call_id: String,
    /// Tool kind (`read`, `edit`, `execute`, …, or an unknown future value).
    pub kind: String,
    /// `pending` / `in_progress` / `completed` / `failed`.
    pub status: String,
    /// Programmatic tool name, when supplied.
    pub name: Option<String>,
    /// Reported locations (display only; never file-read authority).
    pub locations: Vec<String>,
    /// Reported diffs.
    pub diffs: Vec<Diff>,
    /// Raw input, when supplied (replaced by a short note when too large).
    pub raw_input: Option<Value>,
    /// Embedded terminals (not supported by this client; counted only).
    pub terminals: usize,
}

/// One plan entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanEntry {
    /// Task description.
    pub content: String,
    /// `pending` / `in_progress` / `completed`.
    pub status: String,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// One transcript item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// Stable id within the session.
    pub id: String,
    /// Kind.
    pub kind: ItemKind,
    /// Tool title; empty for messages.
    pub title: String,
    /// Message text, or tool output text.
    pub text: String,
    /// Inline images as data URLs.
    #[serde(default)]
    pub images: Vec<String>,
    /// Tool state, for tool calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<Tool>,
    /// Plan entries, for plans.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<Vec<PlanEntry>>,
    /// The agent's message id, when supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Some of this item's content was dropped to stay within the bounds.
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
    /// The text reached [`MAX_ITEM_TEXT`] (it ends with a marker); later
    /// text for this item is dropped.
    #[serde(default, skip_serializing_if = "is_false")]
    pub text_full: bool,
    /// What else was dropped (`images`, `diffs`, …), for a visible notice.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omitted: Vec<String>,
}

impl Item {
    /// Record that content of `reason` was dropped. `true` when new.
    fn omit(&mut self, reason: &str) -> bool {
        self.truncated = true;
        if self.omitted.iter().any(|known| known == reason) {
            return false;
        }
        self.omitted.push(reason.to_string());
        true
    }

    /// Set whether `reason` applies, for collections an update replaces.
    fn omit_if(&mut self, reason: &str, dropped: bool) {
        if dropped {
            self.omit(reason);
        } else {
            self.omitted.retain(|known| known != reason);
        }
    }
}

/// One turn: a user prompt and everything the agent produced for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    /// Host turn id (live turns) or a replay id (loaded history).
    pub id: String,
    /// Items, in order.
    pub items: Vec<Item>,
    /// How the turn ended; `None` while running or unknown (replay).
    pub outcome: Option<TurnOutcome>,
    /// Whether the turn came from `session/load` replay.
    pub replayed: bool,
    /// Next item ordinal. Ordinals are never reused, so ids stay stable
    /// when earlier items of the turn are evicted.
    #[serde(default)]
    pub next_item: u64,
    /// Items evicted from inside this turn (shown as a gap).
    #[serde(default)]
    pub omitted: u64,
}

impl Turn {
    fn new(id: String, replayed: bool) -> Self {
        Self {
            id,
            items: Vec::new(),
            outcome: None,
            replayed,
            next_item: 0,
            omitted: 0,
        }
    }

    fn next_ordinal(&mut self) -> u64 {
        let ordinal = self.next_item.max(self.items.len() as u64);
        self.next_item = ordinal + 1;
        ordinal
    }
}

/// What changed in the transcript, for live event translation.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// A new item appeared.
    Started(Item),
    /// Text was appended to a message item.
    Delta {
        /// Item id.
        item_id: String,
        /// Item kind.
        kind: ItemKind,
        /// Appended text.
        delta: String,
    },
    /// An item was replaced (tool calls, plans, images).
    Updated(Item),
}

/// Session-level state carried by an update rather than transcript content.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionSignal {
    /// Full replacement of the select configuration options.
    ConfigOptions(Vec<Value>),
    /// The legacy current mode changed.
    Mode(String),
    /// Title metadata: `Some(None)` clears it, `None` leaves it unchanged.
    Title(Option<Option<String>>),
    /// Context window usage.
    Usage {
        /// Tokens in context.
        used: u64,
        /// Context window size.
        size: u64,
    },
}

/// Recognize the session-level update kinds.
pub fn session_signal(update: &Value) -> Option<SessionSignal> {
    match update["sessionUpdate"].as_str()? {
        "config_option_update" => {
            Some(SessionSignal::ConfigOptions(select_options(&update["configOptions"])))
        },
        "current_mode_update" => update["currentModeId"]
            .as_str()
            .map(|mode| SessionSignal::Mode(mode.to_string())),
        "session_info_update" => {
            let title = match update.get("title") {
                None => None,
                Some(Value::Null) => Some(None),
                Some(Value::String(title)) => Some(Some(title.chars().take(512).collect())),
                Some(_) => return None,
            };
            Some(SessionSignal::Title(title))
        },
        "usage_update" => Some(SessionSignal::Usage {
            used: update["used"].as_u64()?,
            size: update["size"].as_u64()?,
        }),
        _ => None,
    }
}

/// A bounded, folded conversation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    /// Retained turns, oldest first.
    pub turns: VecDeque<Turn>,
    /// Whether older history was evicted (or never received).
    pub truncated: bool,
    #[serde(skip)]
    bytes: usize,
    #[serde(skip)]
    items: usize,
    #[serde(skip)]
    replays: u64,
}

fn serialized_len(value: &Value) -> usize {
    serde_json::to_vec(value).map_or(0, |bytes| bytes.len())
}

/// Every retained byte of `item`, plus a fixed overhead.
fn weight(item: &Item) -> usize {
    let tool = item.tool.as_ref().map_or(0, |tool| {
        tool.call_id.len()
            + tool.kind.len()
            + tool.status.len()
            + tool.name.as_ref().map_or(0, String::len)
            + tool.locations.iter().map(String::len).sum::<usize>()
            + tool
                .diffs
                .iter()
                .map(|d| {
                    d.path.len() + d.new_text.len() + d.old_text.as_ref().map_or(0, String::len)
                })
                .sum::<usize>()
            + tool.raw_input.as_ref().map_or(0, serialized_len)
    });
    let plan = item.plan.as_ref().map_or(0, |plan| {
        plan.iter()
            .map(|e| e.content.len() + e.status.len())
            .sum::<usize>()
    });
    ITEM_OVERHEAD
        + item.id.len()
        + item.title.len()
        + item.text.len()
        + item.message_id.as_ref().map_or(0, String::len)
        + item.images.iter().map(String::len).sum::<usize>()
        + item.omitted.iter().map(String::len).sum::<usize>()
        + tool
        + plan
}

/// Cut `text` to `limit` bytes plus a visible marker. `true` when cut.
fn clip(text: &mut String, limit: usize) -> bool {
    if text.len() <= limit {
        return false;
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str(TRUNCATED);
    true
}

/// Keep `image` when the item's image budget allows. `false` when dropped.
fn push_image(images: &mut Vec<String>, image: String) -> bool {
    let used: usize = images.iter().map(String::len).sum();
    if images.len() >= MAX_ITEM_IMAGES || used + image.len() > MAX_ITEM_IMAGE_BYTES {
        return false;
    }
    images.push(image);
    true
}

/// Text and image carried by one content block.
fn block_content(block: &Value) -> (String, Option<String>) {
    match block["type"].as_str() {
        Some("text") => (block["text"].as_str().unwrap_or("").to_string(), None),
        Some("image") => {
            let mime = block["mimeType"].as_str().unwrap_or("");
            let data = block["data"].as_str().unwrap_or("");
            let image = (mime.starts_with("image/") && !data.is_empty())
                .then(|| format!("data:{mime};base64,{data}"));
            (String::new(), image)
        },
        Some("resource_link") => {
            let name = block["title"]
                .as_str()
                .or_else(|| block["name"].as_str())
                .unwrap_or("");
            (format!("[{name}]({})", block["uri"].as_str().unwrap_or("")), None)
        },
        Some("resource") => {
            let resource = &block["resource"];
            match resource["text"].as_str() {
                Some(text) => (text.to_string(), None),
                None => (format!("[{}]", resource["uri"].as_str().unwrap_or("resource")), None),
            }
        },
        Some("audio") => ("[audio]".to_string(), None),
        _ => (String::new(), None),
    }
}

fn message_kind(update: &str) -> Option<ItemKind> {
    match update {
        "user_message_chunk" => Some(ItemKind::UserMessage),
        "agent_message_chunk" => Some(ItemKind::AgentMessage),
        "agent_thought_chunk" => Some(ItemKind::Thought),
        _ => None,
    }
}

fn kind_tag(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::UserMessage => "u",
        ItemKind::AgentMessage => "m",
        ItemKind::Thought => "r",
        ItemKind::ToolCall => "t",
        ItemKind::Plan => "p",
    }
}

fn new_item(id: String, kind: ItemKind) -> Item {
    Item {
        id,
        kind,
        title: String::new(),
        text: String::new(),
        images: Vec::new(),
        tool: None,
        plan: None,
        message_id: None,
        truncated: false,
        text_full: false,
        omitted: Vec::new(),
    }
}

/// Apply supplied tool fields; collections are replaced, never appended, and
/// so are the omissions they report.
fn apply_tool_fields(item: &mut Item, update: &Value) {
    let tool = item.tool.get_or_insert_with(Tool::default);
    if let Some(title) = update["title"].as_str() {
        item.title = title.chars().take(1024).collect();
    }
    if let Some(kind) = update["kind"].as_str() {
        tool.kind = kind.chars().take(64).collect();
    }
    if let Some(status) = update["status"].as_str() {
        tool.status = status.chars().take(64).collect();
    }
    if let Some(name) = update["name"].as_str() {
        tool.name = Some(name.chars().take(256).collect());
    }
    let mut input_dropped = None;
    if let Some(raw) = update.get("rawInput").filter(|raw| !raw.is_null()) {
        let size = serialized_len(raw);
        input_dropped = Some(size > MAX_RAW_INPUT);
        tool.raw_input = Some(if size > MAX_RAW_INPUT {
            Value::String(format!("[input omitted: {size} bytes]"))
        } else {
            raw.clone()
        });
    }
    let mut locations_dropped = None;
    if let Some(locations) = update["locations"].as_array() {
        let mut dropped = locations.len() > MAX_LOCATIONS;
        tool.locations = locations
            .iter()
            .filter_map(|location| location["path"].as_str())
            .take(MAX_LOCATIONS)
            .map(|path| {
                let mut path = path.to_string();
                dropped |= clip(&mut path, MAX_LOCATION);
                path
            })
            .collect();
        locations_dropped = Some(dropped);
    }
    let mut content_dropped = None;
    if let Some(content) = update["content"].as_array() {
        let mut text = String::new();
        let mut images = Vec::new();
        let mut diff_budget = MAX_ITEM_TEXT;
        let (mut images_dropped, mut diffs_dropped) = (false, false);
        tool.diffs.clear();
        tool.terminals = 0;
        for entry in content.iter().take(256) {
            match entry["type"].as_str() {
                Some("content") => {
                    let (piece, image) = block_content(&entry["content"]);
                    if !piece.is_empty() {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(&piece);
                    }
                    if let Some(image) = image {
                        images_dropped |= !push_image(&mut images, image);
                    }
                },
                Some("diff") => {
                    let (Some(path), Some(new_text)) =
                        (entry["path"].as_str(), entry["newText"].as_str())
                    else {
                        continue;
                    };
                    let old_text = entry["oldText"].as_str();
                    let size = path.len() + new_text.len() + old_text.map_or(0, str::len);
                    if tool.diffs.len() >= MAX_DIFFS || size > diff_budget {
                        diffs_dropped = true;
                        continue;
                    }
                    diff_budget -= size;
                    tool.diffs.push(Diff {
                        path: path.to_string(),
                        old_text: old_text.map(str::to_string),
                        new_text: new_text.to_string(),
                    });
                },
                Some("terminal") => tool.terminals += 1,
                _ => {},
            }
        }
        item.text_full = clip(&mut text, MAX_ITEM_TEXT);
        item.text = text;
        item.images = images;
        content_dropped = Some((content.len() > 256, images_dropped, diffs_dropped));
    }
    if let Some(dropped) = input_dropped {
        item.omit_if(OMITTED_INPUT, dropped);
    }
    if let Some(dropped) = locations_dropped {
        item.omit_if(OMITTED_LOCATIONS, dropped);
    }
    if let Some((entries, images, diffs)) = content_dropped {
        item.omit_if(OMITTED_CONTENT, entries);
        item.omit_if(OMITTED_IMAGES, images);
        item.omit_if(OMITTED_DIFFS, diffs);
    }
    item.truncated = item.text_full || !item.omitted.is_empty();
}

impl Transcript {
    /// An empty transcript; `missing_history` marks that earlier history
    /// exists but is not available here.
    pub fn new(missing_history: bool) -> Self {
        Self {
            truncated: missing_history,
            ..Self::default()
        }
    }

    /// A transcript holding `turns` as read from a host (controller-side
    /// recovery). Ids and ordinals continue exactly where the host's did.
    pub fn from_turns(turns: impl IntoIterator<Item = Turn>, truncated: bool) -> Self {
        let mut transcript = Self {
            turns: turns.into_iter().collect(),
            truncated,
            ..Self::default()
        };
        transcript.replays = transcript
            .turns
            .iter()
            .filter_map(|turn| turn.id.strip_prefix("replay:")?.parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        transcript.recount();
        transcript.enforce_bounds();
        transcript
    }

    /// Number of retained items.
    pub fn len(&self) -> usize {
        self.items
    }

    /// Whether nothing is retained.
    pub fn is_empty(&self) -> bool {
        self.items == 0
    }

    /// Retained bytes as charged against [`MAX_TRANSCRIPT_BYTES`].
    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }

    /// The retained turn `id`.
    pub fn turn(&self, id: &str) -> Option<&Turn> {
        self.turns.iter().rev().find(|turn| turn.id == id)
    }

    /// Recompute the bounds bookkeeping after deserialization.
    pub fn recount(&mut self) {
        self.items = self.turns.iter().map(|t| t.items.len()).sum();
        self.bytes = self.turns.iter().flat_map(|t| &t.items).map(weight).sum();
    }

    /// Open a live turn for a prompt the host sent.
    pub fn begin_turn(&mut self, id: &str, user_text: &str, images: &[String]) -> Vec<Change> {
        let mut user = new_item(format!("{id}:u"), ItemKind::UserMessage);
        user.text = user_text.to_string();
        user.text_full = clip(&mut user.text, MAX_ITEM_TEXT);
        user.truncated = user.text_full;
        for image in images {
            if !push_image(&mut user.images, image.clone()) {
                user.omit(OMITTED_IMAGES);
            }
        }
        self.bytes += weight(&user);
        self.items += 1;
        let mut turn = Turn::new(id.to_string(), false);
        turn.next_item = 1;
        turn.items.push(user.clone());
        self.turns.push_back(turn);
        self.enforce_bounds();
        vec![Change::Started(user)]
    }

    /// Record how a live turn ended.
    pub fn end_turn(&mut self, id: &str, outcome: TurnOutcome) {
        if let Some(turn) = self.turns.iter_mut().rev().find(|turn| turn.id == id) {
            turn.outcome = Some(outcome);
        }
    }

    /// The turn an update belongs to: the live `turn` when given, otherwise
    /// a replay turn (a replayed user message opens a new one).
    fn target_turn(&mut self, turn: Option<&str>, opens: bool) -> usize {
        if let Some(id) = turn {
            if let Some(index) = self.turns.iter().rposition(|t| t.id == id) {
                return index;
            }
            self.turns.push_back(Turn::new(id.to_string(), false));
            return self.turns.len() - 1;
        }
        let reuse = self.turns.back().is_some_and(|last| {
            last.replayed
                && !(opens
                    && last
                        .items
                        .iter()
                        .any(|item| item.kind != ItemKind::UserMessage))
        });
        if !reuse {
            self.replays += 1;
            self.turns
                .push_back(Turn::new(format!("replay:{}", self.replays), true));
        }
        self.turns.len() - 1
    }

    /// Fold one `session/update` payload. `turn` is the live host turn the
    /// update arrived during, or `None` for replay / out-of-turn updates.
    /// Session-level kinds (see [`session_signal`]) and unknown kinds change
    /// nothing here.
    pub fn apply(&mut self, turn: Option<&str>, update: &Value) -> Vec<Change> {
        let Some(name) = update["sessionUpdate"].as_str() else {
            return Vec::new();
        };
        let changes = if let Some(kind) = message_kind(name) {
            // A live turn already holds the prompt the host sent; an echoed
            // user chunk would duplicate it.
            if kind == ItemKind::UserMessage && turn.is_some() {
                return Vec::new();
            }
            self.chunk(turn, kind, update)
        } else {
            match name {
                "tool_call" | "tool_call_update" => self.tool(turn, update),
                "plan" => self.plan(turn, update),
                _ => return Vec::new(),
            }
        };
        self.enforce_bounds();
        changes
    }

    fn chunk(&mut self, turn: Option<&str>, kind: ItemKind, update: &Value) -> Vec<Change> {
        let (text, image) = block_content(&update["content"]);
        if text.is_empty() && image.is_none() {
            return Vec::new();
        }
        // Compared and stored under the same key, so one long id is one
        // message and two ids never merge.
        let message_id = update["messageId"].as_str().map(protocol_key);
        let index = self.target_turn(turn, kind == ItemKind::UserMessage);
        let turn = &mut self.turns[index];
        let continues = turn
            .items
            .last()
            .is_some_and(|last| last.kind == kind && last.message_id == message_id);
        if continues {
            let Some(last) = turn.items.last_mut() else { return Vec::new() };
            let before = weight(last);
            let mut changes = Vec::new();
            // Only the text budget stops text; a dropped image does not.
            if !text.is_empty() && !last.text_full {
                let room = MAX_ITEM_TEXT.saturating_sub(last.text.len());
                let mut delta = text;
                if delta.len() > room {
                    // Streaming crossed the item limit: keep what fits and
                    // say so, both in the text and on the item.
                    let mut end = room;
                    while !delta.is_char_boundary(end) {
                        end -= 1;
                    }
                    delta.truncate(end);
                    delta.push_str(TRUNCATED);
                    last.text_full = true;
                    last.truncated = true;
                }
                last.text.push_str(&delta);
                changes.push(Change::Delta {
                    item_id: last.id.clone(),
                    kind,
                    delta,
                });
            }
            if let Some(image) = image {
                if push_image(&mut last.images, image) || last.omit(OMITTED_IMAGES) {
                    changes.push(Change::Updated(last.clone()));
                }
            }
            self.bytes = self.bytes.saturating_sub(before) + weight(last);
            return changes;
        }
        let ordinal = turn.next_ordinal();
        let mut item = new_item(format!("{}:{}{ordinal}", turn.id, kind_tag(kind)), kind);
        item.text = text;
        item.text_full = clip(&mut item.text, MAX_ITEM_TEXT);
        item.truncated = item.text_full;
        if let Some(image) = image {
            if !push_image(&mut item.images, image) {
                item.omit(OMITTED_IMAGES);
            }
        }
        item.message_id = message_id;
        self.bytes += weight(&item);
        self.items += 1;
        turn.items.push(item.clone());
        vec![Change::Started(item)]
    }

    fn find_tool(&mut self, call_id: &str) -> Option<&mut Item> {
        self.turns
            .iter_mut()
            .rev()
            .flat_map(|turn| turn.items.iter_mut().rev())
            .find(|item| {
                item.kind == ItemKind::ToolCall
                    && item
                        .tool
                        .as_ref()
                        .is_some_and(|tool| tool.call_id == call_id)
            })
    }

    fn tool(&mut self, turn: Option<&str>, update: &Value) -> Vec<Change> {
        let Some(call_id) = update["toolCallId"].as_str() else {
            return Vec::new();
        };
        // Compared exactly (see the module docs): no prefix aliasing.
        let call_id = protocol_key(call_id);
        if let Some(existing) = self.find_tool(&call_id) {
            let before = weight(existing);
            apply_tool_fields(existing, update);
            let after = weight(existing);
            let snapshot = existing.clone();
            self.bytes = self.bytes.saturating_sub(before) + after;
            return vec![Change::Updated(snapshot)];
        }
        let index = self.target_turn(turn, false);
        let mut item = new_item(format!("tool:{call_id}"), ItemKind::ToolCall);
        item.tool = Some(Tool {
            call_id,
            kind: "other".into(),
            status: "pending".into(),
            ..Tool::default()
        });
        apply_tool_fields(&mut item, update);
        // Tool ids are the agent's; the ordinal still advances so message
        // ids that follow never collide with evicted ones.
        self.turns[index].next_ordinal();
        self.bytes += weight(&item);
        self.items += 1;
        self.turns[index].items.push(item.clone());
        vec![Change::Started(item)]
    }

    fn plan(&mut self, turn: Option<&str>, update: &Value) -> Vec<Change> {
        let Some(entries) = update["entries"].as_array() else { return Vec::new() };
        let plan: Vec<PlanEntry> = entries
            .iter()
            .take(256)
            .filter_map(|entry| {
                Some(PlanEntry {
                    content: entry["content"].as_str()?.chars().take(2048).collect(),
                    status: entry["status"]
                        .as_str()
                        .unwrap_or("pending")
                        .chars()
                        .take(32)
                        .collect(),
                })
            })
            .collect();
        let index = self.target_turn(turn, false);
        let turn = &mut self.turns[index];
        let id = format!("{}:plan", turn.id);
        if let Some(existing) = turn.items.iter_mut().find(|item| item.id == id) {
            let before = weight(existing);
            existing.plan = Some(plan);
            existing.omit_if(OMITTED_PLAN, entries.len() > 256);
            existing.truncated = !existing.omitted.is_empty();
            let snapshot = existing.clone();
            self.bytes = self.bytes.saturating_sub(before) + weight(&snapshot);
            return vec![Change::Updated(snapshot)];
        }
        let mut item = new_item(id, ItemKind::Plan);
        item.plan = Some(plan);
        item.omit_if(OMITTED_PLAN, entries.len() > 256);
        self.bytes += weight(&item);
        self.items += 1;
        turn.items.push(item.clone());
        vec![Change::Started(item)]
    }

    fn over_bounds(&self) -> bool {
        self.bytes > MAX_TRANSCRIPT_BYTES || self.items > MAX_ITEMS || self.turns.len() > MAX_TURNS
    }

    /// Evict until the bounds hold: whole oldest turns first, then the
    /// oldest items of the one remaining turn, keeping its prompt and its
    /// newest item (whose own size is bounded per item).
    fn enforce_bounds(&mut self) {
        while self.over_bounds() {
            if self.turns.len() > 1 {
                if let Some(turn) = self.turns.pop_front() {
                    self.items = self.items.saturating_sub(turn.items.len());
                    self.bytes = self
                        .bytes
                        .saturating_sub(turn.items.iter().map(weight).sum::<usize>());
                    self.truncated = true;
                }
                continue;
            }
            let Some(turn) = self.turns.front_mut() else { return };
            let last = turn.items.len().saturating_sub(1);
            let victim = (0..last)
                .find(|&index| !(index == 0 && turn.items[0].kind == ItemKind::UserMessage));
            let Some(victim) = victim else { return };
            let removed = turn.items.remove(victim);
            turn.omitted += 1;
            self.items = self.items.saturating_sub(1);
            self.bytes = self.bytes.saturating_sub(weight(&removed));
            self.truncated = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use serde_json::json;

    use super::*;

    fn chunk(kind: &str, text: &str) -> Value {
        json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}})
    }

    fn exact_bytes(transcript: &Transcript) -> usize {
        transcript
            .turns
            .iter()
            .flat_map(|t| &t.items)
            .map(weight)
            .sum()
    }

    #[test]
    fn live_chunks_append_and_echoed_prompts_are_ignored() {
        let mut transcript = Transcript::default();
        transcript.begin_turn("t1", "hello", &[]);
        assert!(transcript
            .apply(Some("t1"), &chunk("user_message_chunk", "hello"))
            .is_empty());
        let first = transcript.apply(Some("t1"), &chunk("agent_message_chunk", "Hi"));
        assert!(
            matches!(&first[0], Change::Started(item) if item.text == "Hi" && item.id == "t1:m1")
        );
        let second = transcript.apply(Some("t1"), &chunk("agent_message_chunk", " there"));
        assert!(matches!(&second[0], Change::Delta { delta, .. } if delta == " there"));
        let thought = transcript.apply(Some("t1"), &chunk("agent_thought_chunk", "hmm"));
        assert!(matches!(&thought[0], Change::Started(item) if item.kind == ItemKind::Thought));
        let turn = &transcript.turns[0];
        assert_eq!(turn.items.len(), 3);
        assert_eq!(turn.items[1].text, "Hi there");
    }

    #[test]
    fn message_ids_split_messages() {
        let mut transcript = Transcript::default();
        let mut a = chunk("agent_message_chunk", "a");
        a["messageId"] = json!("m1");
        let mut b = chunk("agent_message_chunk", "b");
        b["messageId"] = json!("m2");
        transcript.apply(Some("t"), &a);
        transcript.apply(Some("t"), &b);
        assert_eq!(transcript.turns[0].items.len(), 2);
    }

    #[test]
    fn tool_updates_replace_supplied_collections() {
        let mut transcript = Transcript::default();
        transcript.apply(
            Some("t"),
            &json!({
                "sessionUpdate": "tool_call", "toolCallId": "c1", "title": "Edit a", "kind": "edit",
                "status": "pending",
                "content": [{"type": "content", "content": {"type": "text", "text": "one"}}],
                "locations": [{"path": "/w/a"}],
            }),
        );
        let changes = transcript.apply(
            Some("t"),
            &json!({
                "sessionUpdate": "tool_call_update", "toolCallId": "c1", "status": "completed",
                "content": [{"type": "diff", "path": "/w/a", "oldText": "x", "newText": "y"}],
            }),
        );
        let Change::Updated(item) = &changes[0] else { panic!("update expected") };
        let tool = item.tool.as_ref().expect("tool");
        assert_eq!(tool.status, "completed");
        assert_eq!(item.text, "", "replaced content drops the old text");
        assert_eq!(tool.diffs.len(), 1);
        assert_eq!(tool.locations, vec!["/w/a".to_string()], "absent locations stay");
        assert_eq!(item.title, "Edit a");
    }

    #[test]
    fn replay_groups_by_user_message_and_plans_replace() {
        let mut transcript = Transcript::default();
        transcript.apply(None, &chunk("user_message_chunk", "q1"));
        transcript.apply(None, &chunk("agent_message_chunk", "a1"));
        transcript.apply(
            None,
            &json!({"sessionUpdate": "plan", "entries": [{"content": "x", "status": "pending"}]}),
        );
        transcript.apply(
            None,
            &json!({"sessionUpdate": "plan", "entries": [{"content": "x", "status": "completed"}]}),
        );
        transcript.apply(None, &chunk("user_message_chunk", "q2"));
        transcript.apply(None, &chunk("agent_message_chunk", "a2"));
        assert_eq!(transcript.turns.len(), 2);
        assert!(transcript
            .turns
            .iter()
            .all(|turn| turn.replayed && turn.outcome.is_none()));
        let plan = transcript.turns[0]
            .items
            .iter()
            .find(|i| i.kind == ItemKind::Plan)
            .expect("plan");
        assert_eq!(plan.plan.as_ref().expect("entries")[0].status, "completed");
        assert_eq!(transcript.len(), 5);
    }

    #[test]
    fn bounds_evict_whole_turns_and_mark_truncation() {
        let mut transcript = Transcript::default();
        let big = "x".repeat(MAX_ITEM_TEXT);
        for turn in 0..40 {
            let id = format!("t{turn}");
            transcript.begin_turn(&id, "q", &[]);
            transcript.apply(Some(&id), &chunk("agent_message_chunk", &big));
        }
        assert!(transcript.truncated);
        assert!(transcript.bytes <= MAX_TRANSCRIPT_BYTES);
        assert_eq!(transcript.turns.back().expect("newest").id, "t39");
        let mut copy = transcript.clone();
        copy.recount();
        assert_eq!(copy.bytes, transcript.bytes);
    }

    #[test]
    fn one_long_turn_of_tool_calls_with_large_input_stays_bounded() {
        let mut transcript = Transcript::default();
        transcript.begin_turn("t", "start", &[]);
        let input = json!({"blob": "i".repeat(16 * 1024)});
        let huge = json!({"blob": "h".repeat(MAX_RAW_INPUT * 2)});
        for n in 0..6000 {
            let raw = if n % 100 == 0 { &huge } else { &input };
            transcript.apply(
                Some("t"),
                &json!({"sessionUpdate": "tool_call", "toolCallId": format!("call-{n}"),
                    "title": "Run", "kind": "execute", "status": "completed", "rawInput": raw}),
            );
        }
        assert_eq!(transcript.turns.len(), 1, "a single turn");
        assert!(transcript.len() <= MAX_ITEMS);
        assert!(transcript.retained_bytes() <= MAX_TRANSCRIPT_BYTES);
        assert_eq!(transcript.retained_bytes(), exact_bytes(&transcript), "every byte is charged");
        let turn = &transcript.turns[0];
        assert!(turn.omitted > 0, "evicted items are counted");
        assert!(transcript.truncated, "and the transcript says it is incomplete");
        assert_eq!(turn.items[0].id, "t:u", "the prompt stays");
        assert_eq!(turn.items.last().expect("newest").id, "tool:call-5999");
        let huge_kept = turn.items.iter().filter(|item| item.truncated).all(|item| {
            item.tool
                .as_ref()
                .is_some_and(|t| t.raw_input.as_ref().is_some_and(Value::is_string))
        });
        assert!(huge_kept, "oversized input is replaced by a note");
    }

    #[test]
    fn ids_are_never_reused_after_in_turn_eviction() {
        let mut transcript = Transcript::default();
        transcript.begin_turn("t", "q", &[]);
        let text = "y".repeat(MAX_ITEM_TEXT / 2);
        let mut ids = HashSet::new();
        for n in 0..200 {
            let mut update = chunk("agent_message_chunk", &text);
            update["messageId"] = json!(format!("msg-{n}"));
            for change in transcript.apply(Some("t"), &update) {
                if let Change::Started(item) = change {
                    assert!(ids.insert(item.id.clone()), "id {} reused", item.id);
                }
            }
        }
        assert!(transcript.turns[0].omitted > 0);
        assert!(transcript.retained_bytes() <= MAX_TRANSCRIPT_BYTES);
    }

    #[test]
    fn repeated_images_on_one_message_are_capped_and_flagged() {
        let mut transcript = Transcript::default();
        transcript.begin_turn("t", "q", &[]);
        let data = "A".repeat(512 * 1024);
        let image = json!({"sessionUpdate": "agent_message_chunk",
            "content": {"type": "image", "mimeType": "image/png", "data": data}});
        for _ in 0..50 {
            transcript.apply(Some("t"), &image);
        }
        let item = &transcript.turns[0].items[1];
        assert!(item.images.len() <= MAX_ITEM_IMAGES);
        assert!(item.images.iter().map(String::len).sum::<usize>() <= MAX_ITEM_IMAGE_BYTES);
        assert!(item.truncated);
        assert_eq!(transcript.retained_bytes(), exact_bytes(&transcript));
    }

    #[test]
    fn streaming_past_the_item_limit_marks_the_truncation() {
        let mut transcript = Transcript::default();
        transcript.begin_turn("t", "q", &[]);
        let piece = "z".repeat(100 * 1024);
        let mut deltas = Vec::new();
        for _ in 0..8 {
            for change in transcript.apply(Some("t"), &chunk("agent_message_chunk", &piece)) {
                if let Change::Delta {
                    delta, ..
                } = change
                {
                    deltas.push(delta);
                }
            }
        }
        let item = &transcript.turns[0].items[1];
        assert!(item.truncated, "the item says it was cut");
        assert!(item.text.ends_with(TRUNCATED), "and the text shows it");
        assert!(item.text.len() <= MAX_ITEM_TEXT + TRUNCATED.len());
        assert!(
            deltas.last().is_some_and(|d| d.ends_with(TRUNCATED)),
            "the controller sees the marker"
        );
        // Nothing more is appended once cut.
        assert!(transcript
            .apply(Some("t"), &chunk("agent_message_chunk", "more"))
            .is_empty());
    }

    #[test]
    fn long_protocol_ids_never_alias_and_one_long_message_stays_one_item() {
        let shared = "p".repeat(MAX_PROTOCOL_ID);
        let (first, second) = (format!("{shared}-one"), format!("{shared}-two"));
        let mut transcript = Transcript::default();
        transcript.begin_turn("t", "q", &[]);
        for id in [&first, &second] {
            transcript.apply(
                Some("t"),
                &json!({"sessionUpdate": "tool_call", "toolCallId": id, "title": id}),
            );
        }
        let tools: Vec<&Item> = transcript.turns[0]
            .items
            .iter()
            .filter(|i| i.kind == ItemKind::ToolCall)
            .collect();
        assert_eq!(tools.len(), 2, "a shared prefix is not one tool");
        assert_ne!(tools[0].id, tools[1].id);
        // An update for the second reaches the second only.
        transcript.apply(
            Some("t"),
            &json!({"sessionUpdate": "tool_call_update", "toolCallId": second, "status": "completed"}),
        );
        let status = |n: usize| {
            transcript.turns[0]
                .items
                .iter()
                .filter(|i| i.kind == ItemKind::ToolCall)
                .nth(n)
                .and_then(|i| i.tool.as_ref())
                .map(|t| t.status.clone())
        };
        assert_eq!(status(0).as_deref(), Some("pending"));
        assert_eq!(status(1).as_deref(), Some("completed"));
        // A verbatim id can never equal an oversized id's key.
        assert!(protocol_key(&first).len() > MAX_PROTOCOL_ID);
        assert_eq!(protocol_key("short"), "short");

        let long_message = "m".repeat(MAX_PROTOCOL_ID * 4);
        for piece in ["a", "b", "c"] {
            let mut update = chunk("agent_message_chunk", piece);
            update["messageId"] = json!(long_message);
            transcript.apply(Some("t"), &update);
        }
        let messages: Vec<&Item> = transcript.turns[0]
            .items
            .iter()
            .filter(|i| i.kind == ItemKind::AgentMessage)
            .collect();
        assert_eq!(messages.len(), 1, "chunks of one long id are one message");
        assert_eq!(messages[0].text, "abc");
    }

    #[test]
    fn a_dropped_image_or_diff_is_named_and_later_text_still_arrives() {
        let mut transcript = Transcript::default();
        transcript.begin_turn("t", "q", &[]);
        let oversized = "A".repeat(MAX_ITEM_IMAGE_BYTES + 1);
        let image = json!({"sessionUpdate": "agent_message_chunk",
            "content": {"type": "image", "mimeType": "image/png", "data": oversized}});
        transcript.apply(Some("t"), &image);
        let text = transcript.apply(Some("t"), &chunk("agent_message_chunk", "the caption"));
        assert!(
            matches!(&text[0], Change::Delta { delta, .. } if delta == "the caption"),
            "text after a dropped image is kept"
        );
        let message = &transcript.turns[0].items[1];
        assert_eq!(message.text, "the caption");
        assert!(message.images.is_empty());
        assert_eq!(message.omitted, vec![OMITTED_IMAGES.to_string()]);
        assert!(message.truncated && !message.text_full);

        let big = "d".repeat(MAX_ITEM_TEXT);
        transcript.apply(
            Some("t"),
            &json!({"sessionUpdate": "tool_call", "toolCallId": "edit", "kind": "edit",
            "content": [
                {"type": "diff", "path": "/w/a", "newText": "small"},
                {"type": "diff", "path": "/w/b", "newText": big},
            ]}),
        );
        let tool = transcript.turns[0]
            .items
            .iter()
            .find(|i| i.kind == ItemKind::ToolCall)
            .expect("tool");
        assert_eq!(tool.tool.as_ref().map(|t| t.diffs.len()), Some(1));
        assert_eq!(tool.omitted, vec![OMITTED_DIFFS.to_string()]);
        // A later update that fits replaces the content and the notice.
        transcript.apply(
            Some("t"),
            &json!({"sessionUpdate": "tool_call_update", "toolCallId": "edit",
                "content": [{"type": "diff", "path": "/w/a", "newText": "fits"}]}),
        );
        let tool = transcript.turns[0]
            .items
            .iter()
            .find(|i| i.kind == ItemKind::ToolCall)
            .expect("tool");
        assert!(tool.omitted.is_empty() && !tool.truncated);
        assert_eq!(transcript.retained_bytes(), exact_bytes(&transcript));
    }

    #[test]
    fn a_recovered_turn_continues_with_the_hosts_ids() {
        let mut host = Transcript::default();
        host.begin_turn("t", "q", &[]);
        host.apply(Some("t"), &chunk("agent_message_chunk", "He"));
        let mut replica = Transcript::from_turns([host.turns[0].clone()], false);
        for update in [
            chunk("agent_message_chunk", "llo"),
            json!({"sessionUpdate": "tool_call", "toolCallId": "c", "title": "ls"}),
            chunk("agent_message_chunk", "done"),
        ] {
            host.apply(Some("t"), &update);
            replica.apply(Some("t"), &update);
        }
        assert_eq!(replica.turns[0], host.turns[0]);
        assert_eq!(replica.retained_bytes(), host.retained_bytes());
    }

    #[test]
    fn session_level_updates_are_signals_not_items() {
        let mut transcript = Transcript::default();
        for update in [
            json!({"sessionUpdate": "config_option_update", "configOptions": []}),
            json!({"sessionUpdate": "current_mode_update", "currentModeId": "code"}),
            json!({"sessionUpdate": "usage_update", "used": 1, "size": 2}),
            json!({"sessionUpdate": "session_info_update", "title": null}),
            json!({"sessionUpdate": "future_kind"}),
        ] {
            assert!(transcript.apply(Some("t"), &update).is_empty());
        }
        assert!(transcript.is_empty());
        assert_eq!(
            session_signal(&json!({"sessionUpdate": "session_info_update", "title": null})),
            Some(SessionSignal::Title(Some(None)))
        );
        assert_eq!(
            session_signal(&json!({"sessionUpdate": "session_info_update", "updatedAt": "x"})),
            Some(SessionSignal::Title(None))
        );
        assert_eq!(
            session_signal(&json!({"sessionUpdate": "usage_update", "used": 3, "size": 9})),
            Some(SessionSignal::Usage {
                used: 3,
                size: 9
            })
        );
    }
}
