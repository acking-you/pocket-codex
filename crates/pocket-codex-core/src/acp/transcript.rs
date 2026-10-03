//! Hub-side folding of ACP `session/update`s into a bounded transcript.
//!
//! The hub is the only place that folds (T1): every forwarded update carries
//! the authoritative item id, so a controller holding only a tail window stays
//! consistent by merging on id.
//!
//! ```text
//!   turn 0: items before the first user message
//!   turn n: user message n and everything after it, until user message n+1
//!
//!   ids   m:{kind}:{messageId}   chunk with a messageId
//!         s:{turn}:{ordinal}     chunk without a messageId
//!         tc:{toolCallId}        tool call
//!         p:{turn}               plan
//!         n:{turn}:{ordinal}     hub notice
//! ```

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    types::{ContentBlock, PlanEntry, TextContent, ToolCall, ToolCallUpdate},
    update::{MessageChunk, SessionUpdate},
};

/// Maximum number of items kept.
pub const MAX_ITEMS: usize = 4_000;
/// Maximum approximate size of all items.
pub const MAX_BYTES: usize = 48 * 1024 * 1024;
/// Maximum text per item; later text is dropped and `truncated` set.
pub const MAX_ITEM_TEXT: usize = 1024 * 1024;
/// Maximum serialized size of `rawInput` / `rawOutput`.
pub const MAX_RAW_JSON: usize = 256 * 1024;
/// Length of turn previews, in characters.
const PREVIEW_CHARS: usize = 200;

/// Item kinds.
pub mod kind {
    /// User message.
    pub const USER: &str = "user";
    /// Agent message.
    pub const AGENT: &str = "agent";
    /// Agent thought.
    pub const THOUGHT: &str = "thought";
    /// Tool call.
    pub const TOOL: &str = "tool";
    /// Plan.
    pub const PLAN: &str = "plan";
    /// Hub notice.
    pub const NOTICE: &str = "notice";
}

/// One folded transcript item.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubItem {
    /// Stable item id (see the module docs).
    pub id: String,
    /// Turn number.
    pub turn: u32,
    /// `user` | `agent` | `thought` | `tool` | `plan` | `notice`
    pub kind: String,
    /// Message content.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<ContentBlock>,
    /// Tool call state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<ToolCall>,
    /// Plan entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<Vec<PlanEntry>>,
    /// ACP `messageId` of the chunks folded into this item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// When the hub first saw this item live; `None` for replays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at_ms: Option<i64>,
    /// Text beyond [`MAX_ITEM_TEXT`] was dropped.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

impl HubItem {
    fn new(id: String, turn: u32, kind: &str) -> Self {
        Self {
            id,
            turn,
            kind: kind.to_string(),
            content: Vec::new(),
            tool: None,
            plan: None,
            message_id: None,
            observed_at_ms: None,
            truncated: false,
        }
    }

    /// Text of the first text block.
    pub fn text(&self) -> &str {
        self.content
            .iter()
            .find_map(ContentBlock::as_text)
            .unwrap_or("")
    }
}

/// Summary of one turn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnInfo {
    /// Turn number.
    pub turn: u32,
    /// Id of the user item that opened the turn.
    pub user_item_id: Option<String>,
    /// First characters of the user text.
    pub user_preview: String,
    /// First characters of the turn's last agent message.
    pub agent_preview: String,
    /// Live turns only.
    pub started_at_ms: Option<i64>,
    /// Live turns only.
    pub completed_at_ms: Option<i64>,
    /// Live turns only.
    pub stop_reason: Option<String>,
}

impl TurnInfo {
    fn new(turn: u32) -> Self {
        Self {
            turn,
            user_item_id: None,
            user_preview: String::new(),
            agent_preview: String::new(),
            started_at_ms: None,
            completed_at_ms: None,
            stop_reason: None,
        }
    }
}

/// What [`Transcript::apply`] changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Applied {
    /// An item was created or changed; `delta` is the text appended by a
    /// chunk.
    Item {
        /// Item id.
        id: String,
        /// The item is new.
        created: bool,
        /// Text appended by this update.
        delta: Option<String>,
    },
    /// Session-level state (usage, title, config, modes, commands).
    Session,
    /// Nothing changed.
    Ignored,
}

/// A bounded, folded session transcript.
#[derive(Clone, Debug, Default)]
pub struct Transcript {
    generation: String,
    items: Vec<HubItem>,
    sizes: Vec<usize>,
    bytes: usize,
    by_id: HashMap<String, usize>,
    by_message: HashMap<(String, String), usize>,
    turns: Vec<TurnInfo>,
    current_turn: u32,
    live_turn: Option<u32>,
    dropped_turns: u32,
    anon: HashMap<u32, u32>,
    notices: HashMap<u32, u32>,
    last_agent: HashMap<u32, String>,
}

impl Transcript {
    /// An empty transcript of `generation`.
    pub fn new(generation: impl Into<String>) -> Self {
        Self {
            generation: generation.into(),
            ..Self::default()
        }
    }

    /// Current generation.
    pub fn generation(&self) -> &str {
        &self.generation
    }

    /// Replace the generation.
    pub fn set_generation(&mut self, generation: impl Into<String>) {
        self.generation = generation.into();
    }

    /// Record the hub-originated user message of a live prompt; opens the
    /// next turn.
    pub fn begin_live_turn(&mut self, prompt: &[ContentBlock], now_ms: i64) -> (u32, String) {
        self.current_turn += 1;
        let turn = self.current_turn;
        self.live_turn = Some(turn);
        let id = self.anon_id(turn);
        let mut item = HubItem::new(id.clone(), turn, kind::USER);
        item.observed_at_ms = Some(now_ms);
        for block in prompt {
            append_block(&mut item, block);
        }
        let info = self.turn_mut(turn);
        info.started_at_ms = Some(now_ms);
        info.user_item_id = Some(id.clone());
        self.push(item);
        self.enforce_budget();
        (turn, id)
    }

    /// Fold one update. `now_ms` is `Some` for live updates, `None` for
    /// replays.
    pub fn apply(&mut self, update: &SessionUpdate, now_ms: Option<i64>) -> Applied {
        let applied = match update {
            SessionUpdate::UserMessageChunk(chunk) => {
                if self.live_turn.is_some() {
                    return Applied::Ignored;
                }
                self.user_chunk(chunk, now_ms)
            },
            SessionUpdate::AgentMessageChunk(chunk) => self.chunk(kind::AGENT, chunk, now_ms),
            SessionUpdate::AgentThoughtChunk(chunk) => self.chunk(kind::THOUGHT, chunk, now_ms),
            SessionUpdate::ToolCall(call) => self.tool_call(call, now_ms),
            SessionUpdate::ToolCallUpdate(update) => self.tool_update(update, now_ms),
            SessionUpdate::Plan {
                entries,
            } => self.plan(entries, now_ms),
            SessionUpdate::Unknown(_) => return Applied::Ignored,
            _ => return Applied::Session,
        };
        self.enforce_budget();
        applied
    }

    /// Close the live turn.
    pub fn end_live_turn(&mut self, stop_reason: &str, now_ms: i64) {
        if let Some(turn) = self.live_turn.take() {
            let info = self.turn_mut(turn);
            info.completed_at_ms = Some(now_ms);
            info.stop_reason = Some(stop_reason.to_string());
        }
    }

    /// The live turn, if a prompt is running.
    pub fn live_turn(&self) -> Option<u32> {
        self.live_turn
    }

    /// Insert a hub notice into the current turn; returns its id.
    pub fn push_notice(&mut self, text: &str, now_ms: Option<i64>) -> String {
        let turn = self.current_turn;
        let ordinal = self.notices.entry(turn).or_insert(0);
        let base = format!("n:{turn}:{ordinal}");
        *ordinal += 1;
        let id = self.unique(base);
        let mut item = HubItem::new(id.clone(), turn, kind::NOTICE);
        item.observed_at_ms = now_ms;
        append_block(&mut item, &ContentBlock::text(text));
        self.turn_mut(turn);
        self.push(item);
        self.enforce_budget();
        id
    }

    /// Mark that earlier history exists but is not available (resume-only
    /// agents); sets `dropped_turns` to at least 1.
    pub fn mark_history_unavailable(&mut self) {
        self.dropped_turns = self.dropped_turns.max(1);
    }

    /// All items, oldest first.
    pub fn items(&self) -> &[HubItem] {
        &self.items
    }

    /// One item by id.
    pub fn item(&self, id: &str) -> Option<&HubItem> {
        self.by_id.get(id).map(|&i| &self.items[i])
    }

    /// Turn summaries, oldest first.
    pub fn turns(&self) -> &[TurnInfo] {
        &self.turns
    }

    /// One turn summary.
    pub fn turn_info(&self, turn: u32) -> Option<&TurnInfo> {
        self.turns.iter().rev().find(|t| t.turn == turn)
    }

    /// Current turn number.
    pub fn current_turn(&self) -> u32 {
        self.current_turn
    }

    /// Number of whole turns dropped for the budget (or marked unavailable).
    pub fn dropped_turns(&self) -> u32 {
        self.dropped_turns
    }

    /// Items older than `before` (or the newest), oldest first; returns
    /// `has_older`. An unknown `before` yields an empty window.
    pub fn window_before(&self, before: Option<&str>, limit: usize) -> (Vec<HubItem>, bool) {
        let end = match before {
            None => self.items.len(),
            Some(id) => match self.by_id.get(id) {
                Some(&index) => index,
                None => return (Vec::new(), false),
            },
        };
        let start = end.saturating_sub(limit);
        (self.items[start..end].to_vec(), start > 0)
    }

    /// Items of `turn` after `after`, oldest first; returns `has_more`.
    pub fn turn_items(&self, turn: u32, after: Option<&str>, limit: usize) -> (Vec<HubItem>, bool) {
        let lo = self.items.partition_point(|i| i.turn < turn);
        let hi = self.items.partition_point(|i| i.turn <= turn);
        let start = match after {
            None => lo,
            Some(id) => match self.by_id.get(id) {
                Some(&index) if (lo..hi).contains(&index) => index + 1,
                _ => return (Vec::new(), false),
            },
        };
        let end = start.saturating_add(limit).min(hi);
        (self.items[start..end].to_vec(), end < hi)
    }

    /// After a reload: walk `previous` and `self` in order and reuse
    /// `previous`'s ids for items that are equal, stopping at the first
    /// difference. Returns true when every item of `previous` except its last
    /// one matched, i.e. the reload only extended the conversation.
    ///
    /// Equality: tool items by `toolCallId`; other items by `kind`, `turn`,
    /// `content` and `plan` (ids, message ids, timestamps and truncation are
    /// ignored).
    pub fn adopt_ids_from(&mut self, previous: &Transcript) -> bool {
        let old_ids: Vec<String> = self.items.iter().map(|i| i.id.clone()).collect();
        let mut matched = 0;
        for (mine, theirs) in self.items.iter_mut().zip(&previous.items) {
            if !same_item(mine, theirs) {
                break;
            }
            mine.id = theirs.id.clone();
            matched += 1;
        }
        let mut taken: HashSet<String> =
            self.items[..matched].iter().map(|i| i.id.clone()).collect();
        let reserved: HashSet<String> = old_ids[matched..].iter().cloned().collect();
        for item in &mut self.items[matched..] {
            if taken.contains(&item.id) {
                let mut n = 1;
                while taken.contains(&format!("{}~{n}", item.id))
                    || reserved.contains(&format!("{}~{n}", item.id))
                {
                    n += 1;
                }
                item.id = format!("{}~{n}", item.id);
            }
            taken.insert(item.id.clone());
        }
        let renames: HashMap<String, String> = old_ids
            .into_iter()
            .zip(self.items.iter().map(|i| i.id.clone()))
            .filter(|(old, new)| old != new)
            .collect();
        for info in &mut self.turns {
            if let Some(new) = info.user_item_id.as_ref().and_then(|id| renames.get(id)) {
                info.user_item_id = Some(new.clone());
            }
        }
        for id in self.last_agent.values_mut() {
            if let Some(new) = renames.get(id) {
                *id = new.clone();
            }
        }
        self.reindex();
        matched >= previous.items.len().saturating_sub(1)
    }

    /// Approximate size of all items in bytes.
    pub fn approx_bytes(&self) -> usize {
        self.bytes
    }

    fn user_chunk(&mut self, chunk: &MessageChunk, now_ms: Option<i64>) -> Applied {
        if let Some(last) = self.items.last() {
            if last.kind == kind::USER && last.message_id == chunk.message_id {
                let index = self.items.len() - 1;
                return self.append(index, &chunk.content);
            }
        }
        self.current_turn += 1;
        let turn = self.current_turn;
        let id = self.create(kind::USER, chunk.message_id.as_deref(), now_ms);
        self.turn_mut(turn).user_item_id = Some(id.clone());
        let index = self.items.len() - 1;
        let delta = self.append_created(index, &chunk.content);
        Applied::Item {
            id,
            created: true,
            delta,
        }
    }

    fn chunk(&mut self, kind: &str, chunk: &MessageChunk, now_ms: Option<i64>) -> Applied {
        let existing = match &chunk.message_id {
            Some(mid) => self
                .by_message
                .get(&(kind.to_string(), mid.clone()))
                .copied(),
            None => self
                .items
                .last()
                .filter(|last| last.kind == kind && last.message_id.is_none())
                .map(|_| self.items.len() - 1),
        };
        if let Some(index) = existing {
            return self.append(index, &chunk.content);
        }
        let id = self.create(kind, chunk.message_id.as_deref(), now_ms);
        let index = self.items.len() - 1;
        let delta = self.append_created(index, &chunk.content);
        Applied::Item {
            id,
            created: true,
            delta,
        }
    }

    fn tool_call(&mut self, call: &ToolCall, now_ms: Option<i64>) -> Applied {
        let mut tool = call.clone();
        bound_raw(&mut tool.raw_input);
        bound_raw(&mut tool.raw_output);
        let id = format!("tc:{}", call.tool_call_id);
        if let Some(&index) = self.by_id.get(&id) {
            self.items[index].tool = Some(tool);
            self.resize(index);
            return Applied::Item {
                id,
                created: false,
                delta: None,
            };
        }
        let mut item = HubItem::new(id.clone(), self.current_turn, kind::TOOL);
        item.tool = Some(tool);
        item.observed_at_ms = now_ms;
        self.turn_mut(self.current_turn);
        self.push(item);
        Applied::Item {
            id,
            created: true,
            delta: None,
        }
    }

    fn tool_update(&mut self, update: &ToolCallUpdate, now_ms: Option<i64>) -> Applied {
        let id = format!("tc:{}", update.tool_call_id);
        let (index, created) = match self.by_id.get(&id) {
            Some(&index) => (index, false),
            None => {
                let mut item = HubItem::new(id.clone(), self.current_turn, kind::TOOL);
                item.tool = Some(ToolCall {
                    tool_call_id: update.tool_call_id.clone(),
                    title: update.tool_call_id.clone(),
                    ..ToolCall::default()
                });
                item.observed_at_ms = now_ms;
                self.turn_mut(self.current_turn);
                self.push(item);
                (self.items.len() - 1, true)
            },
        };
        let tool = self.items[index].tool.get_or_insert_with(|| ToolCall {
            tool_call_id: update.tool_call_id.clone(),
            ..ToolCall::default()
        });
        patch_tool(tool, update);
        self.resize(index);
        Applied::Item {
            id,
            created,
            delta: None,
        }
    }

    fn plan(&mut self, entries: &[PlanEntry], now_ms: Option<i64>) -> Applied {
        let turn = self.current_turn;
        let id = format!("p:{turn}");
        if let Some(&index) = self.by_id.get(&id) {
            self.items[index].plan = Some(entries.to_vec());
            self.resize(index);
            return Applied::Item {
                id,
                created: false,
                delta: None,
            };
        }
        let mut item = HubItem::new(id.clone(), turn, kind::PLAN);
        item.plan = Some(entries.to_vec());
        item.observed_at_ms = now_ms;
        self.turn_mut(turn);
        self.push(item);
        Applied::Item {
            id,
            created: true,
            delta: None,
        }
    }

    /// Push an empty message item of `kind` in the current turn; returns its
    /// id.
    fn create(&mut self, kind: &str, message_id: Option<&str>, now_ms: Option<i64>) -> String {
        let turn = self.current_turn;
        let id = match message_id {
            Some(mid) => self.unique(format!("m:{kind}:{mid}")),
            None => self.anon_id(turn),
        };
        let mut item = HubItem::new(id.clone(), turn, kind);
        item.message_id = message_id.map(str::to_string);
        item.observed_at_ms = now_ms;
        self.turn_mut(turn);
        if kind == kind::AGENT {
            self.last_agent.insert(turn, id.clone());
        }
        self.push(item);
        id
    }

    fn append_created(&mut self, index: usize, block: &ContentBlock) -> Option<String> {
        match self.append(index, block) {
            Applied::Item {
                delta, ..
            } => delta,
            _ => None,
        }
    }

    fn append(&mut self, index: usize, block: &ContentBlock) -> Applied {
        let delta = append_block(&mut self.items[index], block);
        self.resize(index);
        self.refresh_previews(index);
        Applied::Item {
            id: self.items[index].id.clone(),
            created: false,
            delta,
        }
    }

    fn refresh_previews(&mut self, index: usize) {
        let item = &self.items[index];
        let turn = item.turn;
        let preview: String = item.text().chars().take(PREVIEW_CHARS).collect();
        let id = item.id.clone();
        let kind = item.kind.clone();
        let is_last_agent = self.last_agent.get(&turn) == Some(&id);
        let info = self.turn_mut(turn);
        if kind == kind::USER && info.user_item_id.as_deref() == Some(id.as_str()) {
            info.user_preview = preview;
        } else if kind == kind::AGENT && is_last_agent {
            info.agent_preview = preview;
        }
    }

    fn anon_id(&mut self, turn: u32) -> String {
        loop {
            let ordinal = self.anon.entry(turn).or_insert(0);
            let id = format!("s:{turn}:{ordinal}");
            *ordinal += 1;
            if !self.by_id.contains_key(&id) {
                return id;
            }
        }
    }

    fn unique(&self, base: String) -> String {
        if !self.by_id.contains_key(&base) {
            return base;
        }
        let mut n = 1;
        loop {
            let id = format!("{base}~{n}");
            if !self.by_id.contains_key(&id) {
                return id;
            }
            n += 1;
        }
    }

    fn turn_mut(&mut self, turn: u32) -> &mut TurnInfo {
        let position = self.turns.iter().rposition(|t| t.turn == turn);
        let index = match position {
            Some(index) => index,
            None => {
                let at = self.turns.partition_point(|t| t.turn < turn);
                self.turns.insert(at, TurnInfo::new(turn));
                at
            },
        };
        &mut self.turns[index]
    }

    fn push(&mut self, item: HubItem) {
        let index = self.items.len();
        let size = estimate(&item);
        self.by_id.insert(item.id.clone(), index);
        if let Some(mid) = &item.message_id {
            self.by_message
                .insert((item.kind.clone(), mid.clone()), index);
        }
        self.items.push(item);
        self.sizes.push(size);
        self.bytes += size;
        self.refresh_previews(index);
    }

    fn resize(&mut self, index: usize) {
        let size = estimate(&self.items[index]);
        self.bytes = self.bytes - self.sizes[index] + size;
        self.sizes[index] = size;
    }

    fn reindex(&mut self) {
        self.by_id.clear();
        self.by_message.clear();
        for (index, item) in self.items.iter().enumerate() {
            self.by_id.insert(item.id.clone(), index);
            if let Some(mid) = &item.message_id {
                self.by_message
                    .insert((item.kind.clone(), mid.clone()), index);
            }
        }
    }

    /// Drop the oldest whole turns while over budget; the current turn stays.
    fn enforce_budget(&mut self) {
        let mut remove = 0;
        let mut bytes = self.bytes;
        let mut dropped = Vec::new();
        while self.items.len() - remove > MAX_ITEMS || bytes > MAX_BYTES {
            let Some(first) = self.items.get(remove) else { break };
            let turn = first.turn;
            if turn >= self.current_turn {
                break;
            }
            while self.items.get(remove).is_some_and(|i| i.turn == turn) {
                bytes -= self.sizes[remove];
                remove += 1;
            }
            dropped.push(turn);
        }
        if remove == 0 {
            return;
        }
        self.items.drain(..remove);
        self.sizes.drain(..remove);
        self.bytes = bytes;
        self.turns.retain(|t| !dropped.contains(&t.turn));
        for turn in &dropped {
            self.last_agent.remove(turn);
        }
        self.dropped_turns += dropped.len() as u32;
        self.reindex();
    }
}

/// Append `block` to `item`; returns the text actually appended.
fn append_block(item: &mut HubItem, block: &ContentBlock) -> Option<String> {
    let ContentBlock::Text(text) = block else {
        item.content.push(block.clone());
        return None;
    };
    if item.truncated {
        return None;
    }
    let current = item
        .content
        .iter()
        .find_map(ContentBlock::as_text)
        .map_or(0, str::len);
    let room = MAX_ITEM_TEXT.saturating_sub(current);
    let piece = if text.text.len() <= room {
        text.text.as_str()
    } else {
        item.truncated = true;
        let mut cut = room;
        while !text.text.is_char_boundary(cut) {
            cut -= 1;
        }
        &text.text[..cut]
    };
    match item.content.iter_mut().find_map(|b| match b {
        ContentBlock::Text(t) => Some(t),
        _ => None,
    }) {
        Some(first) => first.text.push_str(piece),
        None => item.content.push(ContentBlock::Text(TextContent {
            text: piece.to_string(),
            ..text.clone()
        })),
    }
    (!piece.is_empty()).then(|| piece.to_string())
}

fn patch_tool(tool: &mut ToolCall, update: &ToolCallUpdate) {
    if let Some(title) = &update.title {
        tool.title = title.clone();
    }
    if let Some(kind) = &update.kind {
        tool.kind = Some(kind.clone());
    }
    if let Some(status) = &update.status {
        tool.status = Some(status.clone());
    }
    if let Some(content) = &update.content {
        tool.content = content.clone();
    }
    if let Some(locations) = &update.locations {
        tool.locations = locations.clone();
    }
    if let Some(raw) = &update.raw_input {
        tool.raw_input = Some(raw.clone());
        bound_raw(&mut tool.raw_input);
    }
    if let Some(raw) = &update.raw_output {
        tool.raw_output = Some(raw.clone());
        bound_raw(&mut tool.raw_output);
    }
    if let Some(name) = &update.name {
        tool.name = Some(name.clone());
    }
    if update.meta.is_some() {
        tool.meta = update.meta.clone();
    }
}

/// Replace raw JSON larger than [`MAX_RAW_JSON`] with a size marker.
fn bound_raw(raw: &mut Option<Value>) {
    let Some(value) = raw else { return };
    let size = serde_json::to_vec(value).map_or(0, |v| v.len());
    if size > MAX_RAW_JSON {
        *raw = Some(json!({ "_pcxTruncated": size }));
    }
}

fn block_size(block: &ContentBlock) -> usize {
    match block {
        ContentBlock::Text(t) => t.text.len(),
        ContentBlock::Image(i) => i.data.len() + i.mime_type.len(),
        ContentBlock::Audio(a) => a.data.len() + a.mime_type.len(),
        ContentBlock::ResourceLink(l) => l.uri.len() + l.name.len(),
        ContentBlock::Resource(r) => json_size(&r.resource),
        ContentBlock::Unknown(v) => json_size(v),
    }
}

fn json_size(value: &Value) -> usize {
    serde_json::to_vec(value).map_or(0, |v| v.len())
}

fn estimate(item: &HubItem) -> usize {
    let content: usize = item.content.iter().map(block_size).sum();
    let tool = item
        .tool
        .as_ref()
        .and_then(|t| serde_json::to_vec(t).ok())
        .map_or(0, |v| v.len());
    let plan: usize = item
        .plan
        .iter()
        .flatten()
        .map(|e| e.content.len() + e.priority.len() + e.status.len())
        .sum();
    128 + item.id.len() + content + tool + plan
}

fn block_eq(a: &ContentBlock, b: &ContentBlock) -> bool {
    match (a, b) {
        (ContentBlock::Text(x), ContentBlock::Text(y)) => x.text == y.text,
        (ContentBlock::Image(x), ContentBlock::Image(y)) => {
            x.data == y.data && x.mime_type == y.mime_type
        },
        (ContentBlock::Audio(x), ContentBlock::Audio(y)) => {
            x.data == y.data && x.mime_type == y.mime_type
        },
        (ContentBlock::ResourceLink(x), ContentBlock::ResourceLink(y)) => {
            x.uri == y.uri && x.name == y.name
        },
        (ContentBlock::Resource(x), ContentBlock::Resource(y)) => x.resource == y.resource,
        (x, y) => x == y,
    }
}

fn same_item(a: &HubItem, b: &HubItem) -> bool {
    if a.kind == kind::TOOL || b.kind == kind::TOOL {
        let id = |i: &HubItem| i.tool.as_ref().map(|t| t.tool_call_id.clone());
        return a.kind == b.kind && id(a) == id(b);
    }
    a.kind == b.kind
        && a.turn == b.turn
        && a.plan == b.plan
        && a.content.len() == b.content.len()
        && a.content
            .iter()
            .zip(&b.content)
            .all(|(x, y)| block_eq(x, y))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn update(value: Value) -> SessionUpdate {
        serde_json::from_value(value).expect("update")
    }

    fn agent(text: &str, mid: Option<&str>) -> SessionUpdate {
        let mut v = json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}});
        if let Some(mid) = mid {
            v["messageId"] = json!(mid);
        }
        update(v)
    }

    fn thought(text: &str) -> SessionUpdate {
        update(
            json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": text}}),
        )
    }

    fn user(text: &str, mid: Option<&str>) -> SessionUpdate {
        let mut v = json!({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": text}});
        if let Some(mid) = mid {
            v["messageId"] = json!(mid);
        }
        update(v)
    }

    fn replay() -> Vec<SessionUpdate> {
        vec![
            user("hello", None),
            agent("hi ", None),
            agent("there", None),
            update(
                json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "ls", "status": "pending"}),
            ),
            update(
                json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed"}),
            ),
            agent("done", None),
            user("again", None),
            agent("ok", Some("a2")),
        ]
    }

    fn folded(updates: &[SessionUpdate]) -> Transcript {
        let mut t = Transcript::new("e.0");
        for u in updates {
            t.apply(u, None);
        }
        t
    }

    #[test]
    fn chunks_with_message_id_merge() {
        let mut t = Transcript::new("g");
        let first = t.apply(&agent("Hel", Some("m1")), Some(1));
        assert_eq!(first, Applied::Item {
            id: "m:agent:m1".into(),
            created: true,
            delta: Some("Hel".into())
        });
        t.apply(&thought("hmm"), Some(2));
        let again = t.apply(&agent("lo", Some("m1")), Some(3));
        assert_eq!(again, Applied::Item {
            id: "m:agent:m1".into(),
            created: false,
            delta: Some("lo".into())
        });
        assert_eq!(t.item("m:agent:m1").expect("item").text(), "Hello");
        assert_eq!(t.items().len(), 2);
    }

    #[test]
    fn chunks_without_id_merge_until_kind_changes() {
        let mut t = Transcript::new("g");
        t.apply(&agent("a", None), None);
        t.apply(&agent("b", None), None);
        t.apply(&thought("t"), None);
        t.apply(&agent("c", None), None);
        let ids: Vec<_> = t
            .items()
            .iter()
            .map(|i| (i.id.as_str(), i.text()))
            .collect();
        assert_eq!(ids, vec![("s:0:0", "ab"), ("s:0:1", "t"), ("s:0:2", "c")]);
        assert_eq!(t.current_turn(), 0);
    }

    #[test]
    fn live_turn_ignores_user_echo() {
        let mut t = Transcript::new("g");
        let (turn, id) = t.begin_live_turn(&[ContentBlock::text("do it")], 10);
        assert_eq!((turn, id.as_str()), (1, "s:1:0"));
        assert_eq!(t.apply(&user("do it", None), Some(11)), Applied::Ignored);
        t.apply(&agent("done", None), Some(12));
        t.end_live_turn("end_turn", 20);
        assert_eq!(t.items().len(), 2);
        let info = t.turn_info(1).expect("turn");
        assert_eq!(info.user_preview, "do it");
        assert_eq!(info.agent_preview, "done");
        assert_eq!(info.started_at_ms, Some(10));
        assert_eq!(info.completed_at_ms, Some(20));
        assert_eq!(info.stop_reason.as_deref(), Some("end_turn"));
        // After the live turn a replayed user message opens a new turn.
        t.apply(&user("next", None), None);
        assert_eq!(t.current_turn(), 2);
    }

    #[test]
    fn tool_call_upsert_and_patch() {
        let mut t = Transcript::new("g");
        let created = t.apply(
            &update(json!({"sessionUpdate": "tool_call_update", "toolCallId": "x", "status": "in_progress"})),
            None,
        );
        assert_eq!(created, Applied::Item {
            id: "tc:x".into(),
            created: true,
            delta: None
        });
        let tool = t.item("tc:x").and_then(|i| i.tool.clone()).expect("tool");
        assert_eq!(tool.title, "x");
        t.apply(
            &update(json!({"sessionUpdate": "tool_call", "toolCallId": "x", "title": "Read file", "kind": "read",
                "locations": [{"path": "/a"}], "rawInput": {"p": 1}})),
            None,
        );
        t.apply(
            &update(json!({"sessionUpdate": "tool_call_update", "toolCallId": "x", "status": "completed",
                "content": [{"type": "content", "content": {"type": "text", "text": "out"}}],
                "locations": []})),
            None,
        );
        let tool = t.item("tc:x").and_then(|i| i.tool.clone()).expect("tool");
        assert_eq!(tool.title, "Read file");
        assert_eq!(tool.kind.as_deref(), Some("read"));
        assert_eq!(tool.status.as_deref(), Some("completed"));
        assert!(tool.locations.is_empty());
        assert_eq!(tool.content.len(), 1);
        assert_eq!(tool.raw_input, Some(json!({"p": 1})));
        assert_eq!(t.items().len(), 1);
    }

    #[test]
    fn plan_replaces_within_turn() {
        let mut t = Transcript::new("g");
        let plan = |c: &str| {
            update(
                json!({"sessionUpdate": "plan", "entries": [{"content": c, "priority": "high", "status": "pending"}]}),
            )
        };
        t.apply(&user("go", None), None);
        t.apply(&plan("a"), None);
        t.apply(&agent("x", None), None);
        assert_eq!(t.apply(&plan("b"), None), Applied::Item {
            id: "p:1".into(),
            created: false,
            delta: None
        });
        assert_eq!(t.item("p:1").and_then(|i| i.plan.clone()).expect("plan")[0].content, "b");
        t.apply(&user("next", None), None);
        assert_eq!(t.apply(&plan("c"), None), Applied::Item {
            id: "p:2".into(),
            created: true,
            delta: None
        });
    }

    #[test]
    fn oversized_text_is_truncated() {
        let mut t = Transcript::new("g");
        let big = "é".repeat(MAX_ITEM_TEXT / 2 - 1);
        t.apply(&agent(&big, Some("m")), None);
        let Applied::Item {
            delta, ..
        } = t.apply(&agent("abcdef", Some("m")), None)
        else {
            panic!("item");
        };
        let item = t.item("m:agent:m").expect("item");
        assert!(item.truncated);
        assert!(item.text().len() <= MAX_ITEM_TEXT);
        assert_eq!(delta.as_deref(), Some("ab"));
        assert_eq!(t.apply(&agent("more", Some("m")), None), Applied::Item {
            id: "m:agent:m".into(),
            created: false,
            delta: None
        });
    }

    #[test]
    fn oversized_raw_output_is_replaced() {
        let mut t = Transcript::new("g");
        let big = "x".repeat(MAX_RAW_JSON + 10);
        t.apply(
            &update(json!({"sessionUpdate": "tool_call", "toolCallId": "t", "title": "t", "rawOutput": big})),
            None,
        );
        let tool = t.item("tc:t").and_then(|i| i.tool.clone()).expect("tool");
        assert_eq!(tool.raw_output, Some(json!({"_pcxTruncated": MAX_RAW_JSON + 12})));
        t.apply(
            &update(
                json!({"sessionUpdate": "tool_call_update", "toolCallId": "t", "rawInput": big}),
            ),
            None,
        );
        let tool = t.item("tc:t").and_then(|i| i.tool.clone()).expect("tool");
        assert_eq!(tool.raw_input, Some(json!({"_pcxTruncated": MAX_RAW_JSON + 12})));
    }

    #[test]
    fn budget_drops_oldest_whole_turns() {
        let mut t = Transcript::new("g");
        let per_turn = 200;
        for turn in 1..=30u32 {
            t.apply(&user(&format!("u{turn}"), None), None);
            for i in 1..per_turn {
                t.apply(&agent("x", Some(&format!("{turn}-{i}"))), None);
            }
        }
        assert!(t.items().len() <= MAX_ITEMS);
        assert_eq!(t.current_turn(), 30);
        let first = t.items()[0].turn;
        assert!(first > 1);
        // Turns start at 1 here (nothing precedes the first user message).
        assert_eq!(t.dropped_turns(), first - 1);
        assert_eq!(t.turns()[0].turn, first);
        assert!(t.items().iter().filter(|i| i.turn == first).count() == per_turn);
        assert!(t.item("m:agent:1-1").is_none());
        assert_eq!(t.item("m:agent:30-1").map(|i| i.turn), Some(30));
        let (window, has_older) = t.window_before(None, 10);
        assert_eq!(window.len(), 10);
        assert!(has_older);
    }

    #[test]
    fn adopt_ids_keeps_generation_after_live_turns() {
        let mut live = folded(&replay());
        live.begin_live_turn(&[ContentBlock::text("third")], 100);
        live.apply(&agent("answer", Some("a3")), Some(101));
        live.end_live_turn("end_turn", 102);
        let live_user = live
            .turn_info(3)
            .and_then(|t| t.user_item_id.clone())
            .expect("user");

        let mut reloaded_updates = replay();
        reloaded_updates.push(user("third", Some("u3")));
        reloaded_updates.push(agent("answer", Some("a3")));
        reloaded_updates.push(agent("extra", Some("a4")));
        let mut reloaded = folded(&reloaded_updates);
        assert!(reloaded.adopt_ids_from(&live));
        let ids: Vec<_> = reloaded.items().iter().map(|i| i.id.clone()).collect();
        let live_ids: Vec<_> = live.items().iter().map(|i| i.id.clone()).collect();
        assert_eq!(&ids[..live_ids.len()], &live_ids[..]);
        assert_eq!(reloaded.turn_info(3).and_then(|t| t.user_item_id.clone()), Some(live_user));
        // The message-id index follows the adopted ids.
        let Applied::Item {
            id, ..
        } = reloaded.apply(&agent("!", Some("a4")), Some(1))
        else {
            panic!("item");
        };
        assert_eq!(reloaded.item(&id).map(HubItem::text), Some("extra!"));
        let unique: HashSet<_> = reloaded.items().iter().map(|i| &i.id).collect();
        assert_eq!(unique.len(), reloaded.items().len());
    }

    #[test]
    fn divergent_reload_bumps_generation() {
        let previous = folded(&replay());
        let mut different = replay();
        different[1] = agent("rewritten ", None);
        let mut reloaded = folded(&different);
        assert!(!reloaded.adopt_ids_from(&previous));
        // Only the last item differing still counts as an extension.
        let mut tail_changed = replay();
        let last = tail_changed.len() - 1;
        tail_changed[last] = agent("ok, longer", Some("a2"));
        let mut reloaded = folded(&tail_changed);
        assert!(reloaded.adopt_ids_from(&previous));
    }

    #[test]
    fn ids_are_stable_across_identical_replays() {
        let a = folded(&replay());
        let b = folded(&replay());
        assert_eq!(a.items(), b.items());
        let ids: Vec<_> = a.items().iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["s:1:0", "s:1:1", "tc:t1", "s:1:2", "s:2:0", "m:agent:a2"]);
        let (items, more) = a.turn_items(1, Some("s:1:1"), 1);
        assert_eq!(items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), vec!["tc:t1"]);
        assert!(more);
        assert_eq!(a.turns().len(), 2);
        assert_eq!(a.turns()[0].agent_preview, "done");
    }
}
