//! The turn loop.
//!
//! The `agent` module is the only writer of the event stream, and the loop is
//! the only place that calls the provider. Hooks and the permission gate are
//! pure value transformations it applies in a fixed order (spec §3):
//! `hook.pre -> gate -> [ask] -> dispatch -> hook.post -> append`. The pre-hook
//! runs before the gate, so it can stop an ask from happening but can never
//! bypass one; its output is a constraint, and the effective verdict is the
//! supremum of that constraint and the gate's verdict.
//!
//! Three invariants hold from the first ticket onward:
//!
//! 1. every `tool_call` gets exactly one result;
//! 2. the provider is never called while a `tool_call` lacks a result;
//! 3. only this loop writes to the log.

use std::time::Instant;

use futures::StreamExt;

use crate::context;
use crate::events::{
    hook_format, last_assistant_has_tool_calls, pending_tool_calls, ContextSource, Decision,
    DecisionSource, EventPayload, Role, SpeakerId, StopReason, ToolCallId, SCHEMA_VERSION,
};
use crate::hooks::{self, Constraint, HookPoint};
use crate::permissions::{self, Answer, PermissionRequest};
use crate::provider::projection::project;
use crate::provider::{ChatRequest, Provider, StreamEvent, ToolCall, ToolChoice};
use crate::render::RenderHandle;
use crate::session::Session;
use crate::tools::{CallFacts, DispatchOutcome, GuardedCall, PendingCall, ToolError, ToolOutput};
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

/// Record one pinned context injection.
///
/// The injection is a first-class event so `project` stays a function of the
/// stream plus the rules (spec §10): the content the model replays is the
/// content that was recorded, not whatever the file says today. `ContextInjected`
/// is attributed to `User` (spec §5), and projection turns it into the first
/// `user` message — never merged, and never dropped by [`context::trim`].
pub fn record_context_injection(
    session: &mut Session,
    render: &RenderHandle,
    source: ContextSource,
    content: &str,
) -> Result<(), Error> {
    emit(
        session,
        render,
        &SpeakerId::User,
        EventPayload::ContextInjected {
            source,
            content: content.to_owned(),
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
    // The projection branches on the model's field-level facts, so they are
    // read once from the provider rather than re-derived per iteration. The
    // drop policy is a value too, so it is built once outside the loop.
    let caps = provider.caps();
    let trim_policy = context::TrimPolicy::default();
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

        // Projection only attributes; trimming is the next pure step and the
        // only place anything is dropped. A trim that cannot fit the budget has
        // exhausted every droppable class, which is the turn's hard failure
        // (spec §10).
        let projected = project(session.log(), speaker, &caps);
        let messages = match context::trim(projected, context::usable_input(&caps), &trim_policy) {
            Ok(messages) => messages,
            Err(error) => {
                render.diagnostic(&format!("context budget: {error}"));
                return end_turn(session, render, speaker, StopReason::Error, last_text);
            }
        };

        let request = ChatRequest {
            model: session.config().model.clone(),
            messages,
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
        // else. The pre-hook runs first, then the permission gate; a refusal (a
        // hook's tighten/skip/failure, a policy deny, a user deny, or a headless
        // `Ask` downgrade) synthesizes its one error result here, so the tool is
        // never reached. The dispatcher then owns the shared guardrails (read
        // before edit, the per-path write locks, read-set invalidation) so no
        // tool can opt out, and the post-hook runs once the result is in the log.
        for call in &tool_calls {
            let tool_call_id = ToolCallId::new(call.id.clone());
            emit(
                session,
                render,
                speaker,
                EventPayload::ToolCallStarted {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: call.name.clone(),
                    args: parse_tool_args(&call.arguments),
                },
            )?;

            // Everything is read off the session before the read set is borrowed.
            let paths = session.paths().clone();
            let locks = session.path_locks().clone();
            let skills = session.skills().clone();
            // `repo_map` ranks by what this session is working on, so its input is
            // recomputed from the stream — but only for a call that will use it.
            let repo_map = if call.name == context::repo_map::REPO_MAP_TOOL {
                context::repo_map::RepoMapInput {
                    context: context::repo_map::RankContext::from_session(
                        session.events(),
                        session.cwd(),
                    ),
                    tokens: session.config().repo_map_tokens,
                }
            } else {
                context::repo_map::RepoMapInput::default()
            };
            let mut pending = PendingCall {
                tool_call_id: tool_call_id.as_str().to_owned(),
                tool_name: call.name.clone(),
                args: parse_tool_args(&call.arguments),
                outputs_dir: session.outputs_dir().to_path_buf(),
                paths,
                locks,
                skills,
                repo_map,
            };

            let started = Instant::now();
            // Resolve the call once: the gate, the hook and the guardrails read
            // the same facts, and this is the only step that touches the
            // filesystem for path resolution.
            let mut facts =
                match session
                    .tools()
                    .facts(&pending.tool_name, &pending.args, &pending.paths)
                {
                    Ok(facts) => facts,
                    Err(error) => {
                        // No tool to judge and nothing to run: the call's one
                        // result is the failure.
                        emit_completed(
                            session,
                            render,
                            speaker,
                            tool_call_id,
                            Err(error),
                            started,
                        )?;
                        continue;
                    }
                };

            // ① hook.pre. It runs before the gate, so it can stop an ask from
            //    happening; its constraint is merged with the gate's verdict
            //    below. A `Rewrite` changes what the gate and the tool see, which
            //    is why it must happen here rather than after the gate.
            //
            //    Exactly one `HookExecuted` is recorded per invocation, whatever
            //    the outcome, so the stream is a complete record of the mount
            //    point and ticket 19 can group by hook result.
            let mut hook_verdict: Option<Decision> = None;
            if let Some(hook) = session.hook().cloned() {
                let constraint = {
                    let history = hooks::public_history(session.events());
                    let pre_call = hooks::PreHookCall {
                        tool_call_id: pending.tool_call_id.as_str(),
                        tool_name: &facts.tool_name,
                        args: &pending.args,
                        effect: &facts.effect,
                        write_targets: &facts.write_targets,
                        read_targets: &facts.read_paths,
                        argv: facts.argv.as_deref(),
                        cwd: session.cwd(),
                        history: &history,
                    };
                    hook.pre(&pre_call).await
                };
                let outcome = match &constraint {
                    Ok(constraint) => constraint.outcome(),
                    Err(error) => hook_format::failed(&error.to_string()),
                };
                record_hook(
                    session,
                    render,
                    speaker,
                    HookPoint::PreToolUse,
                    hook.command(),
                    outcome,
                )?;

                // Only `Tighten` forces a verdict; the rest are flow.
                hook_verdict = constraint.as_ref().ok().and_then(Constraint::tightening);

                match constraint {
                    Ok(Constraint::Continue | Constraint::Tighten(_)) => {}
                    Ok(Constraint::Rewrite(new_args)) => {
                        // The gate and the tool both see the rewritten call, so
                        // the facts are resolved again before either reads them.
                        pending.args = new_args;
                        facts = match session.tools().facts(
                            &pending.tool_name,
                            &pending.args,
                            &pending.paths,
                        ) {
                            Ok(facts) => facts,
                            Err(error) => {
                                emit_completed(
                                    session,
                                    render,
                                    speaker,
                                    tool_call_id,
                                    Err(error),
                                    started,
                                )?;
                                continue;
                            }
                        };
                    }
                    Ok(Constraint::Skip) => {
                        let skipped =
                            ToolError::message("hook skipped execution: the tool did not run");
                        emit_completed(
                            session,
                            render,
                            speaker,
                            tool_call_id,
                            Err(skipped),
                            started,
                        )?;
                        continue;
                    }
                    Ok(Constraint::Stop) => {
                        // The turn ends here, but this call was already started,
                        // so it is still owed exactly one result. The remaining
                        // calls in the batch were never started and so are not
                        // owed one.
                        let stopped =
                            ToolError::message("hook stopped the turn: the tool did not run");
                        emit_completed(
                            session,
                            render,
                            speaker,
                            tool_call_id,
                            Err(stopped),
                            started,
                        )?;
                        return end_turn(session, render, speaker, StopReason::Aborted, last_text);
                    }
                    Err(error) => {
                        // Fail-closed: block the action, diagnose it, and
                        // synthesize the call's one error result. The turn
                        // continues, so a broken hook stays diagnosable instead
                        // of becoming fatal.
                        render.diagnostic(&format!(
                            "hook.pre failed for {}: {error}; the action is blocked",
                            pending.tool_name
                        ));
                        let blocked =
                            ToolError::message(format!("hook failed, action blocked: {error}"));
                        emit_completed(
                            session,
                            render,
                            speaker,
                            tool_call_id,
                            Err(blocked),
                            started,
                        )?;
                        continue;
                    }
                }
            }

            // ② the gate, ③ the ask, ④ dispatch. The effective verdict is the
            //    supremum of the hook's constraint and the gate's own verdict.
            let mut dispatched = false;
            let outcome = match authorize(
                session,
                render,
                speaker,
                &tool_call_id,
                &pending.args,
                &facts,
                hook_verdict,
            )
            .await?
            {
                Authorized::Refuse { message } => {
                    DispatchOutcome::failure(ToolError::message(message), false)
                }
                Authorized::Allow => {
                    // The guardrails are a pure read of the facts plus this
                    // agent's read set; the decision is applied to the read set
                    // here, in the loop.
                    match facts.guardrails(session.read_set()) {
                        GuardedCall::Refused(error) => DispatchOutcome::failure(error, false),
                        GuardedCall::Run(allowed) => {
                            dispatched = true;
                            let outcome = session.tools().dispatch(&pending, &allowed).await;
                            // A read is only a read if it worked: a failed read
                            // must not license a later write.
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
            };

            // The result enters the log before the post-hook runs, so a hook that
            // hangs cannot hide a result the renderer should already have seen.
            let (ok, output, error) = match &outcome.result {
                Ok(output) => (true, Some(output.text.clone()), None),
                Err(error) => (false, None, Some(error.to_string())),
            };
            emit_completed(
                session,
                render,
                speaker,
                tool_call_id,
                outcome.result,
                started,
            )?;

            // ⑤ hook.post. It runs only when the tool really ran, and its failure
            //    can only drop feedback.
            if dispatched {
                run_post_hook(
                    session,
                    render,
                    speaker,
                    &pending,
                    ok,
                    output.as_deref(),
                    error.as_deref(),
                )
                .await?;
            }
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

/// Apply the permission gate to one call, merge the pre-hook's constraint into
/// its verdict, and, when the effective verdict is `Ask`, ask the user through
/// the injected port.
///
/// The gate itself is pure and never asks, never reads the environment and never
/// writes an event. Everything interactive lives here: the ask, the
/// session-scoped "always allow", and the headless downgrade of `Ask` to `Deny`
/// (whose reason lands in `PermissionDecided`, so the audit can tell a
/// no-terminal refusal apart from a policy one).
///
/// `hook_verdict` is the verdict a pre-hook's `Tighten` forced, if any. The
/// effective verdict is the supremum of the two — the hook can raise a verdict
/// but never lower one, because the only tightenings that exist are `Ask` and
/// `Deny`.
async fn authorize(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    tool_call_id: &ToolCallId,
    args: &serde_json::Value,
    facts: &CallFacts,
    hook_verdict: Option<Decision>,
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

    // The one merge: `Allow < Ask < Deny`. A hook that tightens to what the gate
    // already said changes nothing, and so is not the source of the recorded
    // decision.
    let effective = hooks::effective_verdict(verdict.decision, hook_verdict);
    let tightened_by_hook = hook_verdict.is_some_and(|hook| hook > verdict.decision);
    let hook_note = tightened_by_hook.then_some("(a hook tightened the verdict)");
    let annotated = annotate_reason(&verdict.reason, hook_note);

    match effective {
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
            let source = if tightened_by_hook {
                DecisionSource::Hook
            } else {
                DecisionSource::Policy
            };
            record_decision(
                session,
                render,
                speaker,
                &request_id,
                Decision::Deny,
                source,
                annotated.clone(),
            )?;
            Ok(refuse(&annotated))
        }
        Decision::Ask => {
            // A hook may have been the reason this ask exists; carry that into
            // the prompt and the audit without turning it into a fourth state.
            let ask_reason = annotated;

            let Some(asker) = session.asker().cloned() else {
                // The gate keeps its faithful `Ask`; the loop is where "there is
                // no answerer" turns it into a refusal, and it says so.
                let reason = format!("{ask_reason}; downgraded to deny: no interactive answerer");
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
                reason: ask_reason.clone(),
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
                        "reason": ask_reason,
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
                        format!("user approved: {ask_reason}"),
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
                        format!("user approved always: {ask_reason}"),
                    )?;
                    Ok(Authorized::Allow)
                }
                Answer::Deny => {
                    let reason = format!("user denied: {ask_reason}");
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

/// Append a hook's note to a verdict reason, when a hook raised the verdict.
fn annotate_reason(reason: &str, hook_note: Option<&str>) -> String {
    match hook_note {
        Some(note) => format!("{reason} {note}"),
        None => reason.to_owned(),
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

/// Append the one `ToolCallCompleted` a call is owed, whichever path produced
/// it: a real dispatch, a permission refusal, a hook skip or a hook failure.
///
/// Keeping the synthesis in one place is what makes "every `tool_call` gets
/// exactly one result" checkable rather than a convention spread over five
/// branches.
fn emit_completed(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    tool_call_id: ToolCallId,
    result: Result<ToolOutput, ToolError>,
    started: Instant,
) -> Result<(), Error> {
    let duration_ms = started.elapsed().as_millis() as u64;
    // Truncation is part of the pipeline that runs **before** the event is
    // appended (spec §10): an oversized body is spilled to disk and the stream
    // carries a self-contained preview plus a pointer. It never fails the call.
    let max_tokens = session.config().max_tool_result_tokens;
    // A success body and a failure body truncate the same way; only which
    // payload field carries the preview differs.
    let (ok, text) = match result {
        Ok(output) => (true, output.text),
        Err(error) => (false, error.to_string()),
    };
    let preview = context::truncate_result(
        &text,
        tool_call_id.as_str(),
        session.outputs_dir(),
        max_tokens,
    )
    .preview;
    let (output, error) = if ok {
        (Some(preview), None)
    } else {
        (None, Some(preview))
    };
    emit(
        session,
        render,
        speaker,
        EventPayload::ToolCallCompleted {
            tool_call_id,
            ok,
            output,
            error,
            duration_ms,
        },
    )
}

/// Append one `HookExecuted`.
///
/// Both mount points and every outcome — including a failure — go through here,
/// so ticket 19 can group the stream by hook result without a second event kind.
fn record_hook(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    point: HookPoint,
    command: &str,
    outcome: String,
) -> Result<(), Error> {
    emit(
        session,
        render,
        speaker,
        EventPayload::HookExecuted {
            point: point.as_str().to_owned(),
            command: command.to_owned(),
            outcome,
        },
    )
}

/// Run `hook.post` for a call whose tool really ran.
///
/// Failure here is deliberately asymmetric with `hook.pre`: the world has
/// already changed, so the most a broken post-hook can do is lose its feedback.
/// The failure is recorded and diagnosed; the call's result is already in the log
/// and stays there.
async fn run_post_hook(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    pending: &PendingCall,
    ok: bool,
    output: Option<&str>,
    error: Option<&str>,
) -> Result<(), Error> {
    let Some(hook) = session.hook().cloned() else {
        return Ok(());
    };
    let feedback = {
        let history = hooks::public_history(session.events());
        let post_call = hooks::PostHookCall {
            tool_call_id: &pending.tool_call_id,
            tool_name: &pending.tool_name,
            args: &pending.args,
            ok,
            output,
            error,
            history: &history,
        };
        hook.post(&post_call).await
    };

    match feedback {
        Ok(None) => record_hook(
            session,
            render,
            speaker,
            HookPoint::PostToolUse,
            hook.command(),
            hook_format::OUTCOME_CONTINUE.to_owned(),
        ),
        Ok(Some(text)) => record_hook(
            session,
            render,
            speaker,
            HookPoint::PostToolUse,
            hook.command(),
            hook_format::feedback(&text),
        ),
        Err(error) => {
            render.diagnostic(&format!(
                "hook.post failed for {}: {error}; the feedback is dropped",
                pending.tool_name
            ));
            record_hook(
                session,
                render,
                speaker,
                HookPoint::PostToolUse,
                hook.command(),
                hook_format::failed(&error.to_string()),
            )
        }
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
