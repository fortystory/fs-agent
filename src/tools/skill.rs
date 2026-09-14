//! The built-in `skill(name)` tool: load one skill's instructions on demand.
//!
//! This is the one funnel for full-text loading (spec §9): the result is an
//! ordinary tool result, so it is accounted for, subject to truncation, and
//! covered by the same permission language as any other call. It is
//! [`Effect::ReadOnly`] because it touches no workspace path — the library it
//! reads was discovered at assembly time, before any model-supplied path
//! exists — so it stays usable in the read-only modes and can run concurrently.

use async_trait::async_trait;
use serde_json::Value;

use crate::provider::ToolSpec;

use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// Load a discovered skill body by name.
pub struct SkillTool;

#[async_trait]
impl Tool for SkillTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "skill".to_owned(),
            description: "Load a skill's full instructions by name. The skills catalog in the \
                          conversation lists the available names and when each one applies."
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the skill to load, exactly as it appears in the skills catalog."
                    }
                },
                "required": ["name"]
            }),
        }
    }

    fn effect(&self, _args: &Value) -> Effect {
        Effect::ReadOnly
    }

    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| ToolError::message("skill: a non-empty `name` is required"))?;
        ctx.skills
            .load(name)
            .map(ToolOutput::new)
            .map_err(|error| ToolError::message(error.to_string()))
    }
}
