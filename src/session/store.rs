//! 会话存储（spec §11）：一场会话一个目录，一个 cwd 一个桶。
//!
//! 一场会话是一个**可搬运的目录**：一份 JSONL 事件流，加上流所指向的 `outputs/` 产物，别无其
//! 他。把这个目录拷到另一台机器上，拷过去的就是整场会话。
//!
//! ```text
//! <root>/<cwd-slug>/<session-id>/
//!     log.jsonl
//!     outputs/
//! ```
//!
//! cwd 的 slug 只用来**分桶**：权威绑定是 `SessionStarted` 里记下的 `cwd`。所以不带 id 的
//! `--continue` 永远不需要一个全局索引 —— 它扫这个目录所在的桶，取最近写下的那份日志。按 id 续
//! （`-c <id>` / `--session <id>`）先扫本桶、再全 store，而续上别处那一场时工作目录取的就是那条
//! `SessionStarted` 里的 cwd。
//!
//! root 是注入的，从不从环境里读：库不读任何环境，所以 store 住在哪里由调用方（CLI）决定
//! （spec §1）。
//!
//! store 建出来的一切都是**仅属主**的（会话目录与产物目录 `0700`，事件流 `0600`），因为一场会
//! 话装着用户的源码，以及经打码之后、它小心保住的那些秘密（spec §11、§20）。

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;

use crate::events::{Event, EventPayload, SessionId};

/// 会话目录里事件流的文件名。
pub const LOG_FILE: &str = "log.jsonl";
/// 会话目录里工具产物的目录名。
pub const OUTPUTS_DIR: &str = "outputs";

/// 一条流里记下的那个工作区：第一条 `SessionStarted.cwd`。
///
/// 桶名是编码过的、读不回来，所以这是「这场会话开在哪」唯一权威的来源。
pub fn started_cwd(events: &[Event]) -> Option<String> {
    events.iter().find_map(|event| match &event.payload {
        EventPayload::SessionStarted { cwd, .. } => Some(cwd.clone()),
        _ => None,
    })
}

/// 一场会话开在哪里 —— 读它自己的那条流。
///
/// 读不出来（流坏了、或者它还在 `create` 与第一条事件之间）就答 `None`，调用方退回请求的目录。
pub fn session_cwd(session: &StoredSession) -> Option<String> {
    started_cwd(&crate::events::read_events(&session.log_path).ok()?)
}

/// 一个注入 root 下的各会话目录。
#[derive(Debug, Clone)]
pub struct SessionStore {
    root: PathBuf,
}

/// 磁盘上真实存在的一个会话目录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSession {
    pub id: SessionId,
    /// 会话目录，即可搬运的那个单元。
    pub dir: PathBuf,
    /// `dir` 里的 JSONL 事件流。
    pub log_path: PathBuf,
    /// 流里的产物（`<tool_call_id>.before`、`.txt`）落在哪里。
    pub outputs_dir: PathBuf,
}

impl SessionStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 某一个 cwd 的会话所在的桶。
    ///
    /// cwd 先做 canonicalize，所以同一个目录的两种写法共用一个桶。canonicalize 不了的 cwd
    /// （它还不存在）就按原样打 slug。
    pub fn bucket(&self, cwd: &Path) -> PathBuf {
        let canonical = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        self.root.join(cwd_slug(&canonical))
    }

    /// 新建一个会话目录，返回它的标识与各路径。
    ///
    /// 事件流本身由组装点创建（它是唯一打开日志的地方）；store 负责**目录**的形状与它的权限。
    pub fn create(&self, cwd: &Path) -> io::Result<StoredSession> {
        let bucket = self.bucket(cwd);
        create_private_dirs(&bucket)?;

        // 两个 id 只有在同一秒碰上同一个随机后缀时才会撞；重试让那种可能性不再是一个 bug。
        for _ in 0..8 {
            let id = new_session_id();
            let dir = bucket.join(id.as_str());
            match create_private_dir(&dir) {
                Ok(true) => {
                    let outputs_dir = dir.join(OUTPUTS_DIR);
                    create_private_dir(&outputs_dir)?;
                    return Ok(StoredSession {
                        log_path: dir.join(LOG_FILE),
                        outputs_dir,
                        dir,
                        id,
                    });
                }
                Ok(false) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "试了 8 次都没能分到一个唯一的会话 id",
        ))
    }

    /// 这个 cwd 会接着跑的那场会话：桶里最近写下的那份流；桶里什么都没有时是 `None`。
    pub fn latest(&self, cwd: &Path) -> io::Result<Option<StoredSession>> {
        Ok(self.list(cwd)?.into_iter().next())
    }

    /// 这场会话在不在这个 cwd 的桶里。
    ///
    /// 「按 id 接着跑一场会话」要它判「是不是在**请求的那个工作区**里」：是就沿用请求的目录，不是
    /// 才去问那场会话自己的 cwd。判桶而不是比路径 —— `fs-agent --cwd .` 这样的相对写法与流里记的
    /// 绝对路径不相等，但它们说的是同一个工作区。
    pub fn is_in_bucket(&self, cwd: &Path, id: &SessionId) -> io::Result<bool> {
        Ok(self.list(cwd)?.iter().any(|session| session.id == *id))
    }

    /// 这个 cwd 的桶里每一场会话，最近写下的在前。
    ///
    /// 一个条目只有带着 `log.jsonl` 才算会话：在 `create` 与第一条 `SessionStarted` 之间进程就
    /// 死了的目录，没有可接着跑的流。
    pub fn list(&self, cwd: &Path) -> io::Result<Vec<StoredSession>> {
        let bucket = self.bucket(cwd);
        let mut sessions = scan_bucket(&bucket)?;
        sort_newest_first(&mut sessions);
        Ok(sessions)
    }

    /// store 里每一场会话，跨所有桶，最近写下的在前。
    ///
    /// 以工作区为单位的问题（不带 id 的 `--continue`、`prune`）走 [`SessionStore::list`]；这个
    /// 是全 store 的版本：全日账本要它（厂商的配额窗口是跨工作区一起花的，spec §17），按 id 找
    /// 一场会话也要它（`sessions show <id>`、`-c <id>`）。
    pub fn list_all(&self) -> io::Result<Vec<StoredSession>> {
        let buckets = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };

        let mut sessions = Vec::new();
        for bucket in buckets {
            let path = bucket?.path();
            if !path.is_dir() {
                continue;
            }
            sessions.extend(scan_bucket(&path)?);
        }
        sort_newest_first(&mut sessions);
        Ok(sessions)
    }

    /// 删掉一个会话目录以及里面的一切。
    pub fn delete(&self, session: &StoredSession) -> io::Result<()> {
        std::fs::remove_dir_all(&session.dir)
    }

    /// 在这个 cwd 的桶里只留下最近 `keep` 场会话，返回被删掉的那些。
    ///
    /// 什么都不自动 prune（spec §11）：这是手动的那根杠杆。
    pub fn prune(&self, cwd: &Path, keep: usize) -> io::Result<Vec<StoredSession>> {
        let mut removed = Vec::new();
        for session in self.list(cwd)?.into_iter().skip(keep) {
            self.delete(&session)?;
            removed.push(session);
        }
        Ok(removed)
    }
}

/// 某一个桶直属的每一个会话目录。
fn scan_bucket(bucket: &Path) -> io::Result<Vec<StoredSession>> {
    let entries = match std::fs::read_dir(bucket) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    let mut sessions = Vec::new();
    for entry in entries {
        let dir = entry?.path();
        if !dir.is_dir() {
            continue;
        }
        let log_path = dir.join(LOG_FILE);
        if !log_path.is_file() {
            continue;
        }
        let Some(name) = dir.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        sessions.push(StoredSession {
            id: SessionId::new(name),
            outputs_dir: dir.join(OUTPUTS_DIR),
            log_path,
            dir,
        });
    }
    Ok(sessions)
}

/// 最新的在前。id 以它自己的 UTC 创建时间戳开头，所以在 mtime 打平时，它按唯一合理的方向决
/// 出先后。
fn sort_newest_first(sessions: &mut [StoredSession]) {
    sessions.sort_by(|a, b| {
        modified(&b.log_path)
            .cmp(&modified(&a.log_path))
            .then_with(|| b.id.cmp(&a.id))
    });
}

/// 一个新的会话 id：`<UTC 时间戳>-<短的随机后缀>`（spec §11）。
///
/// 时间戳让 id 能按时间排序、按时间读；后缀让同一秒里铸出的两场会话彼此区分。`--continue` 之
/// 后这个 id **永不变**，这正是两家 provider 的前缀缓存都还能命中的原因。
pub fn new_session_id() -> SessionId {
    SessionId::new(format!(
        "{}-{}",
        Utc::now().format("%Y%m%dT%H%M%SZ"),
        short_suffix()
    ))
}

/// 从操作系统取种子的八位十六进制。
///
/// `RandomState` 每个进程从系统随机源取一次种子，每次调用再扰动一次，这对「两个 id 不撞」已经
/// 够了，而且不必引入随机数依赖。会话 id 要的是互不相同，不是不可预测。
fn short_suffix() -> String {
    use std::hash::{BuildHasher, Hasher};

    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    hasher.write_u128(nanos);
    format!("{:08x}", hasher.finish() as u32)
}

/// 一个规范 cwd 对应的、对文件系统安全且稳定的桶名。
///
/// 可读前缀让人还能分辨各桶；哈希后缀是让两个不同路径在清洗后形状相同时（`/a/b` 与 `/a-b`）仍
/// 然区分开的东西，而这个哈希是自己的、不是 `DefaultHasher` 的，所以工具链升级不会让昨天的会话
/// 成为孤儿。
fn cwd_slug(cwd: &Path) -> String {
    const MAX_READABLE: usize = 64;

    let raw = cwd.to_string_lossy();
    let mut readable: String = raw
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect();
    if readable.len() > MAX_READABLE {
        // 尾部（项目目录那一段）才是标识性的部分。
        readable = readable[readable.len() - MAX_READABLE..].to_owned();
    }
    format!("{readable}-{:016x}", fnv1a(raw.as_bytes()))
}

/// FNV-1a，64 位：一个永远跨版本稳定的极小哈希。
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// 一个路径最后一次被写入的时间；读不出的路径按古老排序。
pub(super) fn modified(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .unwrap_or(UNIX_EPOCH)
}

/// 创建 `path` 以及沿途缺失的祖先，仅属主（`0700`）。
///
/// 缺失的祖先是同样建的，所以整条 store 路径 —— 不只是它的叶子 —— 都是私有的；用户本来就有的
/// 祖先保持原样。模式由 `mkdir` 本身施加，所以没有任何目录曾经短暂地世界可读。`0700` 没有组位
/// 或其他位可供 `umask 022` 放宽（spec §11、§20）；在不存在 unix 模式的地方，这次调用是空操作。
fn create_private_dirs(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            create_private_dirs(parent)?;
        }
    }
    create_private_dir(path).map(|_| ())
}

/// 创建一个仅属主的目录。`Ok(false)` 表示它已经存在。
fn create_private_dir(path: &Path) -> io::Result<bool> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;

        builder.mode(0o700);
    }
    match builder.create(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
    }
}
