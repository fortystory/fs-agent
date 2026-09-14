//! The built-in `repo_map(focus?)` tool: the workspace's symbols on demand.
//!
//! The same shape as `skill(name)` (spec §9): a built-in **read-only** tool whose
//! product is an ordinary tool result, so it is accounted for, truncated and
//! dropped by the same machinery as any other call. It is deliberately **not**
//! injected: an injected map would have to be refreshed as files change, and
//! every refresh would push the history after it out of the cached prefix.
//!
//! It reads no workspace path the model supplied — it walks the session cwd
//! itself — so `effect()` is [`Effect::ReadOnly`] and it declares no read paths,
//! which keeps it available in the read-only modes.

use async_trait::async_trait;
use serde_json::Value;

use crate::context::repo_map::{RepoMap, REPO_MAP_TOOL};
use crate::provider::ToolSpec;

use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// Render the workspace's symbol map within the configured budget.
pub struct RepoMapTool {
    map: RepoMap,
}

impl RepoMapTool {
    /// Compile the official tags query once, when the tool table is assembled.
    pub fn new() -> Self {
        Self {
            map: RepoMap::new(),
        }
    }
}

impl Default for RepoMapTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for RepoMapTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: REPO_MAP_TOOL.to_owned(),
            description: "Map this workspace's Rust symbols: each file and the functions, types, \
                          traits, modules and macros it defines. Call it first when exploring an \
                          unfamiliar repository. Pass `focus` (identifiers, a subsystem, a path) \
                          to bias the map toward one part of the code. The map is ranked by what \
                          this session recently read or mentioned and is capped by a fixed \
                          budget; it is a snapshot of the files as they are now."
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "focus": {
                        "type": "string",
                        "description": "Optional words to bias the map toward: identifiers, a subsystem, or a path. There is no `tokens` argument; the budget is configuration."
                    }
                }
            }),
        }
    }

    fn effect(&self, _args: &Value) -> Effect {
        Effect::ReadOnly
    }

    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        // The budget is configuration, never an argument: a `tokens` key the model
        // sends anyway is ignored, not honoured.
        let focus = args
            .get("focus")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|focus| !focus.is_empty());
        let context = ctx.repo_map.context.clone().with_focus(focus);
        let map = self.map.build(ctx.cwd, &context, ctx.repo_map.tokens);
        if map.is_empty() {
            return Ok(ToolOutput::new(format!(
                "no Rust symbols found under {}",
                ctx.cwd.display()
            )));
        }
        Ok(ToolOutput::new(map))
    }
}
