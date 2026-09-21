//! The built-in `task(brief)` tool: dispatch an executor (spec §16).
//!
//! The tool itself is a thin shell. Everything an executor *is* — its nested
//! session, its own budget, the permissions that travel to it, the events it
//! writes — belongs to the `agent` layer (`agent::executor`), which drives turns
//! and owns the event stream. The shell exists so that dispatching an executor is
//! an ordinary tool call: it gets exactly one result, it is judged by the
//! permission gate, and it needs no second delivery mechanism.
//!
//! Two properties are the tool's own:
//!
//! * `effect` is [`ReadOnly`](Effect::ReadOnly) because `effect` classifies
//!   **workspace** side effects (spec §7) and dispatching touches no workspace
//!   path. That is what lets several `task` calls in one batch run at once; the
//!   real write exclusion happens on the executor's own calls, through the shared
//!   path locks.
//! * it is not [`delegable`](Tool::delegable), so an executor's table has no
//!   `task` at all (recursion depth one, spec §16).

use async_trait::async_trait;
use serde_json::Value;

use crate::provider::ToolSpec;

use super::tool::{Effect, ExecutorSpawner, Tool, ToolContext, ToolError, ToolOutput};

/// The tool name, named once so the registry, the loop and the tests cannot
/// drift apart.
pub const TASK_TOOL: &str = "task";

/// Dispatch an executor to carry out one task.
pub struct TaskTool;

#[async_trait]
impl Tool for TaskTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TASK_TOOL.to_owned(),
            description: "Dispatch an executor: a separate agent with its own context and its own \
                          budget that carries out one task in this workspace. Write the brief so it \
                          stands alone — the executor sees the repository rules and your brief, not \
                          this conversation. It reports back a summary; you do not see its steps, its \
                          tool calls or their output. It cannot dispatch further executors."
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "brief": {
                        "type": "string",
                        "description": "The task, self-contained: what to do, where, and what the \
                                        summary should report back."
                    }
                },
                "required": ["brief"]
            }),
        }
    }

    /// Dispatching touches no workspace path: the executor's own calls are what
    /// may write, each judged by the gate on the way (spec §16).
    fn effect(&self, _args: &Value) -> Effect {
        Effect::ReadOnly
    }

    fn delegable(&self) -> bool {
        false
    }

    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        let brief = args
            .get("brief")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|brief| !brief.is_empty())
            .ok_or_else(|| {
                ToolError::message(format!("{TASK_TOOL}: a non-empty `brief` is required"))
            })?;
        let Some(spawner): Option<&dyn ExecutorSpawner> = ctx.executor else {
            return Err(ToolError::message(format!(
                "{TASK_TOOL}: this session mounted no executor port"
            )));
        };
        spawner.spawn(brief).await
    }
}
