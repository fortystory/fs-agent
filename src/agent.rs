//! The turn loop.
//!
//! The `agent` layer is the only writer of the event stream, and the loop is the
//! only place that calls the provider. Hooks and the permission gate are pure
//! value transformations it applies in a fixed order (spec §3):
//! `hook.pre -> gate -> [ask] -> dispatch -> hook.post -> append`. The pre-hook
//! runs before the gate, so it can stop an ask from happening but can never
//! bypass one; its output is a constraint, and the effective verdict is the
//! supremum of that constraint and the gate's verdict.
//!
//! Three invariants hold from the first ticket onward:
//!
//! 1. every `tool_call` gets exactly one result;
//! 2. the provider is never called while a `tool_call` lacks a result;
//! 3. every event goes through [`append_event`] — the loop's own path and the
//!    executor port it drives are the same single writer.
//!
//! [`executor`] is a submodule of this layer rather than a boundary of its own:
//! running an executor means driving a turn, so it is control flow (spec §1, §16).
//!
//! [`cancel`] is the plumbing of the one gesture that stops a turn early
//! (spec §6): the turn selects on it while a provider stream is in flight and
//! while a tool runs, and an executor holds the same plumbing so one gesture
//! reaches the whole chain below it.

mod cancel;
mod executor;
mod history;
pub mod replay;

pub use cancel::{CancelObserver, CancelSignal};
pub use history::{recover_pending_calls, undo_last_edit, UndoOutcome};
pub use replay::{replay, ReplayError};

use std::sync::Arc;
use std::time::Instant;

use futures::StreamExt;

use crate::context;
use crate::events::{
    hook_format, last_assistant_has_tool_calls, pending_tool_calls_of, superseded_seqs,
    total_usage, ContextSource, Decision, DecisionSource, Event, EventLog, EventPayload,
    HistoryReason, ParticipantId, Redactor, Role, RoundMode, SpeakerId, StopReason, ToolCallId,
    SCHEMA_VERSION,
};
use crate::hooks::{self, Constraint, HookPoint};
use crate::permissions::{self, Answer, PermissionRequest};
use crate::provider::capability::ModelCaps;
use crate::provider::projection::project;
use crate::provider::{ChatRequest, Message, Provider, StreamEvent, ToolCall, ToolChoice};
use crate::render::RenderHandle;
use crate::session::Session;
use crate::tools::{
    AllowedCall, BashLimits, CallFacts, DispatchOutcome, GuardedCall, PendingCall, ToolError,
    ToolOutput, TASK_TOOL,
};
use crate::Error;

use executor::{spawned_executors, ExecutorPort};

/// How much of the stream one turn is allowed to see.
///
/// A single-agent turn sees everything. A turn inside a discussion round sees
/// everything up to and including that round's `RoundStarted`, plus its own
/// later events — never the other debater's events in the same round (spec §15).
///
/// This is a *structural* cut, not a timing hope. The two debaters are in flight
/// at once, so "neither has answered yet" is a race that a fast fake provider
/// loses immediately; cutting the stream at a `seq` is the only form of
/// independence that holds whatever order the two turns interleave in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnScope {
    /// Everything in the log: an ordinary single-agent turn.
    Whole,
    /// Everything up to and including `seq == before_seq` (the round's
    /// `RoundStarted`), plus the acting speaker's own later events.
    Round { before_seq: u64 },
    /// An executor's own window: the pinned injections and its own events, and
    /// nothing else.
    ///
    /// An executor is not a participant in the conversation that dispatched it.
    /// It works from its brief, so the dispatching session's speech and the other
    /// speaker's answers are not in its window — which is also what keeps a long
    /// discussion from being replayed into every executor's context. The brief
    /// itself arrives through the stream, as `ExecutorSpawned` (spec §5, §16).
    Executor,
}

/// This turn's view of the stream, under `scope`.
fn scoped_events(session: &Session, speaker: &SpeakerId, scope: TurnScope) -> Vec<Event> {
    scoped_events_slice(&session.events(), speaker, scope)
}

/// [`scoped_events`] over an explicit slice.
///
/// The slice form is what `replay` needs: it holds a snapshot already cut at the
/// call it is reproducing, so it cannot go through the live log. The two share
/// this one rule, which is what keeps a recomputed window identical to a live
/// one (spec §15, §18).
pub(crate) fn scoped_events_slice(
    events: &[Event],
    speaker: &SpeakerId,
    scope: TurnScope,
) -> Vec<Event> {
    let mut events = events.to_vec();
    match scope {
        TurnScope::Whole => {}
        TurnScope::Round { before_seq } => {
            events.retain(|event| event.seq <= before_seq || &event.speaker_id == speaker);
        }
        // A pinned injection is the session head every agent replays, so it
        // survives the cut whether or not the executor could have seen it live
        // (an executor is spawned after the injections were recorded).
        TurnScope::Executor => events.retain(|event| {
            &event.speaker_id == speaker
                || matches!(
                    event.payload,
                    EventPayload::ContextInjected { .. } | EventPayload::SessionStarted { .. }
                )
        }),
    }
    events
}

/// Project → prepend the private identity → trim.
///
/// The one place a turn's provider `messages` are built (spec §5, §10, §15), so
/// `replay` reproduces the loop instead of approximating it: the same projection,
/// the same leading `system` identity and the same trim policy, in the same
/// order. The identity never enters the log, which is exactly why this has to be
/// a shared function rather than two call sites that agree today.
pub(crate) fn build_messages(
    events: &[Event],
    speaker: &SpeakerId,
    caps: &ModelCaps,
    identity: Option<&str>,
    trim_policy: &context::TrimPolicy,
) -> Result<Vec<Message>, context::TrimError> {
    let projected = project(events, speaker, caps);
    // The agent's private identity leads the request and never enters the log
    // (spec §15). It is counted against the budget but pinned: `trim` treats a
    // leading `system` message as part of the head that is never dropped, so the
    // protocol instructions cannot be trimmed away while the question that needs
    // them stays.
    let projected = match identity {
        Some(identity) => {
            let mut messages = Vec::with_capacity(projected.len() + 1);
            messages.push(Message::System {
                content: identity.to_owned(),
                name: None,
            });
            messages.extend(projected);
            messages
        }
        None => projected,
    };
    context::trim(projected, context::usable_input(caps), trim_policy)
}

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

/// Retire every live plan-mode instruction, returning how many there were.
///
/// An instruction describes a state, so it has to stop being replayed when the
/// state ends — the user leaves plan mode, or a killed process resumes into a
/// different mode. History is never rewritten (spec §2), so "stop saying this"
/// is a `HistorySuperseded` over the injections, the same mechanism `/undo`
/// uses for an exchange. Their records stay in the log; projection drops them,
/// which is why letting the mode change leave one live costs the prefix cache a
/// miss exactly once, at the gesture.
pub fn retire_plan_instructions(
    session: &mut Session,
    render: &RenderHandle,
) -> Result<usize, Error> {
    let events = session.events();
    let retired = superseded_seqs(&events);
    let targets: Vec<u64> = events
        .iter()
        .filter(|event| !retired.contains(&event.seq))
        .filter(|event| {
            matches!(
                &event.payload,
                EventPayload::ContextInjected {
                    source: ContextSource::PlanMode,
                    ..
                }
            )
        })
        .map(|event| event.seq)
        .collect();
    if targets.is_empty() {
        return Ok(0);
    }
    let count = targets.len();
    emit(
        session,
        render,
        &SpeakerId::User,
        EventPayload::HistorySuperseded {
            targets,
            reason: HistoryReason::ModeChange,
            summary: Some("the session is no longer in plan mode".to_owned()),
        },
    )?;
    Ok(count)
}

/// Run one complete turn for `speaker` and return why it stopped.
///
/// `scope` is how much of the stream this turn may see. It only ever *removes*
/// the other debater's same-round events, so a single-agent turn passes
/// [`TurnScope::Whole`] and sees exactly the stream it always saw.
///
/// `cancelled` is this turn's view of the session's cancel gesture (spec §6). It
/// is selected on while a provider stream is in flight and while a tool runs, and
/// it is cloned into every executor this turn dispatches, so one gesture stops
/// the chain below it too.
pub async fn run_turn(
    session: &mut Session,
    speaker: &SpeakerId,
    provider: &Arc<dyn Provider>,
    render: &RenderHandle,
    scope: TurnScope,
    cancelled: &CancelObserver,
) -> Result<TurnOutcome, Error> {
    let max_iterations = session.config().max_iterations;
    // The projection branches on the model's field-level facts, so they are
    // read once from the provider rather than re-derived per iteration. The
    // drop policy is a value too, so it is built once outside the loop.
    let caps = provider.caps();
    let trim_policy = context::TrimPolicy::default();
    let mut iteration: u32 = 0;
    let mut last_text = String::new();
    // Executor ids are `<parent>-<n>`, counted off the stream rather than
    // allocated, so a resumed session cannot hand out an id it already used. The
    // count is taken once and then carried across this turn's batches.
    let mut executors_spawned = spawned_executors(&scoped_events(session, speaker, scope), speaker);
    // The values every call of this turn is processed with. They do not change
    // between iterations, so they are bundled once, outside the loop.
    let context = TurnContext {
        render,
        speaker,
        scope,
        provider,
        cancelled,
    };

    loop {
        // One snapshot per iteration: the log is shared with any other debater
        // in flight, so every read of it is a snapshot rather than a borrow.
        let events = scoped_events(session, speaker, scope);

        // Invariant 2: a pending tool_call means the log is owed a result, so
        // the provider must not be called. Scoped to this speaker: with two
        // debaters in flight, the other's unfinished call is not this turn's
        // business — reading it as one's own would end the turn with an error
        // that has nothing to do with it.
        if !pending_tool_calls_of(&events, speaker).is_empty() {
            return end_turn(session, render, speaker, StopReason::Error, last_text);
        }

        // A gesture that landed between iterations stops the turn before another
        // provider call is opened (spec §6). Checking here rather than only
        // inside the stream keeps a cancelled turn from making a request it
        // would immediately abandon.
        if cancelled.is_cancelled() {
            render.diagnostic("turn cancelled before the next model call");
            return end_turn(session, render, speaker, StopReason::Aborted, last_text);
        }

        // The session's hard stop (spec §17): once the cumulative spend reaches
        // the allowance this turn opens no further call and ends
        // `BudgetExhausted` — degrade and wrap up, never a half-finished unit.
        // The sum comes off the **whole** log, not this turn's scoped view: the
        // allowance is the session's, and an executor's window deliberately
        // excludes the debaters whose spend it shares.
        //
        // An executor's own turn is **exempt**: the hard stop refuses to dispatch
        // new executors and lets the ones already running finish (spec §17), so
        // nothing here may stop one mid-work. Its spend still lands on the stream.
        let budget = session.config().budget.clone();
        let spent = total_usage(&session.events()).total_tokens();
        let gated = scope != TurnScope::Executor;
        if gated {
            if let Some(note) = budget.exhausted_note(spent) {
                render.diagnostic(&note);
                return end_turn(
                    session,
                    render,
                    speaker,
                    StopReason::BudgetExhausted,
                    last_text,
                );
            }
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
        let messages =
            match build_messages(&events, speaker, &caps, session.identity(), &trim_policy) {
                Ok(messages) => messages,
                Err(error) => {
                    render.diagnostic(&format!("context budget: {error}"));
                    return end_turn(session, render, speaker, StopReason::Error, last_text);
                }
            };

        // The pre-flight half of the session gate (spec §17). The estimate is
        // crude — characters / 4 — so the threshold is a multiple of what is
        // left rather than an exact comparison, and a call that plainly would not
        // fit is never sent. Output tokens are not estimated; the allowance's own
        // cumulative check picks up whatever the call really costs. An executor's
        // turn is exempt here too: it was already running when the money ran out.
        if gated {
            let estimated = context::estimate_messages_tokens(&messages);
            if !budget.admits_estimate(spent, estimated) {
                render.diagnostic(&budget.estimate_refusal_note(estimated));
                return end_turn(
                    session,
                    render,
                    speaker,
                    StopReason::BudgetExhausted,
                    last_text,
                );
            }
        }

        let request = ChatRequest {
            model: session.config().model.clone(),
            messages,
            tools: session.tools().specs(),
            tool_choice: ToolChoice::Auto,
            params: session.config().params.clone(),
            cache_key: Some(session.id().as_str().to_owned()),
        };

        // The request may itself still be in flight — an adapter hands back a
        // stream only once the transport answers — so the send is selectable
        // too: a gesture must not have to wait for a stalled connection.
        let mut cancel = cancelled.clone();
        let sent = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                render.diagnostic("turn cancelled while the model call was in flight");
                return end_turn(session, render, speaker, StopReason::Aborted, last_text);
            }
            sent = provider.send(request) => sent,
        };
        let mut stream = match sent {
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
        let mut aborted = false;

        loop {
            tokio::select! {
                // The gesture wins a tie against an item that is ready: stopping
                // now is the whole point of pressing it.
                biased;
                _ = cancel.cancelled() => {
                    aborted = true;
                    break;
                }
                item = stream.next() => match item {
                    Some(Ok(StreamEvent::TextDelta(delta))) => {
                        render.text_delta(speaker, &delta);
                        text.push_str(&delta);
                    }
                    Some(Ok(StreamEvent::ReasoningDelta(delta))) => {
                        render.reasoning_delta(speaker, &delta);
                        reasoning.push_str(&delta);
                    }
                    Some(Ok(StreamEvent::ToolCallStarted { .. })) => {
                        // Fragments are assembled by the adapter; the loop only sees
                        // the completed call.
                    }
                    Some(Ok(StreamEvent::ToolCallCompleted {
                        id,
                        name,
                        arguments,
                        ..
                    })) => {
                        tool_calls.push(ToolCall {
                            id,
                            name,
                            arguments,
                        });
                    }
                    Some(Ok(StreamEvent::Usage(usage))) => {
                        emit(
                            session,
                            render,
                            speaker,
                            EventPayload::UsageRecorded { usage },
                        )?;
                    }
                    Some(Ok(StreamEvent::Finished { finish_reason })) => {
                        // The stream ended on `[DONE]`. `finish_reason` is diagnostic
                        // only; the turn's stop reason comes from the loop's own
                        // continuation query, never from the provider.
                        render.diagnostic(&format!("provider stream finished: {finish_reason:?}"));
                        saw_done = true;
                        break;
                    }
                    Some(Err(error)) => {
                        render.diagnostic(&format!("provider stream error: {error}"));
                        failed = true;
                        break;
                    }
                    // The stream just stopped. Nothing completed, so nothing lands
                    // in the log; the `[DONE]` check below turns it into an error.
                    None => break,
                },
            }
        }

        // An interrupted stream is dropped here, with the turn: that is what
        // "stop the in-flight provider stream" means for a real adapter, and it
        // is why the partially received text stays out of the log — a turn that
        // never reached `[DONE]` produced no completed unit (spec §6).
        if aborted {
            render.diagnostic("turn cancelled while the model stream was in flight");
            return end_turn(session, render, speaker, StopReason::Aborted, text);
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
        // `Ask` downgrade) synthesizes its one error result, so the tool is never
        // reached. The dispatcher then owns the shared guardrails (read before
        // edit, the per-path write locks, read-set invalidation) so no tool can
        // opt out, and the post-hook runs once the result is in the log.
        //
        // A `task` call is judged exactly like any other call and then deferred:
        // the batch's deferred calls run together, because dispatching touches no
        // workspace path and several executors working at once is the point
        // (spec §16). Everything else still runs inline, where it always did.
        let mut deferred: Vec<DeferredCall> = Vec::new();
        for call in &tool_calls {
            // A gesture that landed before this call was started owes nothing
            // for it: nothing of it is on the stream yet. The deferred calls
            // were started already, so each still gets the one result it is
            // owed (spec §6).
            if cancelled.is_cancelled() {
                close_deferred_calls(session, render, speaker, deferred, CANCELLED_BEFORE_RUN)?;
                render.diagnostic("turn cancelled before the tool calls ran");
                return end_turn(session, render, speaker, StopReason::Aborted, last_text);
            }

            match process_call(session, &context, &mut executors_spawned, call).await? {
                Disposition::Finished => {}
                Disposition::Deferred(call) => deferred.push(*call),
                // A gesture stopped the turn (a pre-hook's `Stop`, or a cancel
                // that caught the call in flight). The calls that were started
                // and deferred are still owed exactly one result each; they
                // never ran, so it says so.
                Disposition::Stopped(why) => {
                    close_deferred_calls(session, render, speaker, deferred, why)?;
                    return end_turn(session, render, speaker, StopReason::Aborted, last_text);
                }
            }
        }
        run_deferred(session, render, speaker, deferred).await?;

        last_text = text;

        if last_assistant_has_tool_calls(&session.events(), speaker) {
            continue;
        }

        return end_turn(session, render, speaker, StopReason::Completed, last_text);
    }
}

/// The one result each stopping gesture gives a `tool_call`.
///
/// Named once because both flow through [`close_deferred_calls`] and the model
/// reads them: whether the tool ran decides whether the workspace may have
/// changed.
const HOOK_STOPPED_TURN: &str = "hook stopped the turn: the tool did not run";
const CANCELLED_BEFORE_RUN: &str = "the turn was cancelled: the tool did not run";
/// The one result a `task` call gets when the session's token allowance is gone
/// (spec §17): the executor was never dispatched, so the workspace is untouched
/// by it.
const BUDGET_NO_NEW_EXECUTOR: &str = "session token budget exhausted: no new executor was \
                                       dispatched. Work that was already running was left to \
                                       finish.";
/// A cancel that caught the call in flight: the tool's future was dropped, so
/// whether it took effect is unknown — the same honesty the crash-recovery
/// result carries.
const CANCELLED_IN_FLIGHT: &str = "the turn was cancelled while this call was in flight, so its \
                                    result is unknown. It was not re-run; check the workspace \
                                    before relying on either outcome.";

/// What the loop must do with one call once the hook and the gate have spoken.
enum Disposition {
    /// This call is done: its one result is in the log.
    Finished,
    /// The call may run, and runs with the rest of the batch's deferred calls.
    Deferred(Box<DeferredCall>),
    /// The turn ends here. The caller closes the batch's started-but-undispatched
    /// calls with `why`, then records the abort.
    Stopped(&'static str),
}

/// One authorized call the batch runs alongside its siblings.
struct DeferredCall {
    pending: PendingCall,
    allowed: AllowedCall,
    started: Instant,
}

/// Give every started-but-undispatched call of a batch the one result it is owed
/// when the turn ends before [`run_deferred`] reaches it.
///
/// A `task` call is recorded as started before it is deferred, so it is owed a
/// result even though the executor never ran. `why` is the gesture that stopped
/// the turn; the shape is one place, so a stopping path cannot forget a call.
fn close_deferred_calls(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    deferred: Vec<DeferredCall>,
    why: &str,
) -> Result<(), Error> {
    for call in deferred {
        emit_completed(
            session,
            render,
            speaker,
            ToolCallId::new(call.pending.tool_call_id.clone()),
            Err(ToolError::message(why)),
            call.started,
        )?;
    }
    Ok(())
}

/// What one decided call produced, ready to be recorded.
struct CallCompletion<'a> {
    pending: &'a PendingCall,
    /// The paths the call may touch, when it got past the gate's yes.
    allowed: Option<&'a AllowedCall>,
    started: Instant,
    /// Whether the tool really ran: only then does the post-hook mount.
    dispatched: bool,
    outcome: DispatchOutcome,
}

/// The turn's own values, lent to every call it processes.
///
/// Who is acting, how much of the stream it may see, what it answers with, and
/// how it can be stopped are the same for every call in a batch, so they are
/// handed over once as one value instead of five.
struct TurnContext<'a> {
    render: &'a RenderHandle,
    speaker: &'a SpeakerId,
    scope: TurnScope,
    provider: &'a Arc<dyn Provider>,
    cancelled: &'a CancelObserver,
}

/// Carry one tool call from `ToolCallStarted` to its one result: resolve it, run
/// the pre-hook, ask the gate, and — unless the call is a deferred `task` — run
/// the tool.
///
/// One function rather than inline code so the deferred path and the inline path
/// cannot drift: both end in [`finish_call`].
async fn process_call(
    session: &mut Session,
    context: &TurnContext<'_>,
    executors_spawned: &mut u32,
    call: &ToolCall,
) -> Result<Disposition, Error> {
    let TurnContext {
        render,
        speaker,
        scope,
        provider,
        cancelled,
    } = *context;
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
                &scoped_events(session, speaker, scope),
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
        // The `bash` tool's two limits travel with the call, like the repo map's
        // budget: configuration reaches a tool through the context it is handed,
        // never by reaching into the session.
        bash: BashLimits {
            default_timeout_ms: session.config().bash_timeout_ms,
            max_timeout_ms: session.config().max_bash_timeout_ms,
        },
        executor: None,
    };

    let started = Instant::now();
    // Resolve the call once: the gate, the hook and the guardrails read the same
    // facts, and this is the only step that touches the filesystem for path
    // resolution.
    let mut facts = match session
        .tools()
        .facts(&pending.tool_name, &pending.args, &pending.paths)
    {
        Ok(facts) => facts,
        Err(error) => {
            // No tool to judge and nothing to run: the call's one result is
            // the failure.
            emit_completed(session, render, speaker, tool_call_id, Err(error), started)?;
            return Ok(Disposition::Finished);
        }
    };

    // ① hook.pre. It runs before the gate, so it can stop an ask from happening;
    //    its constraint is merged with the gate's verdict below. A `Rewrite`
    //    changes what the gate and the tool see, which is why it must happen here
    //    rather than after the gate.
    //
    //    Exactly one `HookExecuted` is recorded per invocation, whatever the
    //    outcome, so the stream is a complete record of the mount point and
    //    ticket 19 can group by hook result.
    let mut hook_verdict: Option<Decision> = None;
    if let Some(hook) = session.hook().cloned() {
        let constraint = {
            let history = hooks::public_history(&session.events());
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
                // The gate and the tool both see the rewritten call, so the
                // facts are resolved again before either reads them.
                pending.args = new_args;
                facts =
                    match session
                        .tools()
                        .facts(&pending.tool_name, &pending.args, &pending.paths)
                    {
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
                            return Ok(Disposition::Finished);
                        }
                    };
            }
            Ok(Constraint::Skip) => {
                let skipped = ToolError::message("hook skipped execution: the tool did not run");
                emit_completed(
                    session,
                    render,
                    speaker,
                    tool_call_id,
                    Err(skipped),
                    started,
                )?;
                return Ok(Disposition::Finished);
            }
            Ok(Constraint::Stop) => {
                // The turn ends here, but this call was already started, so it is
                // still owed exactly one result. The remaining calls in the batch
                // were never started and so are not owed one.
                let stopped = ToolError::message(HOOK_STOPPED_TURN);
                emit_completed(
                    session,
                    render,
                    speaker,
                    tool_call_id,
                    Err(stopped),
                    started,
                )?;
                return Ok(Disposition::Stopped(HOOK_STOPPED_TURN));
            }
            Err(error) => {
                // Fail-closed: block the action, diagnose it, and synthesize the
                // call's one error result. The turn continues, so a broken hook
                // stays diagnosable instead of becoming fatal.
                render.diagnostic(&format!(
                    "hook.pre failed for {}: {error}; the action is blocked",
                    pending.tool_name
                ));
                let blocked = ToolError::message(format!("hook failed, action blocked: {error}"));
                emit_completed(
                    session,
                    render,
                    speaker,
                    tool_call_id,
                    Err(blocked),
                    started,
                )?;
                return Ok(Disposition::Finished);
            }
        }
    }

    // ② the gate, ③ the ask. The effective verdict is the supremum of the hook's
    //    constraint and the gate's own verdict.
    let authorized = authorize(
        session,
        render,
        speaker,
        &tool_call_id,
        &pending.args,
        &facts,
        hook_verdict,
    )
    .await?;

    let (allowed, outcome) = match authorized {
        Authorized::Refuse { message } => (
            None,
            DispatchOutcome::failure(ToolError::message(message), false),
        ),
        Authorized::Allow => {
            // The guardrails are a pure read of the facts plus this agent's read
            // set; the decision is applied to the read set in `finish_call`.
            match facts.guardrails(session.read_set()) {
                GuardedCall::Refused(error) => (None, DispatchOutcome::failure(error, false)),
                GuardedCall::Run(allowed) => {
                    // A `task` call is dispatched through a port the loop builds
                    // right here: this layer is the one that holds the provider
                    // and the renderer, and building the port per authorized call
                    // is what gives each executor its own id before anything of it
                    // is recorded.
                    if pending.tool_name == TASK_TOOL {
                        // The session's hard stop at its second landing point
                        // (spec §17): an exhausted session dispatches no **new**
                        // executor. Executors already running are not touched —
                        // they finish on their own turn cap. The call was started,
                        // so it still gets its one result, and that result says
                        // the executor never ran.
                        let spent = total_usage(&session.events()).total_tokens();
                        if let Some(note) = session.config().budget.exhausted_note(spent) {
                            render.diagnostic(&format!("{note}; no new executor was dispatched"));
                            let refused = ToolError::message(BUDGET_NO_NEW_EXECUTOR);
                            emit_completed(
                                session,
                                render,
                                speaker,
                                tool_call_id,
                                Err(refused),
                                started,
                            )?;
                            return Ok(Disposition::Finished);
                        }
                        *executors_spawned += 1;
                        pending.executor = Some(Arc::new(ExecutorPort::new(
                            session,
                            speaker,
                            provider,
                            render,
                            ParticipantId::new(format!("{speaker}-{executors_spawned}")),
                            cancelled,
                        )));
                        return Ok(Disposition::Deferred(Box::new(DeferredCall {
                            pending,
                            allowed,
                            started,
                        })));
                    }
                    // ④ dispatch, inline: a call that touches the workspace runs
                    //    where it always did, in the batch's order. The gesture is
                    //    selected on here too, so a tool that is in flight is
                    //    dropped where it stands; the call still keeps its one
                    //    required result, synthesized below (spec §6).
                    let tools = session.shared_tools();
                    let mut cancel = cancelled.clone();
                    let outcome = tokio::select! {
                        biased;
                        _ = cancel.cancelled() => {
                            let stopped = ToolError::message(CANCELLED_IN_FLIGHT);
                            emit_completed(
                                session,
                                render,
                                speaker,
                                tool_call_id,
                                Err(stopped),
                                started,
                            )?;
                            return Ok(Disposition::Stopped(CANCELLED_BEFORE_RUN));
                        }
                        outcome = tools.dispatch(&pending, &allowed) => outcome,
                    };
                    finish_call(
                        session,
                        render,
                        speaker,
                        CallCompletion {
                            pending: &pending,
                            allowed: Some(&allowed),
                            started,
                            dispatched: true,
                            outcome,
                        },
                    )
                    .await?;
                    return Ok(Disposition::Finished);
                }
            }
        }
    };

    finish_call(
        session,
        render,
        speaker,
        CallCompletion {
            pending: &pending,
            allowed,
            started,
            dispatched: false,
            outcome,
        },
    )
    .await?;
    Ok(Disposition::Finished)
}

/// Run the batch's deferred calls together, at most
/// [`SessionConfig::max_parallel_executors`] at a time, and record their results
/// in the batch's order.
///
/// This is what makes "several executors in one batch run at once" true without
/// a second delivery mechanism: every call is still an ordinary tool call with
/// exactly one result, and the write exclusion that matters happens inside the
/// executors, on the shared path locks (spec §16). The cap is a cost and rate
/// gate, not a safety gate.
async fn run_deferred(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    deferred: Vec<DeferredCall>,
) -> Result<(), Error> {
    if deferred.is_empty() {
        return Ok(());
    }
    let cap = session.config().max_parallel_executors.max(1);
    // A shared handle, so the futures borrow the tool table rather than the
    // session: the session is the loop's to mutate again as the results land.
    let tools = session.shared_tools();
    // Collected before awaiting, so every future borrows the same `deferred`.
    let mut batch = Vec::with_capacity(deferred.len());
    for call in &deferred {
        batch.push(tools.dispatch(&call.pending, &call.allowed));
    }
    let outcomes = futures::stream::iter(batch)
        .buffered(cap)
        .collect::<Vec<_>>()
        .await;

    for (call, outcome) in deferred.into_iter().zip(outcomes) {
        finish_call(
            session,
            render,
            speaker,
            CallCompletion {
                pending: &call.pending,
                allowed: Some(&call.allowed),
                started: call.started,
                dispatched: true,
                outcome,
            },
        )
        .await?;
    }
    Ok(())
}

/// Everything that follows a decided call: the read set, the call's one result,
/// and the post-hook.
///
/// One implementation for both dispatch paths, so the invariants hold wherever
/// the tool actually ran: a read is only a read if it succeeded, a failed match
/// withdraws the path's read permission, and the result enters the log before the
/// post-hook runs (a hook that hangs cannot hide a result the renderer should
/// already have seen).
async fn finish_call(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    completion: CallCompletion<'_>,
) -> Result<(), Error> {
    let CallCompletion {
        pending,
        allowed,
        started,
        dispatched,
        outcome,
    } = completion;

    if let Some(allowed) = allowed {
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
    }

    let (ok, output, error) = match &outcome.result {
        Ok(output) => (true, Some(output.text.clone()), None),
        Err(error) => (false, None, Some(error.to_string())),
    };
    emit_completed(
        session,
        render,
        speaker,
        ToolCallId::new(pending.tool_call_id.clone()),
        outcome.result,
        started,
    )?;

    // ⑤ hook.post. It runs only when the tool really ran, and its failure can
    //    only drop feedback.
    if dispatched {
        run_post_hook(
            session,
            render,
            speaker,
            pending,
            ok,
            output.as_deref(),
            error.as_deref(),
        )
        .await?;
    }
    Ok(())
}

/// One debater at runtime: its own session (its read set, its private identity,
/// its model) and the provider that answers for it.
pub struct Debater {
    pub speaker: SpeakerId,
    pub session: Session,
    /// Shared, because the executors this debater dispatches answer on the same
    /// client (spec §16: an executor's model is inherited by default).
    pub provider: Arc<dyn Provider>,
}

/// The synthesizer (CONTEXT.md: 合成器): a session to write into, and a provider
/// to call.
///
/// It has no speaker and no tools because it is not an agent: it is one call the
/// harness makes on its own behalf (spec §15).
pub struct Synthesizer {
    pub session: Session,
    pub provider: Box<dyn Provider>,
}

/// A discussion: the roster, the closing call, and the round cap.
pub struct Discussion {
    debaters: Vec<Debater>,
    synthesizer: Synthesizer,
    max_rounds: u32,
}

/// How a discussion ended, and what the synthesizer made of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscussionOutcome {
    /// Why the debate phase stopped, or `Error` when nobody answered at all.
    pub reason: StopReason,
    /// The synthesizer's three-band product. Empty when the discussion failed
    /// before the closing call, or when that call itself produced nothing.
    pub synthesis: String,
    /// How many debate rounds ran: one when there was no divergence, two when
    /// there was.
    pub rounds: u32,
    /// Debaters that were absent from at least one round, in roster order. The
    /// stream carries why (their own `TurnEnded { Error }` plus the missing
    /// `MessageCompleted`); this is only the summary.
    pub absent: Vec<SpeakerId>,
}

impl Discussion {
    /// Take a roster. The roster's shape is validated at assembly, so this does
    /// not re-check it.
    pub fn new(debaters: Vec<Debater>, synthesizer: Synthesizer, max_rounds: u32) -> Self {
        Self {
            debaters,
            synthesizer,
            max_rounds,
        }
    }

    pub fn session_id(&self) -> &crate::events::SessionId {
        self.debaters[0].session.id()
    }

    /// The session that records for the discussion as a whole.
    ///
    /// The debaters share one log, so any of their sessions can append a round
    /// boundary or read the stream back. Going through one accessor keeps "which
    /// session speaks for the discussion" a decision made in one place instead of
    /// a `debaters[0]` repeated at every call site.
    fn recorder(&mut self) -> &mut Session {
        &mut self.debaters[0].session
    }

    /// The shared stream, as a snapshot.
    fn stream(&self) -> Vec<Event> {
        self.debaters[0].session.events()
    }
}

/// Run one discussion: an independent first round, a targeted second only when
/// the conclusions conflict, then the synthesizer's single call (spec §15).
///
/// The control flow lives in this layer because this layer is the only writer of
/// the event stream and the only caller of a provider (spec §3); [`crate::discussion`]
/// holds the rules this function applies.
///
/// Failure semantics, all of them "record and continue, never re-run": a single
/// side failing is that side's absence for the round and the discussion carries
/// on; both sides failing is a round that ends `Error` plus a `SessionError`, with
/// no closing call.
pub async fn run_discussion(
    discussion: &mut Discussion,
    render: &RenderHandle,
    question: &str,
    cancelled: &CancelObserver,
) -> Result<DiscussionOutcome, Error> {
    // The question is the user's own message, recorded once before the rounds so
    // both debaters project the same one.
    record_user_message(discussion.recorder(), render, question)?;

    let mut rounds: u32 = 0;
    let mut absent: Vec<SpeakerId> = Vec::new();
    // The session's allowance is one value shared by every participant (spec
    // §17); assembly refuses a roster that disagrees about it, so the recorder's
    // copy speaks for the discussion.
    let budget = discussion.debaters[0].session.config().budget.clone();

    let reason = loop {
        // A gesture that landed before this round opened opens nothing: the
        // debate phase ends where it stands (spec §6).
        if cancelled.is_cancelled() {
            break StopReason::Aborted;
        }

        // The session's hard stop (spec §17): an exhausted session opens no
        // further round and goes straight to the synthesizer, which is the one
        // call that can never be skipped. Reaching the top of this loop with
        // `rounds > 0` means the round before it did **not** end the debate, so
        // that round is the one this reason closes; with nothing run yet there is
        // no round boundary to record and the reason travels on
        // `DiscussionOutcome` alone.
        let spent = total_usage(&discussion.stream()).total_tokens();
        if let Some(note) = budget.exhausted_note(spent) {
            render.diagnostic(&format!("{note}; going straight to synthesis"));
            if rounds > 0 {
                record_round_ended(
                    discussion.recorder(),
                    render,
                    rounds,
                    StopReason::BudgetExhausted,
                )?;
            }
            break StopReason::BudgetExhausted;
        }

        rounds += 1;
        let mode = if rounds == 1 {
            RoundMode::Independent
        } else {
            RoundMode::Targeted
        };
        let started = record_round_started(discussion.recorder(), render, rounds, mode)?;

        // Both debaters answer at once. `join_all` polls the two turns alternately
        // on this task: while one awaits its provider stream the other makes
        // progress, which is all "concurrently" can mean for two network-bound
        // turns, and it keeps both turns writing through one shared log.
        let scope = TurnScope::Round {
            before_seq: started.seq,
        };
        let turns = futures::future::join_all(discussion.debaters.iter_mut().map(|debater| {
            run_turn(
                &mut debater.session,
                &debater.speaker,
                &debater.provider,
                render,
                scope,
                cancelled,
            )
        }))
        .await;
        for turn in turns {
            // A log write failure is the one thing a turn returns as an error;
            // the stream is then unusable, so neither is the discussion.
            turn?;
        }

        // The gesture outranks the round's own verdict. A round the user stopped
        // is not a debate result, and reading whatever single answer happened to
        // land before the press as agreement is exactly the misread the absence
        // query exists to prevent (spec §6).
        if cancelled.is_cancelled() {
            record_round_ended(discussion.recorder(), render, rounds, StopReason::Aborted)?;
            break StopReason::Aborted;
        }

        // Read the round back off the stream. Attendance, order, agreement and
        // absence are all queries over events, never loop state (spec §15).
        let attendance =
            crate::discussion::protocol::round_attendance(&discussion.stream(), rounds);
        for speaker in &attendance.absent {
            if !absent.contains(speaker) {
                absent.push(speaker.clone());
            }
        }

        // Nobody answered at all: a session-level failure rather than a debate
        // result, and there is nothing for the synthesizer to synthesize. Unless
        // the budget is what stopped every side — that is the hard stop doing its
        // job (degrade and wrap up), and recording it as a fault would make the
        // gate look like a breakage (spec §17).
        if attendance.answers.is_empty() {
            if budget.is_exhausted(total_usage(&discussion.stream()).total_tokens()) {
                record_round_ended(
                    discussion.recorder(),
                    render,
                    rounds,
                    StopReason::BudgetExhausted,
                )?;
                break StopReason::BudgetExhausted;
            }
            record_round_ended(discussion.recorder(), render, rounds, StopReason::Error)?;
            record_session_error(
                discussion.recorder(),
                render,
                "discussion_failed",
                "no debater answered this round",
            )?;
            return Ok(DiscussionOutcome {
                reason: StopReason::Error,
                synthesis: String::new(),
                rounds,
                absent,
            });
        }

        let outcome = crate::discussion::protocol::round_outcome(&attendance);
        if outcome == crate::discussion::protocol::RoundOutcome::Diverged {
            let positions = attendance
                .answers
                .iter()
                .map(|(_, answer)| crate::discussion::position_of(answer))
                .collect();
            record_divergence(
                discussion.recorder(),
                render,
                rounds,
                &crate::discussion::divergence_topic(question),
                positions,
            )?;
        }

        // The session's hard stop already had its say at the top of this loop:
        // an exhausted session never reaches this verdict, it goes straight to
        // the synthesizer (spec §17). What is left here is the protocol's own
        // four reasons.
        match crate::discussion::plan_after_round(outcome, rounds, discussion.max_rounds) {
            crate::discussion::RoundPlan::Stop(reason) => {
                record_round_ended(discussion.recorder(), render, rounds, reason)?;
                break reason;
            }
            // No `RoundEnded` for a round that did not end the debate: the next
            // `RoundStarted` closes it. That keeps every `RoundEnded` a reason the
            // renderer can act on, which is what the terminal four values are for.
            crate::discussion::RoundPlan::TargetedRound => continue,
        }
    };

    // A cancelled discussion goes nowhere near the synthesizer: the gesture means
    // stop, and the closing call is a provider call like any other. Ending the
    // debate phase `Aborted` rather than `Error` is what keeps "the user stopped
    // it" from being recorded as a failure (spec §6).
    if reason == StopReason::Aborted {
        return Ok(DiscussionOutcome {
            reason,
            synthesis: String::new(),
            rounds,
            absent,
        });
    }

    // The synthesizer: the one call that can never be skipped. It is not a turn
    // and not a participant, but it is bracketed by a round so the stream still
    // says when it ran.
    let synthesis_round = rounds + 1;
    record_round_started(
        discussion.recorder(),
        render,
        synthesis_round,
        RoundMode::Synthesis,
    )?;
    let prompt = crate::discussion::synthesis_prompt(question, &discussion.stream());
    let synthesis = run_single_shot(
        &mut discussion.synthesizer.session,
        discussion.synthesizer.provider.as_ref(),
        render,
        &prompt,
        cancelled,
    )
    .await?;

    // A gesture that reached the closing call ends the discussion there too: no
    // partial product, and no `synthesis_failed` for a call the user stopped. A
    // gesture that arrived after the call had already reached `[DONE]` does not
    // undo it — a completed unit stays completed, exactly as a turn's own
    // completed message does.
    let cancelled_in_synthesis = synthesis.is_none() && cancelled.is_cancelled();
    let ended = if cancelled_in_synthesis {
        StopReason::Aborted
    } else if synthesis.is_some() {
        StopReason::Completed
    } else {
        record_session_error(
            &mut discussion.synthesizer.session,
            render,
            "synthesis_failed",
            "the synthesizer's call produced no product",
        )?;
        StopReason::Error
    };
    record_round_ended(
        &mut discussion.synthesizer.session,
        render,
        synthesis_round,
        ended,
    )?;

    Ok(DiscussionOutcome {
        // The gesture, not the debate phase, is what stopped this discussion.
        reason: if cancelled_in_synthesis {
            StopReason::Aborted
        } else {
            reason
        },
        synthesis: synthesis.unwrap_or_default(),
        rounds,
        absent,
    })
}

/// One independent single-shot call: the synthesizer's shape (spec §15).
///
/// Not a turn: no `TurnStarted`, no `TurnEnded`, no tools, no iteration. Its
/// product is a `MessageCompleted` from `System` — the harness's own voice, and
/// the renderer's final artifact — and its usage still lands on the stream, where
/// the session's spend is summed from (spec §17).
///
/// `Ok(None)` means the call produced no product: a provider failure, a stream
/// that never reached `[DONE]`, an empty answer, or a cancel gesture (spec §6).
/// That is not a log error, but the caller still has to say what it means — a
/// discussion failure (`SessionError`, `RoundEnded { Error }`) or a cancellation
/// (`RoundEnded { Aborted }`, no error). Only the caller holds the gesture, so
/// only the caller can tell the two apart.
///
/// The session's token allowance deliberately does **not** gate this call: the
/// synthesizer is the one call that can never be skipped (spec §17), which is
/// why the hard stop degrades the debate phase into it rather than past it.
pub async fn run_single_shot(
    session: &mut Session,
    provider: &dyn Provider,
    render: &RenderHandle,
    prompt: &str,
    cancelled: &CancelObserver,
) -> Result<Option<String>, Error> {
    // Checked before the request is built, not only inside the stream: a call
    // that has not been sent yet must not be sent after the gesture.
    if cancelled.is_cancelled() {
        return Ok(None);
    }

    let mut messages = Vec::new();
    if let Some(identity) = session.identity() {
        messages.push(Message::System {
            content: identity.to_owned(),
            name: None,
        });
    }
    messages.push(Message::User {
        content: prompt.to_owned(),
        name: None,
        // Not a `ContextInjected` projection: the synthesizer's own brief is the
        // only `user` message here and this path never trims (spec §15).
        injected: false,
    });

    let request = ChatRequest {
        model: session.config().model.clone(),
        messages,
        tools: Vec::new(),
        tool_choice: ToolChoice::None,
        params: session.config().params.clone(),
        cache_key: Some(session.id().as_str().to_owned()),
    };

    let mut cancel = cancelled.clone();
    let sent = tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            render.diagnostic("synthesizer call cancelled");
            return Ok(None);
        }
        sent = provider.send(request) => sent,
    };
    let mut stream = match sent {
        Ok(stream) => stream,
        Err(error) => {
            render.diagnostic(&format!("synthesizer provider error: {error}"));
            return Ok(None);
        }
    };

    let mut text = String::new();
    let mut saw_done = false;
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                render.diagnostic("synthesizer call cancelled while its stream was in flight");
                // No `[DONE]`, so no product: dropping the stream is the whole
                // effect a gesture has here.
                return Ok(None);
            }
            item = stream.next() => match item {
                Some(Ok(StreamEvent::TextDelta(delta))) => {
                    render.text_delta(&SpeakerId::System, &delta);
                    text.push_str(&delta);
                }
                Some(Ok(StreamEvent::ReasoningDelta(delta))) => {
                    render.reasoning_delta(&SpeakerId::System, &delta);
                }
                Some(Ok(StreamEvent::Usage(usage))) => {
                    emit(
                        session,
                        render,
                        &SpeakerId::System,
                        EventPayload::UsageRecorded { usage },
                    )?;
                }
                Some(Ok(StreamEvent::Finished { finish_reason })) => {
                    render.diagnostic(&format!("synthesizer stream finished: {finish_reason:?}"));
                    saw_done = true;
                    break;
                }
                // No tools were offered, so a call here is a protocol violation
                // rather than work to dispatch. It still must not be dispatched: the
                // synthesizer has no tool table to dispatch into.
                Some(Ok(StreamEvent::ToolCallStarted { .. }))
                | Some(Ok(StreamEvent::ToolCallCompleted { .. })) => {
                    render.diagnostic("synthesizer asked for a tool; ignored");
                }
                Some(Err(error)) => {
                    render.diagnostic(&format!("synthesizer stream error: {error}"));
                    break;
                }
                None => break,
            },
        }
    }

    if !saw_done || text.trim().is_empty() {
        return Ok(None);
    }
    // Redacted once here, before it is both emitted and returned: the product
    // the discussion hands back is the same text the stream carries.
    let text = session.redacted(&text);
    emit(
        session,
        render,
        &SpeakerId::System,
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: text.clone(),
            reasoning: None,
        },
    )?;
    Ok(Some(text))
}

/// Record a round boundary. The protocol decides *when* (spec §15); this layer
/// writes, so "only the loop writes the stream" stays literally true.
pub fn record_round_started(
    session: &mut Session,
    render: &RenderHandle,
    round: u32,
    mode: RoundMode,
) -> Result<Event, Error> {
    emit_returning(
        session,
        render,
        &SpeakerId::System,
        EventPayload::RoundStarted { round, mode },
    )
}

/// Record the end of a round that ended the debate, with the protocol's reason.
pub fn record_round_ended(
    session: &mut Session,
    render: &RenderHandle,
    round: u32,
    reason: StopReason,
) -> Result<(), Error> {
    emit(
        session,
        render,
        &SpeakerId::System,
        EventPayload::RoundEnded { round, reason },
    )
}

/// Record a conflict: the topic, and each side's stated position.
pub fn record_divergence(
    session: &mut Session,
    render: &RenderHandle,
    round: u32,
    topic: &str,
    positions: Vec<String>,
) -> Result<(), Error> {
    emit(
        session,
        render,
        &SpeakerId::System,
        EventPayload::DivergenceRecorded {
            round,
            topic: topic.to_owned(),
            positions,
        },
    )
}

/// Record a session-level failure: a run failure the model never sees (spec §2).
pub fn record_session_error(
    session: &mut Session,
    render: &RenderHandle,
    code: &str,
    detail: &str,
) -> Result<(), Error> {
    emit(
        session,
        render,
        &SpeakerId::System,
        EventPayload::SessionError {
            code: code.to_owned(),
            detail: detail.to_owned(),
        },
    )
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
        permissions::decide(&session.policy(), speaker, &call)
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
    // The pre-stream pipeline is redact -> truncate -> spill (spec §10, §20),
    // and this is where it runs: redaction first, so the `.txt` artifact on disk
    // is redacted too, not just the preview that enters the stream. The tool
    // above already ran on the true value — only what leaves the process is
    // scrubbed.
    let max_tokens = session.config().max_tool_result_tokens;
    // A success body and a failure body truncate the same way; only which
    // payload field carries the preview differs.
    let (ok, text) = match result {
        Ok(output) => (true, output.text),
        Err(error) => (false, error.to_string()),
    };
    let text = session.redacted(&text);
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
        let history = hooks::public_history(&session.events());
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
///
/// The returned text is redacted like everything else that leaves the harness:
/// the same value the stream carries is what a front end or an executor summary
/// gets, so no second, unscrubbed copy of a key exists in memory to be printed.
fn end_turn(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    reason: StopReason,
    text: String,
) -> Result<TurnOutcome, Error> {
    emit(session, render, speaker, EventPayload::TurnEnded { reason })?;
    let text = session.redacted(&text);
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

/// Append one event to the stream and narrate it to the renderer.
///
/// **The one write path** (spec §3, invariant 3): the loop reaches it through a
/// session, and the executor port it drives reaches it through the shared log
/// handle — which is what makes "the `agent` layer is the single writer" one
/// function rather than a convention.
///
/// Redaction is the last thing that happens **before** the append and the only
/// thing that happens to the payload on its way in (spec §20): every free-text
/// field is scrubbed with the session's [`Redactor`], so the stream, the file
/// and the renderer all carry the same text the model will replay. The tool that
/// produced the text ran earlier, on the true value.
///
/// The log is a cheap shared handle, so appending through a clone is the same
/// append the session would have made: one writer, one `seq`, one line.
pub(super) fn append_event(
    log: &EventLog,
    redactor: &Redactor,
    render: &RenderHandle,
    speaker_id: SpeakerId,
    mut payload: EventPayload,
) -> Result<Event, Error> {
    let mut log = log.clone();
    payload.redact(redactor);
    let event = log.append(speaker_id, payload)?;
    render.logged(&event);
    Ok(event)
}

fn emit(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    payload: EventPayload,
) -> Result<(), Error> {
    emit_returning(session, render, speaker, payload).map(|_| ())
}

/// Like [`emit`], but hands the event back.
///
/// The round loop needs the `seq` of a `RoundStarted`: that number is the cut a
/// round's projection window is taken at (spec §15).
fn emit_returning(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    payload: EventPayload,
) -> Result<Event, Error> {
    append_event(
        session.log(),
        session.redactor(),
        render,
        speaker.clone(),
        payload,
    )
}
