//! The session store (spec §11): one directory per session, one bucket per cwd.
//!
//! A session is a **movable directory**: a JSONL event stream plus the `outputs/`
//! artifacts the stream points at, and nothing else. Copying the directory to
//! another machine copies the whole session.
//!
//! ```text
//! <root>/<cwd-slug>/<session-id>/
//!     log.jsonl
//!     outputs/
//! ```
//!
//! The cwd slug only **buckets**: the authoritative binding is the `cwd` recorded
//! in `SessionStarted`. `--continue` therefore never needs a global index — it
//! scans this directory's bucket and takes the log written to most recently.
//!
//! The root is injected, never read from the environment: the library reads no
//! environment, so the caller (the CLI) decides where the store lives (spec §1).
//!
//! Everything the store creates is **owner-only** (`0700` for the session and
//! artifact directories, `0600` for the event stream), because a session holds
//! the user's source and, with redaction, the secrets it was careful to keep
//! (spec §11, §20).

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;

use crate::events::SessionId;

/// The event stream's file name inside a session directory.
pub const LOG_FILE: &str = "log.jsonl";
/// The tool artifacts' directory name inside a session directory.
pub const OUTPUTS_DIR: &str = "outputs";

/// Session directories under one injected root.
#[derive(Debug, Clone)]
pub struct SessionStore {
    root: PathBuf,
}

/// One session directory that exists on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSession {
    pub id: SessionId,
    /// The session directory: the movable unit.
    pub dir: PathBuf,
    /// The JSONL event stream inside `dir`.
    pub log_path: PathBuf,
    /// Where the stream's artifacts (`<tool_call_id>.before`, `.txt`) land.
    pub outputs_dir: PathBuf,
}

impl SessionStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The bucket one cwd's sessions live in.
    ///
    /// The cwd is canonicalized first, so two spellings of one directory share a
    /// bucket. A cwd that cannot be canonicalized (it does not exist yet) is
    /// slugged as given.
    pub fn bucket(&self, cwd: &Path) -> PathBuf {
        let canonical = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        self.root.join(cwd_slug(&canonical))
    }

    /// Create a fresh session directory and return its identity and paths.
    ///
    /// The event stream itself is created by the assembly point (it is the one
    /// place that opens a log); the store owns the *directory* shape and its
    /// permissions.
    pub fn create(&self, cwd: &Path) -> io::Result<StoredSession> {
        let bucket = self.bucket(cwd);
        create_private_dirs(&bucket)?;

        // Two ids can collide only if the same second of the clock and the same
        // random suffix meet; retrying keeps that possibility from being a bug.
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
            "could not allocate a unique session id after 8 attempts",
        ))
    }

    /// The session this cwd would resume: the bucket's most recently written
    /// stream, or `None` when the bucket holds nothing.
    pub fn latest(&self, cwd: &Path) -> io::Result<Option<StoredSession>> {
        Ok(self.list(cwd)?.into_iter().next())
    }

    /// Every session in this cwd's bucket, most recently written first.
    ///
    /// An entry is a session only if its `log.jsonl` exists: a directory whose
    /// process died between `create` and the first `SessionStarted` holds no
    /// stream to resume.
    pub fn list(&self, cwd: &Path) -> io::Result<Vec<StoredSession>> {
        let bucket = self.bucket(cwd);
        let mut sessions = scan_bucket(&bucket)?;
        sort_newest_first(&mut sessions);
        Ok(sessions)
    }

    /// Every session in the store, across every bucket, most recently written
    /// first.
    ///
    /// A workspace-scoped question (`--continue`, `prune`) goes through
    /// [`SessionStore::list`]; this is the store-wide one the daily ledger asks,
    /// because a vendor's quota window is spent across workspaces (spec §17).
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

    /// Remove one session directory and everything in it.
    pub fn delete(&self, session: &StoredSession) -> io::Result<()> {
        std::fs::remove_dir_all(&session.dir)
    }

    /// Remove all but the `keep` most recent sessions in this cwd's bucket,
    /// returning what was removed.
    ///
    /// Nothing prunes automatically (spec §11): this is the manual lever.
    pub fn prune(&self, cwd: &Path, keep: usize) -> io::Result<Vec<StoredSession>> {
        let mut removed = Vec::new();
        for session in self.list(cwd)?.into_iter().skip(keep) {
            self.delete(&session)?;
            removed.push(session);
        }
        Ok(removed)
    }
}

/// Every session directory directly under one bucket.
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

/// Newest first. Ids begin with their UTC creation stamp, so they break an mtime
/// tie in the only sensible direction.
fn sort_newest_first(sessions: &mut [StoredSession]) {
    sessions.sort_by(|a, b| {
        modified(&b.log_path)
            .cmp(&modified(&a.log_path))
            .then_with(|| b.id.cmp(&a.id))
    });
}

/// A fresh session id: `<UTC timestamp>-<short random suffix>` (spec §11).
///
/// The timestamp makes ids sort and read chronologically; the suffix keeps two
/// sessions minted in the same second distinct. The id **never changes** across
/// `--continue`, which is what keeps both providers' prefix caches hitting.
pub fn new_session_id() -> SessionId {
    SessionId::new(format!(
        "{}-{}",
        Utc::now().format("%Y%m%dT%H%M%SZ"),
        short_suffix()
    ))
}

/// Eight hex digits seeded from the OS.
///
/// `RandomState` is seeded once per process from the system random source and
/// perturbed per call, which is enough for "two ids do not collide" without a
/// random-number dependency. Session ids need distinctness, not unpredictability.
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

/// A filesystem-safe, stable bucket name for one canonical cwd.
///
/// The readable prefix keeps a human able to tell buckets apart; the hash suffix
/// is what makes two different paths with the same sanitized form (`/a/b` and
/// `/a-b`) distinct, and it is a hash we own rather than `DefaultHasher`'s, so a
/// toolchain upgrade does not orphan yesterday's sessions.
fn cwd_slug(cwd: &Path) -> String {
    const MAX_READABLE: usize = 64;

    let raw = cwd.to_string_lossy();
    let mut readable: String = raw
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect();
    if readable.len() > MAX_READABLE {
        // The tail (the project directory) is the identifying part.
        readable = readable[readable.len() - MAX_READABLE..].to_owned();
    }
    format!("{readable}-{:016x}", fnv1a(raw.as_bytes()))
}

/// FNV-1a, 64-bit: a tiny hash that is stable across releases forever.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// When a path was last written; an unreadable path sorts as ancient.
pub(super) fn modified(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .unwrap_or(UNIX_EPOCH)
}

/// Create `path`, and any missing ancestor, owner-only (`0700`).
///
/// Missing ancestors are created the same way, so the whole store path — not
/// just its leaf — is private; ancestors the user already had are left as they
/// are. The mode is applied by `mkdir` itself, so no directory is ever briefly
/// world-readable. `0700` has no group or other bits for `umask 022` to widen
/// (spec §11, §20); the call is a no-op where unix modes do not exist.
fn create_private_dirs(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            create_private_dirs(parent)?;
        }
    }
    create_private_dir(path).map(|_| ())
}

/// Create one directory owner-only. `Ok(false)` means it already existed.
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
