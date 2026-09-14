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
pub mod skill;
pub mod tool;

pub use file::{EditFile, ReadFile, WriteFile, MATCH_LEVEL_PREFIX};
pub use paths::{PathLocks, SessionPaths};
pub use registry::{AllowedCall, CallFacts, DispatchOutcome, GuardedCall, PendingCall, Registry};
pub use skill::SkillTool;
pub use tool::{
    Effect, ReadPathResolver, ReadSet, Tool, ToolContext, ToolError, ToolOutput, WritePathResolver,
};

/// The v1 built-in tools. The remaining built-ins (`bash`, `repo_map`, `task`)
/// mount on the same registry as their tickets land.
///
/// `skill` is stateless: it reads the session's discovered skill library through
/// [`ToolContext`], so the tool table stays fixed while the library follows the
/// session's cwd (spec §9).
pub fn builtin() -> Registry {
    let mut registry = Registry::new();
    registry.register(Box::new(ReadFile));
    registry.register(Box::new(WriteFile));
    registry.register(Box::new(EditFile));
    registry.register(Box::new(SkillTool));
    registry
}
