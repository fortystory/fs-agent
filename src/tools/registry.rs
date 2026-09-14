//! The runtime registry and the one dispatch point (spec §3, §7).
//!
//! The registry is a value carried by the session, never a global static: it is
//! assembled at startup, fixed for the life of the prefix cache, and is the
//! mount point dynamic tools plug into.
//!
//! Dispatch is split in two so the guardrails can be enforced without the tool
//! layer ever touching the read set:
//!
//! 1. [`Registry::guardrails`] is a pure read of `(tool, args, read set, cwd)`.
//!    It resolves the write targets, refuses a write whose path has not been
//!    read, and returns the paths whose read permission the call earns.
//! 2. [`Registry::dispatch`] runs the decided call, holding the shared per-path
//!    write locks, and reports whether a failed match withdrew a path's read
//!    permission.
//!
//! The loop is what applies 1's decision to the agent that made the call, which
//! is also why only the `agent` module writes the event stream.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;

use crate::provider::ToolSpec;

use super::paths::{PathLocks, SessionPaths};
use super::tool::{Effect, ReadSet, Tool, ToolContext, ToolError, ToolOutput};

/// The tool table for one session.
#[derive(Default)]
pub struct Registry {
    tools: BTreeMap<String, Box<dyn Tool>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mount a tool. Runtime registration is what makes dynamic tools possible
    /// without a second registry.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.spec().name, tool);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|tool| tool.as_ref())
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Every declaration, in stable name order, as the provider wants them.
    ///
    /// Stable order matters: the tool array is part of the cached prefix.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|tool| tool.spec()).collect()
    }

    /// Decide whether one call may run, and what it is allowed to touch.
    ///
    /// A refusal here means the call never reaches the tool, which is why the
    /// loop synthesizes the one required error result for it.
    pub fn guardrails(
        &self,
        tool_name: &str,
        args: &Value,
        read_set: &ReadSet,
        paths: &SessionPaths,
    ) -> GuardedCall {
        let Some(tool) = self.get(tool_name) else {
            return GuardedCall::refused(ToolError::message(format!(
                "no tool registered: {tool_name}"
            )));
        };

        let effect = tool.effect(args);
        let mut write_targets = Vec::new();
        let mut exclusive = false;
        match &effect {
            Effect::ReadOnly => {}
            // `Exclusive` takes no path locks: it takes the workspace-wide lock,
            // acquired before any path lock so the two orders cannot deadlock.
            Effect::Exclusive => exclusive = true,
            Effect::WritePaths(inputs) => {
                for input in inputs {
                    match paths.resolve(input) {
                        Ok(path) => write_targets.push(path),
                        Err(error) => return GuardedCall::refused(error),
                    }
                }
            }
        }
        // Path order, so two multi-path writes cannot deadlock on each other.
        write_targets.sort();
        write_targets.dedup();

        // Read before edit, enforced for every caller rather than by convention.
        // Only an existing target needs reading: overwriting a file the agent has
        // not looked at is the failure this guards, while creating a new one has
        // nothing to clobber and nothing to read.
        if let Some(path) = write_targets
            .iter()
            .find(|path| path.exists() && !read_set.contains(path))
        {
            return GuardedCall::refused(ToolError::message(format!(
                "read before write: {} exists but has not been read in this session; read it first",
                path.display()
            )));
        }

        // Candidates for the read set. The loop records them only if the call
        // succeeds: a failed read must not license a later write.
        let read_paths = tool
            .read_paths(args)
            .into_iter()
            .filter_map(|path| paths.resolve(&path).ok())
            .collect();

        GuardedCall::Run(AllowedCall {
            write_targets,
            read_paths,
            exclusive,
        })
    }

    /// Run one call whose guardrails have already been applied.
    ///
    /// The tool is looked up again rather than lent, so the caller can hold the
    /// decision while borrowing its own read set mutably elsewhere.
    pub async fn dispatch(&self, call: &PendingCall, allowed: &AllowedCall) -> DispatchOutcome {
        let Some(tool) = self.get(&call.tool_name) else {
            return DispatchOutcome::failure(
                ToolError::message(format!("no tool registered: {}", call.tool_name)),
                false,
            );
        };

        // The workspace lock first, then the path locks, both held for the whole
        // call. `Exclusive` is the only effect that takes the workspace lock.
        let mut guards = Vec::new();
        if allowed.exclusive {
            guards.push(call.locks.lock_exclusive().await);
        }
        for path in &allowed.write_targets {
            guards.push(call.locks.lock(path).await);
        }

        let ctx = ToolContext {
            read_paths: &call.paths,
            write_paths: &call.paths,
            outputs_dir: &call.outputs_dir,
            cwd: call.paths.cwd(),
            tool_call_id: &call.tool_call_id,
            args: &call.args,
        };
        let result = tool.call(&ctx, call.args.clone()).await;
        drop(guards);

        match result {
            Ok(output) => DispatchOutcome::success(output),
            Err(error) => {
                // A failed match on a path this call was writing means the
                // agent's picture of it is stale.
                let invalidated = !allowed.write_targets.is_empty() && error.invalidates_reads();
                DispatchOutcome::failure(error, invalidated)
            }
        }
    }
}

/// The guardrail verdict for one call.
#[derive(Debug, Clone)]
pub enum GuardedCall {
    /// The call may run with these paths.
    Run(AllowedCall),
    /// The call never reaches the tool; the error is the required result.
    Refused(ToolError),
}

impl GuardedCall {
    fn refused(error: ToolError) -> Self {
        GuardedCall::Refused(error)
    }
}

/// What an allowed call may touch, already resolved against the session cwd.
#[derive(Debug, Clone, Default)]
pub struct AllowedCall {
    /// Resolved write targets, locked for the whole call.
    pub write_targets: Vec<PathBuf>,
    /// Resolved reads to record in the agent's read set **if the call succeeds**.
    pub read_paths: Vec<PathBuf>,
    /// True when the call demands the workspace-wide lock.
    pub exclusive: bool,
}

/// Everything the dispatcher needs about one call the loop has already recorded.
#[derive(Debug, Clone)]
pub struct PendingCall {
    pub tool_call_id: String,
    pub tool_name: String,
    pub args: Value,
    pub outputs_dir: PathBuf,
    pub paths: SessionPaths,
    pub locks: PathLocks,
}

/// What one dispatch produced.
#[derive(Debug)]
pub struct DispatchOutcome {
    pub result: Result<ToolOutput, ToolError>,
    /// True when this failure withdrew a path's read permission.
    pub invalidated_reads: bool,
}

impl DispatchOutcome {
    pub fn success(output: ToolOutput) -> Self {
        Self {
            result: Ok(output),
            invalidated_reads: false,
        }
    }

    pub fn failure(error: ToolError, invalidated_reads: bool) -> Self {
        Self {
            result: Err(error),
            invalidated_reads,
        }
    }

    pub fn is_ok(&self) -> bool {
        self.result.is_ok()
    }
}
