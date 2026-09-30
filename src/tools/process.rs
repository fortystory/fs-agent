//! 跑一条 argv，带墙钟上限与整个进程树的杀。
//!
//! 这是每个命令类工具共用的那一半：`bash`（spec §7），以及在 `config.toml` 里声明的动态工具
//! （spec §14）。有两条性质才是要点，而两条都属于这里、不属于任何一个工具：
//!
//! * argv 是**直接** spawn 的 —— 中间不插 shell，所以把值作为单个元素传进来的调用者不会让它
//!   被重新解析一遍；
//! * 子进程被放进它自己的进程组，超时（或调用被丢掉）会 SIGKILL 整个**组**，所以一个又起了
//!   子进程的命令不会把它们留在身后。
//!
//! 结果永远带着退出状态、stdout 与 stderr。非零退出是一条结果，不是 [`ToolError`]；只有
//! spawn 或 wait 失败才是错误，因为只有那时才没有任何东西可报。

use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

use super::tool::ToolError;

/// 结果里那些分节标记，只在这里命名一次，好让测试与文档不会漂离模型真正看到的格式。
pub const EXIT_CODE_PREFIX: &str = "退出码：";
pub const STDOUT_HEADER: &str = "--- 标准输出 ---";
pub const STDERR_HEADER: &str = "--- 标准错误 ---";
pub const TIMEOUT_PREFIX: &str = "超时：";

/// 进程组被杀之后，输出读取者还有多久能看到 EOF。
///
/// 对组里还活着的每个进程，SIGKILL 会立刻合上管道；这条宽限只在有东西刻意离开了组时才要紧，
/// 而那种情况下，把已知的东西报出去好过把整个回合挂住。
const KILL_GRACE: Duration = Duration::from_secs(1);

/// 一条跑完了（或被杀了）的命令产出了什么。
pub struct CommandOutcome {
    /// 这次调用自己的超时有没有触发，并杀掉了整个组。
    pub timed_out: bool,
    /// 当时生效的那条上限，给超时那一行用。
    pub limit: Duration,
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutcome {
    /// 结果文本：退出状态、stdout 与 stderr，各带一个具名的分节。
    ///
    /// 裁剪发生在更靠后的地方，在循环那唯一一条流前流水线里（spec §10），于是超大的正文会溢出
    /// 落盘，而流上留下一条预览加一个指针，任何工具都不必知道这件事。
    pub fn report(&self) -> String {
        let mut text = String::new();
        if self.timed_out {
            text.push_str(&format!(
                "{TIMEOUT_PREFIX}{} ms 内没有跑完；整个进程组已被杀掉\n",
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

/// 人能读的退出状态：退出码，或者杀掉它的那个信号。
pub fn describe_status(status: &ExitStatus) -> String {
    if let Some(code) = status.code() {
        return code.to_string();
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        if let Some(signal) = status.signal() {
            return format!("被信号 {signal} 杀掉");
        }
    }
    "未知".to_owned()
}

/// 在它自己的进程组里 spawn `argv`，捕获两条流，并强制 `limit`。
///
/// 只有 spawn 或 wait 失败才是 [`ToolError`]；命令自己的退出状态是数据。
pub async fn run(
    cwd: &std::path::Path,
    argv: &[String],
    limit: Duration,
) -> Result<CommandOutcome, ToolError> {
    let (program, rest) = argv
        .split_first()
        .ok_or_else(|| ToolError::message("argv 为空，无法运行"))?;

    let mut command = Command::new(program);
    command
        .args(rest)
        .current_dir(cwd)
        // 构造上就非交互：不请求 TTY、stdin 是空的，这里也没有任何东西去凭空设置 `TERM` /
        // `NO_COLOR`（spec §20、`docs/bash.md`）。
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // 它自己的进程组（等价于 `setsid`），这样超时能杀掉这条命令起的那棵树，而不只是直接的
        // 子进程（spec §7）。
        .process_group(0)
        // 直接子进程的第二道防线，万一下面那道守卫被提前解除；它够不到那棵树，那是守卫的活。
        .kill_on_drop(true);

    let mut child = command
        .spawn()
        .map_err(|error| ToolError::message(format!("无法启动 `{program}`：{error}")))?;
    let pid = child
        .id()
        .ok_or_else(|| ToolError::message(format!("`{program}` 在能追踪到它之前就退出了")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ToolError::message("stdout 没有接上管道"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ToolError::message("stderr 没有接上管道"))?;

    // 读取者作为各自的任务跑，所以命令还在跑时管道不会被填满，下面的循环则可以随到随收输出。
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

    // 上限管的是**整次**调用，不只是直接子进程的寿命：一个被放到后台的孙子进程会继承那些输出
    // 管道，于是 `bash -lc "sleep 300 &"` 立刻退出，而管道仍然开着。只等子进程会让回合挂过
    // 超时。
    loop {
        if status.is_some() && !stdout_open && !stderr_open {
            break;
        }
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(deadline) => {
                if timed_out {
                    // SIGKILL 没能合上每一条管道 —— 有个离开了组的进程还攥着一条。把已知的
                    // 东西报出去，好过把回合挂住。
                    break;
                }
                timed_out = true;
                group.kill();
                // 给读取者一小段宽限，好在杀之后看到 EOF。
                deadline = tokio::time::Instant::now() + KILL_GRACE;
            }
            result = &mut wait, if status.is_none() => {
                status = Some(result.map_err(|error| {
                    ToolError::message(format!("无法等待 `{program}`：{error}"))
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
    // 结束 wait future 对 `child` 的借用；杀之后再 wait 会返回第一次 wait 已经算出的那个状态，
    // 或者，如果上面的循环提前 break 了，就在这里回收子进程。
    drop(wait);
    let status = match status {
        Some(status) => status,
        None => child
            .wait()
            .await
            .map_err(|error| ToolError::message(format!("无法回收 `{program}`：{error}")))?,
    };
    Ok(CommandOutcome {
        timed_out,
        limit,
        status,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// 把子进程的一条流读到 EOF，无论命令写了什么。
async fn read_to_end<R>(mut reader: R) -> Vec<u8>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut buffer = Vec::new();
    // 读出错意味着这条流结束了；已经读到的那些字节仍然是这条命令的输出。
    let _ = reader.read_to_end(&mut buffer).await;
    buffer
}

/// 子进程的那个进程组；如果这次调用在组结束之前就结束了，就把它杀掉。
///
/// 超时不是调用提前结束的唯一方式：用户取消时循环会把在飞的工具丢掉（spec §6）。drop 时杀，
/// 正是让「子进程不会滞留在后台」对两条路径都成立的东西。
struct ProcessGroup {
    pid: u32,
    armed: bool,
}

impl ProcessGroup {
    fn new(pid: u32) -> Self {
        Self { pid, armed: true }
    }

    /// 进程已经没了；之后别再给这个（可能被复用的）组 id 发信号。
    fn disarm(&mut self) {
        self.armed = false;
    }

    /// SIGKILL 整个组。子进程是用 `process_group(0)` spawn 的，所以它的 pid 也是它的组 id
    /// （spec §7）。
    fn kill(&self) {
        // 安全性：`killpg` 只读它拿到的那个 id；陈旧的或已经死掉的组返回 `ESRCH`，这里忽略。
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
