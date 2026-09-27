//! 路径解析与逐路径的写锁（spec §7、§12）。
//!
//! 两条护栏住在这里，因为每一个文件工具都要用它们，而且无论谁在调用都必须成立：
//!
//! - **cwd 收容**：模型给的路径被限制在会话 cwd 及其子树内，因为 agent 自己那把 API key
//!   就在用户的家目录里。这条规矩限制的是*模型给出的*路径，不是 harness 自己读的路径。
//!
//!   spec §20 把那个例外称作「一条权限规则放宽它」；那个例外**没有**实现 —— 权限门把一个
//!   解析不了的目标当作任何规则都降不下去的拒绝地板（`permissions::decide`，由
//!   `tests/permission_gate.rs::the_path_limit_is_a_deny_floor` 断言），派发器在任何工具跑
//!   起来之前就拒掉。为什么那套不看专指程度的规则代数表达不出那个例外的安全版本、以及在
//!   这之前是什么在护着那把 key（打码），见 `docs/credentials.md`。
//! - **逐路径的写锁**：锁表在组装期注入、被所有执行者共用，所以两个执行者不能交错写同一个
//!   文件。逐会话一张锁表等于根本没有锁。

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::tool::{ReadPathResolver, ToolError, WritePathResolver};

/// 把一个会话产物按「仅属主」写盘（`0600`）。
///
/// 模式是在创建文件时设的，不是事后再收窄。产物本来就住在 `0700` 的会话目录里，所以这只是
/// 纵深防御：即使会话目录自身的模式没能在一次拷贝中活下来，它也能让一份 `.before` 快照或
/// 一份溢出的 `.txt` 够不着（spec §11、§20）。
pub fn write_owner_only(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.mode(0o600);
    }
    options.open(path)?.write_all(bytes)
}

/// 把模型给的路径解析到某一个会话 cwd 上。
#[derive(Debug, Clone)]
pub struct SessionPaths {
    /// 规范化的会话 cwd。
    cwd: PathBuf,
}

impl SessionPaths {
    /// cwd 只规范化一次，于是收容判定可以只是一个前缀比较。
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        let cwd = cwd.into();
        let cwd = std::fs::canonicalize(&cwd).unwrap_or_else(|_| lexical_absolute(&cwd));
        Self { cwd }
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// 解析一条必须存在的路径（或它的父目录必须存在）。
    ///
    /// 路径存在时结果是绝对且不含符号链接的，所以同一个文件在读集合与锁表里永远给出同一个键。
    pub fn resolve(&self, path: &Path) -> Result<PathBuf, ToolError> {
        let joined = self.join(path);
        let resolved = match std::fs::canonicalize(&joined) {
            Ok(resolved) => resolved,
            Err(_) => {
                // 还不存在的文件经它的父目录解析；父目录必须存在，而符号链接能逃出去靠的正是它。
                let parent = joined.parent().ok_or_else(|| {
                    ToolError::message(format!("{} has no parent directory", joined.display()))
                })?;
                let parent = std::fs::canonicalize(parent).map_err(|error| {
                    ToolError::message(format!("cannot resolve {}: {error}", joined.display()))
                })?;
                match joined.file_name() {
                    Some(name) => parent.join(name),
                    None => parent,
                }
            }
        };
        self.check_contained(&resolved)?;
        Ok(resolved)
    }

    /// 把模型给的路径按字面拼到 cwd 上，不规范化、也不做收容检查。
    ///
    /// 只在 [`SessionPaths::resolve`] 失败时用：权限门仍必须在护栏拒掉这次调用之前看到它，
    /// 所以它拿到的就是那个原始目标。
    pub fn unresolved(&self, path: &Path) -> PathBuf {
        self.join(path)
    }

    fn join(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        }
    }

    /// 收容规则，按组件逐段做前缀比较，这样 `/work-evil` 冒充不了 `/work` 的子路径。
    fn check_contained(&self, resolved: &Path) -> Result<(), ToolError> {
        if resolved.starts_with(&self.cwd) {
            return Ok(());
        }
        Err(ToolError::message(format!(
            "path {} is outside the session workspace {}; file tools are confined to the \
             workspace and no permission rule widens that",
            resolved.display(),
            self.cwd.display()
        )))
    }
}

impl ReadPathResolver for SessionPaths {
    fn resolve_read(&self, path: &Path) -> Result<PathBuf, ToolError> {
        self.resolve(path)
    }
}

impl WritePathResolver for SessionPaths {
    fn resolve_write(&self, path: &Path) -> Result<PathBuf, ToolError> {
        self.resolve(path)
    }
}

/// 一条把 `.` 与 `..` 按字面折叠掉的绝对路径，用于 cwd 自己还不存在、`canonicalize` 跑不了
/// 的那种情形。
fn lexical_absolute(path: &Path) -> PathBuf {
    let base = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    };
    let mut out = PathBuf::new();
    for component in base.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 每条路径一把锁，外加一把工作区级的锁，由进程里每一个写入者共用。
///
/// 克隆很便宜：两者都在一个 `Arc` 后面，所以组装点把**同一批**锁交给每一个嵌套会话。逐会话
/// 建一张锁表会让两个执行者同时写一个文件，那与没有锁是一回事。
#[derive(Debug, Clone, Default)]
pub struct PathLocks {
    table: Arc<Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>>,
    exclusive: Arc<tokio::sync::Mutex<()>>,
}

impl PathLocks {
    pub fn new() -> Self {
        Self::default()
    }

    /// 取 `path` 的那把锁，排在该路径其他写入者后面。
    pub async fn lock(&self, path: &Path) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = {
            let mut table = self.table.lock().expect("路径锁表已中毒");
            table
                .entry(path.to_path_buf())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        lock.lock_owned().await
    }

    /// 取一次 `Exclusive` 调用需要的那把工作区级锁。
    ///
    /// `Exclusive` 的意思是「同一时刻没有别的工作」，这是逐路径的锁表达不了的：那两次调用
    /// 可能一个路径都没点名。
    pub async fn lock_exclusive(&self) -> tokio::sync::OwnedMutexGuard<()> {
        self.exclusive.clone().lock_owned().await
    }
}
