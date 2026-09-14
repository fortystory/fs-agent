//! The append-only event stream: schema, envelope, and log.
//!
//! `events` sits at the bottom of the internal dependency DAG: it depends on no
//! other internal module, and every other module may depend on it. The event
//! log is the single source of truth for a session; an agent's `messages` is a
//! projection of it (see [`crate::provider::projection`]).
//!
//! Durability contract (spec §2): one JSONL file per session, a single writer,
//! one `flush` per line and **no** `fsync`, and a torn final line is tolerated.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Schema version recorded in [`EventPayload::SessionStarted`].
///
/// It is bumped whenever the payload shape changes; there is no promise of
/// backward compatibility across versions.
pub const SCHEMA_VERSION: u32 = 1;

/// Define an owned string identifier that serializes transparently.
///
/// Keeps the three identifier newtypes on one implementation, so they stay
/// consistent and none of them decays into a bare `String`.
macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }
    };
}

string_id! {
    /// Stable identifier of a participant that can act: a debater or an executor.
    ///
    /// The glossary forbids `agent` as a type name; the participant role lives
    /// in [`SpeakerId`].
    ParticipantId
}

string_id! {
    /// Identifier of a session. Never changes across `--continue`, so provider
    /// prefix caches keep hitting.
    SessionId
}

string_id! {
    /// Identifier of one tool call, unique within its session.
    ToolCallId
}

/// Who is speaking. Attribution is never inferred from a provider `role`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SpeakerId {
    Debater(ParticipantId),
    Executor(ParticipantId),
    User,
    System,
}

impl fmt::Display for SpeakerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpeakerId::Debater(id) => write!(f, "{id}"),
            SpeakerId::Executor(id) => write!(f, "executor:{id}"),
            SpeakerId::User => f.write_str("user"),
            SpeakerId::System => f.write_str("system"),
        }
    }
}

/// Provider-level role carried by a completed message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
}

/// Why a loop stopped. One enum is shared by the turn loop, the discussion
/// round loop, the session, and executors; nested loops each leave their own
/// reason behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    // The five single-loop values.
    Completed,
    MaxIterations,
    Aborted,
    MistakeLimit,
    Error,
    // The three discussion values.
    Consensus,
    NoDivergence,
    RoundsExhausted,
    // The one budget value.
    BudgetExhausted,
}

impl StopReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            StopReason::Completed => "Completed",
            StopReason::MaxIterations => "MaxIterations",
            StopReason::Aborted => "Aborted",
            StopReason::MistakeLimit => "MistakeLimit",
            StopReason::Error => "Error",
            StopReason::Consensus => "Consensus",
            StopReason::NoDivergence => "NoDivergence",
            StopReason::RoundsExhausted => "RoundsExhausted",
            StopReason::BudgetExhausted => "BudgetExhausted",
        }
    }
}

impl fmt::Display for StopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Token accounting normalized across providers.
///
/// `cached_tokens` and `miss_tokens` are first-class: they are the only way to
/// tell whether prefix caching is doing anything.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub miss_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
}

/// Where a pinned injection came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextSource {
    AgentsMd,
    SkillsCatalog,
    PlanMode,
}

/// The mode a discussion round was run in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoundMode {
    Independent,
    Targeted,
    Synthesis,
}

/// Why a history range stopped being authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HistoryReason {
    Regenerate,
    Undo,
    Compaction,
}

/// The closed three-state permission verdict.
///
/// The variant order **is** the decision lattice: `Allow < Ask < Deny`. Both the
/// permission gate's rules (spec §12) and a pre-hook's tightening (spec §3)
/// merge by taking the supremum on this order, so there is exactly one merge
/// semantic in the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Decision {
    Allow,
    Ask,
    Deny,
}

impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Ask => "ask",
            Decision::Deny => "deny",
        }
    }

    /// The supremum of two verdicts: the stricter one wins.
    pub fn join(self, other: Decision) -> Decision {
        self.max(other)
    }

    /// Whether this action travels down the delegation chain by default.
    ///
    /// `Deny` and `Ask` are constraints and inherit; `Allow` does not — so
    /// "inherit denials, never inherits allowances" is the default's natural
    /// result rather than a special case (spec §12).
    pub fn default_propagate(self) -> bool {
        !matches!(self, Decision::Allow)
    }
}

/// Where a permission verdict came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionSource {
    User,
    Hook,
    Policy,
}

/// One entry on the single event bus.
///
/// Consumers take what they need by filtering; there is no visibility field,
/// because visibility is an output of projection rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EventPayload {
    // Session skeleton.
    SessionStarted {
        session_id: SessionId,
        cwd: String,
        schema_version: u32,
    },
    ContextInjected {
        source: ContextSource,
        content: String,
    },
    SessionEnded {
        reason: StopReason,
    },
    // Discussion protocol (the slots; timing is decided by the protocol).
    RoundStarted {
        round: u32,
        mode: RoundMode,
    },
    RoundEnded {
        round: u32,
        reason: StopReason,
    },
    DivergenceRecorded {
        round: u32,
        topic: String,
        positions: Vec<String>,
    },
    // One agent Turn.
    TurnStarted {
        agent: SpeakerId,
        iteration: u32,
    },
    MessageCompleted {
        role: Role,
        text: String,
        reasoning: Option<String>,
    },
    ToolCallStarted {
        tool_call_id: ToolCallId,
        tool_name: String,
        args: serde_json::Value,
    },
    ToolCallCompleted {
        tool_call_id: ToolCallId,
        ok: bool,
        output: Option<String>,
        error: Option<String>,
        duration_ms: u64,
    },
    UsageRecorded {
        usage: Usage,
    },
    TurnEnded {
        reason: StopReason,
    },
    // Permissions.
    PermissionAsked {
        request_id: String,
        tool_call_id: ToolCallId,
        request: serde_json::Value,
    },
    PermissionDecided {
        request_id: String,
        decision: Decision,
        source: DecisionSource,
        reason: Option<String>,
    },
    // Hooks.
    HookExecuted {
        point: String,
        command: String,
        outcome: String,
    },
    // Executors.
    ExecutorSpawned {
        executor_id: ParticipantId,
        parent: ParticipantId,
        brief: String,
    },
    ExecutorFinished {
        executor_id: ParticipantId,
        reason: StopReason,
        summary: String,
    },
    // Errors: two kinds, because their visibility differs.
    AgentError {
        message: String,
        recoverable: bool,
    },
    SessionError {
        code: String,
        detail: String,
    },
    // History operations.
    HistorySuperseded {
        targets: Vec<u64>,
        reason: HistoryReason,
        summary: Option<String>,
    },
}

impl EventPayload {
    /// A short, stable name for diagnostics and assertions.
    pub fn kind(&self) -> &'static str {
        match self {
            EventPayload::SessionStarted { .. } => "SessionStarted",
            EventPayload::ContextInjected { .. } => "ContextInjected",
            EventPayload::SessionEnded { .. } => "SessionEnded",
            EventPayload::RoundStarted { .. } => "RoundStarted",
            EventPayload::RoundEnded { .. } => "RoundEnded",
            EventPayload::DivergenceRecorded { .. } => "DivergenceRecorded",
            EventPayload::TurnStarted { .. } => "TurnStarted",
            EventPayload::MessageCompleted { .. } => "MessageCompleted",
            EventPayload::ToolCallStarted { .. } => "ToolCallStarted",
            EventPayload::ToolCallCompleted { .. } => "ToolCallCompleted",
            EventPayload::UsageRecorded { .. } => "UsageRecorded",
            EventPayload::TurnEnded { .. } => "TurnEnded",
            EventPayload::PermissionAsked { .. } => "PermissionAsked",
            EventPayload::PermissionDecided { .. } => "PermissionDecided",
            EventPayload::HookExecuted { .. } => "HookExecuted",
            EventPayload::ExecutorSpawned { .. } => "ExecutorSpawned",
            EventPayload::ExecutorFinished { .. } => "ExecutorFinished",
            EventPayload::AgentError { .. } => "AgentError",
            EventPayload::SessionError { .. } => "SessionError",
            EventPayload::HistorySuperseded { .. } => "HistorySuperseded",
        }
    }
}

/// The envelope. `seq` is the only identity of an event: it is the JSONL line
/// number, so there is no second identity scheme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    pub at: DateTime<Utc>,
    pub speaker_id: SpeakerId,
    pub payload: EventPayload,
}

impl Event {
    /// Build an envelope for tests and in-memory construction. The log stamps
    /// its own `seq` and `at` when appending.
    pub fn new(seq: u64, speaker_id: SpeakerId, payload: EventPayload) -> Self {
        Self {
            seq,
            at: Utc::now(),
            speaker_id,
            payload,
        }
    }
}

/// Query: which `tool_call`s have no result yet?
///
/// Pending work is a query over the log, never hidden loop state. The turn loop
/// must not call the provider while this is non-empty.
pub fn pending_tool_calls(events: &[Event]) -> Vec<ToolCallId> {
    let mut pending: Vec<ToolCallId> = Vec::new();
    for event in events {
        match &event.payload {
            EventPayload::ToolCallStarted { tool_call_id, .. } => {
                if !pending.contains(tool_call_id) {
                    pending.push(tool_call_id.clone());
                }
            }
            EventPayload::ToolCallCompleted { tool_call_id, .. } => {
                pending.retain(|id| id != tool_call_id);
            }
            _ => {}
        }
    }
    pending
}

/// Query: did the acting speaker's most recent assistant message ask for a tool?
///
/// This is the loop's continuation predicate. `finish_reason` and `[DONE]` are
/// never consulted for control flow.
pub fn last_assistant_has_tool_calls(events: &[Event], speaker: &SpeakerId) -> bool {
    let mut has_tool_calls = false;
    for event in events {
        if &event.speaker_id != speaker {
            continue;
        }
        match &event.payload {
            EventPayload::MessageCompleted {
                role: Role::Assistant,
                ..
            } => {
                has_tool_calls = false;
            }
            EventPayload::ToolCallStarted { .. } => {
                has_tool_calls = true;
            }
            _ => {}
        }
    }
    has_tool_calls
}

/// Append-only JSONL log for one session.
///
/// The writer is owned and only [`EventLog::append`] takes `&mut self`, so
/// there is exactly one writer per file. In-memory events are a cache of the
/// file, kept in sync by the single writer.
#[derive(Debug)]
pub struct EventLog {
    path: PathBuf,
    writer: BufWriter<File>,
    events: Vec<Event>,
    next_seq: u64,
}

impl EventLog {
    /// Create a fresh log. Fails if the file already exists; the parent
    /// directory must exist.
    pub fn create(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&path)?;
        Ok(Self {
            path,
            writer: BufWriter::new(file),
            events: Vec::new(),
            next_seq: 1,
        })
    }

    /// Open an existing log for appending.
    ///
    /// A torn final line (a crash mid-write) is dropped, so `seq` keeps meaning
    /// "line number" and the next append starts on a whole line.
    pub fn open(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let events = read_events(&path)?;
        repair_before_append(&path)?;
        let next_seq = events.len() as u64 + 1;
        let file = OpenOptions::new().append(true).open(&path)?;
        Ok(Self {
            path,
            writer: BufWriter::new(file),
            events,
            next_seq,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// All events read so far, in order.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Append one event. Flushes the line but never `fsync`s.
    pub fn append(&mut self, speaker_id: SpeakerId, payload: EventPayload) -> io::Result<Event> {
        let event = Event {
            seq: self.next_seq,
            at: Utc::now(),
            speaker_id,
            payload,
        };
        let mut line =
            serde_json::to_string(&event).expect("Event payloads are always JSON-serializable");
        line.push('\n');
        self.writer.write_all(line.as_bytes())?;
        self.writer.flush()?;
        self.events.push(event.clone());
        self.next_seq += 1;
        Ok(event)
    }

    /// Flush buffered bytes to the OS. Not an `fsync`.
    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

/// Read every complete event from a JSONL log.
///
/// An unparsable **final** line is treated as a torn write and skipped; an
/// unparsable line anywhere else is corruption and returns an error.
pub fn read_events(path: impl AsRef<Path>) -> io::Result<Vec<Event>> {
    let path = path.as_ref();
    let file = File::open(path)?;
    let mut events = Vec::new();
    let mut lines = BufReader::new(file).lines().enumerate().peekable();
    while let Some((index, line)) = lines.next() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Event>(&line) {
            Ok(event) => events.push(event),
            Err(_) if lines.peek().is_none() => break,
            Err(error) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("event log {}: line {}: {error}", path.display(), index + 1),
                ));
            }
        }
    }
    Ok(events)
}

/// Make the tail of an existing log safe to append to.
///
/// A complete event that merely lacks its terminating newline is kept and
/// terminated; only an unparsable torn tail is dropped. Complete events are
/// never removed, so `seq` keeps meaning "line number" after a reopen.
fn repair_before_append(path: &Path) -> io::Result<()> {
    let bytes = std::fs::read(path)?;
    if bytes.is_empty() || bytes.ends_with(b"\n") {
        return Ok(());
    }
    let tail_start = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map(|position| position + 1)
        .unwrap_or(0);
    let tail = &bytes[tail_start..];

    if serde_json::from_slice::<Event>(tail).is_ok() {
        let mut file = OpenOptions::new().append(true).open(path)?;
        file.write_all(b"\n")?;
        file.flush()?;
    } else {
        let file = OpenOptions::new().write(true).open(path)?;
        file.set_len(tail_start as u64)?;
    }
    Ok(())
}
