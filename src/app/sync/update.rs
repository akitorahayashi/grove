use std::path::{Path, PathBuf};

use crate::AppError;
use crate::app::phases::PhaseTask;
use crate::git::{GitClient, GitUpdateBlock, GitUpdateOutcome, Restoration};
use crate::repositories::RepositoryDefinition;

use super::check::default_branch_block_reason;
use super::{BlockedReason, Entry, Outcome};

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
}

impl PhaseTask for Task<'_> {
    fn repository(&self) -> &RepositoryDefinition {
        self.repository
    }

    fn resource(&self) -> &Path {
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

    let divergence = git.branch_divergence(task.repository.path(), &task.default_branch)?;
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

    let result = git.update_default_branch(task.repository.path(), &task.default_branch)?;
    let (update, restoration) = match result {
        GitUpdateOutcome::Blocked(GitUpdateBlock::DetachedHead) => {
            return Ok(Entry::new(
                task.repository,
                Outcome::Blocked { reason: BlockedReason::DetachedHead },
            ));
        }
        GitUpdateOutcome::Blocked(GitUpdateBlock::DirtyWorkingTree) => {
            return Ok(Entry::new(
                task.repository,
                Outcome::Skipped { reason: super::SkippedReason::DirtyWorkingTree },
            ));
        }
        GitUpdateOutcome::Failed { primary, restoration } => {
            let message = restoration_message(primary, restoration);
            return Ok(Entry::new(
                task.repository,
                Outcome::Blocked { reason: BlockedReason::UpdateFailed(message) },
            ));
        }
        GitUpdateOutcome::Completed { update, restoration } => (update, restoration),
    };

    if update.changed() {
        if let Restoration::Failed(message) = restoration {
            return Ok(Entry::new(
                task.repository,
                Outcome::UpdatedButRestorationFailed {
                    branch: task.default_branch.clone(),
                    before: update.before().to_string(),
                    after: update.after().to_string(),
                    message,
                },
            ));
        }
        Ok(Entry::new(
            task.repository,
            Outcome::Updated {
                branch: task.default_branch.clone(),
                before: update.before().to_string(),
                after: update.after().to_string(),
            },
        ))
    } else {
        match restoration {
            Restoration::Failed(message) => Ok(Entry::new(
                task.repository,
                Outcome::Blocked {
                    reason: BlockedReason::UpdateFailed(format!(
                        "default branch was current, but restoring the original branch failed: {message}"
                    )),
                },
            )),
            Restoration::NotNeeded | Restoration::Restored => Ok(Entry::new(
                task.repository,
                Outcome::Current { branch: task.default_branch.clone() },
            )),
        }
    }
}

fn restoration_message(primary: String, restoration: Restoration) -> String {
    match restoration {
        Restoration::NotNeeded => primary,
        Restoration::Restored => format!("{primary}; restored the original branch"),
        Restoration::Failed(restoration) => {
            format!("{primary}; restoring the original branch also failed: {restoration}")
        }
    }
}
