//! 渲染层在宿主侧起的一次性只读子进程
//! （`.scratch/diff-page/spec.md` 实现决定 §3、§8）。
//!
//! 三条纪律，都是这一层与 `tools::process` 分家的地方：
//!
//! * **不进事件流、不过权限门、不过沙箱** —— 权限门约束的是**模型**的读写，而这里是人在
//!   自己的终端里看自己的仓库。[`super::opener`] 起 `xdg-open` 是同一条立场的先例，区别只
//!   在于那一边不等退出、也不收输出。
//! * **不在任何一帧里 await**：调用方把它 `tokio::spawn` 出去，结果经通道回填（那一套形状
//!   在 [`super::file_index`]）。这里的 [`run`] 只保证自己带上墙钟上限。
//! * **进程组**：`process_group(0)` 让它进自己的组，于是超时或被丢时 `killpg` 杀掉的是整
//!   棵树，而不只是直接子进程 —— `git` 会先跑它自己的 hooks / 外部 diff 驱动，那些都在同
//!   一个组里（`.scratch/diff-page/research/03-renderer-subprocess-and-config.md` 第二节）。
//!
//! 它刻意不知道 git，也不知道「改动页」：票 14 那一档外部工具走的是同一个 [`run`]（多一个
//! stdin 与几个环境变量）。

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// 跑一次子进程要的那几样。
#[derive(Clone, Copy)]
pub struct Run<'a> {
    /// 程序名。按 `PATH` 找，**绝不过 shell**。
    pub program: &'a str,
    /// argv，按**整元素**传进去（`.` 里的东西都是字面量，没有切词这一步）。
    pub args: &'a [String],
    /// 这一次调用的工作目录。
    pub cwd: &'a Path,
    /// 喂给 stdin 的东西；`None` 时接 `/dev/null`。
    pub stdin: Option<&'a str>,
    /// 额外给的环境变量。
    pub env: &'a [(&'a str, &'a str)],
    /// 要从继承来的环境里**清掉**的变量。
    ///
    /// 不是洁癖：`GIT_DIR` / `GIT_WORK_TREE` / `GIT_INDEX_FILE` 会让 git 看错仓库、甚至看
    /// 另一套 index（`.scratch/diff-page/research/02-git-readouts.md` 第二节实测）。
    pub unset: &'a [&'a str],
    /// 墙钟上限。
    pub limit: Duration,
}

/// 一次子进程的结果。
///
/// stderr **不带回来**：这一层的两个调用方都不把它当输出（git 的错误文案随 locale 变，
/// 判据是退出码；外部工具吐的 stderr 画进正文只会被读成 diff）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finished {
    /// 退出码 0，带上 stdout。
    Ok(String),
    /// 非零退出（`None` 是拿不到码，比如被信号杀掉）。
    Exit(Option<i32>),
    /// 程序不在 `PATH` 上（`ErrorKind::NotFound`）。
    NotFound,
    /// 到了墙钟上限，整组已被杀。
    Timeout,
}

/// 跑一次，带上超时与进程组。
pub async fn run(run: Run<'_>) -> Finished {
    let mut command = Command::new(run.program);
    command
        .args(run.args)
        .current_dir(run.cwd)
        // 一次只读调用**不该**有交互：stdin 要么是我们喂进去的，要么是空的。
        .stdin(if run.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        // 它自己的进程组（等价于 `setsid`），于是超时杀得掉整棵树。
        .process_group(0)
        // 直接子进程的第二道防线，够不到那棵树 —— 那是上面那条的活。
        .kill_on_drop(true);
    for name in run.unset {
        command.env_remove(name);
    }
    for (name, value) in run.env {
        command.env(name, value);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Finished::NotFound,
        // 起不来但不是「找不到」：权限、argv 太长、资源不够。都是「这次没跑成」。
        Err(_) => return Finished::Exit(None),
    };
    let pid = child.id().unwrap_or(0);
    let mut group = ProcessGroup::new(pid);

    // stdin 交给一个后台任务写：一个读得慢（或者根本不读）的子进程会把管道写满，而这一帧
    // 不该为此多等一个超时。子进程被杀之后这个任务拿到 EPIPE，自己就结束了。
    if let Some(input) = run.stdin
        && let Some(mut pipe) = child.stdin.take()
    {
        let bytes = input.as_bytes().to_vec();
        tokio::spawn(async move {
            let _ = pipe.write_all(&bytes).await;
            let _ = pipe.shutdown().await;
        });
    }

    match tokio::time::timeout(run.limit, child.wait_with_output()).await {
        Ok(Ok(output)) => {
            group.disarm();
            let text = String::from_utf8_lossy(&output.stdout).into_owned();
            if output.status.success() {
                Finished::Ok(text)
            } else {
                Finished::Exit(output.status.code())
            }
        }
        // 等它的时候出了 IO 错误：子进程的状态无从得知，所以**保留**这条进程组、让 `Drop`
        // 兜底杀一次（真正要防的是「子进程滞留在后台」）。
        Ok(Err(_)) => Finished::Exit(None),
        // 超时：`group` 的 `Drop` 里 `killpg` 整组。
        Err(_) => Finished::Timeout,
    }
}

/// 子进程的那个进程组；这一次调用提前散场时把整组杀掉。
///
/// 形状照 `src/tools/process.rs` 那一份（那边是工具域的私有件，借不过来）：`process_group(0)`
/// 之后子进程的 pid 也是它的组 id；`disarm` 是防 PID 复用 —— 已经等着的那次 wait 成功了，
/// 之后就不该再给一个可能被操作系统发给别人的组 id 发信号。
struct ProcessGroup {
    pid: u32,
    armed: bool,
}

impl ProcessGroup {
    fn new(pid: u32) -> Self {
        Self { pid, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }

    /// SIGKILL 整个组。陈旧的或已经死掉的组返回 `ESRCH`，忽略。
    fn kill(&self) {
        if self.pid == 0 {
            return;
        }
        // 安全性：`killpg` 只读它拿到的那个 id。
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

#[cfg(test)]
mod tests {
    use super::*;

    fn run_in(cwd: &Path, program: &str, args: &[&str], limit: Duration) -> Finished {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("一个够用的运行时");
        runtime.block_on(run(Run {
            program,
            args: &args,
            cwd,
            stdin: None,
            env: &[],
            unset: &[],
            limit,
        }))
    }

    #[test]
    fn a_program_that_is_not_on_the_path_says_so_instead_of_pretending_to_fail() {
        // 「找不到 git」与「git 报错了」是两句话：前者是环境问题，页里要分开说
        // （`.scratch/diff-page/spec.md` §5）。这一层只把 `ErrorKind::NotFound` 认出来。
        let dir = std::env::temp_dir();
        assert_eq!(
            run_in(&dir, "heng-not-a-real-program", &[], Duration::from_secs(2)),
            Finished::NotFound
        );
    }

    #[test]
    fn stdout_comes_back_verbatim_and_a_nonzero_exit_keeps_its_code() {
        let dir = std::env::temp_dir();
        assert_eq!(
            run_in(
                &dir,
                "sh",
                &["-c", "printf '一\\n二\\n'"],
                Duration::from_secs(2)
            ),
            Finished::Ok("一\n二\n".to_owned())
        );
        assert_eq!(
            run_in(&dir, "sh", &["-c", "exit 3"], Duration::from_secs(2)),
            Finished::Exit(Some(3))
        );
    }

    #[test]
    fn a_program_that_never_finishes_is_killed_at_the_limit() {
        let dir = std::env::temp_dir();
        let started = std::time::Instant::now();
        let outcome = run_in(&dir, "sh", &["-c", "sleep 30"], Duration::from_millis(200));
        assert_eq!(outcome, Finished::Timeout);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "超时是墙钟的上限，不是「等它自己结束」"
        );
    }

    #[test]
    fn stdin_reaches_the_program_and_the_timeout_still_covers_a_slow_reader() {
        let dir = std::env::temp_dir();
        let args: Vec<String> = ["-c", "cat"].iter().map(|arg| (*arg).to_owned()).collect();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("一个够用的运行时");
        let outcome = runtime.block_on(run(Run {
            program: "sh",
            args: &args,
            cwd: &dir,
            stdin: Some("喂进去的一行\n"),
            env: &[],
            unset: &[],
            limit: Duration::from_secs(2),
        }));
        assert_eq!(outcome, Finished::Ok("喂进去的一行\n".to_owned()));
    }
}
