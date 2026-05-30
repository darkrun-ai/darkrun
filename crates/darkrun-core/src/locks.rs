//! Advisory `mkdir`-based locks with stale-holder recovery.
//!
//! Why mkdir locks: `mkdir` is atomic on POSIX
//! and Windows, needs no native bindings, and is cross-process — multiple
//! engine processes coordinate purely through the filesystem.
//!
//! Stale recovery: the lock dir holds a `holder.json` carrying the pid +
//! timestamp. On acquire, if the dir is older than [`STALE_AFTER`] and its
//! pid is no longer alive (verified via `kill(pid, 0)` through `nix`), the
//! lock is stolen.

use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// Backoff between acquire attempts.
const ACQUIRE_RETRY: Duration = Duration::from_millis(50);
/// Total time an acquire blocks before giving up.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(30);
/// A holder dir older than this is a stale-recovery candidate.
const STALE_AFTER: Duration = Duration::from_secs(5 * 60);

/// The holder stamp written into a lock dir.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Holder {
    /// PID of the holding process.
    pid: i32,
    /// Acquisition time as a Unix-epoch millisecond stamp.
    at: u64,
    /// Diagnostic tag.
    tag: String,
}

/// Probe whether a pid is still alive.
///
/// `kill(pid, 0)` is a no-op signal: `Ok` means alive; `ESRCH` means the
/// process is truly gone; `EPERM` means it exists but we can't signal it
/// (alive, cross-user) — treated as alive so we never steal a live lock.
///
/// The `nix`-backed implementation is unix-only. On targets without process
/// signalling (notably `wasm32-unknown-unknown`, where this crate is pulled in
/// only for its domain types by the website), we conservatively treat every pid
/// as alive — the filesystem lock engine is not exercised there.
#[cfg(unix)]
fn is_alive(pid: i32) -> bool {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    match kill(Pid::from_raw(pid), None) {
        Ok(()) => true,
        Err(nix::errno::Errno::EPERM) => true,
        Err(_) => false,
    }
}

/// Non-unix fallback: assume alive so a live lock is never stolen.
#[cfg(not(unix))]
fn is_alive(_pid: i32) -> bool {
    true
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A held advisory lock. Released on drop.
#[derive(Debug)]
pub struct LockGuard {
    dir: PathBuf,
    released: bool,
}

impl LockGuard {
    /// The path of the held lock directory.
    pub fn path(&self) -> &Path {
        &self.dir
    }

    /// Explicitly release the lock. Idempotent.
    pub fn release(mut self) {
        self.release_inner();
    }

    fn release_inner(&mut self) {
        if self.released {
            return;
        }
        // Best-effort: a lingering dir is reclaimed by the next stale check.
        let _ = fs::remove_dir_all(&self.dir);
        self.released = true;
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        self.release_inner();
    }
}

/// A filesystem lock manager rooted at `.darkrun/locks` under a repo root.
#[derive(Debug, Clone)]
pub struct LockManager {
    root: PathBuf,
}

impl LockManager {
    /// Create a manager whose locks live under `<repo_root>/.darkrun/locks`.
    pub fn new(repo_root: impl AsRef<Path>) -> Self {
        let root = repo_root.as_ref().join(".darkrun").join("locks");
        LockManager { root }
    }

    /// The directory under which lock dirs are created.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Acquire a named advisory lock, blocking (with backoff) until it is
    /// available, the timeout elapses, or a stale holder is reclaimed.
    pub fn acquire(&self, name: &str, tag: &str) -> Result<LockGuard> {
        fs::create_dir_all(&self.root).map_err(|source| CoreError::Io {
            path: self.root.clone(),
            source,
        })?;
        let lock_dir = self.root.join(name);
        let deadline = std::time::Instant::now() + ACQUIRE_TIMEOUT;

        loop {
            if self.try_acquire(&lock_dir, tag) {
                return Ok(LockGuard {
                    dir: lock_dir,
                    released: false,
                });
            }
            if self.is_stale(&lock_dir) && self.steal(&lock_dir, tag) {
                return Ok(LockGuard {
                    dir: lock_dir,
                    released: false,
                });
            }
            if std::time::Instant::now() >= deadline {
                return Err(CoreError::LockTimeout {
                    name: name.to_string(),
                    timeout_ms: ACQUIRE_TIMEOUT.as_millis() as u64,
                });
            }
            std::thread::sleep(ACQUIRE_RETRY);
        }
    }

    /// Run `f` while holding a lock named `name`. The lock is released even
    /// if `f` panics (via `LockGuard`'s `Drop`).
    pub fn with_lock<T>(
        &self,
        name: &str,
        tag: &str,
        f: impl FnOnce() -> T,
    ) -> Result<T> {
        let guard = self.acquire(name, tag)?;
        let out = f();
        guard.release();
        Ok(out)
    }

    fn try_acquire(&self, lock_dir: &Path, tag: &str) -> bool {
        // `create_dir` (non-recursive) fails if the dir exists — that is the
        // atomic acquire test.
        if fs::create_dir(lock_dir).is_err() {
            return false;
        }
        let holder = Holder {
            pid: process::id() as i32,
            at: now_millis(),
            tag: tag.to_string(),
        };
        // Best-effort stamp; a holder-less dir is treated as wedged by the
        // stale check.
        if let Ok(json) = serde_json::to_string_pretty(&holder) {
            let _ = fs::write(lock_dir.join("holder.json"), json);
        }
        true
    }

    fn read_holder(&self, lock_dir: &Path) -> Option<Holder> {
        let raw = fs::read_to_string(lock_dir.join("holder.json")).ok()?;
        serde_json::from_str::<Holder>(&raw).ok()
    }

    /// True when the lock dir is old enough and its holder is dead (or absent).
    pub fn is_stale(&self, lock_dir: &Path) -> bool {
        let Ok(meta) = fs::metadata(lock_dir) else {
            // Vanished mid-check — not stale, just gone.
            return false;
        };
        let modified = meta.modified().ok();
        let age = modified
            .and_then(|m| SystemTime::now().duration_since(m).ok())
            .unwrap_or(Duration::ZERO);
        if age < STALE_AFTER {
            return false;
        }
        match self.read_holder(lock_dir) {
            None => true, // no holder file — dir is wedged
            Some(h) => !is_alive(h.pid),
        }
    }

    fn steal(&self, lock_dir: &Path, tag: &str) -> bool {
        if fs::remove_dir_all(lock_dir).is_err() {
            return false;
        }
        self.try_acquire(lock_dir, tag)
    }
}
