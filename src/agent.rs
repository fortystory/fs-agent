//! The turn loop.
//!
//! The `agent` module is the only writer of the event stream, and the loop is
//! the only place that calls the provider. Hooks and the permission gate become
//! pure value transformations it applies in a fixed order (spec §3); ticket 01
//! has neither yet, so the order is `project -> provider -> append`.
//!
//! Three invariants hold from this first ticket onward:
//!
//! 1. every `tool_call` gets exactly one result;
//! 2. the provider is never called while a `tool_call` lacks a result;
//! 3. only this loop writes to the log.

use futures::StreamExt;

use crate::events::{
    last_assistant_has_tool_calls, pending_tool_calls, EventPayload, Role, SpeakerId, StopReason,
    ToolCallId, SCHEMA_VERSION,
};
use crate::provider::projection::project;
use crate::provider::{ChatRequest, Provider, StreamEvent, ToolCall, ToolChoice};
use crate::render::RenderHandle;
use crate::session::Session;
use crate::Error;

/// How a turn ended, plus the assistant text of its last message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnOutcome {
    pub reason: StopReason,
    pub text: String,
}

/// Record the session skeleton. The agent module owns every write to the log,
/// so even the one-off `SessionStarted` event is recorded here.
pub fn record_session_started(session: &mut Session, render: &RenderHandle) -> Result<(), Error> {
    emit(
        session,
        render,
        &SpeakerId::System,
        EventPayload::SessionStarted {
            session_id: session.id().clone(),
            cwd: session.cwd().to_string_lossy().into_owned(),
            schema_version: SCHEMA_VERSION,
        },
    )
}

/// Record the user's own message before a turn runs.
pub fn record_user_message(
    session: &mut Session,
    render: &RenderHandle,
    text: &str,
) -> Result<(), Error> {
    emit(
        session,
        render,
        &SpeakerId::User,
        EventPayload::MessageCompleted {
            role: Role::User,
            text: text.to_owned(),
            reasoning: None,
        },
    )
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
            return end_turn(session, render, speaker, StopReason::Error, last_text);
        }

        if iteration >= max_iterations {
            return end_turn(
                session,
                render,
                speaker,
                StopReason::MaxIterations,
                last_text,
            );
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
            params: session.config().params.clone(),
            cache_key: Some(session.id().as_str().to_owned()),
        };

        let mut stream = match provider.send(request).await {
            Ok(stream) => stream,
            Err(error) => {
                render.diagnostic(&format!("provider error: {error}"));
                return end_turn(session, render, speaker, StopReason::Error, last_text);
            }
        };

        let mut text = String::new();
        let mut reasoning = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut failed = false;
        let mut saw_done = false;

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
                    // The stream ended on `[DONE]`. `finish_reason` is diagnostic
                    // only; the turn's stop reason comes from the loop's own
                    // continuation query, never from the provider.
                    render.diagnostic(&format!("provider stream finished: {finish_reason:?}"));
                    saw_done = true;
                    break;
                }
                Err(error) => {
                    render.diagnostic(&format!("provider stream error: {error}"));
                    failed = true;
                    break;
                }
            }
        }

        // Only `[DONE]` ends a message: a stream that failed or just stopped
        // without it produced no completed unit, so nothing lands in the log.
        if failed || !saw_done {
            if !failed {
                render.diagnostic("provider stream ended without [DONE]");
            }
            return end_turn(session, render, speaker, StopReason::Error, text);
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

        // Ticket 01 has no tool registry yet, so every call gets an explicit
        // failure result. This keeps "exactly one result per tool_call" true and
        // makes the continuation path real; ticket 03 replaces it with dispatch.
        for call in &tool_calls {
            emit(
                session,
                render,
                speaker,
                EventPayload::ToolCallStarted {
                    tool_call_id: ToolCallId::new(call.id.clone()),
                    tool_name: call.name.clone(),
                    args: parse_tool_args(&call.arguments),
                },
            )?;
            emit(
                session,
                render,
                speaker,
                EventPayload::ToolCallCompleted {
                    tool_call_id: ToolCallId::new(call.id.clone()),
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

        return end_turn(session, render, speaker, StopReason::Completed, last_text);
    }
}

/// Record the reason the turn stopped and return the outcome.
fn end_turn(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    reason: StopReason,
    text: String,
) -> Result<TurnOutcome, Error> {
    emit(session, render, speaker, EventPayload::TurnEnded { reason })?;
    Ok(TurnOutcome { reason, text })
}

/// Arguments arrive as a JSON string; a call with no parameters sends nothing,
/// which means "no arguments" rather than the JSON literal `null`.
fn parse_tool_args(arguments: &str) -> serde_json::Value {
    if arguments.trim().is_empty() {
        return serde_json::json!({});
    }
    serde_json::from_str(arguments).unwrap_or(serde_json::Value::Null)
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
