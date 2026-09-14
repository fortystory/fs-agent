//! The turn loop.
//!
//! The `agent` module is the only writer of the event stream, and the loop is
//! the only place that calls the provider. Hooks and the permission gate are
//! pure value transformations it applies in a fixed order (spec §3): the gate
//! (ticket 04) already sits between the provider and dispatch — `hook.pre`
//! arrives in ticket 05 — so the order is
//! `project -> provider -> gate -> [ask] -> dispatch -> append`.
//!
//! Three invariants hold from the first ticket onward:
//!
//! 1. every `tool_call` gets exactly one result;
//! 2. the provider is never called while a `tool_call` lacks a result;
//! 3. only this loop writes to the log.

use std::time::Instant;

use futures::StreamExt;

use crate::events::{
    last_assistant_has_tool_calls, pending_tool_calls, Decision, DecisionSource, EventPayload,
    Role, SpeakerId, StopReason, ToolCallId, SCHEMA_VERSION,
};
use crate::permissions::{self, Answer, PermissionRequest};
use crate::provider::projection::project;
use crate::provider::{ChatRequest, Provider, StreamEvent, ToolCall, ToolChoice};
use crate::render::RenderHandle;
use crate::session::Session;
use crate::tools::{CallFacts, DispatchOutcome, GuardedCall, PendingCall, ToolError};
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
            tools: session.tools().specs(),
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

        // Every `tool_call` gets exactly one result, produced here and nowhere
        // else. The permission gate runs first; a refusal (a policy deny, a user
        // deny, or a headless `Ask` downgrade) synthesizes its one error result
        // here, so the tool is never reached. The dispatcher then owns the
        // shared guardrails (read before edit, the per-path write locks,
        // read-set invalidation) so no tool can opt out.
        for call in &tool_calls {
            let tool_call_id = ToolCallId::new(call.id.clone());
            let args = parse_tool_args(&call.arguments);
            emit(
                session,
                render,
                speaker,
                EventPayload::ToolCallStarted {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: call.name.clone(),
                    args: args.clone(),
                },
            )?;

            // Everything is read off the session before the read set is borrowed.
            let paths = session.paths().clone();
            let locks = session.path_locks().clone();
            let pending = PendingCall {
                tool_call_id: tool_call_id.as_str().to_owned(),
                tool_name: call.name.clone(),
                args,
                outputs_dir: session.outputs_dir().to_path_buf(),
                paths,
                locks,
            };

            let started = Instant::now();
            // Resolve the call once: the gate and the guardrails read the same
            // facts, and this is the only step that touches the filesystem for
            // path resolution.
            let facts = session
                .tools()
                .facts(&pending.tool_name, &pending.args, &pending.paths);

            let outcome = match facts {
                Err(error) => DispatchOutcome::failure(error, false),
                Ok(facts) => {
                    match authorize(
                        session,
                        render,
                        speaker,
                        &tool_call_id,
                        &pending.args,
                        &facts,
                    )
                    .await?
                    {
                        Authorized::Refuse { message } => {
                            DispatchOutcome::failure(ToolError::message(message), false)
                        }
                        Authorized::Allow => {
                            // The guardrails are a pure read of the facts plus
                            // this agent's read set; the decision is applied to
                            // the read set here, in the loop.
                            match facts.guardrails(session.read_set()) {
                                GuardedCall::Refused(error) => {
                                    DispatchOutcome::failure(error, false)
                                }
                                GuardedCall::Run(allowed) => {
                                    let outcome =
                                        session.tools().dispatch(&pending, &allowed).await;
                                    // A read is only a read if it worked: a
                                    // failed read must not license a later write.
                                    if outcome.is_ok() {
                                        session.record_reads(&allowed.read_paths);
                                    }
                                    if outcome.invalidated_reads {
                                        if let Some(path) = outcome
                                            .result
                                            .as_ref()
                                            .err()
                                            .and_then(ToolError::invalidated_path)
                                        {
                                            session.invalidate_read(path);
                                        }
                                    }
                                    outcome
                                }
                            }
                        }
                    }
                }
            };
            let duration_ms = started.elapsed().as_millis() as u64;

            emit(
                session,
                render,
                speaker,
                match outcome.result {
                    Ok(output) => EventPayload::ToolCallCompleted {
                        tool_call_id,
                        ok: true,
                        output: Some(output.text),
                        error: None,
                        duration_ms,
                    },
                    Err(error) => EventPayload::ToolCallCompleted {
                        tool_call_id,
                        ok: false,
                        output: None,
                        error: Some(error.to_string()),
                        duration_ms,
                    },
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

/// What the loop must do with one call once the gate and the user have spoken.
enum Authorized {
    Allow,
    /// The call never reaches the tool; the message is its one required result.
    Refuse {
        message: String,
    },
}

/// Apply the permission gate to one call and, when it answers `Ask`, ask the
/// user through the injected port.
///
/// The gate itself is pure and never asks, never reads the environment and never
/// writes an event. Everything interactive lives here: the ask, the
/// session-scoped "always allow", and the headless downgrade of `Ask` to `Deny`
/// (whose reason lands in `PermissionDecided`, so the audit can tell a
/// no-terminal refusal apart from a policy one).
async fn authorize(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    tool_call_id: &ToolCallId,
    args: &serde_json::Value,
    facts: &CallFacts,
) -> Result<Authorized, Error> {
    let request_id = format!("perm-{tool_call_id}");
    let path_error = facts.path_error.as_ref().map(ToString::to_string);
    let verdict = {
        let call = permissions::Call {
            tool_name: &facts.tool_name,
            effect: &facts.effect,
            write_targets: &facts.write_targets,
            read_targets: &facts.read_paths,
            argv: facts.argv.as_deref(),
            cwd: session.cwd(),
            home: session.home(),
            path_error: path_error.as_deref(),
        };
        permissions::decide(session.policy(), speaker, &call)
    };

    match verdict.decision {
        Decision::Allow => {
            record_decision(
                session,
                render,
                speaker,
                &request_id,
                Decision::Allow,
                DecisionSource::Policy,
                verdict.reason,
            )?;
            Ok(Authorized::Allow)
        }
        Decision::Deny => {
            record_decision(
                session,
                render,
                speaker,
                &request_id,
                Decision::Deny,
                DecisionSource::Policy,
                verdict.reason.clone(),
            )?;
            Ok(refuse(&verdict.reason))
        }
        Decision::Ask => {
            let Some(asker) = session.asker().cloned() else {
                // The gate keeps its faithful `Ask`; the loop is where "there is
                // no answerer" turns it into a refusal, and it says so.
                let reason = format!(
                    "{}; downgraded to deny: no interactive answerer",
                    verdict.reason
                );
                record_decision(
                    session,
                    render,
                    speaker,
                    &request_id,
                    Decision::Deny,
                    DecisionSource::Policy,
                    reason.clone(),
                )?;
                return Ok(refuse(&reason));
            };

            let request = PermissionRequest {
                request_id: request_id.clone(),
                tool_call_id: tool_call_id.as_str().to_owned(),
                tool_name: facts.tool_name.clone(),
                args: args.clone(),
                reason: verdict.reason.clone(),
            };
            emit(
                session,
                render,
                speaker,
                EventPayload::PermissionAsked {
                    request_id: request_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    request: serde_json::json!({
                        "tool": facts.tool_name,
                        "args": args,
                        "reason": verdict.reason,
                    }),
                },
            )?;

            match asker.ask(&request).await {
                Answer::Allow => {
                    record_decision(
                        session,
                        render,
                        speaker,
                        &request_id,
                        Decision::Allow,
                        DecisionSource::User,
                        format!("user approved: {}", verdict.reason),
                    )?;
                    Ok(Authorized::Allow)
                }
                Answer::AlwaysAllow => {
                    // Session policy only: no `config.toml` write, no event.
                    session
                        .remember_allow(permissions::Rule::always_allow(speaker, &facts.tool_name));
                    record_decision(
                        session,
                        render,
                        speaker,
                        &request_id,
                        Decision::Allow,
                        DecisionSource::User,
                        format!("user approved always: {}", verdict.reason),
                    )?;
                    Ok(Authorized::Allow)
                }
                Answer::Deny => {
                    let reason = format!("user denied: {}", verdict.reason);
                    record_decision(
                        session,
                        render,
                        speaker,
                        &request_id,
                        Decision::Deny,
                        DecisionSource::User,
                        reason.clone(),
                    )?;
                    Ok(refuse(&reason))
                }
            }
        }
    }
}

/// The one synthesized-refusal message shape, so every refusal path reads the
/// same way in the tool result.
fn refuse(reason: &str) -> Authorized {
    Authorized::Refuse {
        message: format!("permission denied: {reason}"),
    }
}

/// Record one permission verdict. Every call gets exactly one of these, asked or
/// not, so `decision × source` is a complete counter (ticket 19).
fn record_decision(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    request_id: &str,
    decision: Decision,
    source: DecisionSource,
    reason: String,
) -> Result<(), Error> {
    emit(
        session,
        render,
        speaker,
        EventPayload::PermissionDecided {
            request_id: request_id.to_owned(),
            decision,
            source,
            reason: Some(reason),
        },
    )
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
