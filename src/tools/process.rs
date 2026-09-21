//! Running one argv with a wall-clock limit and a whole-process-tree kill.
//!
//! This is the shared half of every command tool: `bash` (spec §7) and the
//! dynamic tools declared in `config.toml` (spec §14). Two properties are the
//! point, and both belong here rather than in either tool:
//!
//! * the argv is spawned **directly** — no shell is interposed, so a caller that
//!   passes a value as one element cannot have it re-parsed;
//! * the child is put in its own process group and a timeout (or a dropped call)
//!   SIGKILLs the **group**, so a command that started children does not leave
//!   them behind.
//!
//! The result always carries the exit status, stdout and stderr. A non-zero exit
//! is a result, not a [`ToolError`]; only a failure to spawn or wait is an error,
//! because only then is there nothing to report.

use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

use super::tool::ToolError;

/// The result's section markers, named once so tests and docs cannot drift from
/// the format the model actually sees.
pub const EXIT_CODE_PREFIX: &str = "exit code: ";
pub const STDOUT_HEADER: &str = "--- stdout ---";
pub const STDERR_HEADER: &str = "--- stderr ---";
pub const TIMEOUT_PREFIX: &str = "timed out after ";

/// How long the output readers get to see EOF after the process group is killed.
///
/// SIGKILL closes the pipes at once for every process still in the group; the
/// grace only matters when something deliberately left the group, and in that
/// case returning what is known beats hanging the turn.
const KILL_GRACE: Duration = Duration::from_secs(1);

/// What one finished (or killed) command produced.
pub struct CommandOutcome {
    /// Whether this call's own timeout fired and killed the group.
    pub timed_out: bool,
    /// The cap that was in force, for the timeout line.
    pub limit: Duration,
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutcome {
    /// The result text: exit status, stdout and stderr in named sections.
    ///
    /// Truncation happens later, in the loop's one pre-stream pipeline (spec
    /// §10), so an oversized body spills and the stream keeps a preview plus a
    /// pointer without any tool knowing about it.
    pub fn report(&self) -> String {
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
pub fn describe_status(status: &ExitStatus) -> String {
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

/// Spawn `argv` in its own process group, capture both streams, and enforce
/// `limit`.
///
/// Only a spawn or wait failure is a [`ToolError`]; the command's own exit status
/// is data.
pub async fn run(
    cwd: &std::path::Path,
    argv: &[String],
    limit: Duration,
) -> Result<CommandOutcome, ToolError> {
    let (program, rest) = argv
        .split_first()
        .ok_or_else(|| ToolError::message("cannot run an empty argv"))?;

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
        // the tree the command started and not just the direct child (spec §7).
        .process_group(0)
        // A second line of defence for the direct child if the guard below is
        // ever disarmed early; it does not reach the tree, which is the guard's
        // job.
        .kill_on_drop(true);

    let mut child = command
        .spawn()
        .map_err(|error| ToolError::message(format!("cannot spawn `{program}`: {error}")))?;
    let pid = child.id().ok_or_else(|| {
        ToolError::message(format!("`{program}` exited before it could be tracked"))
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ToolError::message("stdout was not piped"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ToolError::message("stderr was not piped"))?;

    // Readers run as their own tasks, so a pipe cannot fill up while the command
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

    // The limit bounds the **whole** call, not just the direct child's lifetime: a
    // backgrounded grandchild inherits the output pipes, so `bash -lc "sleep 300 &"`
    // exits at once while the pipes stay open. Waiting only on the child would
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
                    ToolError::message(format!("cannot wait for `{program}`: {error}"))
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
    // the status the first wait already computed, or reaps the child if the loop
    // broke early.
    drop(wait);
    let status = match status {
        Some(status) => status,
        None => child
            .wait()
            .await
            .map_err(|error| ToolError::message(format!("cannot reap `{program}`: {error}")))?,
    };
    Ok(CommandOutcome {
        timed_out,
        limit,
        status,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// Read a child stream to EOF, whatever the command wrote.
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

/// The child's process group, killed if the call ends before the group does.
///
/// A timeout is not the only way a call stops early: the loop drops an in-flight
/// tool when the user cancels (spec §6). Killing on drop is what keeps "the child
/// does not linger in the background" true for both paths.
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

    /// SIGKILL the whole group. The child was spawned with `process_group(0)`,
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
