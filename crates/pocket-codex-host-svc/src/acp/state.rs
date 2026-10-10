//! The host's authoritative state for one agent: generation, phase,
//! admitted sessions, running and recently finished turns, pending
//! permissions, retained transcripts and the sequenced event log.
//!
//! Every mutation happens under one lock together with the event it emits,
//! so a snapshot (state + last sequence number) and the events after it are
//! always consistent: a controller that reads the snapshot and then streams
//! from its sequence watermark sees every later change exactly once. A
//! history read carries the same watermark, so a controller can rebuild a
//! turn's fold and apply exactly the events that follow it.
//!
//! The generation increases whenever an agent process starts and whenever a
//! running one is invalidated. Work started under an older generation (a
//! prompt task, a reader callback) checks it before writing and is dropped
//! when it no longer matches, so a replaced process can never emit into its
//! successor.
//!
//! A session being reopened (`session/load` or `session/resume`) is
//! *pending*: it accepts no prompt and grants no file authority until the
//! agent confirmed the reopen.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::broadcast;

use super::{
    fold::Transcript,
    jsonrpc::RequestId,
    peer::Peer,
    schema::{Negotiated, PermissionOption, SessionSettings, TurnOutcome},
};

/// Most events retained for replay.
pub const MAX_LOG_EVENTS: usize = 4096;
/// Most event bytes retained for replay.
pub const MAX_LOG_BYTES: usize = 16 * 1024 * 1024;
/// Most sessions admitted at once.
pub const MAX_SESSIONS: usize = 64;
/// Most transcripts retained in memory.
pub const MAX_TRANSCRIPTS: usize = 32;
/// Most pending permission requests.
pub const MAX_PERMISSIONS: usize = 64;
/// Most agent-listed session directories remembered.
pub const MAX_LISTED: usize = 1000;
const MAX_RECENT: usize = 256;
const MAX_LOST: usize = 1024;
const BROADCAST_CAPACITY: usize = 1024;

/// Lifecycle phase of the hosted agent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Phase {
    /// Launching and negotiating.
    #[default]
    Starting,
    /// Negotiated and accepting work.
    Ready,
    /// Not running because it failed (reason is safe to show; it never
    /// contains arguments or agent stderr).
    Failed {
        /// Why.
        reason: String,
    },
    /// Stopped on request.
    Stopped,
}

/// One sequenced event.
#[derive(Debug)]
pub struct LogEntry {
    /// Sequence number (monotonic across generations).
    pub seq: u64,
    /// Generation the event belongs to.
    pub generation: u64,
    /// Serialized JSON body.
    pub body: Arc<str>,
}

/// The bounded event log plus its live fan-out.
pub struct EventLog {
    last: u64,
    ring: VecDeque<Arc<LogEntry>>,
    bytes: usize,
    sender: broadcast::Sender<Arc<LogEntry>>,
}

impl Default for EventLog {
    fn default() -> Self {
        Self {
            last: 0,
            ring: VecDeque::new(),
            bytes: 0,
            sender: broadcast::channel(BROADCAST_CAPACITY).0,
        }
    }
}

impl EventLog {
    fn push(&mut self, generation: u64, mut body: Value) -> u64 {
        self.last += 1;
        body["seq"] = json!(self.last);
        body["generation"] = json!(generation);
        let entry = Arc::new(LogEntry {
            seq: self.last,
            generation,
            body: Arc::from(body.to_string()),
        });
        self.bytes += entry.body.len();
        self.ring.push_back(entry.clone());
        while self.ring.len() > MAX_LOG_EVENTS || self.bytes > MAX_LOG_BYTES {
            match self.ring.pop_front() {
                Some(old) => self.bytes -= old.body.len(),
                None => break,
            }
        }
        let _ = self.sender.send(entry);
        self.last
    }

    /// The last assigned sequence number.
    pub fn last(&self) -> u64 {
        self.last
    }

    /// Retained events after `after`, or `None` when some were evicted (or
    /// `after` is from the future): the caller must resynchronize.
    pub fn since(&self, after: u64) -> Option<Vec<Arc<LogEntry>>> {
        if after > self.last {
            return None;
        }
        let first = self.ring.front().map_or(self.last + 1, |entry| entry.seq);
        if after + 1 < first {
            return None;
        }
        Some(
            self.ring
                .iter()
                .filter(|entry| entry.seq > after)
                .cloned()
                .collect(),
        )
    }

    /// A live receiver of later events.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<LogEntry>> {
        self.sender.subscribe()
    }

    fn clear(&mut self) {
        self.ring.clear();
        self.bytes = 0;
    }
}

/// The running turn of a session.
#[derive(Debug, Clone)]
pub struct ActiveTurn {
    /// Host turn id.
    pub id: String,
    /// Whether cancellation was requested; the turn still runs until the
    /// agent answers the prompt (or ends without it being sent).
    pub cancel_requested: bool,
    /// Whether `session/prompt` was handed to the writer.
    pub dispatched: bool,
}

impl ActiveTurn {
    /// A newly admitted turn.
    pub fn new(id: String) -> Self {
        Self {
            id,
            cancel_requested: false,
            dispatched: false,
        }
    }
}

/// One admitted session of the current generation.
#[derive(Debug, Clone)]
pub struct Session {
    /// Immutable working directory.
    pub cwd: String,
    /// Current settings.
    pub settings: SessionSettings,
    /// Title, when reported.
    pub title: Option<String>,
    /// A title signal (a title or an explicit clear) arrived since this
    /// admission; a confirmed reopen persists it, and only then.
    pub title_reported: bool,
    /// Context usage `(used, size)`, when reported.
    pub usage: Option<(u64, u64)>,
    /// The running turn.
    pub active: Option<ActiveTurn>,
    /// `session/load` or `session/resume` is in progress: no prompts, no
    /// file authority, and updates are not live.
    pub pending: bool,
    /// Updates are `session/load` history replay.
    pub replaying: bool,
    touched: u64,
}

/// A pending `session/request_permission`.
#[derive(Debug, Clone)]
pub struct Permission {
    /// Session.
    pub session: String,
    /// Host turn it arrived during.
    pub turn: String,
    /// The agent's request id, answered verbatim.
    pub acp_id: RequestId,
    /// Offered options.
    pub options: Vec<PermissionOption>,
    /// The tool call it is about (verbatim).
    pub tool_call: Value,
}

#[derive(Debug, Clone)]
struct Recent {
    session: String,
    turn: String,
    outcome: TurnOutcome,
}

/// The state behind the host lock.
#[derive(Default)]
pub struct State {
    /// Current generation.
    pub generation: u64,
    /// Current phase.
    pub phase: Phase,
    /// Negotiation result of the running generation.
    pub negotiated: Option<Negotiated>,
    /// Connection of the running generation.
    pub peer: Option<Peer>,
    /// The agent most recently answered with ACP `auth_required`.
    pub auth_required: bool,
    /// Admitted sessions.
    pub sessions: HashMap<String, Session>,
    /// Sessions the agent listed, with their validated cwd.
    pub listed: HashMap<String, String>,
    /// Pending permissions by host handle.
    pub permissions: HashMap<String, Permission>,
    resolved: VecDeque<String>,
    recent: VecDeque<Recent>,
    transcripts: HashMap<String, (u64, Transcript)>,
    /// Sessions whose retained transcript was evicted: a transcript started
    /// for them later is missing that history.
    lost: HashSet<String>,
    lost_order: VecDeque<String>,
    /// Event log.
    pub log: EventLog,
    clock: u64,
}

impl State {
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    /// Append an event of the current generation.
    pub fn event(&mut self, body: Value) -> u64 {
        let generation = self.generation;
        self.log.push(generation, body)
    }

    /// Emit the current phase.
    pub fn host_event(&mut self) {
        let body = json!({
            "type": "host_state",
            "phase": self.phase,
            "authRequired": self.auth_required,
        });
        self.event(body);
    }

    /// Start a new generation: forget sessions and replay history of the
    /// old one. Returns the new generation.
    pub fn begin_generation(&mut self) -> u64 {
        self.generation += 1;
        self.phase = Phase::Starting;
        self.negotiated = None;
        self.peer = None;
        self.sessions.clear();
        self.listed.clear();
        self.log.clear();
        self.host_event();
        self.generation
    }

    /// Admit a session, evicting the least recently used idle one when the
    /// table is full. Returns the evicted session id. A `pending` session is
    /// provisional until [`State::confirm`].
    pub fn admit(
        &mut self,
        id: &str,
        cwd: &str,
        settings: SessionSettings,
        pending: bool,
        replaying: bool,
    ) -> Option<String> {
        let touched = self.tick();
        let mut evicted = None;
        if !self.sessions.contains_key(id) && self.sessions.len() >= MAX_SESSIONS {
            evicted = self
                .sessions
                .iter()
                .filter(|(_, session)| session.active.is_none() && !session.pending)
                .min_by_key(|(_, session)| session.touched)
                .map(|(id, _)| id.clone());
            if let Some(old) = &evicted {
                self.sessions.remove(old);
            }
        }
        self.sessions.insert(id.to_string(), Session {
            cwd: cwd.to_string(),
            settings,
            title: None,
            title_reported: false,
            usage: None,
            active: None,
            pending,
            replaying,
            touched,
        });
        evicted
    }

    /// The agent confirmed a pending reopen: the session becomes usable.
    /// `false` when it is no longer pending here.
    pub fn confirm(&mut self, id: &str, settings: SessionSettings) -> bool {
        match self.sessions.get_mut(id) {
            Some(session) if session.pending => {
                session.pending = false;
                session.replaying = false;
                session.settings = settings;
                true
            },
            _ => false,
        }
    }

    /// A pending reopen failed: revoke the provisional admission, and the
    /// partial replay with it.
    pub fn revoke(&mut self, id: &str) {
        let pending = self.sessions.get(id).is_some_and(|session| session.pending);
        if !pending {
            return;
        }
        let replaying = self
            .sessions
            .get(id)
            .is_some_and(|session| session.replaying);
        self.sessions.remove(id);
        if replaying {
            self.transcripts.remove(id);
        }
    }

    /// The working directory of an admitted, confirmed session.
    pub fn authorized_cwd(&self, id: &str) -> Option<&str> {
        self.sessions
            .get(id)
            .filter(|session| !session.pending)
            .map(|session| session.cwd.as_str())
    }

    /// Mark a session used.
    pub fn touch(&mut self, id: &str) {
        let now = self.tick();
        if let Some(session) = self.sessions.get_mut(id) {
            session.touched = now;
        }
    }

    fn remember_lost(&mut self, id: &str) {
        if self.lost.insert(id.to_string()) {
            self.lost_order.push_back(id.to_string());
            while self.lost_order.len() > MAX_LOST {
                if let Some(old) = self.lost_order.pop_front() {
                    self.lost.remove(&old);
                }
            }
        }
    }

    /// The least recently used transcript no running or pending session
    /// needs: the one to evict for room.
    fn idle_victim(&self) -> Option<String> {
        self.transcripts
            .iter()
            .filter(|(key, _)| {
                self.sessions
                    .get(*key)
                    .is_none_or(|s| s.active.is_none() && !s.pending)
            })
            .min_by_key(|(_, (touched, _))| *touched)
            .map(|(key, _)| key.clone())
    }

    /// Whether `id` has (or could get) a retained transcript: at most
    /// [`MAX_TRANSCRIPTS`] are ever held, and a running or pending
    /// session's transcript is never evicted for another.
    pub fn transcript_room(&self, id: &str) -> bool {
        self.transcripts.contains_key(id)
            || self.transcripts.len() < MAX_TRANSCRIPTS
            || self.idle_victim().is_some()
    }

    /// Make sure `id` has a retained transcript, evicting the least recently
    /// used idle one when all [`MAX_TRANSCRIPTS`] are taken. A new one is
    /// `truncated` when earlier history is missing (the caller knows so, or
    /// this host evicted it before). `false` — and nothing is created or
    /// evicted — when every retained transcript belongs to a running or
    /// pending session.
    pub fn reserve_transcript(&mut self, id: &str, missing_history: bool) -> bool {
        let now = self.tick();
        if let Some(entry) = self.transcripts.get_mut(id) {
            entry.0 = now;
            return true;
        }
        if self.transcripts.len() >= MAX_TRANSCRIPTS {
            let Some(victim) = self.idle_victim() else { return false };
            self.transcripts.remove(&victim);
            self.remember_lost(&victim);
        }
        let missing = missing_history || self.lost.contains(id);
        self.transcripts
            .insert(id.to_string(), (now, Transcript::new(missing)));
        true
    }

    /// The retained transcript of `id`, marked used.
    pub fn transcript_mut(&mut self, id: &str) -> Option<&mut Transcript> {
        let now = self.tick();
        let entry = self.transcripts.get_mut(id)?;
        entry.0 = now;
        Some(&mut entry.1)
    }

    /// Number of retained transcripts.
    pub fn transcript_count(&self) -> usize {
        self.transcripts.len()
    }

    /// Replace a transcript with an empty one (before `session/load` replay,
    /// which brings the complete history back). `false` when there is no
    /// room (see [`State::reserve_transcript`]); the old one is then kept.
    pub fn reset_transcript(&mut self, id: &str) -> bool {
        if !self.transcript_room(id) {
            return false;
        }
        self.transcripts.remove(id);
        if self.lost.remove(id) {
            self.lost_order.retain(|lost| lost != id);
        }
        self.reserve_transcript(id, false)
    }

    /// The retained transcript of `id`, read-only.
    pub fn transcript_ref(&self, id: &str) -> Option<&Transcript> {
        self.transcripts.get(id).map(|(_, transcript)| transcript)
    }

    /// Whether `handle` was answered recently.
    pub fn was_resolved(&self, handle: &str) -> bool {
        self.resolved.iter().any(|done| done == handle)
    }

    /// Remove a pending permission and emit its resolution.
    pub fn resolve(&mut self, handle: &str, outcome: &str) -> Option<Permission> {
        let permission = self.permissions.remove(handle)?;
        self.resolved.push_back(handle.to_string());
        while self.resolved.len() > MAX_RECENT {
            self.resolved.pop_front();
        }
        let body = json!({
            "type": "permission_resolved",
            "handle": handle,
            "sessionId": permission.session,
            "outcome": outcome,
        });
        self.event(body);
        Some(permission)
    }

    /// End `turn` of `session` with `outcome` exactly once. Returns the
    /// permissions it still had pending (to be answered `cancelled`), or
    /// `None` when the turn was not running (already ended).
    pub fn end_turn(
        &mut self,
        session: &str,
        turn: &str,
        outcome: TurnOutcome,
    ) -> Option<Vec<Permission>> {
        let running = self
            .sessions
            .get(session)
            .and_then(|s| s.active.as_ref())
            .is_some_and(|active| active.id == turn);
        if !running {
            return None;
        }
        if let Some(entry) = self.sessions.get_mut(session) {
            entry.active = None;
        }
        if let Some(transcript) = self.transcript_mut(session) {
            transcript.end_turn(turn, outcome.clone());
        }
        let handles: Vec<String> = self
            .permissions
            .iter()
            .filter(|(_, p)| p.session == session && p.turn == turn)
            .map(|(handle, _)| handle.clone())
            .collect();
        let leftovers = handles
            .iter()
            .filter_map(|handle| self.resolve(handle, "cancelled"))
            .collect();
        self.recent.push_back(Recent {
            session: session.to_string(),
            turn: turn.to_string(),
            outcome: outcome.clone(),
        });
        while self.recent.len() > MAX_RECENT {
            self.recent.pop_front();
        }
        let body = json!({
            "type": "turn_ended",
            "sessionId": session,
            "turnId": turn,
            "outcome": outcome,
        });
        self.event(body);
        Some(leftovers)
    }

    /// Recorded outcomes of `turn` (exactly one for an ended turn).
    pub fn outcomes_of(&self, session: &str, turn: &str) -> Vec<&TurnOutcome> {
        self.recent
            .iter()
            .filter(|r| r.session == session && r.turn == turn)
            .map(|r| &r.outcome)
            .collect()
    }

    /// The running generation ended (process exit or stop): end every turn
    /// as `AgentExited`, withdraw every permission, forget admitted sessions
    /// and move to the next generation with `phase`. Returns `false` when
    /// `generation` was no longer current.
    pub fn invalidate(&mut self, generation: u64, phase: Phase) -> bool {
        if self.generation != generation {
            return false;
        }
        let running: Vec<(String, String)> = self
            .sessions
            .iter()
            .filter_map(|(id, s)| s.active.as_ref().map(|turn| (id.clone(), turn.id.clone())))
            .collect();
        for (session, turn) in running {
            self.end_turn(&session, &turn, TurnOutcome::AgentExited);
        }
        let handles: Vec<String> = self.permissions.keys().cloned().collect();
        for handle in handles {
            self.resolve(&handle, "agent_exited");
        }
        // A replay cut short is not a usable history.
        let partial: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.replaying)
            .map(|(id, _)| id.clone())
            .collect();
        for id in partial {
            self.transcripts.remove(&id);
        }
        self.generation += 1;
        self.phase = phase;
        self.negotiated = None;
        self.peer = None;
        self.sessions.clear();
        self.listed.clear();
        self.log.clear();
        self.host_event();
        true
    }

    /// Remember an agent-listed session directory.
    pub fn remember_listed(&mut self, id: &str, cwd: &str) {
        if self.listed.len() >= MAX_LISTED && !self.listed.contains_key(id) {
            self.listed.clear();
        }
        self.listed.insert(id.to_string(), cwd.to_string());
    }

    /// The session-state event body of `id`.
    pub fn session_body(&self, id: &str) -> Value {
        let Some(session) = self.sessions.get(id) else {
            return json!({"type": "session_state", "sessionId": id, "admitted": false});
        };
        json!({
            "type": "session_state",
            "sessionId": id,
            "admitted": true,
            "cwd": session.cwd,
            "pending": session.pending,
            "configOptions": session.settings.config_options,
            "modes": session.settings.modes,
            "title": session.title,
            "usage": session.usage.map(|(used, size)| json!({"used": used, "size": size})),
            "runningTurnId": session.active.as_ref().map(|t| t.id.clone()),
            "turnDispatched": session.active.as_ref().is_some_and(|t| t.dispatched),
            "cancelRequested": session.active.as_ref().is_some_and(|t| t.cancel_requested),
        })
    }

    /// Emit the state of session `id`.
    pub fn session_event(&mut self, id: &str) {
        let body = self.session_body(id);
        self.event(body);
    }

    /// The permission event body of `handle`.
    pub fn permission_body(handle: &str, permission: &Permission) -> Value {
        json!({
            "type": "permission",
            "handle": handle,
            "sessionId": permission.session,
            "turnId": permission.turn,
            "toolCall": permission.tool_call,
            "options": permission.options,
        })
    }

    /// Everything a controller needs to resynchronize, with the sequence
    /// watermark the event stream continues from. Transcripts are not
    /// included: the controller reads the turns it needs through history,
    /// whose answer carries its own watermark.
    pub fn snapshot(&self, info: Value) -> Value {
        let mut ids: Vec<&String> = self.sessions.keys().collect();
        ids.sort();
        let sessions: Vec<Value> = ids.into_iter().map(|id| self.session_body(id)).collect();
        let permissions: Vec<Value> = self
            .permissions
            .iter()
            .map(|(handle, permission)| Self::permission_body(handle, permission))
            .collect();
        let recent: Vec<Value> = self
            .recent
            .iter()
            .map(|r| json!({"sessionId": r.session, "turnId": r.turn, "outcome": r.outcome}))
            .collect();
        json!({
            "info": info,
            "generation": self.generation,
            "seq": self.log.last(),
            "sessions": sessions,
            "permissions": permissions,
            "recentTurns": recent,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_session(state: &mut State) {
        state.begin_generation();
        state.phase = Phase::Ready;
        state.admit("s", "/w", SessionSettings::default(), false, false);
    }

    #[test]
    fn replay_resumes_after_the_watermark_and_detects_gaps() {
        let mut log = EventLog::default();
        for n in 0..3 {
            log.push(1, json!({"n": n}));
        }
        assert_eq!(log.since(1).expect("retained").len(), 2);
        assert!(log.since(3).expect("caught up").is_empty());
        assert!(log.since(4).is_none(), "future watermark");
        for n in 0..MAX_LOG_EVENTS {
            log.push(1, json!({"n": n}));
        }
        assert!(log.since(0).is_none(), "evicted events force a resync");
    }

    #[test]
    fn a_turn_ends_exactly_once_and_takes_its_permissions_with_it() {
        let mut state = State::default();
        ready_session(&mut state);
        state.sessions.get_mut("s").expect("session").active = Some(ActiveTurn::new("t".into()));
        state.permissions.insert("h".into(), Permission {
            session: "s".into(),
            turn: "t".into(),
            acp_id: RequestId::Number(4),
            options: Vec::new(),
            tool_call: json!({}),
        });
        let stop = TurnOutcome::Stopped {
            stop_reason: "end_turn".into(),
        };
        let leftovers = state.end_turn("s", "t", stop.clone()).expect("ended");
        assert_eq!(leftovers.len(), 1);
        assert!(state.was_resolved("h"));
        assert!(state.end_turn("s", "t", TurnOutcome::AgentExited).is_none(), "no second outcome");
        assert_eq!(state.outcomes_of("s", "t").len(), 1);
        let snapshot = state.snapshot(json!({}));
        assert_eq!(snapshot["recentTurns"][0]["outcome"]["kind"], "stopped");
    }

    #[test]
    fn invalidation_reports_real_outcomes_and_bumps_the_generation() {
        let mut state = State::default();
        ready_session(&mut state);
        let generation = state.generation;
        let mut turn = ActiveTurn::new("t".into());
        turn.cancel_requested = true;
        state.sessions.get_mut("s").expect("session").active = Some(turn);
        assert!(state.invalidate(generation, Phase::Failed {
            reason: "the agent exited".into()
        }));
        assert!(!state.invalidate(generation, Phase::Stopped), "stale generation");
        assert_eq!(state.generation, generation + 1);
        assert!(state.sessions.is_empty());
        let snapshot = state.snapshot(json!({}));
        assert_eq!(snapshot["recentTurns"][0]["outcome"]["kind"], "agentExited");
        assert_eq!(snapshot["generation"], generation + 1);
    }

    #[test]
    fn full_tables_evict_idle_sessions_only() {
        let mut state = State::default();
        state.begin_generation();
        for n in 0..MAX_SESSIONS {
            state.admit(&format!("s{n}"), "/w", SessionSettings::default(), false, false);
        }
        state.sessions.get_mut("s0").expect("s0").active = Some(ActiveTurn::new("t".into()));
        let evicted = state.admit("new", "/w", SessionSettings::default(), false, false);
        assert_eq!(evicted.as_deref(), Some("s1"));
        assert!(state.sessions.contains_key("s0"));
        assert_eq!(state.sessions.len(), MAX_SESSIONS);
    }

    #[test]
    fn pending_reopens_grant_no_authority_and_failure_revokes_them() {
        let mut state = State::default();
        state.begin_generation();
        state.admit("s", "/b", SessionSettings::default(), true, true);
        assert!(state.reset_transcript("s"));
        begin(&mut state, "s", "replay");
        assert_eq!(state.authorized_cwd("s"), None, "provisional");
        state.revoke("s");
        assert!(!state.sessions.contains_key("s"));
        assert!(state.transcript_ref("s").is_none(), "partial replay dropped");

        state.admit("r", "/a", SessionSettings::default(), true, false);
        assert_eq!(state.authorized_cwd("r"), None);
        assert!(state.confirm("r", SessionSettings::default()));
        assert_eq!(state.authorized_cwd("r"), Some("/a"));
        assert!(!state.confirm("r", SessionSettings::default()), "only once");
    }

    #[test]
    fn an_evicted_transcript_is_never_restarted_as_complete() {
        let mut state = State::default();
        state.begin_generation();
        for n in 0..=MAX_TRANSCRIPTS {
            let id = format!("s{n}");
            state.admit(&id, "/w", SessionSettings::default(), false, false);
            assert!(state.reserve_transcript(&id, false));
            begin(&mut state, &id, "t");
        }
        // `s0` was the least recently used and has been evicted.
        assert!(state.reserve_transcript("s0", false));
        let revisited = state.transcript_ref("s0").expect("recreated");
        assert!(revisited.truncated, "the earlier history is missing");
        assert!(revisited.is_empty());
        // A full replay restores completeness.
        assert!(state.reset_transcript("s0"));
        assert!(!state.transcript_ref("s0").expect("transcript").truncated);
    }

    fn begin(state: &mut State, session: &str, turn: &str) {
        state
            .transcript_mut(session)
            .expect("reserved")
            .begin_turn(turn, "q", &[]);
    }

    /// Every retained transcript belongs to a running turn: another session
    /// gets none (no 33rd transcript, and nothing busy is evicted), and an
    /// idle one frees room again.
    #[test]
    fn transcripts_never_exceed_the_pool_when_every_session_is_busy() {
        let mut state = State::default();
        state.begin_generation();
        for n in 0..MAX_TRANSCRIPTS {
            let id = format!("busy{n}");
            state.admit(&id, "/w", SessionSettings::default(), false, false);
            assert!(state.reserve_transcript(&id, false));
            begin(&mut state, &id, "t");
            state.sessions.get_mut(&id).expect("session").active =
                Some(ActiveTurn::new("t".into()));
        }
        let bytes: usize = (0..MAX_TRANSCRIPTS)
            .filter_map(|n| state.transcript_ref(&format!("busy{n}")))
            .map(Transcript::retained_bytes)
            .sum();
        state.admit("extra", "/w", SessionSettings::default(), false, false);
        assert!(!state.transcript_room("extra"));
        assert!(!state.reserve_transcript("extra", false), "refused, not over the pool");
        assert!(!state.reset_transcript("extra"));
        assert_eq!(state.transcript_count(), MAX_TRANSCRIPTS);
        assert!(state.transcript_ref("extra").is_none());
        let after: usize = (0..MAX_TRANSCRIPTS)
            .filter_map(|n| state.transcript_ref(&format!("busy{n}")))
            .map(Transcript::retained_bytes)
            .sum();
        assert_eq!(after, bytes, "no busy history was dropped to make room");
        // One turn ends: its transcript becomes the eviction candidate.
        state.end_turn("busy7", "t", TurnOutcome::AgentExited);
        assert!(state.reserve_transcript("extra", false));
        assert_eq!(state.transcript_count(), MAX_TRANSCRIPTS);
        assert!(state.transcript_ref("busy7").is_none());
        assert!(state.transcript_ref("busy0").is_some(), "busy transcripts stay");
    }
}
