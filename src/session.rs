//! The `Session` value: the only structure that holds mutable state.
//!
//! It owns the event log handle, the session identity, the injected
//! configuration, the tool registry, the shared path locks and this agent's read
//! set. Later tickets add the roster and budgets; an executor is a nested
//! `Session` whose events still append to its parent's stream and whose tool
//! registry and path locks are the same values.
//!
//! `Session` never writes on its own initiative. Its crate-private `append` is
//! called only by the `agent` module, so the agent layer is the single writer of
//! the event stream; tools, hooks, permissions and discussion cannot write.

use std::io;
use std::path::{Path, PathBuf};

use crate::config::SessionConfig;
use crate::events::{Event, EventLog, EventPayload, SessionId, SpeakerId};
use crate::tools::{PathLocks, ReadSet, Registry, SessionPaths};

pub struct Session {
    id: SessionId,
    cwd: PathBuf,
    log: EventLog,
    config: SessionConfig,
    tools: Registry,
    locks: PathLocks,
    paths: SessionPaths,
    outputs_dir: PathBuf,
    /// Paths this agent has read. Never inherited: read permission is per agent.
    read_set: ReadSet,
}

impl Session {
    /// Wrap a freshly created log. Recording `SessionStarted` is the `agent`
    /// module's job, so all writes stay in one place.
    ///
    /// The registry and the lock table are injected: the registry is the tool
    /// table for this session, and the lock table must be the *same* one every
    /// executor uses, or per-path write exclusion is per-session and therefore
    /// no lock at all.
    pub fn new(
        id: SessionId,
        cwd: PathBuf,
        log: EventLog,
        config: SessionConfig,
        tools: Registry,
        locks: PathLocks,
        outputs_dir: PathBuf,
    ) -> Self {
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
        }
    }

    /// Append an event to this session's log.
    ///
    /// Crate-private: only the `agent` module writes. Keeping the write path
    /// narrow is what makes "the log is the single source of truth" checkable.
    pub(crate) fn append(
        &mut self,
        speaker_id: SpeakerId,
        payload: EventPayload,
    ) -> io::Result<Event> {
        self.log.append(speaker_id, payload)
    }

    pub fn events(&self) -> &[Event] {
        self.log.events()
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

    pub fn log_path(&self) -> &Path {
        self.log.path()
    }

    /// The tool table for this session.
    pub fn tools(&self) -> &Registry {
        &self.tools
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
