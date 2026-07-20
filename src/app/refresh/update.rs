use std::path::{Path, PathBuf};

use crate::AppError;
use crate::git::{GitClient, GitRefreshOutcome, GitUpdateBlock};
use crate::repositories::RepositoryDefinition;

use super::check::refresh_block_reason;
use super::{BlockedReason, Entry, Outcome, SkippedReason};

pub(super) struct Task<'a> {
    index: usize,
    repository: &'a RepositoryDefinition,
    common_directory: PathBuf,
    default_branch: String,
}

impl<'a> Task<'a> {
    pub(super) fn new(
        index: usize,
        repository: &'a RepositoryDefinition,
        common_directory: PathBuf,
        default_branch: String,
    ) -> Self {
        Self { index, repository, common_directory, default_branch }
    }

    pub(super) fn index(&self) -> usize {
        self.index
    }

    pub(super) fn repository(&self) -> &RepositoryDefinition {
        self.repository
    }

    pub(super) fn resource(&self) -> &Path {
        &self.common_directory
    }
}

pub(super) fn repository(git: &impl GitClient, task: &Task<'_>) -> Entry {
    match refresh_repository(git, task) {
        Ok(entry) => entry,
        Err(error) => Entry::new(
            task.repository,
            Outcome::Blocked { reason: BlockedReason::UpdateFailed(error.to_string()) },
        ),
    }
}

fn refresh_repository(git: &impl GitClient, task: &Task<'_>) -> Result<Entry, AppError> {
    if let Some(reason) = refresh_block_reason(git, task.repository, &task.default_branch)? {
        return Ok(Entry::new(task.repository, Outcome::Blocked { reason }));
    }

    match git.refresh_default_branch(task.repository.path(), &task.default_branch)? {
        GitRefreshOutcome::Blocked(GitUpdateBlock::DetachedHead) => Ok(Entry::new(
            task.repository,
            Outcome::Blocked { reason: BlockedReason::DetachedHead },
        )),
        GitRefreshOutcome::Blocked(GitUpdateBlock::DirtyWorkingTree) => Ok(Entry::new(
            task.repository,
            Outcome::Skipped { reason: SkippedReason::DirtyWorkingTree },
        )),
        GitRefreshOutcome::Failed(message) => Ok(Entry::new(
            task.repository,
            Outcome::Blocked { reason: BlockedReason::UpdateFailed(message) },
        )),
        GitRefreshOutcome::Completed { update, previous_branch } if update.changed() => {
            Ok(Entry::new(
                task.repository,
                Outcome::Refreshed {
                    branch: task.default_branch.clone(),
                    before: update.before().to_string(),
                    after: update.after().to_string(),
                    previous_branch,
                },
            ))
        }
        GitRefreshOutcome::Completed { previous_branch: Some(previous_branch), .. } => {
            Ok(Entry::new(
                task.repository,
                Outcome::Switched { branch: task.default_branch.clone(), previous_branch },
            ))
        }
        GitRefreshOutcome::Completed { previous_branch: None, .. } => Ok(Entry::new(
            task.repository,
            Outcome::Current { branch: task.default_branch.clone() },
        )),
    }
}
