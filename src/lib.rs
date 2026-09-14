//! fs-agent: a self-hosted coding agent CLI.
//!
//! One append-only event stream plus one projection per agent. The event stream
//! is the single source of truth; an agent's `messages` is recomputed from it,
//! never stored.
//!
//! # The assembly seam
//!
//! [`assemble`] is the one end-to-end seam. It takes an injected [`Provider`],
//! injected render sinks, and injected configuration values, and it **reads no
//! environment** — so a test can drive a whole session with a scripted fake
//! provider and assert the JSONL event stream plus the two sinks. Later tickets
//! add scenarios to this seam; they do not open new mock seams.
//!
//! # Boundaries
//!
//! Twelve top-level modules, depending only downward:
//! `events` · `config` · `provider` · `tools` · `permissions` · `hooks` ·
//! `context` · `agent` · `discussion` · `session` · `render` · `cli`.
//! `events` depends on nothing internal; [`provider::projection`] is a submodule
//! of `provider`, not a boundary.

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
use std::path::PathBuf;

use tokio::task::JoinHandle;

use crate::agent::TurnOutcome;
use crate::config::SessionConfig;
use crate::events::{EventLog, SessionId, SpeakerId};
use crate::provider::Provider;
use crate::render::{RenderHandle, RenderSinks};
use crate::session::Session;
use crate::tools::{PathLocks, Registry};

/// Everything the library needs, all of it injected.
pub struct AssemblyParts {
    /// The model client. Real profiles arrive in ticket 02; tests inject fakes.
    pub provider: Box<dyn Provider>,
    /// The agent this session acts as.
    pub speaker: SpeakerId,
    /// Session working directory, recorded in `SessionStarted`.
    pub cwd: PathBuf,
    /// Path of this session's JSONL event log. Its parent must exist.
    pub log_path: PathBuf,
    /// Session identity; never changes across `--continue`.
    pub session_id: SessionId,
    /// Injected configuration values.
    pub config: SessionConfig,
    /// The tool table. A runtime value, assembled here and never a global.
    pub tools: Registry,
    /// Per-path write locks. The **same** table must reach every executor, or
    /// write exclusion is per session and therefore no lock at all.
    pub locks: PathLocks,
    /// The headless renderer's two explicit sinks.
    pub sinks: RenderSinks,
}

/// The assembled harness the caller drives.
pub struct Harness {
    session: Session,
    provider: Box<dyn Provider>,
    speaker: SpeakerId,
    render: RenderHandle,
    render_task: JoinHandle<()>,
}

/// Assemble a single-agent session. Reads no environment.
pub async fn assemble(parts: AssemblyParts) -> Result<Harness, Error> {
    let AssemblyParts {
        provider,
        speaker,
        cwd,
        log_path,
        session_id,
        config,
        tools,
        locks,
        sinks,
    } = parts;

    // Tool artifacts live beside the event log, so a session stays one movable
    // directory (spec §11). The directory is created lazily by the tool that
    // needs it.
    let outputs_dir = log_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("outputs");

    let (render, render_task) = render::spawn_headless(sinks);
    let log = EventLog::create(log_path)?;
    let mut session = Session::new(session_id, cwd, log, config, tools, locks, outputs_dir);
    agent::record_session_started(&mut session, &render)?;

    Ok(Harness {
        session,
        provider,
        speaker,
        render,
        render_task,
    })
}

impl Harness {
    /// Record a user message and run one turn to completion.
    pub async fn run_turn(&mut self, user_input: &str) -> Result<TurnOutcome, Error> {
        agent::record_user_message(&mut self.session, &self.render, user_input)?;
        agent::run_turn(
            &mut self.session,
            &self.speaker,
            self.provider.as_ref(),
            &self.render,
        )
        .await
    }

    pub fn session_id(&self) -> &SessionId {
        self.session.id()
    }

    /// Where this session's tool artifacts land (`outputs/<tool_call_id>.*`).
    pub fn outputs_dir(&self) -> &std::path::Path {
        self.session.outputs_dir()
    }

    /// Drop the render channel and wait for every buffered render event to be
    /// written to the sinks. Call this before asserting on captured sinks.
    pub async fn shutdown(self) {
        let Harness {
            render,
            render_task,
            ..
        } = self;
        drop(render);
        let _ = render_task.await;
    }
}

/// Errors that stop assembly or the loop itself. Provider failures are turned
/// into `TurnEnded { Error }` by the loop and are not this type.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("event log i/o error: {0}")]
    Io(#[from] io::Error),
}
