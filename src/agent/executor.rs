//! The executor: a nested session a debater dispatches through `task` (spec §16).
//!
//! This is a submodule of `agent` rather than a boundary of its own, because
//! running an executor *is* control flow — it drives a [`run_turn`] — and the
//! `agent` layer stays the only writer of the event stream and the only caller of
//! a provider (spec §1, §3). What lives here is everything true of an executor and
//! not of a debater: its private identity, the port the loop hands to `task`, the
//! brief-to-report plumbing, and the stream queries the report's metadata is
//! derived from.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::config::{LandingPoint, SessionConfig};
use crate::context::skills::Skills;
use crate::events::{
    usage_of, Event, EventLog, EventPayload, ParticipantId, SessionId, SpeakerId, StopReason, Usage,
};
use crate::hooks::Hook;
use crate::permissions::{Asker, Policy};
use crate::provider::Provider;
use crate::render::RenderHandle;
use crate::session::{Session, SessionParts};
use crate::tools::file::WROTE_PATH_PREFIX;
use crate::tools::{ExecutorSpawner, PathLocks, Registry, ToolError, ToolOutput};
use crate::Error;

use super::{append_event, run_turn, CancelObserver, TurnScope};

/// The private identity of an executor (spec §16).
///
/// Like a debater's protocol instructions, it never enters the event stream: it
/// is the one input to a request the stream does not carry (spec §15). It carries
/// no discussion protocol — an executor does not debate, it works.
const EXECUTOR_IDENTITY: &str = "You are an executor. Another agent dispatched you, through \
     the `task` tool, to carry out one piece of work in this repository, and you have your own \
     context: the repository rules and your brief, which is the last user message. Nobody sees \
     your steps — not your tool calls and not their output — so work until the task is done, then \
     answer with a concise report of what you did, what you found and anything the dispatcher \
     must know. That report is the whole of what comes back. You cannot dispatch further \
     executors. If the task cannot be done, say so plainly and explain why instead of guessing.";

/// The port that runs one nested executor (spec §16): [`ExecutorSpawner`], built
/// by the loop and handed to `task` through the tool context.
///
/// It snapshots the dispatching session so the executor can run while that
/// session is busy being dispatched into: everything shared (the log, the path
/// locks, the ask port, the hook, the skill library) is a handle, and everything
/// per-agent (the read set, the identity, the policy) is either fresh or derived.
/// The one thing it cannot snapshot is the provider, which arrives as a shared
/// handle because the executor answers on the dispatcher's client.
pub(super) struct ExecutorPort {
    /// Who dispatched it: the speaker the `ExecutorSpawned` names as `parent`.
    parent: SpeakerId,
    executor_id: ParticipantId,
    cwd: PathBuf,
    log: EventLog,
    session_id: SessionId,
    outputs_dir: PathBuf,
    locks: PathLocks,
    /// The executor's own policy: its mode plus the parent's **propagating** rules
    /// only, so it inherits denials and questions and never an allowance
    /// (spec §12, §16).
    policy: Policy,
    asker: Option<Arc<dyn Asker>>,
    hook: Option<Arc<dyn Hook>>,
    home: Option<PathBuf>,
    skills: Arc<Skills>,
    /// The executor's tool table: the session's table minus what is not
    /// delegable, so `task` is absent by construction (spec §16).
    tools: Arc<Registry>,
    /// The executor's own values: the inherited model, and its own turn cap.
    config: SessionConfig,
    provider: Arc<dyn Provider>,
    render: RenderHandle,
    /// The dispatcher's view of the cancel gesture (spec §6): the executor
    /// watches the **same** gesture, so one press reaches the whole chain below
    /// it. It is an observer, not a signal — an executor cannot cancel its
    /// dispatcher.
    cancelled: CancelObserver,
}

impl ExecutorPort {
    pub(super) fn new(
        session: &Session,
        parent: &SpeakerId,
        provider: &Arc<dyn Provider>,
        render: &RenderHandle,
        executor_id: ParticipantId,
        cancelled: &CancelObserver,
    ) -> Self {
        // The executor's policy: the dispatcher's **stance**, plus every rule the
        // dispatcher marked as propagating, and nothing else. Its authority is a
        // subset of the dispatcher's — an `auto` session's executor may write, a
        // `readonly` session's may not, an `ask` session's asks through the same
        // port — while an *allowance* never travels, because `Allow` does not
        // propagate (spec §12, §16).
        let parent_policy = session.policy();
        let mut policy = Policy::for_mode(parent_policy.mode());
        for rule in parent_policy.inherited_rules() {
            policy.push(rule);
        }

        // The model is inherited unless a profile routes executors elsewhere
        // (spec §16, §17): the executor answers on the dispatcher's client, so an
        // override names a model that client can serve, and only the turn cap and
        // the model are its own. The routing rule itself lives in
        // `SessionConfig::model_for`, which is also the synthesizer's — those two
        // are the only landing points a cheaper model may be routed to.
        let mut config = session.config().clone();
        config.max_iterations = config.executor_max_iterations;
        config.model = config.model_for(LandingPoint::Executor).to_owned();

        Self {
            parent: parent.clone(),
            executor_id,
            cwd: session.cwd().to_path_buf(),
            log: session.log().clone(),
            session_id: session.id().clone(),
            outputs_dir: session.outputs_dir().to_path_buf(),
            locks: session.path_locks().clone(),
            policy,
            asker: session.asker().cloned(),
            hook: session.hook().cloned(),
            home: session.home().map(Path::to_path_buf),
            skills: session.skills().clone(),
            tools: Arc::new(session.tools().for_executor()),
            config,
            provider: Arc::clone(provider),
            render: render.clone(),
            cancelled: cancelled.clone(),
        }
    }

    /// Run the executor to completion and shape the one tool result out of it.
    async fn run(&self, brief: &str) -> Result<ToolOutput, ToolError> {
        let executor = SpeakerId::Executor(self.executor_id.clone());
        // The spawn is recorded before the executor does anything, so the stream
        // reads causally even though the whole thing is one blocking tool call. It
        // is attributed to the executor — it is the executor's lifecycle event,
        // and `parent` is what names the dispatcher — which is also what lets the
        // brief reach the executor's own projection (spec §5, §16).
        append_event(
            &self.log,
            &self.config.redactor,
            &self.render,
            executor.clone(),
            EventPayload::ExecutorSpawned {
                executor_id: self.executor_id.clone(),
                parent: participant_of(&self.parent),
                brief: brief.to_owned(),
            },
        )
        .map_err(spawn_failed)?;

        let mut session = Session::new(SessionParts {
            id: self.session_id.clone(),
            cwd: self.cwd.clone(),
            log: self.log.clone(),
            config: self.config.clone(),
            tools: Arc::clone(&self.tools),
            locks: self.locks.clone(),
            outputs_dir: self.outputs_dir.clone(),
            // A fresh policy value, never the parent's handle: an allowance the
            // parent earned must not reach the child (spec §12).
            policy: Arc::new(Mutex::new(self.policy.clone())),
            asker: self.asker.clone(),
            hook: self.hook.clone(),
            home: self.home.clone(),
            skills: Arc::clone(&self.skills),
            identity: Some(EXECUTOR_IDENTITY.to_owned()),
        });

        // The read set starts empty and neither direction flows (spec §16): the
        // guardrail is about one agent's picture of the workspace, and a child
        // that has not looked at a file has no picture of it.
        let outcome = run_turn(
            &mut session,
            &executor,
            &self.provider,
            &self.render,
            TurnScope::Executor,
            &self.cancelled,
        )
        .await;

        let (reason, summary) = match outcome {
            Ok(outcome) => (outcome.reason, outcome.text),
            // A log write failure is the one fatal outcome: the stream is no
            // longer usable, so there is nothing to report through it either.
            Err(error) => {
                self.render
                    .diagnostic(&format!("executor {}: {error}", self.executor_id));
                (StopReason::Error, String::new())
            }
        };
        append_event(
            &self.log,
            &self.config.redactor,
            &self.render,
            executor.clone(),
            EventPayload::ExecutorFinished {
                executor_id: self.executor_id.clone(),
                reason,
                summary: summary.clone(),
            },
        )
        .map_err(spawn_failed)?;

        // The metadata is derived from the stream (spec §16): the executor's own
        // spend and the files it changed, with no second ledger and no new field.
        let events = self.log.events();
        let usage = usage_of(&events, &executor);
        let changed = changed_files(&events, &self.executor_id);
        let report = executor_report(&self.executor_id, reason, &summary, usage, &changed);

        // The four failure values are an error-content tool result; the
        // discussion is not interrupted by them (spec §16).
        if reason == StopReason::Completed {
            Ok(ToolOutput::new(report))
        } else {
            Err(ToolError::message(report))
        }
    }
}

#[async_trait::async_trait]
impl ExecutorSpawner for ExecutorPort {
    async fn spawn(&self, brief: &str) -> Result<ToolOutput, ToolError> {
        self.run(brief).await
    }
}

/// Turn a write failure into the `task` call's error result.
fn spawn_failed(error: Error) -> ToolError {
    ToolError::message(format!("executor: {error}"))
}

/// The participant a speaker acts as. `parent` is a participant id, so a speaker
/// that is not a participant (which cannot dispatch anyway) folds to its own
/// spelling rather than inventing a second identity.
fn participant_of(speaker: &SpeakerId) -> ParticipantId {
    match speaker {
        SpeakerId::Debater(id) | SpeakerId::Executor(id) => id.clone(),
        other => ParticipantId::new(other.to_string()),
    }
}

/// How many executors this participant has already dispatched, counted off the
/// stream. Ids are `<parent>-<n>` from that count, so a session that is resumed
/// cannot hand out an id it already used.
pub(super) fn spawned_executors(events: &[Event], parent: &SpeakerId) -> u32 {
    let parent = participant_of(parent);
    events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventPayload::ExecutorSpawned { parent: recorded, .. } if recorded == &parent
            )
        })
        .count() as u32
}

/// The files one executor changed, derived from the stream (spec §16).
///
/// A change is a successful call whose result names the file it wrote
/// ([`WROTE_PATH_PREFIX`]). Reading the **result** rather than the call's
/// arguments is what keeps this correct when a `hook.pre` rewrote the call after
/// `ToolCallStarted` recorded what the model asked for.
///
/// It lives here rather than beside the other stream queries in `events` because
/// it parses a tool convention, and `events` depends on nothing (spec §1) — naming
/// `write_file` there would point the dependency arrow the wrong way.
fn changed_files(events: &[Event], executor: &ParticipantId) -> Vec<String> {
    let speaker = SpeakerId::Executor(executor.clone());
    let mut changed: BTreeSet<String> = BTreeSet::new();
    for event in events {
        if event.speaker_id != speaker {
            continue;
        }
        if let EventPayload::ToolCallCompleted {
            ok: true,
            output: Some(output),
            ..
        } = &event.payload
        {
            if let Some(path) = output.strip_prefix(WROTE_PATH_PREFIX) {
                if let Some(path) = path.lines().next().filter(|line| !line.is_empty()) {
                    changed.insert(path.to_owned());
                }
            }
        }
    }
    changed.into_iter().collect()
}

/// The one shape of an executor's reply: summary plus metadata (spec §16).
///
/// Nobody parses this — it is the dispatcher's reading material, delivered as the
/// `task` call's result — so the list of changed files and the token counts sit
/// above the summary where a reader can act on them.
fn executor_report(
    executor: &ParticipantId,
    reason: StopReason,
    summary: &str,
    usage: Usage,
    changed: &[String],
) -> String {
    let files = if changed.is_empty() {
        "none".to_owned()
    } else {
        changed.join(", ")
    };
    format!(
        "executor {executor} finished: {reason}\n\
         files changed: {files}\n\
         tokens: input {}, output {}, cached {}, miss {}\n\
         report:\n{summary}",
        usage.input_tokens, usage.output_tokens, usage.cached_tokens, usage.miss_tokens,
    )
}
