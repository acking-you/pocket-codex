//! Per-session hub state (TRD §4.2.4).

use std::collections::{HashSet, VecDeque};

use pocket_codex_core::acp::{
    pcx::{QueuedInfo, SubmitResult},
    AvailableCommand, ConfigOption, ContentBlock, SessionModeState, Transcript, UsageUpdate,
};
use tokio::{sync::watch, time::Instant};

/// Submissions remembered for idempotency.
const SUBMISSION_LOG_LEN: usize = 256;
/// How long a submission stays remembered.
const SUBMISSION_LOG_TTL: std::time::Duration = std::time::Duration::from_secs(600);
/// Length of queue previews.
const PREVIEW_CHARS: usize = 100;

/// Lifecycle phase of a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Known from a listing; not loaded in the agent.
    Listed,
    /// `session/load` or `session/resume` is running.
    Loading,
    /// Loaded and idle.
    Idle,
    /// A prompt is running.
    Running {
        /// Turn number.
        turn: u32,
        /// Hub submission id.
        submission: String,
    },
    /// `session/close` is running.
    Closing,
}

/// One queued prompt.
#[derive(Clone, Debug, PartialEq)]
pub struct Queued {
    /// Hub submission id.
    pub submission: String,
    /// Controller idempotency key.
    pub client_submission: String,
    /// Prompt content.
    pub prompt: Vec<ContentBlock>,
    /// When it was queued.
    pub queued_at: Instant,
}

impl Queued {
    /// Queue entry shown to controllers.
    pub fn info(&self) -> QueuedInfo {
        QueuedInfo {
            submission_id: self.submission.clone(),
            text_preview: prompt_text(&self.prompt)
                .chars()
                .take(PREVIEW_CHARS)
                .collect(),
        }
    }
}

/// Concatenated text of a prompt.
pub fn prompt_text(prompt: &[ContentBlock]) -> String {
    prompt
        .iter()
        .filter_map(ContentBlock::as_text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Bounded FIFO of (client_submission_id, SubmitResult, recorded_at); entries
/// older than 10 minutes or beyond 256 are dropped on insert.
#[derive(Clone, Debug, Default)]
pub struct SubmissionLog(VecDeque<(String, SubmitResult, Instant)>);

impl SubmissionLog {
    /// A remembered result for `client_submission`.
    pub fn get(&self, client_submission: &str) -> Option<SubmitResult> {
        let now = Instant::now();
        self.0
            .iter()
            .find(|(id, _, at)| id == client_submission && now - *at < SUBMISSION_LOG_TTL)
            .map(|(_, result, _)| result.clone())
    }

    /// Remember a result.
    pub fn insert(&mut self, client_submission: String, result: SubmitResult) {
        let now = Instant::now();
        self.0.retain(|(_, _, at)| now - *at < SUBMISSION_LOG_TTL);
        self.0.push_back((client_submission, result, now));
        while self.0.len() > SUBMISSION_LOG_LEN {
            self.0.pop_front();
        }
    }
}

/// Result of a load, shared with every caller waiting on it.
pub type LoadOutcome = Option<Result<(), super::error::AcpError>>;

/// Hub-side state of one agent session.
#[derive(Debug)]
pub struct HubSession {
    /// Session id.
    pub id: String,
    /// Working directory.
    pub cwd: String,
    /// Title.
    pub title: Option<String>,
    /// Last update reported by the agent (RFC 3339).
    pub updated_at: Option<String>,
    /// Phase.
    pub phase: Phase,
    /// Loaded in the current agent process.
    pub agent_loaded: bool,
    /// Value of `updated_at` when `transcript` was materialized.
    pub transcript_updated_at: Option<String>,
    /// `n` in the `{epoch}.{n}` generation; survives LRU eviction of
    /// `transcript`.
    pub generation_seq: u32,
    /// Last `seq` assigned to a broadcast session notification (T14).
    pub seq: u64,
    /// Folded transcript.
    pub transcript: Option<Transcript>,
    /// Transcript being replayed by a running `session/load`.
    pub replay: Option<Transcript>,
    /// Queued prompts.
    pub queue: VecDeque<Queued>,
    /// Connections subscribed to this session.
    pub subscribers: HashSet<u64>,
    /// Current config options.
    pub config_options: Vec<ConfigOption>,
    /// Current modes.
    pub modes: Option<SessionModeState>,
    /// Available slash commands.
    pub commands: Vec<AvailableCommand>,
    /// Last usage update.
    pub usage: Option<UsageUpdate>,
    /// Last attach, submit or update.
    pub last_activity: Instant,
    /// Last time a controller read the transcript (LRU).
    pub last_access: Instant,
    /// clientSubmissionId → result; 256 entries, 10 minutes.
    pub submissions: SubmissionLog,
    /// Waiters of the running load.
    pub load_rx: Option<watch::Receiver<LoadOutcome>>,
    /// A live turn ended; the next listing becomes the `updatedAt` baseline.
    pub baseline_pending: bool,
    /// A transcript was materialized at least once (re-materializing bumps
    /// `generation_seq`).
    pub materialized: bool,
}

impl HubSession {
    /// A session known only from a listing.
    pub fn listed(id: String, cwd: String) -> Self {
        let now = Instant::now();
        Self {
            id,
            cwd,
            title: None,
            updated_at: None,
            phase: Phase::Listed,
            agent_loaded: false,
            transcript_updated_at: None,
            generation_seq: 0,
            seq: 0,
            transcript: None,
            replay: None,
            queue: VecDeque::new(),
            subscribers: HashSet::new(),
            config_options: Vec::new(),
            modes: None,
            commands: Vec::new(),
            usage: None,
            last_activity: now,
            last_access: now,
            submissions: SubmissionLog::default(),
            load_rx: None,
            baseline_pending: false,
            materialized: false,
        }
    }

    /// A turn is running.
    pub fn running(&self) -> bool {
        matches!(self.phase, Phase::Running { .. })
    }

    /// The running turn.
    pub fn active_turn(&self) -> Option<u32> {
        match self.phase {
            Phase::Running {
                turn, ..
            } => Some(turn),
            _ => None,
        }
    }

    /// Next notification sequence number.
    pub fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    /// Mark activity now.
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
        self.last_access = self.last_activity;
    }
}
