//! Tool boundary (spec §7): the `Tool` trait, the runtime registry, the edit
//! matching ladder, and the dispatch point that enforces the shared guardrails.
//!
//! The registry is a runtime value carried by the session (never a global
//! static), so dynamic tools have a mount point and the tool table stays fixed
//! for the life of the prefix cache.
//!
//! Dependency shape: `edit` and `paths` depend on nothing here, `bash` depends
//! on `tool` only (and spawns a process), `file` depends on `edit`, `tool` and
//! `paths`, and `registry` sits on top. Nothing in `tools` writes events or reads
//! the environment; `bash` hands its command to a shell that inherits the
//! caller's environment, which is the one thing in this boundary that reaches
//! outside the process.

pub mod bash;
pub mod edit;
pub mod file;
pub mod paths;
pub mod registry;
pub mod repo_map;
pub mod skill;
pub mod task;
pub mod tool;

pub use bash::{
    BashTool, BASH_TOOL, EXIT_CODE_PREFIX, STDERR_HEADER, STDOUT_HEADER, TIMEOUT_PREFIX,
};
pub use file::{
    before_artifact, EditCall, EditFile, ReadFile, WriteFile, EDIT_FILE, MATCH_LEVEL_PREFIX,
    READ_FILE, WRITE_FILE, WROTE_PATH_PREFIX,
};
pub use paths::{write_owner_only, PathLocks, SessionPaths};
pub use registry::{AllowedCall, CallFacts, DispatchOutcome, GuardedCall, PendingCall, Registry};
pub use repo_map::RepoMapTool;
pub use skill::SkillTool;
pub use task::{TaskTool, TASK_TOOL};
pub use tool::{
    BashLimits, Effect, ExecutorSpawner, ReadPathResolver, ReadSet, Tool, ToolContext, ToolError,
    ToolOutput, WritePathResolver,
};

/// The v1 built-in tools.
///
/// `skill` and `repo_map` are stateless wrappers around session-carried values:
/// `skill` reads the discovered skill library through [`ToolContext`], and
/// `repo_map` reads the session's ranking context the same way. `task` is the
/// same shape — a stateless wrapper around the executor port the loop injects —
/// and is the one tool an executor's table does not get (spec §16). `bash` is
/// stateless too: its two limits arrive through [`ToolContext`] like the repo
/// map's budget.
pub fn builtin() -> Registry {
    let mut registry = Registry::new();
    registry.register(Box::new(ReadFile));
    registry.register(Box::new(WriteFile));
    registry.register(Box::new(EditFile));
    registry.register(Box::new(BashTool));
    registry.register(Box::new(SkillTool));
    registry.register(Box::new(RepoMapTool::new()));
    registry.register(Box::new(TaskTool));
    registry
}
