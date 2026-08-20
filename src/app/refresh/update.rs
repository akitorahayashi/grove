use crate::AppError;
use crate::git::{GitClient, GitRefreshOutcome, GitUpdateBlock};
use crate::phases::Task as PhaseTask;
use crate::repositories::BranchName;

use super::task::Task;
use super::{BlockedReason, Entry, Outcome, SkippedReason};

pub(super) fn repository(git: &impl GitClient, task: &Task<'_>) -> Entry {
    match refresh_repository(git, task) {
        Ok(entry) => entry,
        Err(error) => Entry::new(
            task.repository(),
            Outcome::Blocked { reason: BlockedReason::UpdateFailed(error.to_string()) },
        ),
    }
}

fn refresh_repository(git: &impl GitClient, task: &Task<'_>) -> Result<Entry, AppError> {
    let repository = task.repository();
    let default_branch = task.default_branch();
    match git.refresh_default_branch(repository.path(), default_branch)? {
        GitRefreshOutcome::Blocked(block) => {
            Ok(Entry::new(repository, blocked_outcome(block, default_branch)))
        }
        GitRefreshOutcome::Failed { message, previous_branch: Some(previous_branch) } => {
            Ok(Entry::new(
                repository,
                Outcome::SwitchedAndBlocked {
                    branch: default_branch.to_string(),
                    previous_branch,
                    reason: BlockedReason::UpdateFailed(message),
                },
            ))
        }
        GitRefreshOutcome::Failed { message, previous_branch: None } => Ok(Entry::new(
            repository,
            Outcome::Blocked { reason: BlockedReason::UpdateFailed(message) },
        )),
        GitRefreshOutcome::Completed { update, previous_branch } if update.changed() => {
            Ok(Entry::new(
                repository,
                Outcome::Refreshed {
                    branch: default_branch.to_string(),
                    before: update.before().to_string(),
                    after: update.after().to_string(),
                    previous_branch,
                },
            ))
        }
        GitRefreshOutcome::Completed { previous_branch: Some(previous_branch), .. } => {
            Ok(Entry::new(
                repository,
                Outcome::Switched { branch: default_branch.to_string(), previous_branch },
            ))
        }
        GitRefreshOutcome::Completed { previous_branch: None, .. } => {
            Ok(Entry::new(repository, Outcome::Current { branch: default_branch.to_string() }))
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
