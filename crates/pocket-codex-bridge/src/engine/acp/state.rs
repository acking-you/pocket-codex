//! Per-connection state of the controller-side ACP engine (TRD §4.4.2): the
//! hub metadata, one [`SessionView`] per attached session, pending requests
//! and unacknowledged submissions.
//!
//! The bridge never folds updates itself (T1): it merges hub items by id.

use std::collections::{HashMap, HashSet};

use pocket_codex_core::acp::{
    pcx::{AttachResult, HubMeta},
    AvailableCommand, ConfigOption, ContentBlock, HubItem, PermissionOption, SessionModeState,
    TextContent, TurnInfo, UsageUpdate,
};
use serde_json::Value;

/// Items kept per session view.
pub const MAX_VIEW_ITEMS: usize = 2000;
/// Notifications buffered while an attach is in flight.
pub const MAX_BUFFERED: usize = 4096;

/// What a pending hub request asks for.
#[derive(Clone, Debug)]
pub enum PendingKind {
    /// Permission with its options.
    Permission(Vec<PermissionOption>),
    /// Form elicitation with its schema.
    Form(Value),
    /// URL elicitation.
    Url,
}

/// One pending request from the hub.
#[derive(Clone, Debug)]
pub struct PendingReq {
    /// AppClient token to answer with.
    pub token: String,
    /// Owning session.
    pub session: Option<String>,
    /// What is asked.
    pub kind: PendingKind,
    /// Connection generation that delivered the current token.
    pub conn_gen: u64,
}

/// One session as the controller knows it.
#[derive(Clone, Debug, Default)]
pub struct SessionView {
    /// Items, oldest first.
    pub items: Vec<HubItem>,
    index: HashMap<String, usize>,
    /// Transcript generation.
    pub generation: String,
    /// Last applied notification sequence number.
    pub seq: u64,
    /// Turn summaries.
    pub turns: Vec<TurnInfo>,
    /// Turns no longer available.
    pub dropped_turns: u32,
    /// Older items exist on the hub.
    pub has_older: bool,
    /// Older history exists but is unavailable.
    pub older_unavailable: bool,
    /// A turn is running.
    pub running: bool,
    /// The running turn.
    pub active_turn: Option<u32>,
    /// Working directory.
    pub cwd: String,
    /// Title.
    pub title: Option<String>,
    /// Last update.
    pub updated_at: Option<String>,
    /// Config options.
    pub config_options: Vec<ConfigOption>,
    /// Modes.
    pub modes: Option<SessionModeState>,
    /// Slash commands.
    pub commands: Vec<AvailableCommand>,
    /// Usage.
    pub usage: Option<UsageUpdate>,
    /// Mode value before switching to plan.
    pub pre_plan_mode: Option<String>,
    /// An attach / reload is in flight; notifications are buffered.
    pub syncing: bool,
    /// Buffered `(seq, method, params)`.
    pub buffered: Vec<(u64, String, Value)>,
    /// The buffer overflowed; re-attach after the response.
    pub overflowed: bool,
    /// Items that already got `item/completed` in the running turn.
    pub completed: HashSet<String>,
    /// Per turn: items read through `thread_turn_page`, and the last id.
    pub turn_reads: HashMap<u32, Vec<HubItem>>,
    /// The hub reported this session as still loading.
    pub loading: bool,
    /// `_pcx/session/loadFailed` arrived (its message).
    pub load_failed: Option<String>,
}

impl SessionView {
    /// Replace the view with an attach / reload snapshot.
    pub fn reset_from(&mut self, r: &AttachResult) {
        self.items = r.items.clone();
        self.reindex();
        self.generation.clone_from(&r.generation);
        self.seq = r.seq;
        self.turns.clone_from(&r.turns);
        self.dropped_turns = r.dropped_turns;
        self.has_older = r.has_older;
        self.older_unavailable = r.older_unavailable;
        self.running = r.running;
        self.active_turn = r.active_turn;
        self.cwd.clone_from(&r.cwd);
        self.title.clone_from(&r.title);
        self.updated_at.clone_from(&r.updated_at);
        self.config_options.clone_from(&r.config_options);
        self.modes.clone_from(&r.modes);
        self.commands.clone_from(&r.commands);
        self.usage.clone_from(&r.usage);
        self.loading = r.loading;
        self.load_failed = None;
        self.turn_reads.clear();
    }

    fn reindex(&mut self) {
        self.index = self
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| (item.id.clone(), i))
            .collect();
    }

    /// Item by id.
    pub fn item(&self, id: &str) -> Option<&HubItem> {
        self.index.get(id).map(|&i| &self.items[i])
    }

    /// Replace an item by id, or append it.
    pub fn upsert(&mut self, item: HubItem) {
        match self.index.get(&item.id) {
            Some(&i) => self.items[i] = item,
            None => {
                self.index.insert(item.id.clone(), self.items.len());
                self.items.push(item);
                self.bound();
            },
        }
    }

    /// Append a chunk to item `id` (creating it when unknown); returns the
    /// item.
    pub fn append_chunk(
        &mut self,
        id: &str,
        kind: &str,
        turn: u32,
        block: &ContentBlock,
    ) -> &HubItem {
        if !self.index.contains_key(id) {
            let item = HubItem {
                id: id.to_string(),
                turn,
                kind: kind.to_string(),
                content: Vec::new(),
                tool: None,
                plan: None,
                message_id: None,
                observed_at_ms: None,
                truncated: false,
            };
            self.index.insert(id.to_string(), self.items.len());
            self.items.push(item);
            self.bound();
        }
        let i = self.index[id];
        let item = &mut self.items[i];
        match block {
            ContentBlock::Text(t) => match item.content.iter_mut().find_map(|b| match b {
                ContentBlock::Text(existing) => Some(existing),
                _ => None,
            }) {
                Some(existing) => existing.text.push_str(&t.text),
                None => item.content.push(ContentBlock::Text(TextContent {
                    text: t.text.clone(),
                    ..t.clone()
                })),
            },
            other => item.content.push(other.clone()),
        }
        &self.items[i]
    }

    /// Drop the oldest items beyond [`MAX_VIEW_ITEMS`].
    fn bound(&mut self) {
        if self.items.len() > MAX_VIEW_ITEMS {
            let excess = self.items.len() - MAX_VIEW_ITEMS;
            self.items.drain(..excess);
            self.has_older = true;
            self.reindex();
        }
    }

    /// Drop every item (generation changed).
    pub fn clear_items(&mut self) {
        self.items.clear();
        self.index.clear();
        self.completed.clear();
        self.turn_reads.clear();
    }

    /// Prepend an older window (deduplicated by id).
    pub fn prepend(&mut self, older: Vec<HubItem>) {
        let fresh: Vec<HubItem> = older
            .into_iter()
            .filter(|i| !self.index.contains_key(&i.id))
            .collect();
        let mut items = fresh;
        items.append(&mut self.items);
        self.items = items;
        self.reindex();
    }

    /// Summary of `turn`.
    pub fn turn_mut(&mut self, turn: u32) -> &mut TurnInfo {
        let position = self.turns.iter().position(|t| t.turn == turn);
        let index = match position {
            Some(i) => i,
            None => {
                let at = self.turns.partition_point(|t| t.turn < turn);
                self.turns.insert(at, TurnInfo {
                    turn,
                    user_item_id: None,
                    user_preview: String::new(),
                    agent_preview: String::new(),
                    started_at_ms: None,
                    completed_at_ms: None,
                    stop_reason: None,
                });
                at
            },
        };
        &mut self.turns[index]
    }
}

/// Everything one connection knows.
#[derive(Default)]
pub struct Shared {
    /// `_meta.pcx` / latest `_pcx/hub/state`.
    pub meta: Option<HubMeta>,
    /// Session id → view.
    pub sessions: HashMap<String, SessionView>,
    /// Hub request id → pending request.
    pub pending: HashMap<String, PendingReq>,
    /// clientSubmissionId → (session, prompt) not yet acknowledged.
    pub unacked: HashMap<String, (String, Vec<ContentBlock>)>,
    /// Sessions waiting for `_pcx/session/loaded` (`Ok`) or `loadFailed`.
    pub loads: HashMap<String, Vec<tokio::sync::oneshot::Sender<Result<(), String>>>>,
    /// Incremented on every reconnect; pending requests not re-sent on the
    /// new connection were resolved meanwhile.
    pub conn_gen: u64,
}
