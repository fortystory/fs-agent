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
pub mod tool;

pub use file::{EditFile, ReadFile, WriteFile, MATCH_LEVEL_PREFIX};
pub use paths::{PathLocks, SessionPaths};
pub use registry::{AllowedCall, DispatchOutcome, GuardedCall, PendingCall, Registry};
pub use tool::{
    Effect, ReadPathResolver, ReadSet, Tool, ToolContext, ToolError, ToolOutput, WritePathResolver,
};

/// The v1 built-in file tools (spec §7). The remaining built-ins (`bash`,
/// `skill`, `repo_map`, `task`) mount on the same registry as their tickets land.
pub fn builtin() -> Registry {
    let mut registry = Registry::new();
    registry.register(Box::new(ReadFile));
    registry.register(Box::new(WriteFile));
    registry.register(Box::new(EditFile));
    registry
}
