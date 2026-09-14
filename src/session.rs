//! The `Session` value: the only structure that holds mutable state.
//!
//! It owns the event log handle, the session identity, and the injected
//! configuration. Later tickets add the roster, budgets, policy and read set;
//! an executor is a nested `Session` whose events still append to its parent's
//! stream.
//!
//! `Session` never writes on its own initiative. Its crate-private `append` is
//! called only by the `agent` module, so the agent layer is the single writer of
//! the event stream; tools, hooks, permissions and discussion cannot write.

use std::io;
use std::path::{Path, PathBuf};

use crate::config::SessionConfig;
use crate::events::{Event, EventLog, EventPayload, SessionId, SpeakerId};

pub struct Session {
    id: SessionId,
    cwd: PathBuf,
    log: EventLog,
    config: SessionConfig,
}

impl Session {
    /// Wrap a freshly created log. Recording `SessionStarted` is the `agent`
    /// module's job, so all writes stay in one place.
    pub fn new(id: SessionId, cwd: PathBuf, log: EventLog, config: SessionConfig) -> Self {
        Self {
            id,
            cwd,
            log,
            config,
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
}
