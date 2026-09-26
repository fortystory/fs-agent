//! The built-in `bash(command, timeout_ms?)` tool: run one shell command in the
//! session workspace (spec §7, §12, §20).
//!
//! Three decisions define this tool:
//!
//! * `effect()` is **always** [`Effect::Exclusive`]: a shell can write anything,
//!   so the dispatcher takes the workspace-wide lock and the permission gate
//!   treats the call as a write. `readonly` therefore refuses it without a
//!   special case, and there is no write exemption left to borrow (the old plan
//!   mode's `PLAN.md` one is gone — `docs/adr/0003-plan-leaves-the-permission-modes.md`).
//! * the command runs as **one argv element** — `["bash", "-lc", command]`
//!   spawned directly, never a command string spliced into a larger shell line —
//!   so the model cannot add a second layer of shell substitution.
//! * the timeout terminates the **process group**, not just the shell, so a
//!   command that started children does not leave them behind. The same guard
//!   kills the group if the call is dropped mid-flight (a cancel gesture).
//!
//! The last two are mechanics every command tool needs, so they live in
//! [`super::process`] and are shared with the dynamic tools (spec §14); this
//! module is the shell-specific part.

use async_trait::async_trait;
use serde_json::Value;

use crate::provider::ToolSpec;

use super::process;
use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// The tool name, named once so the registry, the loop and the tests cannot
/// drift apart.
pub const BASH_TOOL: &str = "bash";

/// The shell and the flag that make it read the command string. `-l` gives the
/// command the user's login environment; `-c` is what takes the one argument.
const SHELL: &str = "bash";
const SHELL_FLAG: &str = "-lc";

/// Run one command through the system shell.
pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: BASH_TOOL.to_owned(),
            description:
                "Run a shell command in the workspace and return its exit code, stdout and \
                          stderr. The command runs through `bash -lc`, non-interactively: there is \
                          no TTY and stdin is empty, so do not start interactive programs. A \
                          wall-clock timeout applies (120s in the default configuration; the \
                          session may configure a different default and maximum) and the whole \
                          process tree is killed when it expires. A non-zero exit is a normal \
                          result. The workspace is held exclusively for the duration, so prefer \
                          short, non-interactive commands."
                    .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to run, exactly as written. It is passed \
                                        to `bash -lc` as a single argument."
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Optional wall-clock cap in milliseconds. Defaults to the \
                                        configured value and is capped by the configured maximum."
                    }
                },
                "required": ["command"]
            }),
        }
    }

    /// A shell can write anything, so this is `Exclusive` for every call,
    /// whatever the command text says (spec §7).
    fn effect(&self, _args: &Value) -> Effect {
        Effect::Exclusive
    }

    /// The argv the gate sees before anything starts: the shell, its flag, and
    /// the command as **one** element. `CommandPrefix` matches this argv, and the
    /// `rm` breaker reads through the wrapper to the command string.
    fn command(&self, args: &Value) -> Option<Vec<String>> {
        argv(args)
    }

    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        let argv = argv(&args).ok_or_else(|| {
            ToolError::message(format!("{BASH_TOOL}: a non-empty `command` is required"))
        })?;
        let requested = requested_timeout_ms(&args)?;
        if requested == Some(0) {
            return Err(ToolError::message(format!(
                "{BASH_TOOL}: `timeout_ms` must be a positive number of milliseconds"
            )));
        }
        let limit = ctx.bash.timeout(requested);
        let outcome = process::run(ctx.cwd, &argv, limit).await?;
        Ok(ToolOutput::new(outcome.report()))
    }
}

/// The one argv this call runs, built from the args so [`Tool::command`] and
/// [`Tool::call`] cannot disagree about what will execute.
fn argv(args: &Value) -> Option<Vec<String>> {
    let command = args
        .get("command")
        .and_then(Value::as_str)
        .filter(|command| !command.trim().is_empty())?;
    Some(vec![
        SHELL.to_owned(),
        SHELL_FLAG.to_owned(),
        command.to_owned(),
    ])
}

/// The model's `timeout_ms`, when it sent one. A value that is present but is not
/// a non-negative integer is an argument error rather than a silent fallback.
fn requested_timeout_ms(args: &Value) -> Result<Option<u64>, ToolError> {
    match args.get("timeout_ms") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_u64().map(Some).ok_or_else(|| {
            ToolError::message(format!(
                "{BASH_TOOL}: `timeout_ms` must be a positive integer of milliseconds"
            ))
        }),
    }
}
