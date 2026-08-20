use std::path::{Path, PathBuf};

use crate::AppError;
use crate::git::{GitClient, GitUpdateBlock, GitUpdateOutcome, Restoration};
use crate::phases::Task as PhaseTask;
use crate::repositories::{BranchName, RepositoryDefinition};

use super::{BlockedReason, Entry, Outcome, SkippedReason};

pub(super) struct Task<'a> {
    index: usize,
    repository: &'a RepositoryDefinition,
    common_directory: PathBuf,
    default_branch: BranchName,
}

impl<'a> Task<'a> {
    pub(super) fn new(
        index: usize,
        repository: &'a RepositoryDefinition,
        common_directory: PathBuf,
        default_branch: BranchName,
    ) -> Self {
        Self { index, repository, common_directory, default_branch }
    }

    pub(super) fn index(&self) -> usize {
        self.index
    }

    pub(super) fn common_directory(&self) -> &Path {
        &self.common_directory
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
    let result = git.update_default_branch(
        task.repository.path(),
        &task.common_directory,
        &task.default_branch,
    )?;
    let (update, restoration) = match result {
        GitUpdateOutcome::Blocked(block) => {
            return Ok(Entry::new(task.repository, blocked_outcome(block, &task.default_branch)));
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
                    branch: task.default_branch.to_string(),
                    before: update.before().to_string(),
                    after: update.after().to_string(),
                    message,
                },
            ));
        }
        Ok(Entry::new(
            task.repository,
            Outcome::Updated {
                branch: task.default_branch.to_string(),
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
                Outcome::Current { branch: task.default_branch.to_string() },
            )),
        }
    }
}

fn blocked_outcome(block: GitUpdateBlock, default_branch: &BranchName) -> Outcome {
    let branch = default_branch.to_string();
    match block {
        GitUpdateBlock::DetachedHead => Outcome::Blocked { reason: BlockedReason::DetachedHead },
        GitUpdateBlock::DirtyWorkingTree => {
            Outcome::Skipped { reason: SkippedReason::DirtyWorkingTree }
        }
        GitUpdateBlock::MissingLocalBranch => {
            Outcome::Blocked { reason: BlockedReason::MissingLocalBranch { branch } }
        }
        GitUpdateBlock::MissingRemoteBranch => {
            Outcome::Blocked { reason: BlockedReason::MissingRemoteBranch { branch } }
        }
        GitUpdateBlock::Diverged => Outcome::Blocked { reason: BlockedReason::Diverged { branch } },
        GitUpdateBlock::AheadOfOrigin => {
            Outcome::Blocked { reason: BlockedReason::AheadOfOrigin { branch } }
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
