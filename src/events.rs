//! The append-only event stream: schema, envelope, and log.
//!
//! `events` sits at the bottom of the internal dependency DAG: it depends on no
//! other internal module, and every other module may depend on it. The event
//! log is the single source of truth for a session; an agent's `messages` is a
//! projection of it (see [`crate::provider::projection`]).
//!
//! Durability contract (spec §2): one JSONL file per session, a single writer,
//! one `flush` per line and **no** `fsync`, and a torn final line is tolerated.

use std::collections::BTreeSet;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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

impl Usage {
    /// The tokens the session's allowance counts: input plus output.
    ///
    /// `cached_tokens` and `miss_tokens` are a **split** of `input_tokens`, so
    /// adding either on top would count the same prompt twice; a vendor counts
    /// reasoning tokens inside `output_tokens` already (spec §17).
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    /// Fold another record's counts into this one.
    ///
    /// `reasoning_tokens` stays `None` until some provider reports them, so a
    /// total never claims a reasoning count nothing measured.
    pub fn accumulate(&mut self, other: Usage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cached_tokens += other.cached_tokens;
        self.miss_tokens += other.miss_tokens;
        if let Some(tokens) = other.reasoning_tokens {
            *self.reasoning_tokens.get_or_insert(0) += tokens;
        }
    }
}

/// Where a pinned injection came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextSource {
    AgentsMd,
    SkillsCatalog,
    /// A skill body the **user** loaded with `/<skill>` (spec §9). It lands at
    /// the tail, unlike the pinned head injections: the model-side prefix never
    /// moves.
    Skill,
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
    /// The session's permission mode changed, so the instruction that described
    /// the old one no longer does (spec §13). Unlike the other three this
    /// retires harness content rather than an exchange.
    ModeChange,
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

    /// Redact every free-text field of this payload, in place.
    ///
    /// The exhaustive match is the point: the schema knows which of its fields
    /// are text a person or a model wrote, and a new payload variant cannot be
    /// added without deciding whether it carries any (spec §20). Identity and
    /// lookup fields (`session_id`, `cwd`, `tool_name`, `code`, ids) are left
    /// alone: they are the stream's keys, not prose, and a redacted key would
    /// break lookups rather than protect anything.
    ///
    /// Args and permission requests are JSON trees the model built, so they are
    /// walked leaf by leaf — a value pasted into a `write_file` argument is the
    /// same leak as one pasted into a message body.
    pub fn redact(&mut self, redactor: &Redactor) {
        match self {
            EventPayload::SessionStarted { .. }
            | EventPayload::SessionEnded { .. }
            | EventPayload::RoundStarted { .. }
            | EventPayload::RoundEnded { .. }
            | EventPayload::TurnStarted { .. }
            | EventPayload::UsageRecorded { .. }
            | EventPayload::TurnEnded { .. } => {}
            EventPayload::ContextInjected { content, .. } => redactor.redact(content),
            EventPayload::DivergenceRecorded {
                topic, positions, ..
            } => {
                redactor.redact(topic);
                for position in positions {
                    redactor.redact(position);
                }
            }
            EventPayload::MessageCompleted {
                text, reasoning, ..
            } => {
                redactor.redact(text);
                if let Some(reasoning) = reasoning {
                    redactor.redact(reasoning);
                }
            }
            EventPayload::ToolCallStarted { args, .. } => redactor.redact_value(args),
            EventPayload::ToolCallCompleted { output, error, .. } => {
                if let Some(output) = output {
                    redactor.redact(output);
                }
                if let Some(error) = error {
                    redactor.redact(error);
                }
            }
            EventPayload::PermissionAsked { request, .. } => redactor.redact_value(request),
            EventPayload::PermissionDecided { reason, .. } => {
                if let Some(reason) = reason {
                    redactor.redact(reason);
                }
            }
            EventPayload::HookExecuted {
                command, outcome, ..
            } => {
                redactor.redact(command);
                redactor.redact(outcome);
            }
            EventPayload::ExecutorSpawned { brief, .. } => redactor.redact(brief),
            EventPayload::ExecutorFinished { summary, .. } => redactor.redact(summary),
            EventPayload::AgentError { message, .. } => redactor.redact(message),
            EventPayload::SessionError { detail, .. } => redactor.redact(detail),
            EventPayload::HistorySuperseded { summary, .. } => {
                if let Some(summary) = summary {
                    redactor.redact(summary);
                }
            }
        }
    }
}

/// The marker a redacted value is replaced with.
///
/// Fixed text rather than a length-preserving mask: the point is that the value
/// is gone, and a marker that kept the original's length would invite reading
/// the shape of the secret back out of the stream.
pub const REDACTED: &str = "[redacted]";

/// Shortest value the redactor will act on.
///
/// Value-level redaction is a blunt instrument: replacing a three-character
/// string would rewrite ordinary prose everywhere it appeared and make the
/// session unreadable, while every vendor key this project holds is far longer
/// (spec §20 says best-effort, not exhaustive).
const MIN_SECRET_CHARS: usize = 8;

/// Value-level, best-effort secret redaction (spec §20).
///
/// The redactor holds the **values** that must not reach the stream — in
/// practice the resolved provider API keys — and replaces each occurrence with
/// [`REDACTED`]. It is applied before an event is appended, which is what makes
/// "the text on the stream equals the text the model saw" true, while the tool
/// that produced the text already ran on the real value.
///
/// It lives in `events` because the stream is at the bottom of the dependency
/// DAG and both `config` (which knows the keys) and `agent` (which is the one
/// writer) have to reach it.
///
/// What it does **not** do, on purpose: it does not guess at unregistered
/// secrets, does not decode encodings, and does not stitch a value back together
/// from fragments. The honest boundary is "the keys this process was configured
/// with" (see `docs/credentials.md`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Redactor {
    /// Longest first, so a value that starts with another value is replaced
    /// whole rather than leaving the longer one's tail behind.
    secrets: Vec<String>,
}

impl Redactor {
    /// Build a redactor over `secrets`, dropping values too short to be keys.
    pub fn new(secrets: impl IntoIterator<Item = String>) -> Self {
        let mut secrets: Vec<String> = secrets
            .into_iter()
            .map(|secret| secret.trim().to_owned())
            .filter(|secret| secret.chars().count() >= MIN_SECRET_CHARS)
            .collect();
        // Deduplicate first, then order by descending length with a stable
        // tiebreak, so the replacement is deterministic whatever order the
        // configuration arrived in.
        secrets.sort();
        secrets.dedup();
        secrets.sort_by_key(|secret| std::cmp::Reverse(secret.chars().count()));
        Self { secrets }
    }

    /// Whether this redactor has any value to hide.
    pub fn is_empty(&self) -> bool {
        self.secrets.is_empty()
    }

    /// Replace every occurrence of every secret in `text`, in place.
    ///
    /// A value with nothing to replace is left as the same allocation: the take
    /// and the write-back are moves, not copies, so the no-secret case pays only
    /// the scans.
    pub fn redact(&self, text: &mut String) {
        if self.is_empty() {
            return;
        }
        let mut redacted = std::mem::take(text);
        for secret in &self.secrets {
            if redacted.contains(secret.as_str()) {
                redacted = redacted.replace(secret.as_str(), REDACTED);
            }
        }
        *text = redacted;
    }

    /// [`Redactor::redact`] as a function from one string to another.
    pub fn redacted(&self, text: &str) -> String {
        let mut redacted = text.to_owned();
        self.redact(&mut redacted);
        redacted
    }

    /// Walk a JSON value and redact every string leaf.
    ///
    /// Object **keys** are left alone: they are schema field names, and a key
    /// that happens to equal a secret is not a value escaping anywhere.
    pub fn redact_value(&self, value: &mut serde_json::Value) {
        if self.is_empty() {
            return;
        }
        match value {
            serde_json::Value::String(text) => self.redact(text),
            serde_json::Value::Array(items) => {
                for item in items {
                    self.redact_value(item);
                }
            }
            serde_json::Value::Object(fields) => {
                for field in fields.values_mut() {
                    self.redact_value(field);
                }
            }
            serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            }
        }
    }
}

/// Text conventions over [`EventPayload::HookExecuted`].
///
/// The hook payload is `{ point, command, outcome }`: the schema has no room for
/// a structured outcome, so an outcome is a line of text with a stable prefix.
/// Producing and parsing share these constants because a drifting convention
/// would silently turn ticket 19's hook metrics into zero (spec §18).
///
/// `feedback:` is the one outcome the projection merges into the tool message it
/// annotates; a `failed:` outcome is dropped instead, which is what makes
/// "a post-hook failure only loses feedback" true on the model's side too.
pub mod hook_format {
    /// `HookExecuted.point` for the pre-mount point.
    pub const POINT_PRE: &str = "pre_tool_use";
    /// `HookExecuted.point` for the post-mount point.
    pub const POINT_POST: &str = "post_tool_use";

    /// A hook produced no change: a pre-hook leaves the call alone, a post-hook
    /// injects no feedback. The `point` says which.
    pub const OUTCOME_CONTINUE: &str = "continue";
    /// A pre-hook replaced the tool arguments.
    pub const OUTCOME_REWRITE: &str = "rewrite";
    /// A pre-hook tightened the effective verdict to ask.
    pub const OUTCOME_TIGHTEN_ASK: &str = "tighten:ask";
    /// A pre-hook tightened the effective verdict to deny.
    pub const OUTCOME_TIGHTEN_DENY: &str = "tighten:deny";
    /// A pre-hook skipped execution.
    pub const OUTCOME_SKIP: &str = "skip";
    /// A pre-hook stopped the turn.
    pub const OUTCOME_STOP: &str = "stop";

    /// Prefix of a post-hook outcome that carries feedback for the model.
    pub const FEEDBACK_PREFIX: &str = "feedback: ";
    /// Prefix of any failed or timed-out hook outcome.
    pub const FAILED_PREFIX: &str = "failed: ";

    /// The marker the projection puts in front of merged feedback.
    pub const FEEDBACK_MARKER: &str = "[hook feedback]";

    /// Build the outcome for feedback a post-hook injected.
    pub fn feedback(text: &str) -> String {
        format!("{FEEDBACK_PREFIX}{text}")
    }

    /// Build the outcome for a hook that failed or timed out.
    pub fn failed(message: &str) -> String {
        format!("{FAILED_PREFIX}{message}")
    }

    /// The feedback an outcome carries, or `None` when it carries none (a plain
    /// `continue`, a failure, or a pre-hook outcome).
    pub fn feedback_text(outcome: &str) -> Option<&str> {
        outcome.strip_prefix(FEEDBACK_PREFIX)
    }
}

/// The shape of [`EventPayload::PermissionAsked`]'s `request` value.
///
/// The event carries the question as JSON rather than as a typed struct, so the
/// keys need one home: a reader of the stream and the loop that writes it must
/// not disagree, and a narration that names the tool would otherwise silently
/// lose it.
pub mod permission_format {
    /// The tool the question is about.
    pub const TOOL: &str = "tool";
    /// The arguments, so a reader can show what would run.
    pub const ARGS: &str = "args";
    /// Why the gate asked.
    pub const REASON: &str = "reason";

    /// The tool name a `PermissionAsked.request` carries, when it carries one.
    pub fn tool_name(request: &serde_json::Value) -> Option<&str> {
        request.get(TOOL).and_then(serde_json::Value::as_str)
    }

    /// The arguments of the call the question is about, when it records them.
    pub fn args(request: &serde_json::Value) -> Option<&serde_json::Value> {
        request.get(ARGS)
    }

    /// Why the gate asked, when the request records it.
    pub fn reason(request: &serde_json::Value) -> Option<&str> {
        request.get(REASON).and_then(serde_json::Value::as_str)
    }

    /// Build the `request` value for a question. Keys come from this module, so
    /// the writer and the readers share one shape.
    pub fn request(tool_name: &str, args: &serde_json::Value, reason: &str) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        map.insert(
            TOOL.to_owned(),
            serde_json::Value::String(tool_name.to_owned()),
        );
        map.insert(ARGS.to_owned(), args.clone());
        map.insert(
            REASON.to_owned(),
            serde_json::Value::String(reason.to_owned()),
        );
        serde_json::Value::Object(map)
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

/// Query: every `seq` some `HistorySuperseded` event has retired.
///
/// A retired event is out of effect but never deleted (spec §2). Projection
/// excludes these seqs and `/undo` looks past them, so the rule has one home.
pub fn superseded_seqs(events: &[Event]) -> BTreeSet<u64> {
    let mut retired = BTreeSet::new();
    for event in events {
        if let EventPayload::HistorySuperseded { targets, .. } = &event.payload {
            retired.extend(targets.iter().copied());
        }
    }
    retired
}

/// Query: which `tool_call`s have no result yet?
///
/// Pending work is a query over the log, never hidden loop state. The turn loop
/// must not call the provider while this is non-empty. This session-wide form is
/// the diagnostics and `--continue` recovery one; the loop uses
/// [`pending_tool_calls_of`], because invariant 2 binds the acting agent.
pub fn pending_tool_calls(events: &[Event]) -> Vec<ToolCallId> {
    pending_of(events, None)
}

/// Query: which of `speaker`'s `tool_call`s have no result yet?
///
/// Invariant 2 is per acting agent. With two debaters in flight at once a
/// session-wide query would read the other's unfinished call as one's own and
/// refuse to call the provider for a reason that has nothing to do with it.
pub fn pending_tool_calls_of(events: &[Event], speaker: &SpeakerId) -> Vec<ToolCallId> {
    pending_of(events, Some(speaker))
}

fn pending_of(events: &[Event], only: Option<&SpeakerId>) -> Vec<ToolCallId> {
    let mut pending: Vec<ToolCallId> = Vec::new();
    for event in events {
        if only.is_some_and(|speaker| &event.speaker_id != speaker) {
            continue;
        }
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

/// Query: the session-cumulative usage, summed from every `UsageRecorded`
/// event (spec §10, §17).
///
/// Session spend is a derived value over the log, never hidden state, so the
/// window layer needs no lock and an executor's usage counts without a second
/// ledger: its events are on the same stream.
///
/// `reasoning_tokens` stays `None` until some provider reports it; once one
/// does, the totals are summed.
pub fn total_usage(events: &[Event]) -> Usage {
    sum_usage(events.iter())
}

/// Query: one speaker's usage, summed from its own `UsageRecorded` events.
///
/// The per-speaker slice of the session total: an executor's spend counts toward
/// the session (spec §16) and is also what the `task` result reports back as
/// metadata, so neither number needs a second ledger.
pub fn usage_of(events: &[Event], speaker: &SpeakerId) -> Usage {
    sum_usage(events.iter().filter(|event| &event.speaker_id == speaker))
}

fn sum_usage<'a>(events: impl Iterator<Item = &'a Event>) -> Usage {
    let mut total = Usage::default();
    for event in events {
        let EventPayload::UsageRecorded { usage } = &event.payload else {
            continue;
        };
        total.accumulate(*usage);
    }
    total
}

/// Append-only JSONL log for one session.
///
/// The handle is cheap to clone, and every clone shares one writer, one
/// in-memory cache and one `next_seq` counter. That sharing is what lets two
/// debaters run their turns concurrently onto one stream (spec §15) while "the
/// log has a single writer" still holds: the lock makes each append atomic, so
/// two events can never interleave inside a line and `seq` keeps meaning "line
/// number". In-memory events are a cache of the file, kept in sync by whichever
/// clone holds the lock.
#[derive(Debug, Clone)]
pub struct EventLog {
    path: PathBuf,
    inner: Arc<Mutex<Inner>>,
}

#[derive(Debug)]
struct Inner {
    writer: BufWriter<File>,
    events: Vec<Event>,
    next_seq: u64,
}

impl EventLog {
    /// Create a fresh log. Fails if the file already exists; the parent
    /// directory must exist.
    ///
    /// The file is made owner-only (`0600`): a stream carries the user's source
    /// and, after redaction, nothing that was secret — but it is still private
    /// (spec §11).
    pub fn create(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let mut options = OpenOptions::new();
        options.create_new(true).append(true);
        // The mode is set at creation rather than narrowed afterwards, so there
        // is no window in which the stream exists world-readable (spec §11, §20).
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;

            options.mode(0o600);
        }
        let file = options.open(&path)?;
        Ok(Self {
            path,
            inner: Arc::new(Mutex::new(Inner {
                writer: BufWriter::new(file),
                events: Vec::new(),
                next_seq: 1,
            })),
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
            inner: Arc::new(Mutex::new(Inner {
                writer: BufWriter::new(file),
                events,
                next_seq,
            })),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn next_seq(&self) -> u64 {
        self.inner().next_seq
    }

    /// A snapshot of every event read so far, in order.
    ///
    /// A snapshot rather than a borrow: two debaters share this handle, so there
    /// is no lifetime at which the caller could hold a reference into it. The
    /// stream is append-only, so a snapshot is a valid prefix of the log — which
    /// is exactly what a round-scoped projection needs (spec §15).
    pub fn events(&self) -> Vec<Event> {
        self.inner().events.clone()
    }

    /// Append one event. Flushes the line but never `fsync`s.
    pub fn append(&mut self, speaker_id: SpeakerId, payload: EventPayload) -> io::Result<Event> {
        let mut inner = self.inner.lock().expect("event log mutex poisoned");
        let event = Event {
            seq: inner.next_seq,
            at: Utc::now(),
            speaker_id,
            payload,
        };
        let mut line =
            serde_json::to_string(&event).expect("Event payloads are always JSON-serializable");
        line.push('\n');
        inner.writer.write_all(line.as_bytes())?;
        inner.writer.flush()?;
        inner.events.push(event.clone());
        inner.next_seq += 1;
        Ok(event)
    }

    /// Flush buffered bytes to the OS. Not an `fsync`.
    pub fn flush(&mut self) -> io::Result<()> {
        self.inner
            .lock()
            .expect("event log mutex poisoned")
            .writer
            .flush()
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("event log mutex poisoned")
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
