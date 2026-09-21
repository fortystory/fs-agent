//! The built-in `bash(command, timeout_ms?)` tool: run one shell command in the
//! session workspace (spec §7, §12, §20).
//!
//! Three decisions define this tool:
//!
//! * `effect()` is **always** [`Effect::Exclusive`]: a shell can write anything,
//!   so the dispatcher takes the workspace-wide lock and the permission gate
//!   treats the call as a write. Plan mode and `readonly` therefore refuse it
//!   without a special case.
//! * the command runs as **one argv element** — `["bash", "-lc", command]`
//!   spawned directly, never a command string spliced into a larger shell line —
//!   so the model cannot add a second layer of shell substitution.
//! * a timeout terminates the **process group**, not just the shell, so a
//!   command that started children does not leave them behind. The same guard
//!   kills the group if the call is dropped mid-flight (a cancel gesture).
//!
//! The result always carries the exit status, stdout and stderr; a non-zero exit
//! is a result, not a [`ToolError`]. Only a failure to spawn (or to wait for) the
//! shell itself is an error, because only then is there nothing to report.

use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::provider::ToolSpec;

use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// The tool name, named once so the registry, the loop and the tests cannot
/// drift apart.
pub const BASH_TOOL: &str = "bash";

/// The shell and the flag that make it read the command string. `-l` gives the
/// command the user's login environment; `-c` is what takes the one argument.
const SHELL: &str = "bash";
const SHELL_FLAG: &str = "-lc";

/// The result's section markers, named once so the tests and `docs/bash.md`
/// cannot drift from the format the model actually sees.
pub const EXIT_CODE_PREFIX: &str = "exit code: ";
pub const STDOUT_HEADER: &str = "--- stdout ---";
pub const STDERR_HEADER: &str = "--- stderr ---";
pub const TIMEOUT_PREFIX: &str = "timed out after ";

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
        let outcome = run(ctx.cwd, &argv, limit).await?;
        Ok(ToolOutput::new(outcome.render()))
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

/// What one finished (or killed) command produced.
struct CommandOutcome {
    /// Whether this call's own timeout fired and killed the group.
    timed_out: bool,
    /// The cap that was in force, for the timeout line.
    limit: Duration,
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

impl CommandOutcome {
    /// The result text: exit status, stdout and stderr in named sections.
    ///
    /// Truncation happens later, in the loop's one pre-stream pipeline (spec
    /// §10), so an oversized body spills and the stream keeps a preview plus a
    /// pointer without this tool knowing about any of it.
    fn render(&self) -> String {
        let mut text = String::new();
        if self.timed_out {
            text.push_str(&format!(
                "{TIMEOUT_PREFIX}{} ms; the process group was killed\n",
                self.limit.as_millis()
            ));
        }
        text.push_str(&format!(
            "{EXIT_CODE_PREFIX}{}\n",
            describe_status(&self.status)
        ));
        text.push_str(STDOUT_HEADER);
        text.push('\n');
        text.push_str(&self.stdout);
        if !self.stdout.is_empty() && !self.stdout.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(STDERR_HEADER);
        text.push('\n');
        text.push_str(&self.stderr);
        if !self.stderr.is_empty() && !self.stderr.ends_with('\n') {
            text.push('\n');
        }
        text
    }
}

/// A human-readable exit status: the code, or the signal that killed it.
fn describe_status(status: &ExitStatus) -> String {
    if let Some(code) = status.code() {
        return code.to_string();
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        if let Some(signal) = status.signal() {
            return format!("killed by signal {signal}");
        }
    }
    "unknown".to_owned()
}

/// Spawn the argv in its own process group, capture both streams, and enforce
/// `limit`.
///
/// Only a spawn or wait failure is a [`ToolError`]; the command's own exit status
/// is data.
async fn run(
    cwd: &std::path::Path,
    argv: &[String],
    limit: Duration,
) -> Result<CommandOutcome, ToolError> {
    let (program, rest) = argv
        .split_first()
        .ok_or_else(|| ToolError::message(format!("{BASH_TOOL}: empty argv")))?;

    let mut command = Command::new(program);
    command
        .args(rest)
        .current_dir(cwd)
        // Non-interactive by construction: no TTY is requested, stdin is empty,
        // and nothing here invents `TERM`/`NO_COLOR` (spec §20, `docs/bash.md`).
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Its own process group (`setsid`-equivalent), so the timeout can kill
        // the tree the command started and not just the shell (spec §7).
        .process_group(0)
        // A second line of defence for the direct child if the guard below is
        // ever disarmed early; it does not reach the tree, which is the guard's
        // job.
        .kill_on_drop(true);

    let mut child = command.spawn().map_err(|error| {
        ToolError::message(format!("{BASH_TOOL}: cannot spawn `{program}`: {error}"))
    })?;
    let pid = child.id().ok_or_else(|| {
        ToolError::message(format!(
            "{BASH_TOOL}: the shell exited before it could be tracked"
        ))
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ToolError::message(format!("{BASH_TOOL}: stdout was not piped")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ToolError::message(format!("{BASH_TOOL}: stderr was not piped")))?;

    // Readers run as their own tasks, so a pipe cannot fill up while the shell
    // is still running and the loop below can collect output as it arrives.
    let mut stdout_reader = tokio::spawn(read_to_end(stdout));
    let mut stderr_reader = tokio::spawn(read_to_end(stderr));

    let mut group = ProcessGroup::new(pid);
    let mut wait = Box::pin(child.wait());
    let mut deadline = tokio::time::Instant::now() + limit;
    let mut status: Option<ExitStatus> = None;
    let mut timed_out = false;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut stdout_open = true;
    let mut stderr_open = true;

    // The limit bounds the **whole** call, not just the shell's lifetime: a
    // backgrounded child inherits the output pipes, so `bash -lc "sleep 300 &"`
    // exits at once while the pipes stay open. Waiting only on the shell would
    // hang the turn past the timeout.
    loop {
        if status.is_some() && !stdout_open && !stderr_open {
            break;
        }
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(deadline) => {
                if timed_out {
                    // SIGKILL did not close every pipe — a process that left the
                    // group is holding one. Report what is known rather than
                    // hang the turn.
                    break;
                }
                timed_out = true;
                group.kill();
                // A short grace for the readers to see EOF after the kill.
                deadline = tokio::time::Instant::now() + KILL_GRACE;
            }
            result = &mut wait, if status.is_none() => {
                status = Some(result.map_err(|error| {
                    ToolError::message(format!(
                        "{BASH_TOOL}: cannot wait for the shell: {error}"
                    ))
                })?);
            }
            result = &mut stdout_reader, if stdout_open => {
                stdout = result.unwrap_or_default();
                stdout_open = false;
            }
            result = &mut stderr_reader, if stderr_open => {
                stderr = result.unwrap_or_default();
                stderr_open = false;
            }
        }
    }

    group.disarm();
    // End the wait future's borrow of `child`; re-waiting after a kill returns
    // the status the first wait already computed, or reaps the shell if the loop
    // broke early.
    drop(wait);
    let status = match status {
        Some(status) => status,
        None => child.wait().await.map_err(|error| {
            ToolError::message(format!(
                "{BASH_TOOL}: cannot reap the killed shell: {error}"
            ))
        })?,
    };
    Ok(CommandOutcome {
        timed_out,
        limit,
        status,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// How long the output readers get to see EOF after the process group is killed.
///
/// SIGKILL closes the pipes at once for every process still in the group; the
/// grace only matters when something deliberately left the group, and in that
/// case returning what is known beats hanging the turn.
const KILL_GRACE: Duration = Duration::from_secs(1);

/// Read a child stream to EOF, whatever the shell wrote.
async fn read_to_end<R>(mut reader: R) -> Vec<u8>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut buffer = Vec::new();
    // A read error means the stream ended; the bytes already read are still the
    // command's output.
    let _ = reader.read_to_end(&mut buffer).await;
    buffer
}

/// The shell's process group, killed if the call ends before the group does.
///
/// A timeout is not the only way a `bash` call stops early: the loop drops an
/// in-flight tool when the user cancels (spec §6). Killing on drop is what keeps
/// "the child does not linger in the background" true for both paths.
struct ProcessGroup {
    pid: u32,
    armed: bool,
}

impl ProcessGroup {
    fn new(pid: u32) -> Self {
        Self { pid, armed: true }
    }

    /// The process is already gone; do not signal its (reusable) group id later.
    fn disarm(&mut self) {
        self.armed = false;
    }

    /// SIGKILL the whole group. The shell was spawned with `process_group(0)`,
    /// so its pid is also its group id (spec §7).
    fn kill(&self) {
        // SAFETY: `killpg` only reads the id it is given; a stale or already-dead
        // group yields `ESRCH`, which is ignored here.
        unsafe {
            libc::killpg(self.pid as libc::pid_t, libc::SIGKILL);
        }
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if self.armed {
            self.kill();
        }
    }
}
