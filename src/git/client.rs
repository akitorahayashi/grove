use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use crate::AppError;
use crate::repositories::{BranchName, RemoteUrl};

use super::{BranchTracking, GitProgress, GitRefreshOutcome, GitUpdateOutcome, WorktreeStatus};

/// An advisory lock for a Git common directory.
///
/// Command-backed clients hold an open, exclusively locked file for the
/// guard's lifetime. Test clients use the default no-op guard unless they need
/// to model cross-process coordination explicitly.
pub(crate) struct RepositoryLock {
    file: Option<File>,
}

impl RepositoryLock {
    pub(super) fn acquire(common_directory: &Path) -> Result<Self, AppError> {
        let path = common_directory.join("grove-operation.lock");
        let file =
            OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;
        file.lock()?;
        Ok(Self { file: Some(file) })
    }

    fn noop() -> Self {
        Self { file: None }
    }
}

impl Drop for RepositoryLock {
    fn drop(&mut self) {
        if let Some(file) = &self.file {
            let _ = File::unlock(file);
        }
    }
}

/// Read-only observation of repository and remote state, plus the fetch and
/// availability checks that refresh what can be observed without advancing a
/// local branch. This is the whole surface the status and inspection consumers
/// need, so their doubles implement only this trait.
pub trait RepositoryProbe: Sync {
    fn verify_available(&self) -> Result<(), AppError>;

    fn lock_repository(&self, _common_directory: &Path) -> Result<RepositoryLock, AppError> {
        Ok(RepositoryLock::noop())
    }

    fn fetch(&self, repository: &Path, progress: &mut dyn GitProgressSink) -> Result<(), AppError>;

    fn common_directory(&self, repository: &Path) -> Result<PathBuf, AppError>;

    fn is_work_tree(&self, repository: &Path) -> Result<bool, AppError>;

    fn worktree_status(&self, repository: &Path) -> Result<Option<WorktreeStatus>, AppError>;

    fn remote_url(&self, repository: &Path) -> Result<Option<RemoteUrl>, AppError>;

    fn default_branch(
        &self,
        repository: &Path,
        configured: Option<&BranchName>,
    ) -> Result<Option<String>, AppError>;

    fn branch_tracking(
        &self,
        repository: &Path,
        branch: &BranchName,
    ) -> Result<BranchTracking, AppError>;
}

/// Creation and maintenance of the bare, single-branch cache entries and the
/// reference-based placement of working clones from them.
pub trait CacheEntry: Sync {
    /// Create a bare, single-branch cache entry at `entry`. When `branch` is
    /// `Some`, that branch is tracked; otherwise the remote HEAD branch is.
    /// When `reference` is `Some`, objects are borrowed from that local
    /// repository and dissociated, so an existing clone seeds the entry
    /// without re-downloading. A fetch refspec binding the tracked branch is
    /// configured so later refreshes advance it. Returns the resolved tracked
    /// branch name.
    fn cache_create(
        &self,
        url: &RemoteUrl,
        entry: &Path,
        branch: Option<&str>,
        reference: Option<&Path>,
        progress: &mut dyn GitProgressSink,
    ) -> Result<String, AppError>;

    /// Refresh a cache entry's tracked branch, pruning deleted refs.
    fn cache_update(
        &self,
        entry: &Path,
        progress: &mut dyn GitProgressSink,
    ) -> Result<(), AppError>;

    /// Point a cache entry at a different branch, then refresh it.
    fn cache_retarget(
        &self,
        entry: &Path,
        branch: &str,
        progress: &mut dyn GitProgressSink,
    ) -> Result<(), AppError>;

    /// Report whether `entry` is a usable Git repository.
    fn cache_verify(&self, entry: &Path) -> Result<bool, AppError>;

    /// Clone `url` into `destination`, borrowing objects from `reference` and
    /// dissociating so the result is self-contained.
    fn clone_with_reference(
        &self,
        url: &RemoteUrl,
        destination: &Path,
        reference: &Path,
        progress: &mut dyn GitProgressSink,
    ) -> Result<(), AppError>;
}

/// Advancing and switching a repository's local default branch.
pub trait DefaultBranch: Sync {
    fn update_default_branch(
        &self,
        repository: &Path,
        branch: &str,
    ) -> Result<GitUpdateOutcome, AppError>;

    fn refresh_default_branch(
        &self,
        repository: &Path,
        branch: &str,
    ) -> Result<GitRefreshOutcome, AppError>;
}

/// The full Git surface grove owns, composed from the focused traits. Any type
/// that implements all three satisfies it, so consumers bound only the traits
/// they use while use cases that need the whole surface bound this.
pub trait GitClient: RepositoryProbe + CacheEntry + DefaultBranch {}

impl<T: RepositoryProbe + CacheEntry + DefaultBranch> GitClient for T {}

pub trait GitProgressSink {
    fn progress(&mut self, progress: GitProgress) -> Result<(), AppError>;
}

#[derive(Debug, Default)]
pub struct NoopGitProgressSink;

impl GitProgressSink for NoopGitProgressSink {
    fn progress(&mut self, _progress: GitProgress) -> Result<(), AppError> {
        Ok(())
    }
}
