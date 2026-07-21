use std::path::{Path, PathBuf};

use crate::AppError;
use crate::repositories::{BranchName, RemoteUrl};

use super::{GitProgress, GitRefreshOutcome, GitUpdateOutcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BranchDivergence {
    ahead: u32,
    behind: u32,
}

impl BranchDivergence {
    pub fn new(ahead: u32, behind: u32) -> Self {
        Self { ahead, behind }
    }

    pub fn ahead(self) -> u32 {
        self.ahead
    }

    pub fn behind(self) -> u32 {
        self.behind
    }
}

/// Contract for Git operations owned by grove.
pub trait GitClient: Sync {
    fn verify_available(&self) -> Result<(), AppError>;

    /// Create a bare, single-branch cache entry at `entry`. When `branch` is
    /// `Some`, that branch is tracked; otherwise the remote HEAD branch is.
    /// A fetch refspec binding the tracked branch is configured so later
    /// refreshes advance it. Returns the resolved tracked branch name.
    fn cache_create(
        &self,
        url: &RemoteUrl,
        entry: &Path,
        branch: Option<&str>,
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

    fn fetch(&self, repository: &Path, progress: &mut dyn GitProgressSink) -> Result<(), AppError>;

    fn common_directory(&self, repository: &Path) -> Result<PathBuf, AppError>;

    fn is_work_tree(&self, repository: &Path) -> Result<bool, AppError>;

    fn current_branch(&self, repository: &Path) -> Result<Option<String>, AppError>;

    fn working_tree_clean(&self, repository: &Path) -> Result<bool, AppError>;

    fn remote_url(&self, repository: &Path) -> Result<Option<RemoteUrl>, AppError>;

    fn default_branch(
        &self,
        repository: &Path,
        configured: Option<&BranchName>,
    ) -> Result<Option<String>, AppError>;

    fn local_branch_exists(&self, repository: &Path, branch: &str) -> Result<bool, AppError>;

    fn remote_branch_exists(&self, repository: &Path, branch: &str) -> Result<bool, AppError>;

    fn branch_divergence(
        &self,
        repository: &Path,
        branch: &str,
    ) -> Result<BranchDivergence, AppError>;

    fn short_revision(&self, repository: &Path, reference: &str) -> Result<String, AppError>;

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
