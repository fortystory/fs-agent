//! The runtime registry and the one dispatch point (spec §3, §7).
//!
//! The registry is a value carried by the session, never a global static: it is
//! assembled at startup, fixed for the life of the prefix cache, and is the
//! mount point dynamic tools plug into.
//!
//! Dispatch is split in three so the guardrails can be enforced without the
//! tool layer ever touching the read set:
//!
//! 1. [`Registry::facts`] resolves one call into a [`CallFacts`]: the declared
//!    effect, the resolved write targets, the read paths and the argv. This is
//!    where model-supplied paths meet the session cwd, so it is where the
//!    filesystem is read; everything downstream is a function of values.
//! 2. [`CallFacts::guardrails`] refuses an unresolvable target and a write whose
//!    path has not been read, and returns the paths whose read permission the
//!    call earns. It consults the read set and the target's existence, never
//!    the registry.
//! 3. [`Registry::dispatch`] runs the decided call, holding the shared per-path
//!    write locks, and reports whether a failed match withdrew a path's read
//!    permission.
//!
//! The loop is what applies 1's decision to the agent that made the call, which
//! is also why only the `agent` module writes the event stream.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use crate::context::repo_map::RepoMapInput;
use crate::context::skills::Skills;
use crate::provider::ToolSpec;
use crate::questions::UserQuestions;

use super::paths::{PathLocks, SessionPaths};
use super::tool::{
    BashLimits, Effect, ExecutorSpawner, ReadSet, Tool, ToolContext, ToolError, ToolOutput,
};

/// The text a read-before-write refusal begins with.
///
/// The result is the only place the refusal is recorded — there is no field for
/// it — so producing it and the observability query that counts it share this
/// constant, the same "convention text" rule the edit match level follows
/// (spec §18). A drifting prefix would silently turn the count into zero.
pub const READ_BEFORE_WRITE_PREFIX: &str = "read before write: ";

/// The tool table for one session.
#[derive(Default)]
pub struct Registry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mount a tool. Runtime registration is what makes dynamic tools possible
    /// without a second registry.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.spec().name, Arc::from(tool));
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(Arc::as_ref)
    }

    /// The table a spawned executor gets (spec §16): the same tools, minus the
    /// ones that are not delegable.
    ///
    /// A fresh value rather than a view, because a table is what a session
    /// dispatches into and what a request's `tools` array is built from. It is a
    /// pure function of the assembled table, so every executor sees the same
    /// declaration order and the prefix cache keeps hitting.
    pub fn for_executor(&self) -> Registry {
        let tools = self
            .tools
            .iter()
            .filter(|(_, tool)| tool.delegable())
            .map(|(name, tool)| (name.clone(), Arc::clone(tool)))
            .collect();
        Registry { tools }
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

    /// Resolve one call into the facts both the permission gate and the
    /// guardrails read: the declared effect, the resolved write targets, the
    /// resolved read paths, and the argv a command tool will run.
    ///
    /// Path resolution is the only impure part (it canonicalizes), so it happens
    /// here, once, and the gate downstream stays a pure function of values. A
    /// target that cannot be resolved is kept in its lexical form and reported
    /// in [`CallFacts::path_error`]: the gate still sees the call, and refuses
    /// it on the path limit rather than recording a verdict the call never got
    /// to use.
    pub fn facts(
        &self,
        tool_name: &str,
        args: &Value,
        paths: &SessionPaths,
    ) -> Result<CallFacts, ToolError> {
        let Some(tool) = self.get(tool_name) else {
            return Err(ToolError::message(format!(
                "no tool registered: {tool_name}"
            )));
        };

        let effect = tool.effect(args);
        let mut write_targets = Vec::new();
        let mut path_error: Option<ToolError> = None;
        if let Effect::WritePaths(inputs) = &effect {
            for input in inputs {
                match paths.resolve(input) {
                    Ok(path) => write_targets.push(path),
                    Err(error) => {
                        path_error.get_or_insert(error);
                        write_targets.push(paths.unresolved(input));
                    }
                }
            }
        }
        // Path order, so two multi-path writes cannot deadlock on each other.
        write_targets.sort();
        write_targets.dedup();

        // Candidates for the read set. The loop records them only if the call
        // succeeds: a failed read must not license a later write. A read the
        // workspace cannot resolve is a path-limit denial like a write's.
        let mut read_paths = Vec::new();
        for path in tool.read_paths(args) {
            match paths.resolve(&path) {
                Ok(resolved) => read_paths.push(resolved),
                Err(error) => {
                    path_error.get_or_insert(error);
                }
            }
        }

        Ok(CallFacts {
            tool_name: tool_name.to_owned(),
            effect,
            write_targets,
            read_paths,
            argv: tool.command(args),
            path_error,
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
            skills: &call.skills,
            repo_map: &call.repo_map,
            bash: &call.bash,
            executor: call.executor.as_deref(),
            questions: call.questions.as_deref(),
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

/// One call, resolved: everything the permission gate and the guardrails read.
///
/// Built by [`Registry::facts`], which is where model-supplied paths meet the
/// session cwd and stop being strings.
#[derive(Debug, Clone)]
pub struct CallFacts {
    /// The tool name the model asked for.
    pub tool_name: String,
    /// The tool's declared workspace effect.
    pub effect: Effect,
    /// Resolved absolute write targets (empty unless the effect is `WritePaths`).
    /// A target that could not be resolved stays in its lexical form.
    pub write_targets: Vec<PathBuf>,
    /// Resolved absolute read paths.
    pub read_paths: Vec<PathBuf>,
    /// The argv a command tool will run, when it runs one.
    pub argv: Option<Vec<String>>,
    /// The first target that could not be resolved against the session cwd. The
    /// gate reads it as the path limit (the raw target is in `write_targets`),
    /// and the guardrails refuse it if the gate is ever bypassed.
    pub path_error: Option<ToolError>,
}

impl CallFacts {
    /// The shared guardrails: containment first, so an unresolvable target never
    /// reaches the tool, then read before edit, enforced for every caller rather
    /// than by convention.
    ///
    /// Only an existing target needs reading: overwriting a file the agent has
    /// not looked at is the failure this guards, while creating a new one has
    /// nothing to clobber and nothing to read. This consults the read set and
    /// each target's existence; it needs no registry.
    pub fn guardrails(&self, read_set: &ReadSet) -> GuardedCall {
        if let Some(error) = &self.path_error {
            return GuardedCall::Refused(error.clone());
        }

        if let Some(path) = self
            .write_targets
            .iter()
            .find(|path| path.exists() && !read_set.contains(path))
        {
            return GuardedCall::Refused(ToolError::message(format!(
                "{READ_BEFORE_WRITE_PREFIX}{} exists but has not been read in this session; \
                 read it first",
                path.display()
            )));
        }

        GuardedCall::Run(AllowedCall {
            write_targets: self.write_targets.clone(),
            read_paths: self.read_paths.clone(),
            exclusive: matches!(self.effect, Effect::Exclusive),
        })
    }
}

/// Everything the dispatcher needs about one call the loop has already recorded.
#[derive(Clone)]
pub struct PendingCall {
    pub tool_call_id: String,
    pub tool_name: String,
    pub args: Value,
    pub outputs_dir: PathBuf,
    pub paths: SessionPaths,
    pub locks: PathLocks,
    /// The session's discovered skill library. Cloned as a handle, like the path
    /// table, so a tool that needs it never reaches into the session.
    pub skills: Arc<Skills>,
    /// The `repo_map` tool's session inputs (spec §9). Owned, because the ranking
    /// context is recomputed per call from the event stream rather than shared.
    pub repo_map: RepoMapInput,
    /// The wall-clock limits a `bash` call runs under (spec §7). Owned: it is a
    /// `Copy` pair of numbers, and building it per call keeps configuration out
    /// of the registry.
    pub bash: BashLimits,
    /// The port that runs a nested executor, for a `task` call (spec §16). Built
    /// per call by the loop, which is what knows the provider and the renderer an
    /// executor needs.
    pub executor: Option<Arc<dyn ExecutorSpawner>>,
    /// The port that puts a model-initiated question to the user, for an
    /// `ask_user_question` call (spec §7). The session's, cloned as a handle like
    /// the skills: the loop is not what answers, so it only carries the port.
    pub questions: Option<Arc<dyn UserQuestions>>,
}

/// Hand-written because the port is an opaque handle: whether one is mounted is
/// the only thing a diagnostic can usefully say about it.
impl std::fmt::Debug for PendingCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingCall")
            .field("tool_call_id", &self.tool_call_id)
            .field("tool_name", &self.tool_name)
            .field("args", &self.args)
            .field("outputs_dir", &self.outputs_dir)
            .field("paths", &self.paths)
            .field("locks", &self.locks)
            .field("skills", &self.skills)
            .field("repo_map", &self.repo_map)
            .field("bash", &self.bash)
            .field("executor", &self.executor.is_some())
            .field("questions", &self.questions.is_some())
            .finish()
    }
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
