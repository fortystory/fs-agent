//! The human-facing views behind the `sessions` CLI (spec §18).
//!
//! Every view here is a **group-by over the event stream**, never a new capture
//! point: the stream is the single source of truth, and a second store of
//! diagnostics would be a second thing to keep honest. `seq` is the only
//! identity, so a view is a sequential read plus an in-memory filter — there is
//! deliberately no index.
//!
//! Four views:
//!
//! * [`list`] / [`summarize`] — which sessions exist, for `sessions ls`;
//! * [`timeline`] — the round-grouped transcript, for `sessions show`, with
//!   [`Filter`] as the escape hatch and tool calls merged with their results;
//! * [`file_history`] — the one view indexed by a **workspace object** instead
//!   of by time ("who changed this file, in which round"), derived from the
//!   `wrote:` line a successful write reports;
//! * [`stats`] — the fixed metric set, including the two quantities nothing else
//!   surfaces: the one-sided absence rate and the edit-ladder downgrade
//!   distribution.
//!
//! The conventional texts the derived metrics read (`edit match level:`,
//! `wrote:`, the read-before-write refusal, the `no match` failure) come from the
//! constants the producers use, never from a literal written twice: a drifting
//! prefix would silently turn a count into zero (spec §18).

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{LandingPoint, PriceTable, Routing, SessionConfig};
use crate::discussion::protocol::round_attendance;
use crate::events::{
    hook_format, ContextSource, Decision, DecisionSource, Event, EventPayload, HistoryReason, Role,
    RoundMode, SessionId, SpeakerId, StopReason, ToolCallId, Usage,
};
use crate::tools::edit::EditError;
use crate::tools::file::{MATCH_LEVEL_PREFIX, WROTE_PATH_PREFIX};
use crate::tools::READ_BEFORE_WRITE_PREFIX;

use super::store::{SessionStore, StoredSession};

// ---------------------------------------------------------------------------
// Listing
// ---------------------------------------------------------------------------

/// One session as a row: enough to find the one you meant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Listing {
    pub id: SessionId,
    /// The movable session directory.
    pub dir: PathBuf,
    /// The authoritative workspace, from `SessionStarted`.
    pub cwd: Option<String>,
    /// When the session started, from its own first event.
    pub started: Option<DateTime<Utc>>,
    /// When the stream was last written, from the file's mtime.
    pub written: Option<DateTime<Utc>>,
    /// The recorded session ending, when there is one.
    pub ended: Option<StopReason>,
    /// Session-cumulative tokens, summed from `UsageRecorded`.
    pub tokens: u64,
    /// Completed messages on the stream.
    pub messages: usize,
    /// Rounds opened.
    pub rounds: usize,
}

/// List sessions: one cwd's bucket, or every bucket when `cwd` is `None`.
///
/// Newest first, which is the order `--continue` would consider them in.
pub fn list(store: &SessionStore, cwd: Option<&Path>) -> io::Result<Vec<Listing>> {
    let sessions = match cwd {
        Some(cwd) => store.list(cwd)?,
        None => store.list_all()?,
    };
    sessions.iter().map(summarize).collect()
}

/// Summarize one stored session from its own stream.
pub fn summarize(session: &StoredSession) -> io::Result<Listing> {
    let events = crate::events::read_events(&session.log_path)?;
    let mut listing = Listing {
        id: session.id.clone(),
        dir: session.dir.clone(),
        cwd: None,
        started: None,
        written: written_at(&session.log_path),
        ended: None,
        tokens: 0,
        messages: 0,
        rounds: 0,
    };
    for event in &events {
        if listing.started.is_none() {
            listing.started = Some(event.at);
        }
        match &event.payload {
            EventPayload::SessionStarted { cwd, .. } => listing.cwd = Some(cwd.clone()),
            EventPayload::SessionEnded { reason } => listing.ended = Some(*reason),
            EventPayload::MessageCompleted { .. } => listing.messages += 1,
            EventPayload::RoundStarted { .. } => listing.rounds += 1,
            _ => {}
        }
    }
    listing.tokens = crate::events::total_usage(&events).total_tokens();
    Ok(listing)
}

fn written_at(path: &Path) -> Option<DateTime<Utc>> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .map(DateTime::<Utc>::from)
}

// ---------------------------------------------------------------------------
// Timeline
// ---------------------------------------------------------------------------

/// The round-grouped transcript (`sessions show`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub session_id: Option<SessionId>,
    pub cwd: Option<String>,
    pub groups: Vec<TimelineGroup>,
}

/// One round's slice of the transcript, plus the stretch before the first round.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TimelineGroup {
    /// The round number, or `None` for the session's prelude (and for a
    /// single-agent session, which has no rounds at all).
    pub round: Option<u32>,
    pub mode: Option<RoundMode>,
    /// How the round ended, when it was the one that ended the debate phase.
    pub ended: Option<StopReason>,
    pub entries: Vec<Entry>,
}

/// One row of the transcript. Tool calls are merged with their results, and a
/// post-hook's feedback with the result it annotates, so a call reads as one
/// thing rather than three.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "entry", rename_all = "snake_case")]
pub enum Entry {
    Message {
        speaker: SpeakerId,
        role: Role,
        text: String,
    },
    Tool {
        speaker: SpeakerId,
        tool_call_id: ToolCallId,
        tool: String,
        args: serde_json::Value,
        /// `None` while the call has no result yet.
        ok: Option<bool>,
        output: Option<String>,
        error: Option<String>,
        duration_ms: Option<u64>,
        /// A post-hook's outcome for this call, merged here because the hook
        /// event carries no `tool_call_id` and the call is what it annotated.
        hook: Option<String>,
    },
    RoundStarted {
        round: u32,
        mode: RoundMode,
    },
    RoundEnded {
        round: u32,
        reason: StopReason,
    },
    SessionEnded {
        reason: StopReason,
    },
    TurnStarted {
        speaker: SpeakerId,
        iteration: u32,
    },
    Divergence {
        round: u32,
        topic: String,
        positions: Vec<String>,
    },
    TurnEnded {
        speaker: SpeakerId,
        reason: StopReason,
    },
    PermissionAsked {
        speaker: SpeakerId,
        request_id: String,
        tool_call_id: ToolCallId,
        request: serde_json::Value,
    },
    PermissionDecided {
        speaker: SpeakerId,
        request_id: String,
        decision: Decision,
        source: DecisionSource,
        reason: Option<String>,
    },
    Hook {
        speaker: SpeakerId,
        point: String,
        command: String,
        outcome: String,
    },
    ExecutorSpawned {
        speaker: SpeakerId,
        executor_id: crate::events::ParticipantId,
        parent: crate::events::ParticipantId,
        brief: String,
    },
    ExecutorFinished {
        speaker: SpeakerId,
        executor_id: crate::events::ParticipantId,
        reason: StopReason,
        summary: String,
    },
    Usage {
        speaker: SpeakerId,
        usage: Usage,
    },
    AgentError {
        speaker: SpeakerId,
        message: String,
        recoverable: bool,
    },
    SessionError {
        speaker: SpeakerId,
        code: String,
        detail: String,
    },
    History {
        speaker: SpeakerId,
        reason: HistoryReason,
        targets: Vec<u64>,
        summary: Option<String>,
    },
    Context {
        speaker: SpeakerId,
        source: ContextSource,
        content: String,
    },
}

impl Entry {
    /// The stable name of this row, for `--kind`.
    pub fn kind(&self) -> &'static str {
        match self {
            Entry::Message { .. } => "MessageCompleted",
            Entry::Tool { .. } => "Tool",
            Entry::RoundStarted { .. } => "RoundStarted",
            Entry::RoundEnded { .. } => "RoundEnded",
            Entry::SessionEnded { .. } => "SessionEnded",
            Entry::TurnStarted { .. } => "TurnStarted",
            Entry::Divergence { .. } => "DivergenceRecorded",
            Entry::TurnEnded { .. } => "TurnEnded",
            Entry::PermissionAsked { .. } => "PermissionAsked",
            Entry::PermissionDecided { .. } => "PermissionDecided",
            Entry::Hook { .. } => "HookExecuted",
            Entry::ExecutorSpawned { .. } => "ExecutorSpawned",
            Entry::ExecutorFinished { .. } => "ExecutorFinished",
            Entry::Usage { .. } => "UsageRecorded",
            Entry::AgentError { .. } => "AgentError",
            Entry::SessionError { .. } => "SessionError",
            Entry::History { .. } => "HistorySuperseded",
            Entry::Context { .. } => "ContextInjected",
        }
    }

    /// The speaker this row is attributed to, when it has one.
    pub fn speaker(&self) -> Option<&SpeakerId> {
        match self {
            Entry::Message { speaker, .. }
            | Entry::Tool { speaker, .. }
            | Entry::TurnStarted { speaker, .. }
            | Entry::TurnEnded { speaker, .. }
            | Entry::PermissionAsked { speaker, .. }
            | Entry::PermissionDecided { speaker, .. }
            | Entry::Hook { speaker, .. }
            | Entry::ExecutorSpawned { speaker, .. }
            | Entry::ExecutorFinished { speaker, .. }
            | Entry::Usage { speaker, .. }
            | Entry::AgentError { speaker, .. }
            | Entry::SessionError { speaker, .. }
            | Entry::History { speaker, .. }
            | Entry::Context { speaker, .. } => Some(speaker),
            Entry::RoundStarted { .. }
            | Entry::RoundEnded { .. }
            | Entry::SessionEnded { .. }
            | Entry::Divergence { .. } => None,
        }
    }

    /// Whether `--only-error` keeps this row: a failed call, a failed or
    /// aborted turn, an error, or a round that ended in one.
    pub fn is_error(&self) -> bool {
        match self {
            Entry::Tool { ok, .. } => *ok == Some(false),
            Entry::TurnEnded { reason, .. } => {
                matches!(reason, StopReason::Error | StopReason::Aborted)
            }
            Entry::RoundEnded { reason, .. } => {
                matches!(reason, StopReason::Error | StopReason::Aborted)
            }
            Entry::ExecutorFinished { reason, .. } => *reason != StopReason::Completed,
            Entry::AgentError { .. } | Entry::SessionError { .. } => true,
            _ => false,
        }
    }

    fn kind_matches(&self, wanted: &str) -> bool {
        canonical_kind(self.kind()) == canonical_kind(wanted)
    }
}

/// Fold a kind name to the one form `--kind` compares in.
///
/// Case, underscores and dashes are all decoration around the same event name;
/// a merged tool row answers to any of the three names it replaces. Public
/// because the CLI's "is this the usage row" test must agree with the filter, and
/// two normalizers would eventually disagree.
pub fn canonical_kind(kind: &str) -> String {
    let normalized: String = kind
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    // A merged tool row answers to any of the three names it replaces.
    if matches!(
        normalized.as_str(),
        "tool" | "toolcall" | "toolcallstarted" | "toolcallcompleted"
    ) {
        "tool".to_owned()
    } else {
        normalized
    }
}

/// Build the round-grouped transcript. Tool calls are merged with their results
/// by `tool_call_id`, and a post-hook's outcome with the result it annotates.
pub fn timeline(events: &[Event]) -> Timeline {
    let mut timeline = Timeline::default();
    let mut open: Option<usize> = None;
    // `tool_call_id -> (group, entry)`: a result may land in a group the
    // transcript has already moved past, so the row is found by identity.
    let mut tools: Vec<(ToolCallId, usize, usize)> = Vec::new();

    for event in events {
        match &event.payload {
            EventPayload::SessionStarted {
                session_id, cwd, ..
            } => {
                timeline.session_id = Some(session_id.clone());
                timeline.cwd = Some(cwd.clone());
            }
            EventPayload::RoundStarted {
                round: started,
                mode,
            } => {
                timeline.groups.push(TimelineGroup {
                    round: Some(*started),
                    mode: Some(*mode),
                    ended: None,
                    entries: vec![Entry::RoundStarted {
                        round: *started,
                        mode: *mode,
                    }],
                });
                open = Some(timeline.groups.len() - 1);
                continue;
            }
            EventPayload::RoundEnded {
                round: ended,
                reason,
            } => {
                let index = open_group(&mut timeline, &mut open);
                timeline.groups[index].entries.push(Entry::RoundEnded {
                    round: *ended,
                    reason: *reason,
                });
                timeline.groups[index].ended = Some(*reason);
                open = None;
                continue;
            }
            EventPayload::ToolCallStarted {
                tool_call_id,
                tool_name,
                args,
            } => {
                let index = open_group(&mut timeline, &mut open);
                let entry = timeline.groups[index].entries.len();
                timeline.groups[index].entries.push(Entry::Tool {
                    speaker: event.speaker_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    tool: tool_name.clone(),
                    args: args.clone(),
                    ok: None,
                    output: None,
                    error: None,
                    duration_ms: None,
                    hook: None,
                });
                tools.push((tool_call_id.clone(), index, entry));
                continue;
            }
            EventPayload::ToolCallCompleted {
                tool_call_id,
                ok,
                output,
                error,
                duration_ms,
            } => {
                if let Some((_, group, entry)) = tools
                    .iter()
                    .rev()
                    .find(|(id, _, _)| id == tool_call_id)
                    .cloned()
                {
                    if let Some(Entry::Tool {
                        ok: slot_ok,
                        output: slot_output,
                        error: slot_error,
                        duration_ms: slot_duration,
                        ..
                    }) = timeline.groups[group].entries.get_mut(entry)
                    {
                        *slot_ok = Some(*ok);
                        *slot_output = output.clone();
                        *slot_error = error.clone();
                        *slot_duration = Some(*duration_ms);
                    }
                } else {
                    // A result whose start is not on this stream is a resume
                    // artifact; show it rather than dropping it.
                    let index = open_group(&mut timeline, &mut open);
                    timeline.groups[index].entries.push(Entry::Tool {
                        speaker: event.speaker_id.clone(),
                        tool_call_id: tool_call_id.clone(),
                        tool: String::new(),
                        args: serde_json::Value::Null,
                        ok: Some(*ok),
                        output: output.clone(),
                        error: error.clone(),
                        duration_ms: Some(*duration_ms),
                        hook: None,
                    });
                }
                continue;
            }
            // A post-hook's feedback belongs with the result it annotates; the
            // hook event carries no id, and the loop emits it immediately after
            // the result, so the latest result is the pairing (spec §3, §19).
            EventPayload::HookExecuted { point, outcome, .. }
                if point == hook_format::POINT_POST =>
            {
                if let Some(entry) = last_tool_entry(&mut timeline) {
                    *entry = Some(outcome.clone());
                }
                continue;
            }
            _ => {}
        }

        let index = open_group(&mut timeline, &mut open);
        if let Some(entry) = entry_of(event) {
            timeline.groups[index].entries.push(entry);
        }
    }

    timeline
}

/// The open group, opening the session's prelude group on first use.
fn open_group(timeline: &mut Timeline, open: &mut Option<usize>) -> usize {
    if let Some(index) = *open {
        return index;
    }
    timeline.groups.push(TimelineGroup::default());
    *open = Some(timeline.groups.len() - 1);
    timeline.groups.len() - 1
}

/// The most recent row that is a tool call, wherever it sits.
fn last_tool_entry(timeline: &mut Timeline) -> Option<&mut Option<String>> {
    for group in timeline.groups.iter_mut().rev() {
        for entry in group.entries.iter_mut().rev() {
            if let Entry::Tool { hook, .. } = entry {
                return Some(hook);
            }
        }
    }
    None
}

fn entry_of(event: &Event) -> Option<Entry> {
    let speaker = event.speaker_id.clone();
    Some(match &event.payload {
        EventPayload::MessageCompleted { role, text, .. } => Entry::Message {
            speaker,
            role: *role,
            text: text.clone(),
        },
        EventPayload::SessionEnded { reason } => Entry::SessionEnded { reason: *reason },
        EventPayload::TurnStarted { iteration, .. } => Entry::TurnStarted {
            speaker,
            iteration: *iteration,
        },
        EventPayload::DivergenceRecorded {
            round,
            topic,
            positions,
        } => Entry::Divergence {
            round: *round,
            topic: topic.clone(),
            positions: positions.clone(),
        },
        EventPayload::TurnEnded { reason } => Entry::TurnEnded {
            speaker,
            reason: *reason,
        },
        EventPayload::PermissionAsked {
            request_id,
            tool_call_id,
            request,
        } => Entry::PermissionAsked {
            speaker,
            request_id: request_id.clone(),
            tool_call_id: tool_call_id.clone(),
            request: request.clone(),
        },
        EventPayload::PermissionDecided {
            request_id,
            decision,
            source,
            reason,
        } => Entry::PermissionDecided {
            speaker,
            request_id: request_id.clone(),
            decision: *decision,
            source: *source,
            reason: reason.clone(),
        },
        EventPayload::HookExecuted {
            point,
            command,
            outcome,
        } => Entry::Hook {
            speaker,
            point: point.clone(),
            command: command.clone(),
            outcome: outcome.clone(),
        },
        EventPayload::ExecutorSpawned {
            executor_id,
            parent,
            brief,
        } => Entry::ExecutorSpawned {
            speaker,
            executor_id: executor_id.clone(),
            parent: parent.clone(),
            brief: brief.clone(),
        },
        EventPayload::ExecutorFinished {
            executor_id,
            reason,
            summary,
        } => Entry::ExecutorFinished {
            speaker,
            executor_id: executor_id.clone(),
            reason: *reason,
            summary: summary.clone(),
        },
        EventPayload::UsageRecorded { usage } => Entry::Usage {
            speaker,
            usage: *usage,
        },
        EventPayload::AgentError {
            message,
            recoverable,
        } => Entry::AgentError {
            speaker,
            message: message.clone(),
            recoverable: *recoverable,
        },
        EventPayload::SessionError { code, detail } => Entry::SessionError {
            speaker,
            code: code.clone(),
            detail: detail.clone(),
        },
        EventPayload::HistorySuperseded {
            reason,
            targets,
            summary,
        } => Entry::History {
            speaker,
            reason: *reason,
            targets: targets.clone(),
            summary: summary.clone(),
        },
        EventPayload::ContextInjected { source, content } => Entry::Context {
            speaker,
            source: source.clone(),
            content: content.clone(),
        },
        // Handled by the grouping loop above.
        EventPayload::SessionStarted { .. }
        | EventPayload::RoundStarted { .. }
        | EventPayload::RoundEnded { .. }
        | EventPayload::ToolCallStarted { .. }
        | EventPayload::ToolCallCompleted { .. } => return None,
    })
}

/// Filters over a [`Timeline`] (`sessions show`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    pub round: Option<u32>,
    pub speaker: Option<SpeakerId>,
    pub kind: Option<String>,
    pub tool: Option<String>,
    pub only_error: bool,
}

impl Filter {
    /// Whether this filter names nothing, in which case the caller can use the
    /// transcript it already built instead of cloning a filtered copy.
    pub fn is_empty(&self) -> bool {
        self.round.is_none()
            && self.speaker.is_none()
            && self.kind.is_none()
            && self.tool.is_none()
            && !self.only_error
    }
}

impl Timeline {
    /// Apply a [`Filter`]; empty groups fall away, so a filtered view stays
    /// round-grouped without empty section headings.
    pub fn filtered(&self, filter: &Filter) -> Timeline {
        let mut timeline = Timeline {
            session_id: self.session_id.clone(),
            cwd: self.cwd.clone(),
            groups: Vec::new(),
        };
        for group in &self.groups {
            if let Some(round) = filter.round {
                if group.round != Some(round) {
                    continue;
                }
            }
            let entries: Vec<Entry> = group
                .entries
                .iter()
                .filter(|entry| matches_filter(entry, filter))
                .cloned()
                .collect();
            if entries.is_empty() {
                continue;
            }
            timeline.groups.push(TimelineGroup {
                round: group.round,
                mode: group.mode,
                ended: group.ended,
                entries,
            });
        }
        timeline
    }
}

fn matches_filter(entry: &Entry, filter: &Filter) -> bool {
    if filter.only_error && !entry.is_error() {
        return false;
    }
    if let Some(speaker) = &filter.speaker {
        if entry.speaker() != Some(speaker) {
            return false;
        }
    }
    if let Some(kind) = &filter.kind {
        if !entry.kind_matches(kind) {
            return false;
        }
    }
    if let Some(tool) = &filter.tool {
        match entry {
            Entry::Tool { tool: name, .. } if name == tool => {}
            _ => return false,
        }
    }
    true
}

// ---------------------------------------------------------------------------
// File history
// ---------------------------------------------------------------------------

/// One changed file, attributed to the agent and round that changed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub tool: String,
    pub speaker: SpeakerId,
    pub round: Option<u32>,
    pub seq: u64,
}

/// The workspace-object view: every file a successful write or edit landed on.
///
/// The path comes from the **result** line (`wrote:`), not from the call's
/// arguments: a `hook.pre` may have rewritten the arguments, so the result is the
/// only record of what was actually written (spec §16, §18).
pub fn file_history(events: &[Event]) -> Vec<FileChange> {
    let mut round: Option<u32> = None;
    let mut names: BTreeMap<ToolCallId, String> = BTreeMap::new();
    let mut changes = Vec::new();

    for event in events {
        match &event.payload {
            EventPayload::RoundStarted { round: started, .. } => round = Some(*started),
            EventPayload::RoundEnded { .. } => round = None,
            EventPayload::ToolCallStarted {
                tool_call_id,
                tool_name,
                ..
            } => {
                names.insert(tool_call_id.clone(), tool_name.clone());
            }
            EventPayload::ToolCallCompleted {
                tool_call_id,
                ok: true,
                output: Some(output),
                ..
            } => {
                if let Some(path) = output
                    .strip_prefix(WROTE_PATH_PREFIX)
                    .and_then(|rest| rest.lines().next())
                {
                    changes.push(FileChange {
                        path: path.to_owned(),
                        tool: names.get(tool_call_id).cloned().unwrap_or_default(),
                        speaker: event.speaker_id.clone(),
                        round,
                        seq: event.seq,
                    });
                }
            }
            _ => {}
        }
    }
    changes
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// One named count, so a JSON consumer can read a distribution without
/// depending on a map's ordering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Count {
    pub name: String,
    pub count: usize,
}

/// The fixed metric set (`sessions stats`), all of it a group-by over the stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub session: SessionStats,
    /// One row per participant: debaters, the synthesizer and every executor.
    pub speakers: Vec<SpeakerStats>,
    pub rounds: Vec<RoundStats>,
    pub absence: AbsenceStats,
    pub executors: ExecutorStats,
    pub edits: EditStats,
    pub guards: GuardStats,
    pub permissions: PermissionStats,
    pub hooks: HookStats,
    /// Rounds whose two sides recorded a conflict.
    pub divergences: usize,
    /// `divergences / debate rounds`, or `None` when there were none.
    pub divergence_rate: Option<f64>,
    /// How the debate phase ended, per reason.
    pub stops: Vec<Count>,
}

/// The session-wide totals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionStats {
    pub tokens: Usage,
    pub cost: Option<f64>,
    pub calls: usize,
    pub messages: usize,
    pub rounds: usize,
}

/// One participant's spend: tokens always, money when a model was named.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeakerStats {
    pub speaker: SpeakerId,
    pub tokens: Usage,
    pub calls: usize,
    pub cost: Option<f64>,
    /// `cached / (cached + miss)`, or `None` when nothing was billed.
    pub hit_rate: Option<f64>,
}

/// One round: the calls it cost and how it was closed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoundStats {
    pub round: u32,
    pub mode: RoundMode,
    pub calls: usize,
    pub ended: Option<StopReason>,
}

/// The absence picture across the debate rounds (spec §15's query).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbsenceStats {
    /// Debate rounds (the synthesis round is not one).
    pub rounds: usize,
    /// Rounds where exactly one side was absent while the other answered.
    pub one_sided: usize,
    /// How often each speaker was the absent one.
    pub per_speaker: Vec<Count>,
    /// `one_sided / rounds`, or `None` when there were no debate rounds.
    pub rate: Option<f64>,
}

/// The executors a session dispatched and why they stopped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutorStats {
    pub spawned: usize,
    pub finished: usize,
    pub by_reason: Vec<Count>,
    pub tokens: Usage,
}

/// The edit ladder's outcome (spec §8): how many edits landed and at which level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditStats {
    pub succeeded: usize,
    /// The match ladder's downgrade distribution (`exact` / the two downgrades).
    pub levels: Vec<Count>,
    /// Edits refused because no ladder level matched — the case that withdraws
    /// the path's read permission.
    pub failed_matches: usize,
}

/// The two guardrails whose refusals are otherwise invisible (spec §7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuardStats {
    pub read_before_write: usize,
    pub invalidated_reads: usize,
}

/// Permission questions asked and how they were decided.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionStats {
    pub asked: usize,
    pub decided: Vec<Count>,
}

/// What the mounted hooks did over the session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookStats {
    pub executed: usize,
    pub pre: usize,
    pub post: usize,
    /// Post-hook outcomes that carried feedback to the model.
    pub feedback: usize,
    /// Hook failures or timeouts (fail-closed pre-hooks, dropped post feedback).
    pub failed: usize,
    pub outcomes: Vec<Count>,
}

/// The model each speaker is priced at, for a view that knows the roster.
///
/// `UsageRecorded` carries no model — the roster lives in configuration, not on
/// the stream — so money is only shown when the caller names the model. Tokens
/// and hit rates need no model and are always reported. The routing rule is
/// [`SessionConfig::model_for`]'s, not a second copy of it (spec §17): a routed
/// synthesizer or executor is priced at the model it actually answered with.
#[derive(Debug, Clone)]
pub struct CostModel {
    config: SessionConfig,
}

impl CostModel {
    /// Price a session whose answerers all use `base_model`.
    pub fn new(base_model: impl Into<String>, pricing: PriceTable) -> Self {
        Self {
            config: SessionConfig::new(base_model).with_pricing(pricing),
        }
    }

    /// Apply the configured `[routing]` overrides, so the synthesizer and the
    /// executors are priced at the models they really ran on.
    pub fn with_routing(mut self, routing: &Routing) -> Self {
        routing.apply(&mut self.config);
        self
    }

    /// The model a speaker is priced at: the system's one routing rule, applied
    /// to a speaker rather than restated here.
    pub fn model_for(&self, speaker: &SpeakerId) -> &str {
        match speaker {
            SpeakerId::System => self.config.model_for(LandingPoint::Synthesizer),
            SpeakerId::Executor(_) => self.config.model_for(LandingPoint::Executor),
            // A debater is never a landing point (spec §17): it answers with the
            // session's own model.
            SpeakerId::Debater(_) | SpeakerId::User => &self.config.model,
        }
    }

    fn cost(&self, speaker: &SpeakerId, usage: Usage) -> Option<f64> {
        self.config.pricing.cost(self.model_for(speaker), usage)
    }
}

/// Compute the metric set. `cost` may be `None`: money is display only and
/// needs a model the stream does not carry (spec §17).
pub fn stats(events: &[Event], cost: Option<&CostModel>) -> Stats {
    let mut agents: BTreeMap<SpeakerId, Usage> = BTreeMap::new();
    let mut agent_calls: BTreeMap<SpeakerId, usize> = BTreeMap::new();
    let mut rounds: Vec<RoundStats> = Vec::new();
    let mut round_index: BTreeMap<u32, usize> = BTreeMap::new();
    let mut stops: BTreeMap<String, usize> = BTreeMap::new();
    let mut hook_outcomes: BTreeMap<String, usize> = BTreeMap::new();
    let mut decisions: BTreeMap<String, usize> = BTreeMap::new();
    let mut levels: BTreeMap<String, usize> = BTreeMap::new();
    let mut executor_reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut per_speaker_absent: BTreeMap<String, usize> = BTreeMap::new();

    let mut session = SessionStats {
        tokens: Usage::default(),
        cost: None,
        calls: 0,
        messages: 0,
        rounds: 0,
    };
    let mut executors = ExecutorStats {
        spawned: 0,
        finished: 0,
        by_reason: Vec::new(),
        tokens: Usage::default(),
    };
    let mut edits = EditStats {
        succeeded: 0,
        levels: Vec::new(),
        failed_matches: 0,
    };
    let mut guards = GuardStats {
        read_before_write: 0,
        invalidated_reads: 0,
    };
    let mut permissions = PermissionStats {
        asked: 0,
        decided: Vec::new(),
    };
    let mut hooks = HookStats {
        executed: 0,
        pre: 0,
        post: 0,
        feedback: 0,
        failed: 0,
        outcomes: Vec::new(),
    };
    let mut divergences = 0;
    let mut round: Option<u32> = None;

    for event in events {
        match &event.payload {
            EventPayload::SessionStarted { .. } => {}
            EventPayload::ContextInjected { .. } => {}
            EventPayload::SessionEnded { reason } => {
                *stops.entry(reason.as_str().to_owned()).or_default() += 1;
            }
            EventPayload::RoundStarted {
                round: started,
                mode,
            } => {
                round = Some(*started);
                session.rounds += 1;
                round_index.insert(*started, rounds.len());
                rounds.push(RoundStats {
                    round: *started,
                    mode: *mode,
                    calls: 0,
                    ended: None,
                });
            }
            EventPayload::RoundEnded {
                round: ended,
                reason,
            } => {
                if let Some(index) = round_index.get(ended) {
                    rounds[*index].ended = Some(*reason);
                }
                *stops.entry(reason.as_str().to_owned()).or_default() += 1;
                round = None;
            }
            EventPayload::DivergenceRecorded { .. } => divergences += 1,
            EventPayload::TurnStarted { .. } => {}
            EventPayload::MessageCompleted { .. } => session.messages += 1,
            EventPayload::ToolCallStarted { .. } => {}
            EventPayload::ToolCallCompleted {
                ok, output, error, ..
            } => {
                let (levels, failed_match) = tally_tool(
                    *ok,
                    output.as_deref(),
                    error.as_deref(),
                    &mut levels,
                    &mut guards,
                );
                edits.succeeded += levels;
                if failed_match {
                    edits.failed_matches += 1;
                }
            }
            EventPayload::UsageRecorded { usage } => {
                session.tokens.accumulate(*usage);
                session.calls += 1;
                agents
                    .entry(event.speaker_id.clone())
                    .or_default()
                    .accumulate(*usage);
                *agent_calls.entry(event.speaker_id.clone()).or_default() += 1;
                if let Some(round) = round {
                    if let Some(index) = round_index.get(&round) {
                        rounds[*index].calls += 1;
                    }
                }
                if matches!(event.speaker_id, SpeakerId::Executor(_)) {
                    executors.tokens.accumulate(*usage);
                }
            }
            EventPayload::TurnEnded { .. } => {}
            EventPayload::PermissionAsked { .. } => permissions.asked += 1,
            EventPayload::PermissionDecided { decision, .. } => {
                *decisions.entry(decision.as_str().to_owned()).or_default() += 1;
            }
            EventPayload::HookExecuted { point, outcome, .. } => {
                hooks.executed += 1;
                match point.as_str() {
                    hook_format::POINT_PRE => hooks.pre += 1,
                    hook_format::POINT_POST => hooks.post += 1,
                    _ => {}
                }
                if hook_format::feedback_text(outcome).is_some() {
                    hooks.feedback += 1;
                }
                if outcome.starts_with(hook_format::FAILED_PREFIX) {
                    hooks.failed += 1;
                }
                *hook_outcomes.entry(outcome.clone()).or_default() += 1;
            }
            EventPayload::ExecutorSpawned { .. } => executors.spawned += 1,
            EventPayload::ExecutorFinished { reason, .. } => {
                executors.finished += 1;
                *executor_reasons
                    .entry(reason.as_str().to_owned())
                    .or_default() += 1;
            }
            EventPayload::AgentError { .. } | EventPayload::SessionError { .. } => {}
            EventPayload::HistorySuperseded { .. } => {}
        }
    }

    // Absence is a question about rounds, so it is asked of the stream per round
    // rather than reconstructed while walking (spec §15's query).
    let debate_rounds: Vec<u32> = rounds
        .iter()
        .filter(|round| round.mode != RoundMode::Synthesis)
        .map(|round| round.round)
        .collect();
    let mut one_sided = 0;
    for round in &debate_rounds {
        let attendance = round_attendance(events, *round);
        if attendance.absent.len() == 1 {
            one_sided += 1;
        }
        for speaker in &attendance.absent {
            *per_speaker_absent.entry(speaker.to_string()).or_default() += 1;
        }
    }
    let absence = AbsenceStats {
        rounds: debate_rounds.len(),
        one_sided,
        per_speaker: counted(per_speaker_absent),
        rate: (!debate_rounds.is_empty()).then(|| one_sided as f64 / debate_rounds.len() as f64),
    };

    let speaker_stats: Vec<SpeakerStats> = agents
        .into_iter()
        .map(|(speaker, tokens)| {
            let billed = tokens.cached_tokens + tokens.miss_tokens;
            SpeakerStats {
                calls: agent_calls.get(&speaker).copied().unwrap_or(0),
                cost: cost.and_then(|cost| cost.cost(&speaker, tokens)),
                hit_rate: (billed > 0).then(|| tokens.cached_tokens as f64 / billed as f64),
                speaker,
                tokens,
            }
        })
        .collect();

    // The session total is the sum of the rows above, so token and money
    // arithmetic can never drift apart.
    session.tokens = speaker_stats
        .iter()
        .fold(Usage::default(), |mut total, speaker| {
            total.accumulate(speaker.tokens);
            total
        });
    session.cost = speaker_stats
        .iter()
        .map(|speaker| speaker.cost)
        .collect::<Option<Vec<f64>>>()
        .map(|amounts| amounts.iter().sum());

    Stats {
        session,
        speakers: speaker_stats,
        rounds,
        absence,
        executors: ExecutorStats {
            by_reason: counted(executor_reasons),
            ..executors
        },
        edits: EditStats {
            levels: counted(levels),
            ..edits
        },
        guards,
        permissions: PermissionStats {
            decided: counted(decisions),
            ..permissions
        },
        hooks: HookStats {
            outcomes: counted(hook_outcomes),
            ..hooks
        },
        divergences,
        divergence_rate: (!debate_rounds.is_empty())
            .then(|| divergences as f64 / debate_rounds.len() as f64),
        stops: counted(stops),
    }
}

/// Classify one finished tool call's result text for the two silent metrics.
///
/// Returns `(match levels reported, whether the failure was a failed match)`.
fn tally_tool(
    ok: bool,
    output: Option<&str>,
    error: Option<&str>,
    levels: &mut BTreeMap<String, usize>,
    guards: &mut GuardStats,
) -> (usize, bool) {
    if ok {
        return output
            .map(|output| (count_levels(output, levels), false))
            .unwrap_or((0, false));
    }
    let Some(error) = error else {
        return (0, false);
    };
    if error.starts_with(READ_BEFORE_WRITE_PREFIX) {
        guards.read_before_write += 1;
    }
    // The invalidation is not a field of its own: a failed match *is* the case
    // that withdraws the read permission. The producer prefixes the path
    // (`edit_file` reports `"<path>: {error}"`), so the suffix is matched
    // against the variant's own rendering — the parser cannot drift from the
    // producer's text (spec §18).
    if error.ends_with(&EditError::NoMatch.to_string()) {
        guards.invalidated_reads += 1;
        return (0, true);
    }
    (0, false)
}

/// Count the match levels reported in one successful edit result.
///
/// The line is `edit match level: <level>: …`; the level is parsed via the
/// producer's prefix so the two cannot drift (spec §18).
fn count_levels(output: &str, levels: &mut BTreeMap<String, usize>) -> usize {
    let mut seen = 0;
    for line in output.lines() {
        let Some(rest) = line.strip_prefix(MATCH_LEVEL_PREFIX) else {
            continue;
        };
        let level = rest
            .split(|ch: char| ch == ':' || ch.is_whitespace())
            .next()
            .unwrap_or_default();
        if level.is_empty() {
            continue;
        }
        *levels.entry(level.to_owned()).or_default() += 1;
        seen += 1;
    }
    seen
}

/// A distribution, most frequent first, then by name so JSON stays stable.
fn counted(counts: BTreeMap<String, usize>) -> Vec<Count> {
    let mut counted: Vec<Count> = counts
        .into_iter()
        .map(|(name, count)| Count { name, count })
        .collect();
    counted.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.name.cmp(&right.name))
    });
    counted
}
