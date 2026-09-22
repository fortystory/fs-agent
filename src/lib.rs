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
//! Thirteen top-level modules, depending only downward:
//! `events` · `config` · `provider` · `tools` · `permissions` · `questions` ·
//! `hooks` · `context` · `agent` · `discussion` · `session` · `render` · `cli`.
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
pub mod questions;
pub mod render;
pub mod session;
pub mod tools;

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::task::JoinHandle;

use crate::agent::{CancelSignal, TurnOutcome};
use crate::config::SessionConfig;
use crate::context::skills::Skills;
use crate::events::{ContextSource, Event, EventLog, EventPayload, Role, SessionId, SpeakerId};
use crate::hooks::Hook;
use crate::permissions::{Asker, Mode, PlanConflict, Policy};
use crate::provider::Provider;
use crate::questions::UserQuestions;
use crate::render::{RenderHandle, Renderer};
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
    /// The port that puts a model-initiated question to the user (spec §7).
    /// `None` means no questionnaire answerer — headless assembly never mounts
    /// one — so the table does not advertise `ask_user_question` and the tool
    /// fails readably if it is somehow called.
    pub questions: Option<Arc<dyn UserQuestions>>,
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
    /// The renderer, chosen at startup. Exactly one of the three modes runs, and
    /// the assembly creates the one channel it consumes (spec §19).
    pub renderer: Renderer,
}

/// One debater in a discussion: who speaks, and what answers for them.
pub struct DebaterParts {
    /// Must be a [`SpeakerId::Debater`]: the protocol compares debaters.
    pub speaker: SpeakerId,
    /// This debater's own model and values.
    pub config: SessionConfig,
    pub provider: Box<dyn Provider>,
    /// The user's **persona** for this side: what it is like, in the user's own words.
    /// Recorded as a private injection before the first round (spec §15).
    pub soul: Option<String>,
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
    /// The renderer, chosen at startup. Exactly one of the three modes runs, and
    /// the assembly creates the one channel it consumes (spec §19).
    pub renderer: Renderer,
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
    /// The session's own end of the cancel gesture (spec §6). The front end
    /// holds one and raises it; turns get observers of it.
    cancel: CancelSignal,
    /// The mode to return to when plan mode ends (spec §13).
    ///
    /// `None` means this session is not in plan mode. It is front-end state, not
    /// session state: gestures never enter the event stream, so `--continue`
    /// assembles a fresh harness and starts from the configured mode.
    plan_restore: Option<Mode>,
    render_task: JoinHandle<()>,
}

/// The assembled discussion the caller drives.
pub struct DiscussionHarness {
    discussion: agent::Discussion,
    /// The shared log handle, for assertions and for `--continue` bookkeeping.
    log: EventLog,
    render: RenderHandle,
    /// The discussion's own end of the cancel gesture (spec §6): one gesture
    /// reaches both debaters and every executor they dispatch.
    cancel: CancelSignal,
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
    /// The session's question port (spec §7). Session-level, shared with every
    /// sibling session like the asker: a debater asks through the same renderer.
    questions: Option<Arc<dyn UserQuestions>>,
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
    fn open(scaffold: SessionScaffold, renderer: Renderer) -> Result<Self, Error> {
        let SessionScaffold {
            cwd,
            log_path,
            session_id,
            tools,
            locks,
            policy,
            asker,
            questions,
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
        // The channel is created here and its consumer end is injected into the
        // one selected renderer: one renderer per process, never concurrent
        // subscribers (spec §19).
        let (render, receiver) = render::channel();
        let render_task = renderer.spawn(receiver);
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
            questions,
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
            questions: self.questions.clone(),
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
        // The mode does not survive a resume (it is not in the stream), so a plan
        // instruction the killed process left live is now stale: retire it, or a
        // resumed session would go on telling the model it may not write while
        // the gate lets it through (spec §13).
        if session.mode() != Mode::Plan {
            let retired = agent::retire_plan_instructions(session, &self.render)?;
            if retired > 0 {
                self.render.diagnostic(&format!(
                    "resumed session: retired {retired} stale plan-mode instruction(s)"
                ));
            }
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
        renderer,
    } = parts;

    let opened = OpenedSession::open(scaffold, renderer)?;
    let mut session = opened.session(config, Some(agent::agent_identity().to_owned()));
    opened.start(&mut session)?;

    Ok(Harness {
        session,
        provider: provider.into(),
        speaker,
        render: opened.render,
        cancel: CancelSignal::new(),
        plan_restore: None,
        render_task: opened.render_task,
    })
}

/// The roster's invariants, checked wherever a discussion is assembled.
///
/// Both ways in — a discussion with a stream of its own ([`assemble_discussion`]) and
/// one on a live session ([`Harness::discuss`]) — ask the same questions, so a roster
/// cannot be acceptable on one path and rejected on the other.
fn validate_roster(
    debaters: &[DebaterParts],
    synthesizer: &SynthesizerParts,
    max_rounds: u32,
) -> Result<(), Error> {
    if debaters.len() != discussion::DEBATERS {
        return Err(Error::Discussion(format!(
            "v1 runs exactly {} debaters, got {}; more would reopen the \
             \"N = 2 does not arbitrate\" decision (spec §15, Out of Scope)",
            discussion::DEBATERS,
            debaters.len()
        )));
    }
    for debater in debaters {
        if !matches!(debater.speaker, SpeakerId::Debater(_)) {
            return Err(Error::Discussion(format!(
                "a debater must speak as a debater, got {}",
                debater.speaker
            )));
        }
    }
    // Two debaters are two participants. Every projection is a function of
    // `speaker_id`, so two debaters sharing one name would hand each side the other's
    // answer as its own (spec §5) — most visibly in the targeted round, whose whole
    // point is that a debater sees the *other* side's reply. Rosters that repeat a
    // model are fine; repeating an *identity* is not.
    let mut speakers = BTreeSet::new();
    for debater in debaters {
        if !speakers.insert(debater.speaker.to_string()) {
            return Err(Error::Discussion(format!(
                "two debaters share the speaker id {}; a discussion needs two \
                 identities, or neither side's messages can be told apart",
                debater.speaker
            )));
        }
    }
    if max_rounds == 0 {
        return Err(Error::Discussion(
            "a discussion needs at least one round".to_owned(),
        ));
    }

    // The token allowance is a **session**-level fact shared by both debaters,
    // the synthesizer and every executor they dispatch (spec §17), so a roster
    // that disagrees about it is a setup error rather than a silently-picked
    // winner.
    let budget = debaters[0].config.budget.clone();
    for debater in debaters.iter().skip(1) {
        if debater.config.budget != budget {
            return Err(Error::Discussion(format!(
                "{} and {} were given different token budgets; the allowance is one value \
                 shared by the whole session (spec §17)",
                debaters[0].speaker, debater.speaker
            )));
        }
    }
    if synthesizer.config.budget != budget {
        return Err(Error::Discussion(
            "the synthesizer was given a different token budget from the debaters; the \
             allowance is one value shared by the whole session (spec §17)"
                .to_owned(),
        ));
    }

    // The redactor is a session-level fact for the same reason, and its
    // disagreement is worse than the budget's: one participant's events would be
    // scrubbed and another's would not, on the **same** stream, with nothing to
    // show for it (spec §20). `Config::session_config` fills the same value into
    // every config, so a mismatch means someone built one by hand.
    let redactor = debaters[0].config.redactor.clone();
    for debater in debaters.iter().skip(1) {
        if debater.config.redactor != redactor {
            return Err(Error::Discussion(format!(
                "{} and {} were given different redactors; the values to scrub are one set \
                 shared by the whole session (spec §20)",
                debaters[0].speaker, debater.speaker
            )));
        }
    }
    if synthesizer.config.redactor != redactor {
        return Err(Error::Discussion(
            "the synthesizer was given a different redactor from the debaters; the values \
             to scrub are one set shared by the whole session (spec §20)"
                .to_owned(),
        ));
    }
    Ok(())
}

/// The three participants of a discussion, on whatever stream `fork` reaches.
///
/// `fork` is the whole difference between the two ways a discussion is assembled: a
/// discussion of its own opens its sessions from the scaffold it just created, and one
/// on a live session forks the session the user is in. What must not differ is decided
/// here — each debater's private identity, and the synthesizer's landing point
/// (spec §15, §17).
fn discussion_participants(
    debaters: Vec<DebaterParts>,
    synthesizer: SynthesizerParts,
    fork: impl Fn(SessionConfig, Option<String>) -> Session,
) -> (Vec<agent::Debater>, agent::Synthesizer) {
    let roster = debaters
        .into_iter()
        .map(|part| {
            let DebaterParts {
                speaker,
                config,
                provider,
                soul,
            } = part;
            let identity = discussion::debater_identity(speaker.to_string().as_str());
            agent::Debater {
                speaker,
                session: fork(config, Some(identity)),
                provider: provider.into(),
                soul,
            }
        })
        .collect();

    let SynthesizerParts {
        mut config,
        provider,
    } = synthesizer;
    // The synthesizer is the other landing point a cheaper model may be routed to
    // (spec §17). The debaters above are assembled straight from their own configs and
    // never pass through this rule, which is what "a debater is never routed" means
    // structurally.
    config.model = config
        .model_for(config::LandingPoint::Synthesizer)
        .to_owned();
    let synthesizer = agent::Synthesizer {
        session: fork(config, Some(discussion::synthesizer_identity())),
        provider,
    };
    (roster, synthesizer)
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
        renderer,
    } = parts;

    let max_rounds = max_rounds.unwrap_or(discussion::DEFAULT_MAX_ROUNDS);
    validate_roster(&debaters, &synthesizer, max_rounds)?;

    let opened = OpenedSession::open(scaffold, renderer)?;

    let (mut roster, synthesizer) =
        discussion_participants(debaters, synthesizer, |config, identity| {
            opened.session(config, identity)
        });
    // The first session records the skeleton for the whole stream, or — on a resume —
    // closes the calls an earlier process left open: it is the session-level head every
    // debater then projects.
    if let Some(first) = roster.first_mut() {
        opened.start(&mut first.session)?;
    }

    Ok(DiscussionHarness {
        discussion: agent::Discussion::new(roster, synthesizer, max_rounds),
        log: opened.log.clone(),
        render: opened.render,
        cancel: CancelSignal::new(),
        render_task: opened.render_task,
    })
}

impl Harness {
    /// Record a user message and run one turn to completion.
    pub async fn run_turn(&mut self, user_input: &str) -> Result<TurnOutcome, Error> {
        agent::record_user_message(&mut self.session, &self.render, user_input)?;
        self.drive_turn().await
    }

    /// One turn, once whatever it starts from is already on the stream.
    ///
    /// The one place a turn is actually driven. Both entry points — a typed prompt and
    /// a bare skill — differ only in what they append first, so they cannot drift on
    /// what a turn *is*.
    async fn drive_turn(&mut self) -> Result<TurnOutcome, Error> {
        // A gesture is scoped to one run: the press that stopped the last turn
        // must not stop this one, or a cancelled session could never be used
        // again in the same process (spec §6).
        self.cancel.reset();
        // This turn's view of the gesture.
        let cancelled = self.cancel.observer();
        agent::run_turn(
            &mut self.session,
            &self.speaker,
            &self.provider,
            &self.render,
            agent::TurnScope::Whole,
            &cancelled,
        )
        .await
    }

    /// Run a discussion **on this session's stream** (spec §15).
    ///
    /// The debaters and the synthesizer are *siblings* of this session: they share its
    /// log, tool table, write locks, permission policy, answerer, skills and renderer,
    /// and differ only in their own model and private identity. Two consequences are
    /// the point:
    ///
    /// * the projection turns **this agent's** turns into `user` messages for them
    ///   (spec §5), so a discussion inherits the session's context — it argues about
    ///   what this session is about, not about a question in a vacuum;
    /// * the rounds are appended to the same stream, after whatever it already holds,
    ///   so one session can carry a turn, a discussion, another turn — and
    ///   `sessions show` reads the whole thing back.
    ///
    /// The roster is validated exactly as [`assemble_discussion`] validates it. A stream
    /// this session did **not** open (a fresh discussion of its own) goes through
    /// [`assemble_discussion`] instead: that path records the skeleton, this one appends
    /// to a stream whose skeleton is already on it.
    pub async fn discuss(
        &mut self,
        question: &str,
        debaters: Vec<DebaterParts>,
        synthesizer: SynthesizerParts,
        max_rounds: Option<u32>,
    ) -> Result<agent::DiscussionOutcome, Error> {
        let max_rounds = max_rounds.unwrap_or(discussion::DEFAULT_MAX_ROUNDS);
        validate_roster(&debaters, &synthesizer, max_rounds)?;
        let (roster, synthesizer) =
            discussion_participants(debaters, synthesizer, |config, identity| {
                self.session.fork(config, identity)
            });
        // No skeleton and no recovery: this stream is live, and its `SessionStarted` is
        // the one at its head.
        let mut discussion = agent::Discussion::new(roster, synthesizer, max_rounds);
        // One discussion is one run, like one turn: the gesture starts clean.
        self.cancel.reset();
        let cancelled = self.cancel.observer();
        agent::run_discussion(&mut discussion, &self.render, question, &cancelled).await
    }

    /// The last thing the **user** asked in this session, or `None`.
    ///
    /// What a bare `/discuss` discusses: "the thing we were just talking about" is the
    /// question worth putting to two models. Only `MessageCompleted` with the user role
    /// counts — a context injection is attributed to the user too, and it is not a
    /// question.
    pub fn last_question(&self) -> Option<String> {
        self.session
            .events()
            .iter()
            .rev()
            .find_map(|event| match &event.payload {
                EventPayload::MessageCompleted {
                    role: Role::User,
                    text,
                    ..
                } => Some(text.clone()),
                _ => None,
            })
    }

    /// The session's end of the cancel gesture (spec §6).
    ///
    /// The front end holds this and raises it on Esc; a second press while
    /// [`is_cancelled`](CancelSignal::is_cancelled) is already true is the front
    /// end's to turn into an exit, because the session itself never needs to
    /// know how it was killed — `--continue` closes whatever the process left
    /// open.
    pub fn cancel_signal(&self) -> CancelSignal {
        self.cancel.clone()
    }

    /// Every discovered skill name, in precedence order: what a front end offers
    /// as `/<name>`.
    pub fn skill_names(&self) -> Vec<&str> {
        self.session.skills().names()
    }

    /// Every discovered skill as `(name, description)`: what a front end's `/` menu
    /// offers, with the line that says what the skill is for.
    ///
    /// This keeps `disable-model-invocation` skills, because the menu is the user's
    /// list — see [`crate::context::skills::Skills::entries`].
    pub fn skill_catalog(&self) -> Vec<(&str, &str)> {
        self.session.skills().entries()
    }

    /// Whether `/<name>` names a discovered skill.
    pub fn has_skill(&self, name: &str) -> bool {
        self.session.skills().get(name).is_some()
    }

    /// Append a skill the **user** named (spec §9) to the context, at the tail.
    ///
    /// This is the invocation `disable-model-invocation: true` reserves: such a
    /// skill is absent from the catalog and [`Skills::load`] refuses it, so a
    /// model that guesses the name still cannot reach it — the user can. The body
    /// is a `ContextInjected { source: Skill }` event, which projects as its own
    /// `user` message after the current history, so the cached prefix never
    /// moves. The caller runs the turn that uses it.
    pub fn load_skill(&mut self, name: &str) -> Result<(), Error> {
        let body = self
            .session
            .skills()
            .invoke(name)
            .map_err(|error| Error::Skill(error.to_string()))?;
        agent::record_context_injection(
            &mut self.session,
            &self.render,
            ContextSource::Skill,
            &body,
        )
    }

    /// Run the turn a bare `/<skill>` asks for (spec §9): load the skill, then call
    /// the model with the body as the last thing it sees.
    ///
    /// There is deliberately **no synthetic user message**. The skill body is already
    /// a `user` message once projected, so a turn that appended one would put words
    /// in the user's mouth — the transcript would show a prompt they never typed, and
    /// the model would answer that instead of the skill. This is why the bare form is
    /// not `load_skill` followed by `run_turn("")`: an empty message is a message.
    pub async fn run_skill(&mut self, name: &str) -> Result<TurnOutcome, Error> {
        self.load_skill(name)?;
        self.drive_turn().await
    }

    /// The mode this session currently runs under.
    pub fn mode(&self) -> Mode {
        self.session.mode()
    }

    /// Enter the hard plan mode (spec §13): the session may read, and the one
    /// thing it may write is the project-root `PLAN.md`.
    ///
    /// The gesture is the only way in — there is deliberately no tool for it, or
    /// "may I write" would be the model's decision to make. Entering records one
    /// pinned instruction; the mode itself stays a session value and never
    /// reaches the event stream.
    ///
    /// An existing `PLAN.md` is put to the user first (overwrite / append /
    /// keep). Returns the answer, or `None` when there was no file to ask about
    /// — including a re-entry, which is a no-op rather than a second injection.
    pub async fn enter_plan_mode(&mut self) -> Result<Option<PlanConflict>, Error> {
        if self.session.mode() == Mode::Plan {
            return Ok(None);
        }
        let plan_path = self.session.cwd().join(permissions::PLAN_FILE_NAME);
        let conflict = if plan_path.exists() {
            Some(self.plan_conflict(&plan_path).await)
        } else {
            None
        };
        if conflict == Some(PlanConflict::Overwrite) {
            // Cleared before anything is appended: if the append then fails the
            // session is not in plan mode and its stream carries no instruction
            // — consistent — while the file stays cleared, which is exactly what
            // the user just chose. Read-before-write would otherwise refuse to
            // clobber a file the model has not read, and the tool never deletes
            // a file the user owns (spec §13).
            std::fs::write(&plan_path, "")?;
        }

        let previous = self.session.mode();
        self.session.set_mode(Mode::Plan);
        if let Err(error) = agent::record_context_injection(
            &mut self.session,
            &self.render,
            ContextSource::PlanMode,
            context::plan_mode_instruction(conflict),
        ) {
            // The mode and the instruction it explains travel together: a stream
            // that never recorded the instruction is a session that is not in
            // plan mode.
            self.session.set_mode(previous);
            return Err(error);
        }
        self.plan_restore = Some(previous);
        Ok(conflict)
    }

    /// Leave plan mode, restoring the mode the session had before it entered
    /// (spec §13). Returns whether the mode changed: leaving a mode this session
    /// is not in does nothing, and appends nothing.
    ///
    /// Leaving **retires** the pinned instruction rather than appending a
    /// contradicting one: the instruction describes a state, and once the state
    /// is over the model must stop being told it is in it. History is not
    /// rewritten to do that — a `HistorySuperseded` retires the injection, the
    /// same way `/undo` retires an exchange (spec §2).
    ///
    /// The **mode** is what says whether this session is in plan mode;
    /// `plan_restore` only remembers the destination. Reading the mode here too
    /// keeps the two from disagreeing if anything else ever changes the mode
    /// while plan mode is on. A session *assembled* in plan mode therefore has
    /// no destination to return to and stays in it: where such a session should
    /// land is the front end's mode-selection surface (ticket 18), which is also
    /// the only place a plan mode can be configured from.
    pub async fn exit_plan_mode(&mut self) -> Result<bool, Error> {
        if self.session.mode() != Mode::Plan {
            self.plan_restore = None;
            return Ok(false);
        }
        let Some(previous) = self.plan_restore.take() else {
            return Ok(false);
        };
        // Retire first: if the append fails the session is still in plan mode,
        // so the instruction still describes it.
        agent::retire_plan_instructions(&mut self.session, &self.render)?;
        self.session.set_mode(previous);
        Ok(true)
    }

    /// Put the existing-plan question to the front end.
    ///
    /// With no answerer there is nobody to ask, and the non-destructive answer is
    /// the only one a session may pick on the user's behalf: a gesture cannot
    /// happen headless, but a script can call one, and it must not clear a file
    /// the user wrote.
    async fn plan_conflict(&self, path: &Path) -> PlanConflict {
        match self.session.asker() {
            Some(asker) => asker.ask_plan_conflict(path).await,
            None => PlanConflict::Keep,
        }
    }

    pub fn session_id(&self) -> &SessionId {
        self.session.id()
    }

    /// Say something to the front end that belongs to no event: the startup
    /// banner, and the interactive loop's plain feedback.
    ///
    /// It goes through the render channel rather than straight to the terminal
    /// because the renderer owns the terminal from assembly on: a second writer
    /// lands inside the live region (spec §19).
    pub fn notice(&self, message: &str) {
        self.render.notice(message);
    }

    /// The whole stream so far, in `seq` order.
    ///
    /// The single-agent counterpart of [`DiscussionHarness::events`]: one
    /// accessor shape for both harnesses, so a caller (or a test) reads the
    /// session's source of truth the same way whichever it holds.
    pub fn events(&self) -> Vec<Event> {
        self.session.events()
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
        // One discussion is one run, like one turn: the gesture starts clean.
        self.cancel.reset();
        let cancelled = self.cancel.observer();
        agent::run_discussion(&mut self.discussion, &self.render, question, &cancelled).await
    }

    /// The discussion's end of the cancel gesture (spec §6), shared by both
    /// debaters and every executor they dispatch.
    pub fn cancel_signal(&self) -> CancelSignal {
        self.cancel.clone()
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
    /// A skill the user named could not be loaded.
    #[error("skill: {0}")]
    Skill(String),
}
