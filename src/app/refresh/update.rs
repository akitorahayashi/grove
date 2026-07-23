use crate::AppError;
use crate::git::{GitClient, GitRefreshOutcome, GitUpdateBlock};
use crate::phases::Task as PhaseTask;

use super::check::refresh_block_reason;
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
    if let Some(reason) = refresh_block_reason(git, repository, default_branch)? {
        return Ok(Entry::new(repository, Outcome::Blocked { reason }));
    }

    match git.refresh_default_branch(repository.path(), default_branch)? {
        GitRefreshOutcome::Blocked(GitUpdateBlock::DetachedHead) => {
            Ok(Entry::new(repository, Outcome::Blocked { reason: BlockedReason::DetachedHead }))
        }
        GitRefreshOutcome::Blocked(GitUpdateBlock::DirtyWorkingTree) => {
            Ok(Entry::new(repository, Outcome::Skipped { reason: SkippedReason::DirtyWorkingTree }))
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
