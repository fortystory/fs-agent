//! The shared presentation layer: events become [`Block`]s once, and plain and
//! TUI paint those blocks their own way.
//!
//! Two rules make this more than a formatting pass:
//!
//! * **A tool call and its result are one block.** The post-hook's feedback
//!   carries no `tool_call_id` — the loop emits it immediately after the result
//!   it annotates — so the call is held open until the next unrelated event
//!   arrives and the feedback (if any) has been merged in. That is the same
//!   grouping `session::observe` performs on a finished stream, done
//!   incrementally for a live one.
//! * **Incremental text passes straight through.** Deltas bypass the log, so
//!   they cannot be re-derived later; the transcript forwards them so a renderer
//!   can paint them as they arrive.

use serde_json::Value;

use crate::events::{
    hook_format, ContextSource, Decision, DecisionSource, Event, EventPayload, HistoryReason,
    ParticipantId, Role, RoundMode, SpeakerId, StopReason, ToolCallId, Usage,
};

use super::{DeltaKind, RenderEvent};

/// One display-ready unit of the transcript.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// Incremental model output, on its way to the renderer.
    Delta {
        speaker: SpeakerId,
        kind: DeltaKind,
        text: String,
    },
    /// A completed message. Its text was already streamed as deltas; a renderer
    /// uses this for the message boundary, not to re-print the body.
    Message {
        speaker: SpeakerId,
        role: Role,
        text: String,
    },
    RoundStarted {
        round: u32,
        mode: RoundMode,
    },
    RoundEnded {
        round: u32,
        reason: StopReason,
    },
    Divergence {
        topic: String,
        positions: Vec<String>,
    },
    /// A tool call, its result, and any post-hook feedback: one block.
    Tool(Box<ToolBlock>),
    TurnStarted {
        speaker: SpeakerId,
        iteration: u32,
    },
    TurnEnded {
        speaker: SpeakerId,
        reason: StopReason,
    },
    PermissionAsked {
        speaker: SpeakerId,
        /// The tool the question is about, when the stream recorded one. It is
        /// what the question is actually asking about, so a narration that shows
        /// only the two ids is unreadable.
        tool_name: Option<String>,
        request_id: String,
        tool_call_id: ToolCallId,
    },
    PermissionDecided {
        speaker: SpeakerId,
        decision: Decision,
        source: DecisionSource,
        reason: Option<String>,
    },
    /// A pre-hook outcome. Post-hook feedback rides on its [`ToolBlock`].
    Hook {
        speaker: SpeakerId,
        point: String,
        outcome: String,
    },
    ExecutorSpawned {
        speaker: SpeakerId,
        executor_id: ParticipantId,
    },
    ExecutorFinished {
        executor_id: ParticipantId,
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
    },
    SessionError {
        code: String,
        detail: String,
    },
    SessionEnded {
        reason: StopReason,
    },
    ContextInjected {
        source: ContextSource,
    },
    History {
        reason: HistoryReason,
        summary: Option<String>,
    },
    Diagnostic(String),
    /// A line that speaks for no speaker and narrates no event, shown as it is.
    Notice(String),
}

/// One tool call, held open until its result (and any post-hook feedback) has
/// arrived.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolBlock {
    pub speaker: SpeakerId,
    pub tool_call_id: ToolCallId,
    pub tool: String,
    pub args: Value,
    pub outcome: Option<ToolOutcome>,
    /// A post-hook's outcome for this call, merged here because the hook event
    /// carries no `tool_call_id` and the call is what it annotated.
    pub hook: Option<String>,
}

/// What a finished (or abandoned) tool call produced.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutcome {
    pub ok: bool,
    pub output: Option<String>,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// The incremental event-to-block state machine.
#[derive(Debug, Default)]
pub struct Transcript {
    pending_tool: Option<ToolBlock>,
}

impl Transcript {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one render event and take the blocks it produced.
    ///
    /// A tool call produces nothing until it is complete (or flushed): that is
    /// what lets a post-hook's feedback land inside the same block.
    pub fn push(&mut self, event: RenderEvent) -> Vec<Block> {
        match event {
            RenderEvent::Delta {
                speaker,
                kind,
                text,
            } => {
                let mut blocks = self.flush();
                blocks.push(Block::Delta {
                    speaker,
                    kind,
                    text,
                });
                blocks
            }
            RenderEvent::Diagnostic(message) => {
                let mut blocks = self.flush();
                blocks.push(Block::Diagnostic(message));
                blocks
            }
            RenderEvent::Notice(message) => {
                let mut blocks = self.flush();
                blocks.push(Block::Notice(message));
                blocks
            }
            RenderEvent::Logged(event) => self.push_logged(event),
        }
    }

    /// Close an open tool block, if any. Call at end of stream so a call whose
    /// result never arrived is still shown.
    pub fn flush(&mut self) -> Vec<Block> {
        match self.pending_tool.take() {
            Some(tool) => vec![Block::Tool(Box::new(tool))],
            None => Vec::new(),
        }
    }

    fn push_logged(&mut self, event: Event) -> Vec<Block> {
        let speaker = event.speaker_id.clone();
        // A post-hook annotates the call it followed, so it must not flush it.
        if let EventPayload::HookExecuted { point, outcome, .. } = &event.payload {
            if point == hook_format::POINT_POST {
                if let Some(tool) = self.pending_tool.as_mut() {
                    tool.hook = Some(outcome.clone());
                    return Vec::new();
                }
            }
        }

        // These events belong to the interval between a call's start and its
        // result: they are narrated, but they must not close the open block.
        // `PermissionAsked`/`PermissionDecided` are the load-bearing case — the
        // loop records a decision for **every** call, asked or not — and the
        // pre-hook fires after `ToolCallStarted` too, so treating any of them as
        // "unrelated" would split every tool call in two.
        let inside_a_call = matches!(
            &event.payload,
            EventPayload::ToolCallCompleted { .. }
                | EventPayload::PermissionAsked { .. }
                | EventPayload::PermissionDecided { .. }
                | EventPayload::HookExecuted { .. }
        );
        let mut blocks = if inside_a_call {
            Vec::new()
        } else {
            self.flush()
        };
        match event.payload {
            EventPayload::ToolCallStarted {
                tool_call_id,
                tool_name,
                args,
            } => {
                self.pending_tool = Some(ToolBlock {
                    speaker,
                    tool_call_id,
                    tool: tool_name,
                    args,
                    outcome: None,
                    hook: None,
                });
                return blocks;
            }
            EventPayload::ToolCallCompleted {
                tool_call_id,
                ok,
                output,
                error,
                duration_ms,
            } => {
                if let Some(tool) = self.pending_tool.as_mut() {
                    if tool.tool_call_id == tool_call_id {
                        tool.outcome = Some(ToolOutcome {
                            ok,
                            output,
                            error,
                            duration_ms,
                        });
                        return Vec::new();
                    }
                }
                // A result with no matching start: surface it rather than drop
                // it, so the transcript still shows that something finished.
                let mut blocks = self.flush();
                blocks.push(Block::Tool(Box::new(ToolBlock {
                    speaker,
                    tool_call_id,
                    tool: "?".to_owned(),
                    args: Value::Null,
                    outcome: Some(ToolOutcome {
                        ok,
                        output,
                        error,
                        duration_ms,
                    }),
                    hook: None,
                })));
                return blocks;
            }
            EventPayload::MessageCompleted { role, text, .. } => {
                blocks.push(Block::Message {
                    speaker,
                    role,
                    text,
                });
            }
            EventPayload::RoundStarted { round, mode } => {
                blocks.push(Block::RoundStarted { round, mode });
            }
            EventPayload::RoundEnded { round, reason } => {
                blocks.push(Block::RoundEnded { round, reason });
            }
            EventPayload::DivergenceRecorded {
                topic, positions, ..
            } => {
                blocks.push(Block::Divergence { topic, positions });
            }
            EventPayload::TurnStarted { iteration, .. } => {
                blocks.push(Block::TurnStarted { speaker, iteration });
            }
            EventPayload::TurnEnded { reason } => {
                blocks.push(Block::TurnEnded { speaker, reason });
            }
            EventPayload::PermissionAsked {
                request_id,
                tool_call_id,
                request,
            } => {
                blocks.push(Block::PermissionAsked {
                    speaker,
                    tool_name: crate::events::permission_format::tool_name(&request)
                        .map(str::to_owned),
                    request_id,
                    tool_call_id,
                });
            }
            EventPayload::PermissionDecided {
                decision,
                source,
                reason,
                ..
            } => {
                blocks.push(Block::PermissionDecided {
                    speaker,
                    decision,
                    source,
                    reason,
                });
            }
            EventPayload::HookExecuted { point, outcome, .. } => {
                blocks.push(Block::Hook {
                    speaker,
                    point,
                    outcome,
                });
            }
            EventPayload::ExecutorSpawned { executor_id, .. } => {
                blocks.push(Block::ExecutorSpawned {
                    speaker,
                    executor_id,
                });
            }
            EventPayload::ExecutorFinished {
                executor_id,
                reason,
                summary,
            } => {
                blocks.push(Block::ExecutorFinished {
                    executor_id,
                    reason,
                    summary,
                });
            }
            EventPayload::UsageRecorded { usage } => {
                blocks.push(Block::Usage { speaker, usage });
            }
            EventPayload::AgentError { message, .. } => {
                blocks.push(Block::AgentError { speaker, message });
            }
            EventPayload::SessionError { code, detail } => {
                blocks.push(Block::SessionError { code, detail });
            }
            EventPayload::SessionEnded { reason } => {
                blocks.push(Block::SessionEnded { reason });
            }
            EventPayload::ContextInjected { source, .. } => {
                blocks.push(Block::ContextInjected { source });
            }
            EventPayload::HistorySuperseded {
                reason, summary, ..
            } => {
                blocks.push(Block::History { reason, summary });
            }
            // The session skeleton is not narration a person reads live.
            EventPayload::SessionStarted { .. } => {}
        }
        blocks
    }
}

/// The one-line summary of a tool call's arguments.
///
/// Values are rendered compactly and the whole thing is capped, so a call with a
/// large body still reads as one line — the same reason `docs` describe the
/// transcript as a log and not a debugger.
pub fn summarize_args(args: &Value) -> String {
    const MAX: usize = 160;
    let rendered = match args {
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| format!("{key}={}", summarize_value(value)))
            .collect::<Vec<_>>()
            .join(", "),
        Value::Null => String::new(),
        other => summarize_value(other),
    };
    truncate(&rendered, MAX)
}

fn summarize_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.replace('\n', "\\n"),
        other => other.to_string(),
    }
}

/// Truncate to `max` characters, marking that it happened.
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}
