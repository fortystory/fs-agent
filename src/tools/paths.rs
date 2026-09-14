//! Path resolution and per-path write locks (spec §7, §12).
//!
//! Two guardrails live here because both are shared by every file tool and must
//! hold no matter which executor is calling:
//!
//! - **cwd containment**: a model-supplied path is confined to the session cwd
//!   and its subtree, because the agent's own API key sits in the user's home
//!   directory. The rule limits paths *the model supplies*, not paths the
//!   harness reads for itself.
//! - **per-path write locks**: the lock table is injected at assembly time and
//!   shared across executors, so two executors cannot interleave writes to one
//!   file. A per-session table would be equivalent to no lock at all.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::tool::{ReadPathResolver, ToolError, WritePathResolver};

/// Resolve model-supplied paths against one session cwd.
#[derive(Debug, Clone)]
pub struct SessionPaths {
    /// Canonical session cwd.
    cwd: PathBuf,
}

impl SessionPaths {
    /// The cwd is canonicalized once, so containment can be a plain prefix test.
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        let cwd = cwd.into();
        let cwd = std::fs::canonicalize(&cwd).unwrap_or_else(|_| lexical_absolute(&cwd));
        Self { cwd }
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Resolve a path that must exist (or whose parent must).
    ///
    /// The result is absolute and symlink-free when the path exists, so the same
    /// file always yields the same key for the read set and the lock table.
    pub fn resolve(&self, path: &Path) -> Result<PathBuf, ToolError> {
        let joined = self.join(path);
        let resolved = match std::fs::canonicalize(&joined) {
            Ok(resolved) => resolved,
            Err(_) => {
                // A file that does not exist yet is resolved through its parent,
                // which must exist and is what a symlink could escape through.
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

    fn join(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        }
    }

    /// The containment rule, as a component-wise prefix test so `/work-evil`
    /// cannot pass for a child of `/work`.
    fn check_contained(&self, resolved: &Path) -> Result<(), ToolError> {
        if resolved.starts_with(&self.cwd) {
            return Ok(());
        }
        Err(ToolError::message(format!(
            "path {} is outside the session workspace {}; file tools are confined to the \
             workspace (a permission rule is how you widen this)",
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

/// An absolute path with `.` and `..` folded lexically, for the case where the
/// cwd itself does not exist yet and `canonicalize` cannot run.
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

/// One lock per path plus one workspace-wide lock, shared by every writer in the
/// process.
///
/// Cheap to clone: both are behind an `Arc`, so the assembly point hands the
/// **same** locks to every nested session. A lock table built per session would
/// let two executors write one file at once, which is the same as no lock.
#[derive(Debug, Clone, Default)]
pub struct PathLocks {
    table: Arc<Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>>,
    exclusive: Arc<tokio::sync::Mutex<()>>,
}

impl PathLocks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take the lock for `path`, waiting behind any other writer of that path.
    pub async fn lock(&self, path: &Path) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = {
            let mut table = self.table.lock().expect("path lock table poisoned");
            table
                .entry(path.to_path_buf())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        lock.lock_owned().await
    }

    /// Take the workspace-wide lock that an `Exclusive` call needs.
    ///
    /// `Exclusive` means "no other work at the same time", which a per-path lock
    /// cannot express: the two calls may name no paths at all.
    pub async fn lock_exclusive(&self) -> tokio::sync::OwnedMutexGuard<()> {
        self.exclusive.clone().lock_owned().await
    }
}
