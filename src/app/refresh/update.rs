use crate::AppError;
use crate::git::{DefaultBranch, GitRefreshOutcome, GitUpdateBlock};
use crate::phases::Task as PhaseTask;
use crate::repositories::BranchName;

use super::task::Task;
use super::{BlockedReason, Entry, Outcome, SkippedReason};

pub(super) fn repository(git: &impl DefaultBranch, task: &Task<'_>) -> Result<Entry, AppError> {
    match refresh_repository(git, task) {
        Ok(entry) => Ok(entry),
        Err(error) => Ok(Entry::new(
            task.repository(),
            Outcome::Blocked { reason: BlockedReason::UpdateFailed(error.demote()?) },
        )),
    }
}

fn refresh_repository(git: &impl DefaultBranch, task: &Task<'_>) -> Result<Entry, AppError> {
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Task, repository};
    use crate::AppError;
    use crate::app::refresh::{BlockedReason, Outcome};
    use crate::git::{DefaultBranch, GitRefreshOutcome, GitUpdateOutcome};
    use crate::repositories::{BranchName, RemoteUrl, RepositoryDefinition, RepositoryName};

    struct FailingRefresh(fn() -> AppError);

    impl DefaultBranch for FailingRefresh {
        fn update_default_branch(
            &self,
            _repository: &Path,
            _branch: &BranchName,
        ) -> Result<GitUpdateOutcome, AppError> {
            unreachable!("refresh never runs the restoring update")
        }

        fn refresh_default_branch(
            &self,
            _repository: &Path,
            _branch: &BranchName,
        ) -> Result<GitRefreshOutcome, AppError> {
            Err((self.0)())
        }
    }

    #[test]
    fn a_git_failure_blocks_the_repository_but_an_internal_failure_aborts() {
        let definition = RepositoryDefinition::new(
            RepositoryName::new("blog").unwrap(),
            PathBuf::from("/workspace/blog"),
            "blog".to_string(),
            RemoteUrl::new("https://example.com/blog.git").unwrap(),
            None,
            PathBuf::from("/workspace/grove.toml"),
            PathBuf::from("/workspace"),
        );
        let task = Task::new(
            0,
            &definition,
            PathBuf::from("/workspace/blog/.git"),
            BranchName::new("main").unwrap(),
        );

        let blocked = repository(
            &FailingRefresh(|| AppError::git_command_failed("git switch", "switch refused")),
            &task,
        )
        .unwrap();
        assert!(matches!(
            blocked.outcome(),
            Outcome::Blocked { reason: BlockedReason::UpdateFailed(message) }
                if message.contains("switch refused")
        ));

        let error =
            repository(&FailingRefresh(|| AppError::internal("lock poisoned")), &task).unwrap_err();
        assert!(error.is_internal());
    }
}
