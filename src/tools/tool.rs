//! The `Tool` trait and the side-effect classification the scheduler routes on.
//!
//! `effect` describes **workspace** side effects, not "has side effects at all":
//! a tool that reads a file is `ReadOnly`, and a tool that spawns an agent
//! without touching the workspace would also be `ReadOnly` (spec §7). The
//! scheduler partitions a batch of calls by this value, so parallel read-only
//! execution is wiring rather than a refactor.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;

use crate::context::repo_map::RepoMapInput;
use crate::context::skills::Skills;
use crate::provider::ToolSpec;

/// The workspace side effect of one planned call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Reads the workspace and writes nothing. These may run concurrently.
    ReadOnly,
    /// Writes exactly these paths (each is an input the tool will resolve).
    WritePaths(Vec<PathBuf>),
    /// Takes the workspace exclusively; nothing else may run at the same time.
    ///
    /// No v1 tool produces this yet — `bash` (a later ticket) is its owner — but
    /// the dispatcher already takes the workspace-wide lock for it, because
    /// `effect()` is the one side-effect vocabulary the scheduler and the
    /// permission gate share (spec §7).
    Exclusive,
}

/// The result of a successful tool call, ready to become a `ToolCallCompleted`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    pub text: String,
}

impl ToolOutput {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

/// Why a tool call failed. `InvalidatesReads` is the one variant the dispatcher
/// reacts to structurally: a write that could not find its match means the
/// agent's picture of the file is stale, so its read permission is withdrawn.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    #[error("{0}")]
    Message(String),
    #[error("{message}")]
    InvalidatesReads { message: String, path: PathBuf },
}

impl ToolError {
    pub fn message(message: impl Into<String>) -> Self {
        ToolError::Message(message.into())
    }

    /// The path whose read permission is withdrawn, if any.
    pub fn invalidated_path(&self) -> Option<&Path> {
        match self {
            ToolError::InvalidatesReads { path, .. } => Some(path),
            ToolError::Message(_) => None,
        }
    }

    /// True when this failure means the path's read permission must be dropped.
    pub fn invalidates_reads(&self) -> bool {
        self.invalidated_path().is_some()
    }
}

/// What a tool is handed for one call. The tool resolves its own paths (it knows
/// which argument carries one), and the resolver is what keeps a model-supplied
/// path inside the session cwd.
pub struct ToolContext<'a> {
    /// Paths the model may read. Reads outside the session cwd are refused.
    pub read_paths: &'a dyn ReadPathResolver,
    /// Paths the model may write.
    pub write_paths: &'a dyn WritePathResolver,
    /// Where oversized output and `.before` snapshots land.
    pub outputs_dir: &'a Path,
    /// Session cwd, for display and for tools that run relative to it.
    pub cwd: &'a Path,
    /// The session's discovered skill library (spec §9). A value discovered at
    /// assembly, so `skill(name)` looks up rather than resolving a path.
    pub skills: &'a Skills,
    /// The `repo_map` tool's session inputs (spec §9): the ranking context and
    /// the configured budget. Bundled into one field so it travels like `skills`.
    pub repo_map: &'a RepoMapInput,
    /// The port that runs a nested executor, for `task` (spec §16). `None` when
    /// the session mounted no port, in which case `task` reports that rather than
    /// pretending to work.
    pub executor: Option<&'a dyn ExecutorSpawner>,
    pub tool_call_id: &'a str,
    pub args: &'a Value,
}

/// The port a tool uses to run a nested executor (spec §16).
///
/// It lives here, and is implemented by the `agent` layer (`agent::executor`),
/// because running an executor means driving a whole turn: that layer is the only
/// writer of the event stream and the only caller of a provider. `tools` knows
/// only the shape, so the dependency arrow still points down.
#[async_trait]
pub trait ExecutorSpawner: Send + Sync {
    /// Run one executor to completion for `brief`, and report back already shaped
    /// as the `task` call's one result.
    ///
    /// The result is the summary plus the metadata derived from the stream
    /// (spec §16); the executor's process reaches no one else.
    async fn spawn(&self, brief: &str) -> Result<ToolOutput, ToolError>;
}

/// Resolve a model-supplied read path against the session cwd.
pub trait ReadPathResolver: Send + Sync {
    fn resolve_read(&self, path: &Path) -> Result<PathBuf, ToolError>;
}

/// Resolve a model-supplied write path against the session cwd.
pub trait WritePathResolver: Send + Sync {
    fn resolve_write(&self, path: &Path) -> Result<PathBuf, ToolError>;
}

/// Whether a path has been read in this session. Read permission is per agent
/// and is never inherited, in either direction (spec §16).
#[derive(Debug, Default)]
pub struct ReadSet {
    paths: HashSet<PathBuf>,
}

impl ReadSet {
    pub fn record(&mut self, path: impl Into<PathBuf>) {
        self.paths.insert(path.into());
    }

    pub fn record_all(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for path in paths {
            self.record(path);
        }
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }

    pub fn invalidate(&mut self, path: &Path) {
        self.paths.remove(path);
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

/// One tool in the runtime registry.
///
/// Object safe and async: `Box<dyn Tool>` is what the registry stores, and
/// `call` takes erased JSON so dynamic tools have a door in without a second
/// trait.
#[async_trait]
pub trait Tool: Send + Sync {
    /// The wire-level declaration, sent to the provider as written.
    fn spec(&self) -> ToolSpec;

    /// The workspace side effect of this call. A pure function of the args.
    fn effect(&self, args: &Value) -> Effect;

    /// Paths this call looked at, so the dispatcher can grant read permission.
    ///
    /// A read tool declares its own path here instead of the dispatcher guessing
    /// at an argument name, which keeps the tool's wire contract its own.
    fn read_paths(&self, _args: &Value) -> Vec<PathBuf> {
        Vec::new()
    }

    /// The argv this call will execute, for a tool that runs a command.
    ///
    /// `bash` is its owner (a later ticket); every other tool answers `None`.
    /// The permission gate's `CommandPrefix` scope and the `rm` circuit breaker
    /// both read this, so a command's argv must be visible **before** the
    /// process starts — which is why the tool declares it here instead of the
    /// gate guessing at a `command` string.
    fn command(&self, _args: &Value) -> Option<Vec<String>> {
        None
    }

    /// Whether this tool is part of an executor's table.
    ///
    /// Recursion depth is one (spec §16), and that is enforced by the tool table
    /// rather than by a rule: `task` answers `false`, so an executor's tool
    /// declaration simply has no way to dispatch another executor. A rule would
    /// be a second, weaker line of defense — the tool would still be advertised
    /// to the model, and a rule bug would be a recursion bug.
    fn delegable(&self) -> bool {
        true
    }

    /// Arguments arrive erased; each tool parses its own shape and reports its
    /// own errors.
    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError>;
}
