//! Dynamically declared tools (spec §14).
//!
//! A user declares a tool in `config.toml` as an argv template plus a JSON
//! Schema; it then behaves like a built-in. Three properties are structural:
//!
//! * **It cannot claim to be read-only.** The declaration syntax has no
//!   side-effect field, and [`Tool::effect`] is `Exclusive` for every call. A
//!   genuinely read-only dynamic tool is therefore serialized workspace-wide —
//!   the documented cost of not trusting a declaration — and the constraint that
//!   remains is the permission gate, because `read-before-edit` cannot see a
//!   `WritePaths` the tool never declares.
//! * **It cannot inject shell.** The argv is spawned directly
//!   ([`super::process`]); a parameter replaces one whole argv element and an
//!   array or object becomes a single element rather than being expanded.
//! * **Its name is lexically recognizable.** Every declared tool is
//!   `custom__<namespace>__<tool>` and no built-in name contains `__`, so
//!   [`is_custom_tool`] is a predicate over the string alone.

use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use crate::config::{parameter_placeholder, ToolDeclaration, CUSTOM_TOOL_SEPARATOR};
use crate::provider::ToolSpec;

use super::process;
use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// Whether a tool name came from configuration rather than from the built-in
/// table. No built-in name contains `__`, so the test is lexical (spec §14).
pub fn is_custom_tool(name: &str) -> bool {
    name.contains(CUSTOM_TOOL_SEPARATOR)
}

/// One dynamically declared tool, wrapping its resolved declaration.
pub struct CustomTool {
    declaration: ToolDeclaration,
}

impl CustomTool {
    pub fn new(declaration: ToolDeclaration) -> Self {
        Self { declaration }
    }

    /// The argv for one call: each `{parameter}` element is replaced by the
    /// argument, and omitted when the argument is absent.
    ///
    /// Substitution is by **whole argv element**. A parameter that is an array or
    /// an object is serialized into one element, never expanded into several, so
    /// no argument can change the shape of the command.
    pub fn argv(&self, args: &Value) -> Vec<String> {
        let mut argv = Vec::with_capacity(self.declaration.command.len());
        for element in &self.declaration.command {
            let Some(name) = parameter_placeholder(element) else {
                // A literal element is used as written.
                argv.push(element.clone());
                continue;
            };
            match args.get(name) {
                // Absent or null omits the element entirely.
                None | Some(Value::Null) => {}
                Some(value) => argv.push(render_argument(value)),
            }
        }
        argv
    }
}

#[async_trait]
impl Tool for CustomTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.declaration.name.clone(),
            description: self.declaration.description.clone(),
            parameters: self.declaration.parameters.clone(),
        }
    }

    /// Always `Exclusive` (spec §14): the declaration has no field for an effect
    /// class, so "this one is really read-only" has nowhere to be said. The
    /// scheduler's conservative path needs no special case.
    fn effect(&self, _args: &Value) -> Effect {
        Effect::Exclusive
    }

    /// The argv the permission gate's `CommandPrefix` scope reads, built the same
    /// way [`Tool::call`] builds it so the gate and the process cannot disagree.
    fn command(&self, args: &Value) -> Option<Vec<String>> {
        let argv = self.argv(args);
        (!argv.is_empty()).then_some(argv)
    }

    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        let argv = self.argv(&args);
        if argv.is_empty() {
            return Err(ToolError::message(format!(
                "{}: the call produced no argv to run",
                self.declaration.name
            )));
        }
        let limit = Duration::from_millis(self.declaration.timeout_ms);
        let outcome = process::run(ctx.cwd, &argv, limit).await?;
        Ok(ToolOutput::new(outcome.report()))
    }
}

/// A parameter value as one argv element.
///
/// A string is used verbatim (no quoting layer exists to escape); every other
/// JSON value is its compact serialization, so an array or object is one
/// element, not a splat.
fn render_argument(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
