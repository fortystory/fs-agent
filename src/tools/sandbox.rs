//! bubblewrap 沙箱的 argv 拼装（`.scratch/sandbox/spec.md` §1、§4）。
//!
//! 这一层最值钱的性质是 [`wrap`] 是个**纯函数**：输入是值、输出是值，不 spawn 任何东西、
//! 不读环境（唯一的 IO 是判断一个路径存不存在）。于是它可以在没有 bubblewrap 的机器上被
//! 逐字断言。
//!
//! 判据不在这个模块里：内核看到只读挂载就拒绝写（`EROFS`），我们只负责把挂载表拼对。

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::{SandboxAvailability, SandboxMode, SandboxSettings};
use crate::permissions::{fold, is_env_file};

use super::tool::ToolError;

/// 沙箱用的那个程序名，只在这里命名一次。
pub const BWRAP: &str = "bwrap";

/// 探测失败时给出的两条出路（沙箱 spec §3）。
pub const WAYS_OUT: &str = "装一个 bubblewrap（让 `bwrap` 出现在 PATH 上），或者把 `[sandbox] mode` 设成 \"off\" \
     显式放弃这一层";

/// 探测跑的那条最小 profile（沙箱 spec §3）。
///
/// 比 `bwrap --version` 强的地方在于它真的建一套 mount namespace：装了 bubblewrap 不等于
/// 它在这个环境里能用（Ubuntu 24.04 的 AppArmor 限制、容器里、WSL1 都会让它起不来）。
/// `--proc /proc` 也在里面 —— 无特权容器里的典型症状正是 `/proc` 建不起来。
pub const PROBE_PROFILE: &[&str] = &[
    "--ro-bind",
    "/",
    "/",
    "--proc",
    "/proc",
    "--dev",
    "/dev",
    "--die-with-parent",
    "--",
    "/bin/true",
];

/// 探一次：`bwrap` 在不在这台机器上、它起不起得来（沙箱 spec §3）。
///
/// 以**退出码**为准，不看 `--version`。在 `search_path` 里找 `bwrap`，但排除会话 cwd
/// 之内的候选 —— 否则有人往工作区里放一个假的可执行文件就够了。
pub fn probe(search_path: Option<&OsStr>, cwd: &Path) -> SandboxAvailability {
    let Some(bwrap) = find_bwrap(search_path, cwd) else {
        return SandboxAvailability::Unavailable {
            reason: format!("PATH 上没有 `{BWRAP}`"),
        };
    };
    let output = Command::new(&bwrap)
        .args(PROBE_PROFILE)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();
    match output {
        Ok(output) if output.status.success() => SandboxAvailability::Available { bwrap },
        Ok(output) => {
            let status = super::process::describe_status(&output.status);
            let stderr = String::from_utf8_lossy(&output.stderr);
            SandboxAvailability::Unavailable {
                reason: format!(
                    "`{BWRAP}` 的最小 profile 起不来（退出码 {status}）：{}",
                    first_line(&stderr)
                ),
            }
        }
        Err(error) => SandboxAvailability::Unavailable {
            reason: format!("无法运行 `{BWRAP}`：{error}"),
        },
    }
}

/// 组装期该填进会话配置的那个值：已经定下来就原样返回，该探才探（`mode = "off"` 永不探）。
pub fn resolve_availability(settings: &SandboxSettings, cwd: &Path) -> SandboxAvailability {
    if !settings.needs_probe() {
        return settings.availability.clone();
    }
    probe(settings.search_path.as_deref(), cwd)
}

/// 在 `PATH` 上找一个可执行的 `bwrap`，跳过落在会话 cwd 里的候选。
///
/// 比较按**词法**路径做、不解析符号链接：工作区里的任何 `bwrap` 一律不信 —— 哪怕它是一个
/// 指向系统二进制的链接。反过来，解析符号链接会把工作区里的一个链接错当成「区外的」文件。
fn find_bwrap(search_path: Option<&OsStr>, cwd: &Path) -> Option<PathBuf> {
    let path = search_path?;
    let cwd = canonical(cwd);
    for entry in std::env::split_paths(path) {
        // `PATH` 里的空项按惯例是「当前目录」；相对项按会话 cwd 折叠（保守侧）。
        let directory = if entry.as_os_str().is_empty() {
            cwd.clone()
        } else if entry.is_absolute() {
            entry
        } else {
            cwd.join(entry)
        };
        if directory.starts_with(&cwd) {
            continue;
        }
        let candidate = directory.join(BWRAP);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// 一个路径是不是「存在、是文件、且至少有一个执行位」。
fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// 诊断文本的第一行：bwrap 的报错可能很长，而理由要能塞进一条工具错误。
fn first_line(text: &str) -> String {
    let line = text.lines().find(|line| !line.trim().is_empty());
    match line {
        Some(line) => line.trim().to_owned(),
        None => "（没有输出）".to_owned(),
    }
}

/// 一次拼装需要的值：可写根、遮罩目录，以及那一档模式。
///
/// 保护路径不在这里 —— 它们由 `cwd` 推出来（`.git/config`、`.git/hooks`、`.env` 家族），
/// 因为「哪些文件存在」是 `cwd` 的函数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxSpec {
    pub mode: SandboxMode,
    /// 除会话 cwd 之外的额外可写根（`[sandbox] writable_roots`）。
    pub writable_roots: Vec<PathBuf>,
    /// 目录被替换成一个空的、只读的 tmpfs。
    pub masks: Vec<PathBuf>,
}

/// `bwrap` 自己的诊断都带这个前缀（实测原文：`bwrap: Can't create file /no-such-dir:
/// Read-only file system`）。它是「沙箱没起来」与「命令失败了」之间唯一稳定的判据 ——
/// 内核给的拒绝消息随 locale 变（中文环境下是「只读文件系统」），拿它做判据会是一处会在
/// 中文环境里静默失效的逻辑。
pub const BWRAP_FAILURE_PREFIX: &str = "bwrap: ";

/// 会话携带的沙箱值：配置加上组装期探到的那个结果。
///
/// 形状与 `BashLimits` 一样 —— 会话配置在组装期变成注入值，工具永不伸手去够会话。
#[derive(Debug, Clone)]
pub struct Sandbox {
    spec: SandboxSpec,
    availability: SandboxAvailability,
    /// 这一次调用额外放开的可写根 —— 升级批准的那批路径
    /// （`.scratch/workspace-mode/spec.md` §4）。单独存，而不是混进 `spec.writable_roots`：
    /// 一条**不存在**的批准路径是一条工具错误，而不是静默跳过 —— `bwrap` 只能绑已经存在的
    /// 源，静默跳过等于用户批了一条什么都不发生的路径。
    grants: Vec<PathBuf>,
}

impl Sandbox {
    /// 从会话配置里那一份值构造。
    pub fn new(settings: &SandboxSettings) -> Self {
        Self {
            spec: SandboxSpec {
                mode: settings.mode,
                writable_roots: settings.writable_roots.clone(),
                masks: settings.masks.clone(),
            },
            availability: settings.availability.clone(),
            grants: Vec::new(),
        }
    }

    /// 这一层是不是被显式关掉了（`mode = "off"`）。
    fn is_off(&self) -> bool {
        self.spec.mode == SandboxMode::Off
    }

    /// 这一次调用额外放开的可写根 —— 升级批准的那批路径。
    ///
    /// 粒度就是**声明的那个路径本身**：文件就绑文件、目录就绑目录，不做父目录提升。因为
    /// [`Sandbox`] 是每次调用现构造的，这一批只活这一次调用：不进任何规则、不写
    /// `config.toml`、也不进会话状态（`.scratch/workspace-mode/spec.md` §4）。
    pub fn with_grants(&self, grants: &[PathBuf]) -> Sandbox {
        let mut sandbox = self.clone();
        for grant in grants {
            if !sandbox.grants.contains(grant) {
                sandbox.grants.push(grant.clone());
            }
        }
        sandbox
    }

    /// 把这一次要跑的 argv 包好。
    ///
    /// 三种情形：关掉了就原样返回；探测说有 `bwrap` 就用**探测到的那个绝对路径**（于是
    /// PATH 中途变了也不影响）；不可用就是 [`ToolError`] —— 这一次调用无处可跑，而那与
    /// 「命令失败了」是两件不同的事。
    pub fn wrap(&self, argv: &[String], cwd: &Path) -> Result<Vec<String>, ToolError> {
        if self.is_off() {
            return Ok(argv.to_vec());
        }
        let SandboxAvailability::Available { bwrap } = &self.availability else {
            // 两支的出路不一样，所以 [`WAYS_OUT`]（「装一个 bubblewrap，或者把 `mode` 设成
            // `off`」）只拼给真正不可用的那一支。
            let reason = match &self.availability {
                SandboxAvailability::Unavailable { reason } => format!("{reason}。{WAYS_OUT}"),
                // 组装期一定会把它定下来；走到这里说明某条新造配置的路径没把那一格带过来
                // （`Session::retarget` 曾经是这么一条）。这一支与这台机器上有没有 `bwrap`
                // 无关，所以**不**拼 `WAYS_OUT`：那句指引会把人支去装一个已经装好的东西
                // （2026-10-08 实测到的那次，执行者与人都被它骗过）。
                SandboxAvailability::Untested | SandboxAvailability::Available { .. } => {
                    "这个会话的沙箱状态还没有定下来：组装期的探测结果没有带到这次调用上。\
                     它与这台机器上有没有 `bwrap` 无关（那是另一句话），所以别在这条上重试\
                     —— 让用户重开会话即可恢复"
                        .to_owned()
                }
            };
            return Err(ToolError::message(format!(
                "沙箱不可用，命令没有跑：{reason}"
            )));
        };
        // 升级批准的那批路径：`bwrap` 只能绑**已经存在**的源（不存在的挂载目标会让整条
        // 命令起不来），所以不存在的那一条在这里明确失败 —— 一条不生效的批准比拒绝更糟，
        // 用户会以为自己放开了什么。粒度仍然是声明的那条路径本身，不做父目录提升。
        if let Some(missing) = self.grants.iter().find(|grant| !grant.exists()) {
            return Err(ToolError::message(format!(
                "升级批准的那条路径不存在：{}。沙箱只能放开已经存在的路径（声明什么就绑什么，\
                 不做父目录提升）：声明它的父目录，或者先把它建出来",
                missing.display()
            )));
        }
        let mut spec = self.spec.clone();
        spec.writable_roots.extend(self.grants.iter().cloned());
        let mut wrapped = wrap(argv, cwd, &spec);
        wrapped[0] = bwrap.display().to_string();
        Ok(wrapped)
    }

    /// 一次调用跑完之后，它的 stderr 是不是在说「沙箱没起来」。
    ///
    /// 判据只有 [`BWRAP_FAILURE_PREFIX`] 一条，而且只在开着沙箱时成立：其余任何非零退出
    /// —— 包括内核给的 `EROFS` —— 都是**命令结果**，原样给模型。命令被沙箱挡回时给出的
    /// 消息随 locale 变，模型读「只读文件系统」本来就懂；拿那种文本做判据，会是一处会在
    /// 中文环境里静默失效的逻辑。
    pub fn failure(&self, stderr: &str) -> Option<String> {
        if self.is_off() || !stderr.starts_with(BWRAP_FAILURE_PREFIX) {
            return None;
        }
        Some(format!("沙箱没有起来，命令没有跑：{}", stderr.trim_end()))
    }
}

/// 把一个 argv 包成 bubblewrap 调用。
///
/// flag 的顺序是行为的一部分，而且有三处顺序上的讲究：
///
/// * `--ro-bind / /` 在所有可写根的 `--bind` **之前**；
/// * `--tmpfs /tmp` 也在它们**之前** —— 挂载是叠上去的，反过来会把落在 `/tmp` 下的会话
///   工作区整个盖掉（2026-10-01 的真机验证：`/tmp` 下的工作区写自己的文件报「只读文件
///   系统」，因为那条 `--bind` 已经被后挂的 tmpfs 遮住了）；
/// * 保护路径的 `--ro-bind` 在所有 `--bind` **之后** —— 反了就等于没保护。
///
/// 不存在的可写根、遮罩目录与保护路径**整条跳过**：`bwrap` 对不存在的挂载目标会直接报错
/// 退出（实测原文是 `bwrap: Can't create file …: Read-only file system`），而
/// `~/.cargo` 这类缓存目录在没装 Rust 的机器上本来就不存在。
pub fn wrap(argv: &[String], cwd: &Path, spec: &SandboxSpec) -> Vec<String> {
    if spec.mode == SandboxMode::Off {
        return argv.to_vec();
    }

    let mut argv_out: Vec<String> = Vec::with_capacity(16 + argv.len());
    argv_out.extend(strings(&[
        "bwrap",
        "--new-session",
        "--die-with-parent",
        "--ro-bind",
        "/",
        "/",
        "--tmpfs",
        "/tmp",
    ]));

    for root in writable_roots(cwd, spec) {
        push_pair(&mut argv_out, "--bind", &root);
    }

    argv_out.extend(strings(&[
        "--dev",
        "/dev",
        "--proc",
        "/proc",
        "--unshare-user",
        "--unshare-pid",
        "--unshare-ipc",
        "--unshare-uts",
    ]));

    for mask in &spec.masks {
        if !mask.exists() {
            continue;
        }
        let mask = canonical(mask);
        let mask = mask.display().to_string();
        argv_out.push("--tmpfs".to_owned());
        argv_out.push(mask.clone());
        argv_out.push("--remount-ro".to_owned());
        argv_out.push(mask);
    }

    for protected in protected_paths(cwd) {
        push_pair(&mut argv_out, "--ro-bind", &protected);
    }

    argv_out.push("--".to_owned());
    argv_out.extend(argv.iter().cloned());
    argv_out
}

/// 会话 cwd 与配置的每一条可写根，按这个顺序，去掉重复的、跳过不存在的。
fn writable_roots(cwd: &Path, spec: &SandboxSpec) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::with_capacity(spec.writable_roots.len() + 1);
    if cwd.exists() {
        roots.push(canonical(cwd));
    }
    for root in &spec.writable_roots {
        if !root.exists() {
            continue;
        }
        let root = canonical(root);
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}

/// 工作区里要压回只读的那些路径，**不是整个 `.git`**。
///
/// `git add` / `git commit` 写的第一样东西是 `.git/index.lock`，把整个 `.git` 压回只读
/// 会让提交全线失败；而地板真正要防的是另外两样：改 `.git/config`（把 push 指向别处）与
/// 改 `.git/hooks`（下次 commit 执行任意代码）。这两个恰好可以用 `--ro-bind` 精确表达。
///
/// `.env` 家族与权限门的地板共用同一个口径（[`is_env_file`]）；shell rc 文件不在工作区
/// 里，`--ro-bind / /` 已经管了。
fn protected_paths(cwd: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for relative in [".git/config", ".git/hooks"] {
        let path = cwd.join(relative);
        if path.exists() {
            paths.push(canonical(&path));
        }
    }
    paths.extend(env_files(cwd));
    paths
}

/// 这条路径是不是**写死的安全默认**之一，因而升级通道不碰它。
///
/// 三类：[`SandboxSpec::masks`] 里的遮罩目录（provider key 与 ssh 私钥所在）、工作区里
/// 的 `.git/config` 与 `.git/hooks`、以及工作区里 `.env` 一族的文件。判据只有这一份 ——
/// 权限门用它，挂载表用它，`.env` 的口径与 [`crate::permissions::is_env_file`] 共用，绝不
/// 抄第二份会漂离的清单（`.scratch/workspace-mode/spec.md` §4、§5）。
///
/// 与[`protected_paths`]的唯一差别是**存在性**：这里按路径判，而不是按「磁盘上现在有
/// 什么」。申请写一个还不存在的 `.env` 或 `.git/config` 同样是越界 —— 那样的批准会让
/// 下一次调用以为自己在保护，实际上文件已经写下去了。
pub fn sealed(path: &Path, cwd: &Path, masks: &[PathBuf]) -> bool {
    let path = absolute(path, cwd);
    let cwd = absolute(cwd, cwd);
    if masks
        .iter()
        .any(|mask| path.starts_with(absolute(mask, &cwd)))
    {
        return true;
    }
    for relative in [".git/config", ".git/hooks"] {
        let protected = absolute(&cwd.join(relative), &cwd);
        if path == protected || path.starts_with(&protected) {
            return true;
        }
    }
    path.starts_with(&cwd) && is_env_file(&path)
}

/// 把模型声明的升级路径变成一条绝对的、`~` 已展开的路径。
///
/// 升级参数里的路径由模型写，可能带 `~`、也可能是相对路径；而门要拿它与遮罩目录比、
/// 沙箱要拿它挂可写根，两处必须是同一批字符串。归一化里**不**要求目标存在：要放开的
/// 常常正是一个还没建的目录（`~/.npm`）。
pub fn escalation_path(raw: &Path, cwd: &Path, home: Option<&Path>) -> PathBuf {
    let expanded = match raw.to_str() {
        Some(text) => crate::config::expand_home(text, home),
        None => raw.to_path_buf(),
    };
    absolute(&expanded, cwd)
}

/// 一条绝对路径：能 `canonicalize` 就解析符号链接，不能就按字面折掉 `.` 与 `..`。
fn absolute(path: &Path, cwd: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    match std::fs::canonicalize(&joined) {
        Ok(resolved) => resolved,
        Err(_) => fold(&joined),
    }
}

/// 工作区顶层**存在的** `.env` 家族文件，按名字排序（于是输出是确定的）。
fn env_files(cwd: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(cwd) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_env_file(path))
        .map(|path| canonical(&path))
        .collect();
    files.sort();
    files
}

/// 规范路径：`--bind` 两边都吃绝对路径，而 symlink 会骗过「看起来在工作区里」的判定。
fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn push_pair(argv: &mut Vec<String>, flag: &str, path: &Path) {
    let path = path.display().to_string();
    argv.push(flag.to_owned());
    argv.push(path.clone());
    argv.push(path);
}

fn strings<'a>(items: &'a [&'a str]) -> impl Iterator<Item = String> + 'a {
    items.iter().map(|item| (*item).to_owned())
}
