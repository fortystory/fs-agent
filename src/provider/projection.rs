//! Projection: `(&[Event], SpeakerId, ModelCaps) -> messages`.
//!
//! A pure function of the stream plus attribution rules. It holds no trimming
//! state; trimming is a separate pure step that lands in ticket 07. The same
//! events always project to the same `messages`, which is what makes "any
//! agent's `messages` is recomputable from the stream + the rules" (spec §5)
//! true.
//!
//! This module owns the one place where "self" versus "other" is decided:
//!
//! * the acting speaker's own turns become `assistant`; everyone else's become
//!   `user` (spec §5), because neither vendor documents how it treats several
//!   consecutive same-role messages, so consecutive other-speaker turns are
//!   merged into one `user` message and each segment inside a discussion round
//!   carries a `[轮 N · 名字]` prefix;
//! * another speaker's tool calls survive as a one-line summary only — the
//!   result body and `reasoning_content` are not projected, and the paired
//!   `tool` result is dropped with them (the wire-level `tool_call` ↔ `tool`
//!   pairing check would reject anything else);
//! * the speaker's own `reasoning_content` is replayed (DeepSeek 400s without
//!   it), and its own tool round-trip plus a `PostToolUse` hook's feedback are
//!   merged by `seq` into the one `tool` message the provider allows.
//!
//! Per-field differences live on [`ModelCaps`] as data, so this file branches
//! on values, never on a vendor name (spec §5).
//!
//! The `[轮 N · 名字]` prefix here is the **model's** prefix. The renderer's
//! human-facing prefix is deliberately a separate generator (spec §5): the two
//! look alike early on, but one repeats per line for a person skimming and the
//! other is written once per merged block for a model.

use std::collections::BTreeSet;

use super::capability::ModelCaps;
use super::{Message, ToolCall};
use crate::events::{hook_format, Event, EventPayload, SpeakerId, ToolCallId};

/// Longest sanitized participant name sent in a `name` field.
///
/// No vendor documents a character set or a length for `name`, so the value is
/// kept to a shape any reasonable implementation accepts (spec §5).
const MAX_NAME_CHARS: usize = 64;

/// Longest rendered argument list kept in another speaker's tool summary.
const MAX_TOOL_SUMMARY_CHARS: usize = 160;

/// Recompute the `messages` an agent should replay from a slice of the event
/// stream.
///
/// The slice rather than the log itself: the stream is append-only, so a prefix
/// of it is a perfectly good input, and a discussion round needs exactly that —
/// a debater's window is cut at its round's `RoundStarted` (spec §15). Passing
/// the events also keeps this function honest about being pure.
///
/// `caps` supplies the provider's field-level facts, so a difference such as
/// whether the model's own reasoning must round-trip is a table value rather
/// than a code path.
pub fn project(events: &[Event], speaker: &SpeakerId, caps: &ModelCaps) -> Vec<Message> {
    let superseded = superseded_seqs(events);
    let mut messages = Vec::new();
    let mut others = OtherBlock::default();
    // Whether the first speech `user` message has been emitted. It is pinned:
    // the head of the conversation must stay byte-stable for the prefix cache,
    // so it never absorbs a later speaker's speech (spec §5). A pinned
    // `ContextInjected` is separate and does not consume this flag.
    let mut head_emitted = false;
    let mut pending: Option<PendingAssistant> = None;
    let mut round: Option<u32> = None;

    for event in events {
        if superseded.contains(&event.seq) {
            continue;
        }
        let mine = &event.speaker_id == speaker;
        // The exhaustive "what enters the model's context" table (spec §5).
        // There is deliberately no `_` arm: a new payload must be classified
        // here rather than silently defaulting to invisible.
        match &event.payload {
            // Session skeleton: identity and harness bookkeeping stay private.
            EventPayload::SessionStarted { .. } => {}
            // A pinned injection belongs to the pinned head and never merges with
            // speech: it must look identical every turn for the prefix cache to
            // keep hitting (spec §5, §10). The leading injections — the project
            // rules and the skills catalog — are **one** `user` message (spec
            // §10, decision 09: "与 AGENTS.md 同一条"), so a run of them merges
            // into a single message rather than becoming consecutive same-role
            // messages. A mid-session injection (plan mode, ticket 15) has
            // history before it and so stays its own message.
            EventPayload::ContextInjected { content, .. } => {
                close_pending_if_settled(&mut messages, &mut pending, speaker);
                flush_others(&mut messages, &mut others, &mut head_emitted);
                let leading = at_pinned_head(&messages);
                match messages.last_mut() {
                    Some(Message::User {
                        content: body,
                        name: None,
                    }) if leading => {
                        body.push('\n');
                        body.push_str(content);
                    }
                    _ => messages.push(Message::User {
                        content: content.clone(),
                        name: None,
                    }),
                }
            }
            EventPayload::SessionEnded { .. } => {}
            // A round is a hard merge boundary, and its number is what the
            // model-side prefix records (spec §5).
            EventPayload::RoundStarted { round: started, .. } => {
                close_pending_if_settled(&mut messages, &mut pending, speaker);
                flush_others(&mut messages, &mut others, &mut head_emitted);
                round = Some(*started);
            }
            EventPayload::RoundEnded { .. } => {
                close_pending_if_settled(&mut messages, &mut pending, speaker);
                flush_others(&mut messages, &mut others, &mut head_emitted);
                round = None;
            }
            EventPayload::DivergenceRecorded { .. } => {}
            EventPayload::TurnStarted { .. } => {}
            EventPayload::MessageCompleted {
                text, reasoning, ..
            } => {
                if mine {
                    close_pending_if_settled(&mut messages, &mut pending, speaker);
                    flush_others(&mut messages, &mut others, &mut head_emitted);
                    pending = Some(PendingAssistant::new(text, reasoning, caps));
                } else if speaks_to_others(&event.speaker_id) {
                    close_pending_if_settled(&mut messages, &mut pending, speaker);
                    push_other(
                        &mut messages,
                        &mut others,
                        &mut head_emitted,
                        round,
                        &event.speaker_id,
                        text,
                    );
                }
            }
            EventPayload::ToolCallStarted {
                tool_call_id,
                tool_name,
                args,
            } => {
                if mine {
                    if let Some(pending) = pending.as_mut() {
                        pending.add_call(tool_call_id, tool_name, args);
                    }
                } else if speaks_to_others(&event.speaker_id) {
                    // Another speaker's tool call survives as one summary line;
                    // its result body does not (spec §5).
                    close_pending_if_settled(&mut messages, &mut pending, speaker);
                    push_other(
                        &mut messages,
                        &mut others,
                        &mut head_emitted,
                        round,
                        &event.speaker_id,
                        &tool_summary(tool_name, args),
                    );
                }
            }
            EventPayload::ToolCallCompleted {
                tool_call_id,
                output,
                error,
                ..
            } => {
                if mine {
                    if let Some(pending) = pending.as_mut() {
                        pending.add_result(tool_call_id, output, error);
                    }
                }
                // An other speaker's result body is never projected.
            }
            EventPayload::UsageRecorded { .. } => {}
            EventPayload::TurnEnded { .. } => {}
            EventPayload::PermissionAsked { .. } => {}
            EventPayload::PermissionDecided { .. } => {}
            EventPayload::HookExecuted { point, outcome, .. } => {
                if mine && point == hook_format::POINT_POST {
                    if let Some(feedback) = hook_format::feedback_text(outcome) {
                        if let Some(pending) = pending.as_mut() {
                            // A post-hook feedback is an appended event, but a
                            // provider allows exactly one `tool` message per
                            // `tool_call`, so it merges into the result it
                            // annotates. `HookExecuted` has no `tool_call_id`,
                            // and the loop emits it immediately after the
                            // result, so "the latest result" is the pairing.
                            pending.add_feedback(feedback);
                        }
                    }
                }
            }
            // An executor reports through the `task` call's tool result and the
            // spawning speaker's own argument, so its process never enters a
            // debater's projection (spec §5).
            EventPayload::ExecutorSpawned { .. } => {}
            EventPayload::ExecutorFinished { .. } => {}
            // The model must see and correct its own error (spec §2); another
            // speaker's error is not this model's to fix.
            EventPayload::AgentError { message, .. } if mine => {
                close_pending_if_settled(&mut messages, &mut pending, speaker);
                flush_others(&mut messages, &mut others, &mut head_emitted);
                messages.push(Message::User {
                    content: message.clone(),
                    name: None,
                });
            }
            EventPayload::AgentError { .. } => {}
            // A session-level failure is not the model's to fix (spec §2).
            EventPayload::SessionError { .. } => {}
            // The superseded range was already filtered out above; the record
            // itself is bookkeeping, not a message.
            EventPayload::HistorySuperseded { .. } => {}
        }
    }

    // The speaker's own group goes before any speech that was buffered while
    // its tool calls were still awaiting results, so a `tool_call` is never
    // separated from the `tool` message that answers it.
    close_pending_if_settled(&mut messages, &mut pending, speaker);
    flush_others(&mut messages, &mut others, &mut head_emitted);
    messages
}

/// Whether an event from this speaker enters someone else's context.
///
/// Only executors are hidden: a debater sees another debater, the synthesizer
/// (`System`) and the human. An executor's own projection is full, so this is
/// consulted only for events that are not the acting speaker's.
fn speaks_to_others(from: &SpeakerId) -> bool {
    !matches!(from, SpeakerId::Executor(_))
}

/// Whether nothing but pinned injections has been emitted yet, so a new
/// injection still belongs to the leading block and merges into it.
///
/// A pinned injection is the only name-less `user` message at this point: speech
/// carries a speaker `name`, and an `AgentError` cannot precede the session-start
/// injections. An empty slice means the first push, where `last_mut` finds
/// nothing and pushes instead of merging.
fn at_pinned_head(messages: &[Message]) -> bool {
    messages
        .iter()
        .all(|message| matches!(message, Message::User { name: None, .. }))
}

/// Every `seq` a [`EventPayload::HistorySuperseded`] event has retired.
fn superseded_seqs(events: &[Event]) -> BTreeSet<u64> {
    let mut retired = BTreeSet::new();
    for event in events {
        if let EventPayload::HistorySuperseded { targets, .. } = &event.payload {
            retired.extend(targets.iter().copied());
        }
    }
    retired
}

/// Add one other-speaker segment, pinning the first `user` message.
fn push_other(
    messages: &mut Vec<Message>,
    others: &mut OtherBlock,
    head_emitted: &mut bool,
    round: Option<u32>,
    speaker: &SpeakerId,
    body: &str,
) {
    if body.is_empty() {
        return;
    }
    let name = speaker_name(speaker);
    if !*head_emitted && !others.is_empty() && !others.has_speaker(&name) {
        // The pinned head ends where a different speaker begins.
        others.flush_into(messages);
        *head_emitted = true;
    }
    others.push(round, &name, body);
}

fn flush_others(messages: &mut Vec<Message>, others: &mut OtherBlock, head_emitted: &mut bool) {
    if others.flush_into(messages) {
        *head_emitted = true;
    }
}

/// A run of consecutive other-speaker turns, merged into one `user` message.
///
/// Merging is by role sequence with no count threshold (spec §5): the goal is
/// to avoid the vendors' undocumented behaviour on consecutive same-role
/// messages, not to shorten anything.
#[derive(Default)]
struct OtherBlock {
    segments: Vec<OtherSegment>,
}

struct OtherSegment {
    speaker: String,
    line: String,
}

impl OtherBlock {
    fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    fn has_speaker(&self, name: &str) -> bool {
        self.segments.iter().any(|segment| segment.speaker == name)
    }

    fn push(&mut self, round: Option<u32>, speaker: &str, body: &str) {
        if body.is_empty() {
            return;
        }
        // Outside a discussion round there is no `N` to write, and a plain CLI
        // session has exactly one other speaker, so the text stays bare and
        // `name` carries the attribution.
        let line = match round {
            Some(round) => format!("[轮 {round} · {speaker}] {body}"),
            None => body.to_owned(),
        };
        self.segments.push(OtherSegment {
            speaker: speaker.to_owned(),
            line,
        });
    }

    /// Emit the merged block, reporting whether one was emitted.
    fn flush_into(&mut self, messages: &mut Vec<Message>) -> bool {
        if self.segments.is_empty() {
            return false;
        }
        // `name` is an enhancement, never the attribution guarantee: the body
        // prefix is. A merged block with several speakers gets no single name.
        let mut names: Vec<&str> = Vec::new();
        for segment in &self.segments {
            if !names.contains(&segment.speaker.as_str()) {
                names.push(&segment.speaker);
            }
        }
        let name = (names.len() == 1).then(|| names[0].to_owned());
        let content = self
            .segments
            .iter()
            .map(|segment| segment.line.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        self.segments.clear();
        messages.push(Message::User { content, name });
        true
    }
}

/// The acting speaker's in-flight assistant message and its tool results.
struct PendingAssistant {
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Vec<ToolCall>,
    results: Vec<Message>,
}

impl PendingAssistant {
    fn new(text: &str, reasoning: &Option<String>, caps: &ModelCaps) -> Self {
        Self {
            content: (!text.is_empty()).then(|| text.to_owned()),
            // Whether the model's own reasoning must round-trip is a capability
            // fact, not a branch on the vendor (spec §4, §5).
            reasoning_content: reasoning.clone().filter(|_| caps.requires_reasoning_replay),
            tool_calls: Vec::new(),
            results: Vec::new(),
        }
    }

    /// Whether a `tool_call` still has no `tool` message. A settled group may
    /// be emitted; an unsettled one must stay open so its pairing survives an
    /// interleaving (which a valid turn never produces).
    fn awaiting_results(&self) -> bool {
        self.results.len() < self.tool_calls.len()
    }

    fn add_call(&mut self, tool_call_id: &ToolCallId, tool_name: &str, args: &serde_json::Value) {
        self.tool_calls.push(ToolCall {
            id: tool_call_id.as_str().to_owned(),
            name: tool_name.to_owned(),
            arguments: args.to_string(),
        });
    }

    /// One `tool_call` gets exactly one `tool` message; its body already merged
    /// the result and any post-hook feedback.
    fn add_result(
        &mut self,
        tool_call_id: &ToolCallId,
        output: &Option<String>,
        error: &Option<String>,
    ) {
        let content = output.clone().or_else(|| error.clone()).unwrap_or_default();
        self.results.push(Message::Tool {
            tool_call_id: tool_call_id.as_str().to_owned(),
            content,
        });
    }

    /// Merge feedback into the result it annotates. `failed:` outcomes never
    /// reach here, which is what makes "a post-hook failure only loses
    /// feedback" true on the model's side too.
    fn add_feedback(&mut self, feedback: &str) {
        if let Some(Message::Tool { content, .. }) = self.results.last_mut() {
            content.push_str("\n\n");
            content.push_str(hook_format::FEEDBACK_MARKER);
            content.push(' ');
            content.push_str(feedback);
        }
    }
}

/// Emit the acting speaker's assistant group, unless its `tool_call`s are still
/// awaiting results that a later event may carry.
fn close_pending_if_settled(
    messages: &mut Vec<Message>,
    pending: &mut Option<PendingAssistant>,
    speaker: &SpeakerId,
) {
    if pending
        .as_ref()
        .is_some_and(PendingAssistant::awaiting_results)
    {
        return;
    }
    let Some(pending) = pending.take() else {
        return;
    };
    messages.push(Message::Assistant {
        content: pending.content,
        reasoning_content: pending.reasoning_content,
        tool_calls: pending.tool_calls,
        name: Some(speaker_name(speaker)),
    });
    messages.extend(pending.results);
}

/// The model-side name for a speaker, sanitized to `[A-Za-z0-9_-]`.
///
/// No vendor documents a character set or length for `name`; the body prefix
/// carries the same information, so a provider that ignores or truncates this
/// loses nothing (spec §5). `SpeakerId`'s display form is the single source of
/// the raw value (`executor:<id>` sanitizes to `executor-<id>`).
fn speaker_name(speaker: &SpeakerId) -> String {
    sanitize_name(&speaker.to_string())
}

fn sanitize_name(raw: &str) -> String {
    let sanitized: String = raw
        .chars()
        .take(MAX_NAME_CHARS)
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "unknown".to_owned()
    } else {
        sanitized
    }
}

/// One line for another speaker's tool call: the tool name plus a truncated
/// rendering of its arguments, keeping "what did they actually check"
/// answerable without paying for the result body (spec §5).
fn tool_summary(tool_name: &str, args: &serde_json::Value) -> String {
    let rendered = match args {
        serde_json::Value::Null => String::new(),
        other => truncate(&other.to_string(), MAX_TOOL_SUMMARY_CHARS),
    };
    if rendered.is_empty() || rendered == "{}" {
        format!("→ {tool_name}")
    } else {
        format!("→ {tool_name}({rendered})")
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut truncated: String = text.chars().take(max_chars).collect();
    truncated.push('…');
    truncated
}
