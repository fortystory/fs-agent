//! The turn loop.
//!
//! This is the only place that writes to the event log and the only place that
//! calls the provider. Hooks and the permission gate become pure value
//! transformations it applies in a fixed order (spec §3); ticket 01 has neither
//! yet, so the order is `project -> provider -> append`.
//!
//! Three invariants hold from this first ticket onward:
//!
//! 1. every `tool_call` gets exactly one result;
//! 2. the provider is never called while a `tool_call` lacks a result;
//! 3. only this loop writes to the log.

use futures::StreamExt;

use crate::events::{
    last_assistant_has_tool_calls, pending_tool_calls, EventPayload, Role, SpeakerId, StopReason,
    ToolCallId,
};
use crate::provider::projection::project;
use crate::provider::{ChatRequest, GenerationParams, Provider, StreamEvent, ToolCall, ToolChoice};
use crate::render::RenderHandle;
use crate::session::Session;
use crate::Error;

/// How a turn ended, plus the assistant text of its last message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnOutcome {
    pub reason: StopReason,
    pub text: String,
}

/// Run one complete turn for `speaker` and return why it stopped.
pub async fn run_turn(
    session: &mut Session,
    speaker: &SpeakerId,
    provider: &dyn Provider,
    render: &RenderHandle,
) -> Result<TurnOutcome, Error> {
    let max_iterations = session.config().max_iterations;
    let mut iteration: u32 = 0;
    let mut last_text = String::new();

    loop {
        // Invariant 2: a pending tool_call means the log is owed a result, so
        // the provider must not be called. Ticket 01 always resolves its calls
        // before looping, so this only fires if a future change breaks that.
        if !pending_tool_calls(session.events()).is_empty() {
            emit(
                session,
                render,
                speaker,
                EventPayload::TurnEnded {
                    reason: StopReason::Error,
                },
            )?;
            return Ok(TurnOutcome {
                reason: StopReason::Error,
                text: last_text,
            });
        }

        if iteration >= max_iterations {
            emit(
                session,
                render,
                speaker,
                EventPayload::TurnEnded {
                    reason: StopReason::MaxIterations,
                },
            )?;
            return Ok(TurnOutcome {
                reason: StopReason::MaxIterations,
                text: last_text,
            });
        }
        iteration += 1;

        emit(
            session,
            render,
            speaker,
            EventPayload::TurnStarted {
                agent: speaker.clone(),
                iteration,
            },
        )?;

        let request = ChatRequest {
            model: session.config().model.clone(),
            messages: project(session.events(), speaker),
            tools: Vec::new(),
            tool_choice: ToolChoice::Auto,
            params: GenerationParams::default(),
            cache_key: Some(session.id().0.clone()),
        };

        let mut stream = match provider.send(request).await {
            Ok(stream) => stream,
            Err(error) => {
                render.diagnostic(&format!("provider error: {error}"));
                emit(
                    session,
                    render,
                    speaker,
                    EventPayload::TurnEnded {
                        reason: StopReason::Error,
                    },
                )?;
                return Ok(TurnOutcome {
                    reason: StopReason::Error,
                    text: last_text,
                });
            }
        };

        let mut text = String::new();
        let mut reasoning = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut failed = false;

        while let Some(item) = stream.next().await {
            match item {
                Ok(StreamEvent::TextDelta(delta)) => {
                    render.text_delta(speaker, &delta);
                    text.push_str(&delta);
                }
                Ok(StreamEvent::ReasoningDelta(delta)) => {
                    render.reasoning_delta(speaker, &delta);
                    reasoning.push_str(&delta);
                }
                Ok(StreamEvent::ToolCallStarted { .. }) => {
                    // Fragments are assembled by the adapter; the loop only sees
                    // the completed call.
                }
                Ok(StreamEvent::ToolCallCompleted {
                    id,
                    name,
                    arguments,
                    ..
                }) => {
                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments,
                    });
                }
                Ok(StreamEvent::Usage(usage)) => {
                    emit(
                        session,
                        render,
                        speaker,
                        EventPayload::UsageRecorded { usage },
                    )?;
                }
                Ok(StreamEvent::Finished { finish_reason }) => {
                    // Diagnostic only. The turn's stop reason comes from the
                    // loop's own continuation query, never from the provider.
                    render.diagnostic(&format!("provider stream finished: {finish_reason:?}"));
                    break;
                }
                Err(error) => {
                    render.diagnostic(&format!("provider stream error: {error}"));
                    failed = true;
                    break;
                }
            }
        }

        if !text.is_empty() || !reasoning.is_empty() || !tool_calls.is_empty() {
            emit(
                session,
                render,
                speaker,
                EventPayload::MessageCompleted {
                    role: Role::Assistant,
                    text: text.clone(),
                    reasoning: (!reasoning.is_empty()).then(|| reasoning.clone()),
                },
            )?;
        }

        if failed {
            emit(
                session,
                render,
                speaker,
                EventPayload::TurnEnded {
                    reason: StopReason::Error,
                },
            )?;
            return Ok(TurnOutcome {
                reason: StopReason::Error,
                text,
            });
        }

        // Ticket 01 has no tool registry yet, so every call gets an explicit
        // failure result. This keeps "exactly one result per tool_call" true and
        // makes the continuation path real; ticket 03 replaces it with dispatch.
        for call in &tool_calls {
            emit(
                session,
                render,
                speaker,
                EventPayload::ToolCallStarted {
                    tool_call_id: ToolCallId(call.id.clone()),
                    tool_name: call.name.clone(),
                    args: serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null),
                },
            )?;
            emit(
                session,
                render,
                speaker,
                EventPayload::ToolCallCompleted {
                    tool_call_id: ToolCallId(call.id.clone()),
                    ok: false,
                    output: None,
                    error: Some(format!("no tool registered: {}", call.name)),
                    duration_ms: 0,
                },
            )?;
        }

        last_text = text;

        if last_assistant_has_tool_calls(session.events(), speaker) {
            continue;
        }

        emit(
            session,
            render,
            speaker,
            EventPayload::TurnEnded {
                reason: StopReason::Completed,
            },
        )?;
        return Ok(TurnOutcome {
            reason: StopReason::Completed,
            text: last_text,
        });
    }
}

fn emit(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    payload: EventPayload,
) -> Result<(), Error> {
    let event = session.append(speaker.clone(), payload)?;
    render.logged(&event);
    Ok(())
}
