use std::path::{Path, PathBuf};

use crate::AppError;
use crate::git::GitClient;
use crate::repositories::RepositoryDefinition;

use super::check::default_branch_block_reason;
use super::{BlockedReason, Entry, Outcome};

pub(super) struct Task<'a> {
    index: usize,
    repository: &'a RepositoryDefinition,
    common_directory: PathBuf,
    default_branch: String,
    current_branch: String,
}

impl<'a> Task<'a> {
    pub(super) fn new(
        index: usize,
        repository: &'a RepositoryDefinition,
        common_directory: PathBuf,
        default_branch: String,
        current_branch: String,
    ) -> Self {
        Self { index, repository, common_directory, default_branch, current_branch }
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
    match update_repository(git, task) {
        Ok(entry) => entry,
        Err(err) => Entry::new(
            task.repository,
            Outcome::Blocked { reason: BlockedReason::UpdateFailed(err.to_string()) },
        ),
    }
}

fn update_repository(git: &impl GitClient, task: &Task<'_>) -> Result<Entry, AppError> {
    if let Some(reason) = default_branch_block_reason(git, task.repository, &task.default_branch)? {
        return Ok(Entry::new(task.repository, Outcome::Blocked { reason }));
    }

    let Some(divergence) = git.branch_divergence(task.repository.path(), &task.default_branch)?
    else {
        return Ok(Entry::new(
            task.repository,
            Outcome::Blocked { reason: BlockedReason::CannotCompareDefaultBranch },
        ));
    };
    if divergence.ahead() > 0 && divergence.behind() > 0 {
        return Ok(Entry::new(
            task.repository,
            Outcome::Blocked {
                reason: BlockedReason::Diverged { branch: task.default_branch.clone() },
            },
        ));
    }
    if divergence.ahead() > 0 {
        return Ok(Entry::new(
            task.repository,
            Outcome::Blocked {
                reason: BlockedReason::AheadOfOrigin { branch: task.default_branch.clone() },
            },
        ));
    }

    let update = git.update_default_branch(
        task.repository.path(),
        &task.default_branch,
        &task.current_branch,
    )?;

    if update.changed() {
        Ok(Entry::new(
            task.repository,
            Outcome::Updated {
                branch: task.default_branch.clone(),
                before: update.before().to_string(),
                after: update.after().to_string(),
            },
        ))
    } else {
        Ok(Entry::new(task.repository, Outcome::Current { branch: task.default_branch.clone() }))
    }
}
