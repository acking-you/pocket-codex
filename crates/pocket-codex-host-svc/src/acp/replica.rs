//! A controller's replica of the host's live turn folds.
//!
//! The controller folds the gateway's `update` events with the host's own
//! fold code, so its items carry the host's ids. That only holds when the
//! replica starts from the host's state: a controller that joins in the
//! middle of a turn, or that lost events (a `reset` after replay overflow),
//! first installs the turn read through history — whose answer carries the
//! event watermark it reflects — and then applies only the events after
//! that watermark. Events at or before it are already part of the installed
//! fold and are skipped, so nothing is duplicated or lost.
//!
//! Memory stays bounded: each session's fold is a bounded [`Transcript`],
//! and only the sessions the caller keeps are retained.

use std::collections::HashMap;

use serde_json::Value;

use super::fold::{Change, Transcript, Turn};

#[derive(Debug, Default)]
struct Fold {
    transcript: Transcript,
    /// Watermark of the installed state: events up to it are reflected.
    seq: u64,
}

/// Live folds of a controller connection, per session.
#[derive(Debug, Default)]
pub struct Replica {
    sessions: HashMap<String, Fold>,
}

impl Replica {
    /// Forget everything (a different host generation or incarnation).
    pub fn clear(&mut self) {
        self.sessions.clear();
    }

    /// Forget one session.
    pub fn remove(&mut self, session: &str) {
        self.sessions.remove(session);
    }

    /// Number of sessions with a fold.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Whether no session has a fold.
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Keep at most `max` folds, dropping ones `keep` does not protect.
    pub fn bound(&mut self, max: usize, keep: impl Fn(&str) -> bool) {
        if self.sessions.len() <= max {
            return;
        }
        let victims: Vec<String> = self
            .sessions
            .keys()
            .filter(|session| !keep(session))
            .take(self.sessions.len() - max)
            .cloned()
            .collect();
        for victim in victims {
            self.sessions.remove(&victim);
        }
    }

    /// Whether an event of `turn` in `session` needs the host's fold first:
    /// the replica has never seen that turn begin.
    pub fn needs_fold(&self, session: &str, turn: Option<&str>) -> bool {
        let Some(turn) = turn else { return false };
        self.sessions
            .get(session)
            .is_none_or(|fold| fold.transcript.turn(turn).is_none())
    }

    /// Install the turns of a history answer (`window`, `truncated`, `seq`)
    /// as this session's fold. `false` when the answer has no turns.
    pub fn install(&mut self, session: &str, history: &Value) -> bool {
        let turns: Vec<Turn> = history["window"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|turn| serde_json::from_value(turn.clone()).ok())
            .collect();
        if turns.is_empty() {
            return false;
        }
        self.sessions.insert(session.to_string(), Fold {
            transcript: Transcript::from_turns(turns, history["truncated"] == true),
            seq: history["seq"].as_u64().unwrap_or(0),
        });
        true
    }

    /// Fold one gateway event body (`turn_started` / `update`); other kinds
    /// change nothing. Events the installed fold already reflects return no
    /// changes.
    ///
    /// An update of a turn this replica never saw begin (and never
    /// installed) changes nothing: folding it alone would start the turn
    /// without its prefix and with the wrong ordinals. The turn stays
    /// [`Replica::needs_fold`] until the host's fold is installed.
    ///
    /// The same holds for a `turn_started` whose prompt carried images: the
    /// event names only their count (image data is never streamed in
    /// events), and a prompt folded without them would weigh less than the
    /// host's, so the two would evict — and number — later items
    /// differently. Such a turn is begun only from the host's fold.
    pub fn apply(&mut self, body: &Value) -> Vec<Change> {
        let Some(session) = body["sessionId"].as_str() else { return Vec::new() };
        let seq = body["seq"].as_u64().unwrap_or(0);
        if self
            .sessions
            .get(session)
            .is_some_and(|fold| fold.seq > 0 && seq <= fold.seq)
        {
            return Vec::new();
        }
        let turn = body["turnId"].as_str();
        match body["type"].as_str() {
            Some("turn_started") if body["imageCount"].as_u64().unwrap_or(0) > 0 => Vec::new(),
            Some("turn_started") => match turn {
                Some(turn) => self
                    .sessions
                    .entry(session.to_string())
                    .or_default()
                    .transcript
                    .begin_turn(turn, body["userText"].as_str().unwrap_or(""), &[]),
                None => Vec::new(),
            },
            Some("update") => {
                if self.needs_fold(session, turn) {
                    return Vec::new();
                }
                match self.sessions.get_mut(session) {
                    Some(fold) => fold.transcript.apply(turn, &body["update"]),
                    None => Vec::new(),
                }
            },
            _ => Vec::new(),
        }
    }

    /// The folded `turn` of `session`.
    pub fn turn(&self, session: &str, turn: &str) -> Option<&Turn> {
        self.sessions.get(session)?.transcript.turn(turn)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn chunk(text: &str) -> Value {
        json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}})
    }

    /// Host events for a turn: begin, then one update per chunk.
    fn host_turn(chunks: &[&str]) -> (Transcript, Vec<Value>) {
        let mut host = Transcript::default();
        let mut events = vec![json!({"type": "turn_started", "sessionId": "s", "turnId": "t",
            "userText": "q", "seq": 1})];
        host.begin_turn("t", "q", &[]);
        for (n, text) in chunks.iter().enumerate() {
            let update = chunk(text);
            host.apply(Some("t"), &update);
            events.push(json!({"type": "update", "sessionId": "s", "turnId": "t",
                "update": update, "seq": n as u64 + 2}));
        }
        (host, events)
    }

    #[test]
    fn joining_mid_turn_continues_with_the_host_ids_and_text() {
        let chunks = ["He", "llo", " wor", "ld"];
        let (host, events) = host_turn(&chunks);
        // The host's state after the first two updates (seq 3), as history.
        let (partial, _) = host_turn(&chunks[..2]);
        let history = json!({"window": [partial.turns[0]], "truncated": false, "seq": 3});
        let mut replica = Replica::default();
        assert!(replica.needs_fold("s", Some("t")));
        assert!(replica.install("s", &history));
        assert!(!replica.needs_fold("s", Some("t")));
        // The stream replays from an older watermark: reflected events skip.
        for event in &events {
            replica.apply(event);
        }
        assert_eq!(replica.turn("s", "t"), host.turns.front());
        let turn = replica.turn("s", "t").expect("turn");
        assert_eq!(turn.items[0].id, "t:u", "the prompt is kept");
        assert_eq!(turn.items[1].id, "t:m1");
        assert_eq!(turn.items[1].text, "Hello world");
    }

    #[test]
    fn an_unseen_turn_is_never_started_from_a_bare_update() {
        let chunks = ["He", "llo", " there"];
        let (host, events) = host_turn(&chunks);
        let mut replica = Replica::default();
        // Joined after the turn began, and history could not be read yet.
        assert!(replica.apply(&events[2]).is_empty());
        assert!(replica.needs_fold("s", Some("t")), "still unsynchronized");
        assert!(replica.turn("s", "t").is_none(), "no partial fold");
        // History becomes readable: install, then the rest of the stream.
        let (partial, _) = host_turn(&chunks[..2]);
        assert!(replica.install("s", &json!({"window": [partial.turns[0]], "seq": 3})));
        for event in &events {
            replica.apply(event);
        }
        assert_eq!(replica.turn("s", "t"), host.turns.front());
    }

    /// A prompt with a large image, then tool calls that push the turn past
    /// the transcript budget: the host evicts early tools *because of the
    /// image's weight*, then an evicted tool is updated and a message
    /// follows. A replica that began the turn from `imageCount` alone would
    /// keep those tools and number later items differently; this one waits
    /// for the host's fold and then agrees exactly.
    #[test]
    fn an_image_prompt_is_begun_from_the_host_fold_so_eviction_and_ids_agree() {
        let image = format!("data:image/png;base64,{}", "A".repeat(1536 * 1024));
        let mut host = Transcript::default();
        host.begin_turn("t", "q", std::slice::from_ref(&image));
        // The event the host emits for this turn (see `AgentHost::prompt`).
        let started = json!({"type": "turn_started", "sessionId": "s", "turnId": "t",
            "userText": "q", "imageCount": 1, "seq": 1});
        let mut replica = Replica::default();
        assert!(replica.apply(&started).is_empty());
        assert!(replica.needs_fold("s", Some("t")), "an image prompt waits for the host's fold");
        // What the history read right after `turn_started` answers.
        let history = json!({"window": [host.turns[0]], "truncated": false, "seq": 1});
        assert!(replica.install("s", &history));
        assert_eq!(replica.turn("s", "t"), host.turns.front(), "the prompt with its image");

        let mut updates = Vec::new();
        for n in 0..120 {
            updates.push(json!({"sessionUpdate": "tool_call", "toolCallId": format!("call-{n}"),
                "title": "Read", "kind": "read", "status": "pending",
                "rawInput": {"blob": "r".repeat(60 * 1024)}}));
        }
        updates.push(json!({"sessionUpdate": "tool_call_update", "toolCallId": "call-0",
            "status": "completed"}));
        updates.push(chunk("after the tools"));
        for (n, update) in updates.iter().enumerate() {
            host.apply(Some("t"), update);
            replica.apply(&json!({"type": "update", "sessionId": "s", "turnId": "t",
                "update": update, "seq": n as u64 + 2}));
        }
        let authoritative = host.turns.front().expect("host turn");
        assert!(authoritative.omitted > 0, "the image's weight made the host evict tools");
        assert_eq!(replica.turn("s", "t"), Some(authoritative), "same items, ids and omissions");
        let ids = |turn: &Turn| turn.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(replica.turn("s", "t").expect("turn")), ids(authoritative));
    }

    #[test]
    fn a_fresh_turn_needs_no_history() {
        let (host, events) = host_turn(&["a", "b"]);
        let mut replica = Replica::default();
        for event in &events {
            replica.apply(event);
        }
        assert_eq!(replica.turn("s", "t"), host.turns.front());
    }
}
