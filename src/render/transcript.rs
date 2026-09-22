//! The shared presentation layer: events become [`Block`]s once, and plain and
//! TUI paint those blocks their own way.
//!
//! Two rules make this more than a formatting pass:
//!
//! * **A tool call and its result are one block, painted on the result.** The
//!   post-hook's feedback carries no `tool_call_id` — the loop emits it immediately
//!   after the result it annotates — so it travels as its own small block, aimed at
//!   the call that has just been painted. Holding the call open to merge it instead
//!   made the call invisible for its whole run: the TUI only painted it once some
//!   later event arrived — the next answer's first streaming delta, or the turn's end
//!   (票 02 §3).
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
        /// The finished reasoning trace, when the provider sent one. This is the only
        /// place the whole trace exists — the deltas are incremental and the log has
        /// no separate reasoning event — so it is what the transcript's "thinking
        /// finished" line holds open for its detail view (票 02 §1).
        reasoning: Option<String>,
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
    /// A tool call and its result: one block, emitted as soon as the result lands.
    Tool(Box<ToolBlock>),
    /// A post-hook's feedback for the call it annotates.
    ///
    /// It travels on its own block because the call it annotates has already been
    /// painted: the loop emits the feedback immediately after the result, and holding
    /// the call open to wait for it is what used to hide the call line for the whole
    /// run (票 02 §3).
    ToolFeedback {
        outcome: String,
    },
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
        /// The tool the question is about, when the stream recorded one.
        tool_name: Option<String>,
        /// The arguments of the call it is asking about, so a painter can show
        /// the command or path a person is approving.
        args: Value,
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
    /// A call has been painted and its post-hook — which carries no `tool_call_id` —
    /// may still be on its way. The next post-hook to arrive annotates that call.
    awaiting_hook: bool,
}

impl Transcript {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one render event and take the blocks it produced.
    ///
    /// A tool call is painted as soon as its **result** arrives. It used to wait for
    /// the next unrelated event so a post-hook could be merged into the same block,
    /// which meant a call was invisible for its whole run — and, in the TUI, until the
    /// model's *next* answer had finished streaming (票 02 §3).
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

    /// Close an open tool block, if any.
    ///
    /// The result is what paints a call, so this is only the fallback: a call whose
    /// result never arrived — a stream that died mid-call. [`Plain`] calls it at end of
    /// stream, so the line still reaches the page. The TUI cannot show it: it has no
    /// frame after the stream closes (a cancelled call is *not* this case — the loop
    /// writes a synthesized result for every call it started, and that result paints
    /// the call).
    ///
    /// [`Plain`]: crate::render::Plain
    pub fn flush(&mut self) -> Vec<Block> {
        match self.pending_tool.take() {
            Some(tool) => vec![Block::Tool(Box::new(tool))],
            None => Vec::new(),
        }
    }

    fn push_logged(&mut self, event: Event) -> Vec<Block> {
        let speaker = event.speaker_id.clone();
        // The expectation lasts exactly one event: the loop emits a call's post-hook
        // immediately after the result it annotates, so anything else arriving first
        // means the hook is not coming — and a hook that then turned up much later
        // must not be pinned onto a call it never annotated. Only the matched-result
        // arm below re-arms it.
        let expected_hook = std::mem::take(&mut self.awaiting_hook);
        if let EventPayload::HookExecuted { point, outcome, .. } = &event.payload {
            if point == hook_format::POINT_POST {
                let mut blocks = self.flush();
                if expected_hook || !blocks.is_empty() {
                    blocks.push(Block::ToolFeedback {
                        outcome: outcome.clone(),
                    });
                    return blocks;
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
                // A new call supersedes any feedback still expected for the last one.
                self.awaiting_hook = false;
                self.pending_tool = Some(ToolBlock {
                    speaker,
                    tool_call_id,
                    tool: tool_name,
                    args,
                    outcome: None,
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
                if self
                    .pending_tool
                    .as_ref()
                    .is_some_and(|tool| tool.tool_call_id == tool_call_id)
                {
                    let mut tool = self.pending_tool.take().expect("just matched");
                    tool.outcome = Some(ToolOutcome {
                        ok,
                        output,
                        error,
                        duration_ms,
                    });
                    // This call may still be annotated by the post-hook that follows.
                    self.awaiting_hook = true;
                    blocks.push(Block::Tool(Box::new(tool)));
                    return blocks;
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
                })));
                return blocks;
            }
            EventPayload::MessageCompleted {
                role,
                text,
                reasoning,
            } => {
                blocks.push(Block::Message {
                    speaker,
                    role,
                    text,
                    reasoning,
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
            EventPayload::PermissionAsked { request, .. } => {
                blocks.push(Block::PermissionAsked {
                    speaker,
                    tool_name: crate::events::permission_format::tool_name(&request)
                        .map(str::to_owned),
                    args: crate::events::permission_format::args(&request)
                        .cloned()
                        .unwrap_or(Value::Null),
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

/// The one-line summary of what a permission question would run, from an event's
/// `request` value. Empty when the stream recorded no arguments.
pub fn summarize_permission_target(request: &Value) -> String {
    match crate::events::permission_format::args(request) {
        Some(args) => summarize_args(args),
        None => String::new(),
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
