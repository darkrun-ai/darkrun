//! The [`GitBackend`] abstraction.
//!
//! darkrun talks to git through a trait so the implementation can be swapped:
//! the default [`Libgit2Backend`](crate::libgit2::Libgit2Backend) drives
//! everything in-process via libgit2, while the
//! [`ShellBackend`](crate::shell::ShellBackend) shells out to the `git`
//! executable. The shell backend exists as a fallback for the handful of
//! worktree operations libgit2 historically handles awkwardly across versions,
//! and as an escape hatch in environments where linking libgit2 is undesirable.

use std::path::{Path, PathBuf};

use crate::error::Result;

/// A registered git worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    /// The worktree's logical name (the directory name git registers it under).
    pub name: String,
    /// The absolute path to the worktree's working directory.
    pub path: PathBuf,
    /// The branch checked out in the worktree, if any (`None` when detached).
    pub branch: Option<String>,
    /// Whether the worktree is locked.
    pub locked: bool,
}

/// Options controlling how a worktree is created.
#[derive(Debug, Clone, Default)]
pub struct CreateOptions {
    /// The committish (branch, tag, or revision) to fork the worktree from.
    /// When `None`, the worktree forks from the repository `HEAD`.
    pub reference: Option<String>,
    /// When set, create (and check out) a new branch with this name in the
    /// worktree. When `None`, the worktree checks out `reference`/`HEAD`
    /// directly (detached when the reference is not a branch).
    pub new_branch: Option<String>,
}

/// The set of git worktree operations darkrun depends on.
///
/// Implementations MUST treat read-only queries (`list_worktrees`,
/// `current_branch`, `is_clean`) as non-mutating and side-effect free.
pub trait GitBackend {
    /// Create a worktree named `name` at `path`. See [`CreateOptions`].
    fn create_worktree(
        &self,
        name: &str,
        path: &Path,
        opts: &CreateOptions,
    ) -> Result<WorktreeInfo>;

    /// List every registered worktree (including the primary working tree).
    fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>>;

    /// Remove the worktree named `name`. When `force` is true, remove it even
    /// if it contains uncommitted or untracked changes.
    fn remove_worktree(&self, name: &str, force: bool) -> Result<()>;

    /// The branch currently checked out in the repository's main working tree,
    /// or `None` when `HEAD` is detached.
    fn current_branch(&self) -> Result<Option<String>>;

    /// Whether the repository's working tree has no pending changes (no
    /// modified, staged, or untracked-but-not-ignored files).
    fn is_clean(&self) -> Result<bool>;
}
