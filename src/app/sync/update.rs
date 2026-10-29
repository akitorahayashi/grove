use std::path::{Path, PathBuf};

use crate::AppError;
use crate::git::{DefaultBranch, GitUpdateBlock, GitUpdateOutcome, Restoration};
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

pub(super) fn repository(git: &impl DefaultBranch, task: &Task<'_>) -> Result<Entry, AppError> {
    match update_repository(git, task) {
        Ok(entry) => Ok(entry),
        Err(err) => Ok(Entry::new(
            task.repository,
            Outcome::Blocked { reason: BlockedReason::UpdateFailed(err.demote()?) },
        )),
    }
}

fn update_repository(git: &impl DefaultBranch, task: &Task<'_>) -> Result<Entry, AppError> {
    let result = git.update_default_branch(task.repository.path(), &task.default_branch)?;
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Task, repository};
    use crate::AppError;
    use crate::app::sync::{BlockedReason, Outcome};
    use crate::git::{DefaultBranch, GitRefreshOutcome, GitUpdateOutcome};
    use crate::repositories::{BranchName, RemoteUrl, RepositoryDefinition, RepositoryName};

    struct FailingUpdate(fn() -> AppError);

    impl DefaultBranch for FailingUpdate {
        fn update_default_branch(
            &self,
            _repository: &Path,
            _branch: &BranchName,
        ) -> Result<GitUpdateOutcome, AppError> {
            Err((self.0)())
        }

        fn refresh_default_branch(
            &self,
            _repository: &Path,
            _branch: &BranchName,
        ) -> Result<GitRefreshOutcome, AppError> {
            unreachable!("sync never refreshes")
        }
    }

    fn definition() -> RepositoryDefinition {
        RepositoryDefinition::new(
            RepositoryName::new("blog").unwrap(),
            PathBuf::from("/workspace/blog"),
            "blog".to_string(),
            RemoteUrl::new("https://example.com/blog.git").unwrap(),
            None,
            PathBuf::from("/workspace/grove.toml"),
            PathBuf::from("/workspace"),
        )
    }

    #[test]
    fn a_git_failure_blocks_the_repository_but_an_internal_failure_aborts() {
        let definition = definition();
        let task = Task::new(
            0,
            &definition,
            PathBuf::from("/workspace/blog/.git"),
            BranchName::new("main").unwrap(),
        );

        let blocked = repository(
            &FailingUpdate(|| AppError::git_command_failed("git merge", "merge refused")),
            &task,
        )
        .unwrap();
        assert!(matches!(
            blocked.outcome(),
            Outcome::Blocked { reason: BlockedReason::UpdateFailed(message) }
                if message.contains("merge refused")
        ));

        let error =
            repository(&FailingUpdate(|| AppError::internal("lock poisoned")), &task).unwrap_err();
        assert!(error.is_internal());
    }
}
