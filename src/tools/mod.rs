//! Tool boundary (spec §7): the `Tool` trait, the runtime registry, the edit
//! matching ladder, and the dispatch point that enforces the shared guardrails.
//!
//! The registry is a runtime value carried by the session (never a global
//! static), so dynamic tools have a mount point and the tool table stays fixed
//! for the life of the prefix cache.
//!
//! Dependency shape: `edit` and `paths` depend on nothing here, `file` depends on
//! `edit`, `tool` and `paths`, and `registry` sits on top. Nothing in `tools`
//! writes events or reads the environment.

pub mod edit;
pub mod file;
pub mod paths;
pub mod registry;
pub mod repo_map;
pub mod skill;
pub mod task;
pub mod tool;

pub use file::{EditFile, ReadFile, WriteFile, MATCH_LEVEL_PREFIX, WROTE_PATH_PREFIX};
pub use paths::{PathLocks, SessionPaths};
pub use registry::{AllowedCall, CallFacts, DispatchOutcome, GuardedCall, PendingCall, Registry};
pub use repo_map::RepoMapTool;
pub use skill::SkillTool;
pub use task::{TaskTool, TASK_TOOL};
pub use tool::{
    Effect, ExecutorSpawner, ReadPathResolver, ReadSet, Tool, ToolContext, ToolError, ToolOutput,
    WritePathResolver,
};

/// The v1 built-in tools. The remaining built-in (`bash`) mounts on the same
/// registry as its ticket lands.
///
/// `skill` and `repo_map` are stateless wrappers around session-carried values:
/// `skill` reads the discovered skill library through [`ToolContext`], and
/// `repo_map` reads the session's ranking context the same way. `task` is the
/// same shape — a stateless wrapper around the executor port the loop injects —
/// and is the one tool an executor's table does not get (spec §16).
pub fn builtin() -> Registry {
    let mut registry = Registry::new();
    registry.register(Box::new(ReadFile));
    registry.register(Box::new(WriteFile));
    registry.register(Box::new(EditFile));
    registry.register(Box::new(SkillTool));
    registry.register(Box::new(RepoMapTool::new()));
    registry.register(Box::new(TaskTool));
    registry
}
