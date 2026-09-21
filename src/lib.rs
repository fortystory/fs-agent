//! fs-agent: a self-hosted coding agent CLI.
//!
//! One append-only event stream plus one projection per agent. The event stream
//! is the single source of truth; an agent's `messages` is recomputed from it,
//! never stored.
//!
//! # The assembly seam
//!
//! [`assemble`] and [`assemble_discussion`] are the two ends of the one
//! end-to-end seam. They take injected [`Provider`]s, injected render sinks, and
//! injected configuration values, and they **read no environment** — so a test
//! can drive a whole session with a scripted fake provider and assert the JSONL
//! event stream plus the two sinks. Later tickets add scenarios to this seam;
//! they do not open new mock seams.
//!
//! A discussion is the same scaffold opened once per participant: one event
//! stream, two debater sessions that share it, and a synthesizer session for the
//! single-shot closing call (spec §15).
//!
//! # Boundaries
//!
//! Twelve top-level modules, depending only downward:
//! `events` · `config` · `provider` · `tools` · `permissions` · `hooks` ·
//! `context` · `agent` · `discussion` · `session` · `render` · `cli`.
//! `events` depends on nothing internal; [`provider::projection`] is a submodule
//! of `provider`, not a boundary. `discussion` never touches `provider`: it holds
//! the protocol's rules, and the `agent` layer drives every call.

pub mod agent;
pub mod cli;
pub mod config;
pub mod context;
pub mod discussion;
pub mod events;
pub mod hooks;
pub mod permissions;
pub mod provider;
pub mod render;
pub mod session;
pub mod tools;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::task::JoinHandle;

use crate::agent::TurnOutcome;
use crate::config::SessionConfig;
use crate::context::skills::Skills;
use crate::events::{ContextSource, Event, EventLog, EventPayload, SessionId, SpeakerId};
use crate::hooks::Hook;
use crate::permissions::{Asker, Policy};
use crate::provider::Provider;
use crate::render::{RenderHandle, RenderSinks};
use crate::session::{Session, SessionParts};
use crate::tools::{PathLocks, Registry};

/// Everything a session needs except who is speaking and which model they use.
///
/// One scaffold can open more than one session, which is what a discussion is:
/// two debaters necessarily share the event stream, the tool table, the path
/// locks, the permission policy, the ask port and the hook, because those are
/// session-level facts rather than per-agent ones (spec §15). The render sinks
/// are deliberately *not* here: there is one renderer per process, so they are
/// consumed once by whoever assembles.
pub struct SessionScaffold {
    /// Session working directory, recorded in `SessionStarted`.
    pub cwd: PathBuf,
    /// Path of this session's JSONL event log. Its parent must exist.
    pub log_path: PathBuf,
    /// Session identity; never changes across `--continue`.
    pub session_id: SessionId,
    /// The tool table. A runtime value, assembled here and never a global.
    pub tools: Registry,
    /// Per-path write locks. The **same** table must reach every executor, or
    /// write exclusion is per session and therefore no lock at all.
    pub locks: PathLocks,
    /// The session's permission policy: a mode plus its rules.
    pub policy: Policy,
    /// The ask port used when the gate answers `Ask`. `None` means no
    /// interactive answerer, so the loop downgrades `Ask` to `Deny`.
    pub asker: Option<Arc<dyn Asker>>,
    /// The strategy mounted at the tool-call hook points. `None` means the loop
    /// calls no hook.
    pub hook: Option<Arc<dyn Hook>>,
    /// The user's home directory, when the caller knows it. Only the `rm`
    /// circuit breaker reads it.
    pub home: Option<PathBuf>,
}

/// Everything a single-agent session needs, all of it injected.
pub struct AssemblyParts {
    /// The session all this runs in.
    pub scaffold: SessionScaffold,
    /// The model client. Real profiles arrive in ticket 02; tests inject fakes.
    pub provider: Box<dyn Provider>,
    /// The agent this session acts as.
    pub speaker: SpeakerId,
    /// This agent's model and budget values.
    pub config: SessionConfig,
    /// The headless renderer's two explicit sinks.
    pub sinks: RenderSinks,
}

/// One debater in a discussion: who speaks, and what answers for them.
pub struct DebaterParts {
    /// Must be a [`SpeakerId::Debater`]: the protocol compares debaters.
    pub speaker: SpeakerId,
    /// This debater's own model and values. Heterogeneous on purpose — the two
    /// debaters are different vendors (spec §15).
    pub config: SessionConfig,
    pub provider: Box<dyn Provider>,
}

/// The synthesizer's one call: not an agent, so it needs no speaker, no tools
/// and no turn (spec §15).
pub struct SynthesizerParts {
    pub config: SessionConfig,
    pub provider: Box<dyn Provider>,
}

/// Everything a discussion needs, all of it injected.
pub struct DiscussionParts {
    /// The session all this runs in. Opened once per debater.
    pub scaffold: SessionScaffold,
    /// Exactly [`discussion::DEBATERS`] debaters (spec §15; N > 2 would reopen
    /// the "N = 2 does not arbitrate" decision, so v1 refuses it).
    pub debaters: Vec<DebaterParts>,
    pub synthesizer: SynthesizerParts,
    /// Cap on debate rounds. `None` takes the protocol's default, one
    /// independent round plus one targeted round
    /// ([`discussion::DEFAULT_MAX_ROUNDS`]); `Some` is how a caller changes it,
    /// and zero is refused.
    pub max_rounds: Option<u32>,
    /// The headless renderer's two explicit sinks.
    pub sinks: RenderSinks,
}

/// The assembled harness the caller drives.
pub struct Harness {
    session: Session,
    /// Shared, because an executor this session dispatches answers on the same
    /// client: an executor's model is inherited unless a profile overrides it
    /// (spec §16).
    provider: Arc<dyn Provider>,
    speaker: SpeakerId,
    render: RenderHandle,
    render_task: JoinHandle<()>,
}

/// The assembled discussion the caller drives.
pub struct DiscussionHarness {
    discussion: agent::Discussion,
    /// The shared log handle, for assertions and for `--continue` bookkeeping.
    log: EventLog,
    render: RenderHandle,
    render_task: JoinHandle<()>,
}

/// A scaffold after its one-time work: one log, one tool table, one lock table,
/// one policy, one renderer. Every session opened from it shares these.
struct OpenedSession {
    id: SessionId,
    cwd: PathBuf,
    log: EventLog,
    tools: Arc<Registry>,
    locks: PathLocks,
    outputs_dir: PathBuf,
    policy: Arc<Mutex<Policy>>,
    asker: Option<Arc<dyn Asker>>,
    hook: Option<Arc<dyn Hook>>,
    home: Option<PathBuf>,
    skills: Arc<Skills>,
    /// `AGENTS.md`, read once before the session exists.
    agents_md: Option<String>,
    /// Whether the stream already carries a `SessionStarted`: a log that does is
    /// a session being continued, not a new one.
    resuming: bool,
    render: RenderHandle,
    render_task: JoinHandle<()>,
}

impl OpenedSession {
    /// The one-time work: create the log and its artifact directory, discover the
    /// skills, share the tool table and the policy, spawn the renderer.
    fn open(scaffold: SessionScaffold, sinks: RenderSinks) -> Result<Self, Error> {
        let SessionScaffold {
            cwd,
            log_path,
            session_id,
            tools,
            locks,
            policy,
            asker,
            hook,
            home,
        } = scaffold;

        // Tool artifacts live beside the event log, so a session stays one
        // movable directory (spec §11). The directory is created lazily by the
        // tool that needs it.
        let outputs_dir = log_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("outputs");
        let (render, render_task) = render::spawn_headless(sinks);
        // Fresh or resumed is decided by the log's existence, which is the
        // caller's decision: it hands over a path it just allocated under a new
        // session id, or one an earlier run left behind. That keeps the library
        // from reading an environment flag (spec §1) while still making
        // `--continue` one code path (spec §11).
        let log = if log_path.exists() {
            EventLog::open(&log_path)?
        } else {
            EventLog::create(&log_path)?
        };
        let resuming = log
            .events()
            .iter()
            .any(|event| matches!(event.payload, EventPayload::SessionStarted { .. }));
        let agents_md = context::load_agents_md(&cwd);
        let skills = Arc::new(Skills::discover(&cwd, home.as_deref()));

        Ok(Self {
            id: session_id,
            cwd,
            log,
            tools: Arc::new(tools),
            locks,
            outputs_dir,
            policy: Arc::new(Mutex::new(policy)),
            asker,
            hook,
            home,
            skills,
            agents_md,
            resuming,
            render,
            render_task,
        })
    }

    /// Open an agent's session. Cheap: every session-level value is shared.
    fn session(&self, config: SessionConfig, identity: Option<String>) -> Session {
        Session::new(SessionParts {
            id: self.id.clone(),
            cwd: self.cwd.clone(),
            log: self.log.clone(),
            config,
            tools: Arc::clone(&self.tools),
            locks: self.locks.clone(),
            outputs_dir: self.outputs_dir.clone(),
            policy: Arc::clone(&self.policy),
            asker: self.asker.clone(),
            hook: self.hook.clone(),
            home: self.home.clone(),
            skills: Arc::clone(&self.skills),
            identity,
        })
    }

    /// The one-time head work: a fresh stream records the session skeleton; a
    /// resumed one closes the calls the killed process never answered.
    ///
    /// A resumed stream gets **no second** `SessionStarted` and no re-injected
    /// context — both are already in the log, and replaying the recorded head is
    /// what makes the resume byte-stable for the prefix cache (spec §10, §11).
    fn start(&self, session: &mut Session) -> Result<(), Error> {
        if !self.resuming {
            return self.record_skeleton(session);
        }
        let recovered = agent::recover_pending_calls(session, &self.render)?;
        if recovered > 0 {
            self.render.diagnostic(&format!(
                "resumed session: closed {recovered} interrupted tool call(s) with an unknown result"
            ));
        }
        Ok(())
    }

    /// Record the session skeleton through one of the sessions: `SessionStarted`,
    /// then the two pinned injections.
    ///
    /// Identity -> rules -> catalog -> history (spec §10). A missing `AGENTS.md`
    /// is not an error, it just means there is no injection. Exactly one session
    /// per stream does this, and a discussion's debaters all project the same
    /// pinned head because of it.
    fn record_skeleton(&self, session: &mut Session) -> Result<(), Error> {
        agent::record_session_started(session, &self.render)?;
        if let Some(content) = &self.agents_md {
            agent::record_context_injection(
                session,
                &self.render,
                ContextSource::AgentsMd,
                content,
            )?;
        }
        if let Some(catalog) = self.skills.catalog() {
            agent::record_context_injection(
                session,
                &self.render,
                ContextSource::SkillsCatalog,
                &catalog,
            )?;
        }
        Ok(())
    }
}

/// Assemble a single-agent session. Reads no environment.
pub async fn assemble(parts: AssemblyParts) -> Result<Harness, Error> {
    let AssemblyParts {
        scaffold,
        provider,
        speaker,
        config,
        sinks,
    } = parts;

    let opened = OpenedSession::open(scaffold, sinks)?;
    let mut session = opened.session(config, None);
    opened.start(&mut session)?;

    Ok(Harness {
        session,
        provider: provider.into(),
        speaker,
        render: opened.render,
        render_task: opened.render_task,
    })
}

/// Assemble a discussion: two debaters on one stream, plus the synthesizer.
///
/// Reads no environment, like [`assemble`]. The debaters are the same session
/// scaffold opened twice, so they share the log, the tools, the path locks and
/// the permission policy; what differs is their speaker, their model and their
/// private identity (spec §15).
pub async fn assemble_discussion(parts: DiscussionParts) -> Result<DiscussionHarness, Error> {
    let DiscussionParts {
        scaffold,
        debaters,
        synthesizer,
        max_rounds,
        sinks,
    } = parts;

    if debaters.len() != discussion::DEBATERS {
        return Err(Error::Discussion(format!(
            "v1 runs exactly {} debaters, got {}; more would reopen the \
             \"N = 2 does not arbitrate\" decision (spec §15, Out of Scope)",
            discussion::DEBATERS,
            debaters.len()
        )));
    }
    for debater in &debaters {
        if !matches!(debater.speaker, SpeakerId::Debater(_)) {
            return Err(Error::Discussion(format!(
                "a debater must speak as a debater, got {}",
                debater.speaker
            )));
        }
    }
    let max_rounds = max_rounds.unwrap_or(discussion::DEFAULT_MAX_ROUNDS);
    if max_rounds == 0 {
        return Err(Error::Discussion(
            "a discussion needs at least one round".to_owned(),
        ));
    }

    let opened = OpenedSession::open(scaffold, sinks)?;

    let mut roster = Vec::with_capacity(debaters.len());
    for (index, debater) in debaters.into_iter().enumerate() {
        let DebaterParts {
            speaker,
            config,
            provider,
        } = debater;
        let identity = discussion::debater_identity(speaker.to_string().as_str());
        let mut session = opened.session(config, Some(identity));
        // The first session records the skeleton for the whole stream, or — on a
        // resume — closes the calls an earlier process left open: it is the
        // session-level head every debater then projects.
        if index == 0 {
            opened.start(&mut session)?;
        }
        roster.push(agent::Debater {
            speaker,
            session,
            provider: provider.into(),
        });
    }

    let synthesizer = {
        let SynthesizerParts { config, provider } = synthesizer;
        agent::Synthesizer {
            session: opened.session(config, Some(discussion::synthesizer_identity())),
            provider,
        }
    };

    Ok(DiscussionHarness {
        discussion: agent::Discussion::new(roster, synthesizer, max_rounds),
        log: opened.log.clone(),
        render: opened.render,
        render_task: opened.render_task,
    })
}

impl Harness {
    /// Record a user message and run one turn to completion.
    pub async fn run_turn(&mut self, user_input: &str) -> Result<TurnOutcome, Error> {
        agent::record_user_message(&mut self.session, &self.render, user_input)?;
        agent::run_turn(
            &mut self.session,
            &self.speaker,
            &self.provider,
            &self.render,
            agent::TurnScope::Whole,
        )
        .await
    }

    pub fn session_id(&self) -> &SessionId {
        self.session.id()
    }

    /// Roll back the session's most recent `edit_file`: restore the bytes it
    /// replaced and retire its events (spec §11).
    ///
    /// `Ok(None)` means there was nothing left to undo. The gesture exists only
    /// at the front end; this is the operation a `/undo` calls, and it never
    /// touches the user's git.
    pub async fn undo_last_edit(&mut self) -> Result<Option<agent::UndoOutcome>, Error> {
        agent::undo_last_edit(&mut self.session, &self.render).await
    }

    /// Where this session's tool artifacts land (`outputs/<tool_call_id>.*`).
    pub fn outputs_dir(&self) -> &std::path::Path {
        self.session.outputs_dir()
    }

    /// Drop the render channel and wait for every buffered render event to be
    /// written to the sinks. Call this before asserting on captured sinks.
    pub async fn shutdown(self) {
        drain_renderer(self.render, self.render_task).await;
    }
}

impl DiscussionHarness {
    /// Put the question to the debaters and run the protocol to its end.
    ///
    /// One harness is one discussion: round numbers are per discussion, so
    /// asking twice on one harness would restart them at one. Build a fresh
    /// harness for a second question.
    pub async fn discuss(&mut self, question: &str) -> Result<agent::DiscussionOutcome, Error> {
        agent::run_discussion(&mut self.discussion, &self.render, question).await
    }

    pub fn session_id(&self) -> &SessionId {
        self.discussion.session_id()
    }

    /// The whole stream so far, in `seq` order.
    pub fn events(&self) -> Vec<Event> {
        self.log.events()
    }

    pub fn log_path(&self) -> &Path {
        self.log.path()
    }

    /// Drop the render channel and wait for every buffered render event to be
    /// written to the sinks. Call this before asserting on captured sinks.
    pub async fn shutdown(self) {
        drain_renderer(self.render, self.render_task).await;
    }
}

/// Drop the render channel and wait for every buffered render event to reach the
/// sinks. Both harnesses shut down the same way; one renderer per process is what
/// makes that literally the same operation.
async fn drain_renderer(render: RenderHandle, render_task: JoinHandle<()>) {
    drop(render);
    let _ = render_task.await;
}

/// Errors that stop assembly or the loop itself. Provider failures are turned
/// into `TurnEnded { Error }` by the loop and are not this type.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("event log i/o error: {0}")]
    Io(#[from] io::Error),
    /// The roster a discussion was assembled with cannot run the protocol.
    #[error("discussion setup: {0}")]
    Discussion(String),
    /// `/undo` could not safely roll the workspace back.
    #[error("undo: {0}")]
    Undo(String),
}
