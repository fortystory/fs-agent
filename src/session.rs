//! The `Session` value: the only structure that holds mutable state.
//!
//! It owns the event log handle, the session identity, and the injected
//! configuration. Later tickets add the roster, budgets, policy and read set;
//! an executor is a nested `Session` whose events still append to its parent's
//! stream.

use std::io;
use std::path::{Path, PathBuf};

use crate::config::SessionConfig;
use crate::events::{Event, EventLog, EventPayload, SessionId, SpeakerId, SCHEMA_VERSION};

pub struct Session {
    id: SessionId,
    cwd: PathBuf,
    log: EventLog,
    config: SessionConfig,
}

impl Session {
    /// Create a session by creating its log and recording `SessionStarted`.
    ///
    /// The returned event is handed back so the caller can broadcast it to the
    /// renderer; `Session` itself does not know about rendering.
    pub fn start(
        id: SessionId,
        cwd: PathBuf,
        log: EventLog,
        config: SessionConfig,
    ) -> io::Result<(Self, Event)> {
        let cwd_string = cwd.to_string_lossy().into_owned();
        let mut session = Self {
            id: id.clone(),
            cwd,
            log,
            config,
        };
        let first = session.append(
            SpeakerId::System,
            EventPayload::SessionStarted {
                session_id: id,
                cwd: cwd_string,
                schema_version: SCHEMA_VERSION,
            },
        )?;
        Ok((session, first))
    }

    /// Append an event to this session's log.
    ///
    /// The session owns the only handle to the log, and `agent`'s turn loop is
    /// the only caller during a turn; tools, hooks, permissions and discussion
    /// never write. Session-skeleton events and the user's own input are the
    /// two writes the assembly boundary makes outside a turn.
    pub fn append(&mut self, speaker_id: SpeakerId, payload: EventPayload) -> io::Result<Event> {
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
