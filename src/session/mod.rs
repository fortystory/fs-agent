//! The `Session` value: the only structure that holds mutable state.
//!
//! It owns the event log handle, the session identity, the injected
//! configuration, the tool registry, the shared path locks, this agent's read
//! set, this agent's private identity, and the session's permission policy. The
//! roster is not here: a discussion's debaters are each their own `Session`,
//! assembled side by side and sharing one log, and an executor is a nested
//! `Session` whose events still append to its parent's stream and whose tool
//! registry and path locks are the same values.
//!
//! `Session` never writes on its own initiative: it hands out the log handle, and
//! `agent::append_event` is the one write path, so the `agent` layer is the single
//! writer of the event stream; tools, hooks, permissions and discussion cannot
//! write.
//!
//! [`store`] is the on-disk counterpart: where a session's directory lives, how
//! `--continue` finds it, and how `prune` removes it. [`ledger`] reads those same
//! directories to answer the one question a single session cannot: how much of a
//! vendor's rolling quota window a UTC day has spent (spec §17).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub mod ledger;
pub mod observe;
pub mod store;

pub use ledger::DayLedger;
pub use store::{new_session_id, SessionStore, StoredSession};

use crate::config::SessionConfig;
use crate::context::skills::Skills;
use crate::events::{Event, EventLog, Redactor, SessionId};
use crate::hooks::Hook;
use crate::permissions::{Asker, Mode, Policy, Rule};
use crate::questions::UserQuestions;
use crate::tools::{PathLocks, ReadSet, Registry, SessionPaths};

/// Everything a session is assembled from. Injected, never read from the
/// environment by the library: the permission policy, the ask port used when the
/// gate answers `Ask`, and the user's home (which only the `rm` circuit breaker
/// reads) all arrive here.
///
/// The tool table and the policy arrive as shared handles because a discussion
/// has more than one session at once (spec §15): two debaters must dispatch into
/// the **same** tools with the **same** per-path locks, and a session-scoped
/// allowance one of them earns must reach the other.
pub struct SessionParts {
    pub id: SessionId,
    pub cwd: PathBuf,
    pub log: EventLog,
    pub config: SessionConfig,
    /// The tool table for this session.
    pub tools: Arc<Registry>,
    /// Per-path write locks. The **same** table must reach every executor, or
    /// write exclusion is per session and therefore no lock at all.
    pub locks: PathLocks,
    /// Where this session's tool artifacts (`outputs/<tool_call_id>.*`) land.
    pub outputs_dir: PathBuf,
    /// The session's permission policy: a mode plus its rules.
    pub policy: Arc<Mutex<Policy>>,
    /// The port the loop asks when the gate answers `Ask`. `None` means there is
    /// no interactive answerer, so the loop downgrades `Ask` to `Deny`.
    pub asker: Option<Arc<dyn Asker>>,
    /// The port a model-initiated question goes through (spec §7). `None` means
    /// no questionnaire answerer, so `ask_user_question` reports that instead of
    /// hanging; the table is built without the tool in that case.
    pub questions: Option<Arc<dyn UserQuestions>>,
    /// The strategy mounted at the two tool-call hook points. `None` means the
    /// loop calls no hook and appends no `HookExecuted` event.
    pub hook: Option<Arc<dyn Hook>>,
    /// The user's home directory, when it is known.
    pub home: Option<PathBuf>,
    /// The skills discovered at assembly (spec §9). Shared with every nested
    /// session, so an executor sees the same catalog as its parent.
    pub skills: Arc<Skills>,
    /// This agent's private identity: the `system` message it is given, and the
    /// one thing about it that never enters the event stream (spec §15). A
    /// discussion's protocol instructions live here precisely so that a round's
    /// `messages` stays recomputable from the stream.
    pub identity: Option<String>,
}

pub struct Session {
    id: SessionId,
    cwd: PathBuf,
    log: EventLog,
    config: SessionConfig,
    tools: Arc<Registry>,
    locks: PathLocks,
    paths: SessionPaths,
    outputs_dir: PathBuf,
    /// Paths this agent has read. Never inherited: read permission is per agent.
    read_set: ReadSet,
    /// The session's permission policy. A value, never an event: `--continue`
    /// returns to the configured mode (spec §12). Shared, because a session
    /// allowance earned inside one debater's turn is a session-scoped fact.
    policy: Arc<Mutex<Policy>>,
    /// The ask port, shared with any nested session so an executor asks through
    /// the same renderer.
    asker: Option<Arc<dyn Asker>>,
    /// The question port, shared with any sibling session for the same reason as
    /// the asker: one keyboard answers for the whole session.
    questions: Option<Arc<dyn UserQuestions>>,
    /// The hook strategy, shared with any nested session so an executor cannot
    /// escape the strategy that constrains its parent (spec §16).
    hook: Option<Arc<dyn Hook>>,
    home: Option<PathBuf>,
    /// The discovered skill library, read by the built-in `skill` tool (spec §9).
    skills: Arc<Skills>,
    /// This agent's private identity, or `None` when it is given none. Never
    /// appended to the log: it is the one input to a request that the stream does
    /// not carry (spec §15).
    identity: Option<String>,
}

impl Session {
    /// Wrap a freshly created log. Recording `SessionStarted` is the `agent`
    /// module's job, so all writes stay in one place.
    pub fn new(parts: SessionParts) -> Self {
        let SessionParts {
            id,
            cwd,
            log,
            config,
            tools,
            locks,
            outputs_dir,
            policy,
            asker,
            questions,
            hook,
            home,
            skills,
            identity,
        } = parts;
        let paths = SessionPaths::new(&cwd);
        Self {
            id,
            cwd,
            log,
            config,
            tools,
            locks,
            paths,
            outputs_dir,
            read_set: ReadSet::default(),
            policy,
            asker,
            questions,
            hook,
            home,
            skills,
            identity,
        }
    }

    /// A snapshot of this session's events, in order.
    ///
    /// A snapshot rather than a borrow: the log is a shared handle, so two
    /// debaters' turns may append to it between two reads, and no borrow could
    /// span that.
    pub fn events(&self) -> Vec<Event> {
        self.log.events()
    }

    /// The session's event log. Read-only here: projection takes the log as its
    /// input, and only the `agent` module appends to it.
    pub fn log(&self) -> &EventLog {
        &self.log
    }

    pub fn id(&self) -> &SessionId {
        &self.id
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// The values this session scrubs from text on its way into the stream
    /// (spec §20).
    ///
    /// Held by [`SessionConfig`] because `Config::session_config` is the one
    /// place configuration becomes injected values; this accessor is how the one
    /// write path and the text that leaves the harness reach it without walking
    /// into the config's fields themselves.
    pub fn redactor(&self) -> &Redactor {
        &self.config.redactor
    }

    /// Redact `text` with this session's values (spec §20).
    ///
    /// The text that leaves the harness — a tool result about to be spilled, a
    /// turn's outcome, a synthesizer's product — goes through here rather than
    /// through the stream's write path, so the two carry the same text.
    pub fn redacted(&self, text: &str) -> String {
        self.config.redactor.redacted(text)
    }

    pub fn log_path(&self) -> &Path {
        self.log.path()
    }

    /// The tool table for this session.
    pub fn tools(&self) -> &Registry {
        &self.tools
    }

    /// The tool table as a shared handle.
    ///
    /// For work that must outlive a borrow of this session: the loop's batch of
    /// deferred executor calls dispatches through the table while it still needs
    /// the session to record their results (spec §16).
    pub fn shared_tools(&self) -> Arc<Registry> {
        Arc::clone(&self.tools)
    }

    /// Where this session's tool artifacts (`outputs/<tool_call_id>.*`) land.
    pub fn outputs_dir(&self) -> &Path {
        &self.outputs_dir
    }

    pub fn paths(&self) -> &SessionPaths {
        &self.paths
    }

    pub fn path_locks(&self) -> &PathLocks {
        &self.locks
    }

    /// The session's permission policy.
    ///
    /// A snapshot by value: the policy is shared, so the gate takes the value it
    /// judges with rather than holding a lock across an interactive ask.
    pub fn policy(&self) -> Policy {
        self.policy.lock().expect("policy mutex poisoned").clone()
    }

    /// Remember a session-scoped allowance. This changes the policy value only:
    /// it writes no `config.toml` and appends no event (spec §12).
    pub fn remember_allow(&mut self, rule: Rule) {
        self.policy
            .lock()
            .expect("policy mutex poisoned")
            .push(rule);
    }

    /// The mode this session currently runs under.
    pub fn mode(&self) -> Mode {
        self.policy.lock().expect("policy mutex poisoned").mode()
    }

    /// Swap the session's mode, keeping its rules. The mode-cycle gesture's one
    /// effect on the policy — a value, never an event, which is why `--continue`
    /// starts from the configured mode (spec §12).
    pub fn set_mode(&self, mode: Mode) {
        self.policy
            .lock()
            .expect("policy mutex poisoned")
            .set_mode(mode);
    }

    /// This agent's private identity, if it has one.
    ///
    /// It reaches the provider as the leading `system` message and never reaches
    /// the event stream (spec §15).
    pub fn identity(&self) -> Option<&str> {
        self.identity.as_deref()
    }

    /// A **sibling** session on the same stream: every session-level value is shared —
    /// the event log, the tool table, the write locks, the permission policy, the
    /// answerer, the hook, the home directory and the discovered skills — and only the
    /// agent's own values differ (its model, its generation parameters, its private
    /// identity).
    ///
    /// This is what lets a discussion run **on a live session** (spec §15): its
    /// debaters are siblings of the session the user is in, so their projection turns
    /// that session's turns into `user` messages and their rounds are appended to the
    /// same stream. The read set is deliberately **not** inherited — read permission is
    /// per agent (spec §12), and a debater has read nothing.
    pub(crate) fn fork(&self, config: SessionConfig, identity: Option<String>) -> Self {
        Self::new(SessionParts {
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

    /// The ask port, if this session has an interactive answerer.
    pub fn asker(&self) -> Option<&Arc<dyn Asker>> {
        self.asker.as_ref()
    }

    /// The question port, if this session can put model-initiated questions to
    /// the user (spec §7). The loop copies it into each call's dispatch context,
    /// where the `ask_user_question` tool reads it.
    pub fn questions(&self) -> Option<&Arc<dyn UserQuestions>> {
        self.questions.as_ref()
    }

    /// The hook strategy, if one is mounted.
    pub fn hook(&self) -> Option<&Arc<dyn Hook>> {
        self.hook.as_ref()
    }

    /// The user's home directory, when it was injected.
    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// The discovered skill library (spec §9). The `skill` tool reads it through
    /// the dispatch context; the catalog injection is computed from it at
    /// assembly.
    pub fn skills(&self) -> &Arc<Skills> {
        &self.skills
    }

    /// This agent's read set. Read-before-edit consults it; a failed match
    /// withdraws a path from it, and a read adds one.
    pub fn read_set(&self) -> &ReadSet {
        &self.read_set
    }

    /// Record paths this agent has read.
    ///
    /// The loop calls this after the call completes, not inside dispatch, so the
    /// read set never has to be borrowed mutably at the same time as the tool
    /// registry it is dispatching into.
    pub fn record_reads(&mut self, paths: &[std::path::PathBuf]) {
        self.read_set.record_all(paths.iter().cloned());
    }

    /// Withdraw this agent's read permission for one path.
    pub fn invalidate_read(&mut self, path: &Path) {
        self.read_set.invalidate(path);
    }
}
