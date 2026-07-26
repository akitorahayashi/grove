use std::path::Path;

use super::command::format_probe;
use super::{
    CommandGitClient, DefaultBranch, GitRefreshOutcome, GitUpdate, GitUpdateBlock,
    GitUpdateOutcome, RepositoryProbe, Restoration,
};
use crate::AppError;
use crate::repositories::BranchName;

impl DefaultBranch for CommandGitClient {
    fn update_default_branch(
        &self,
        repository: &Path,
        branch: &str,
    ) -> Result<GitUpdateOutcome, AppError> {
        let common_directory = self.common_directory(repository)?;
        let _lock = self.lock_repository(&common_directory)?;
        let preparation = match self.prepare_default_branch(repository, branch)? {
            Ok(preparation) => preparation,
            Err(block) => return Ok(GitUpdateOutcome::Blocked(block)),
        };
        let switched =
            self.switch_default_branch(repository, branch, &preparation.current_branch)?;

        if let Err(primary) = self.fast_forward_default_branch(repository, branch) {
            return Ok(GitUpdateOutcome::Failed {
                primary: primary.to_string(),
                restoration: self.restore(repository, switched, &preparation.current_branch),
            });
        }

        Ok(GitUpdateOutcome::Completed {
            update: preparation.update,
            restoration: self.restore(repository, switched, &preparation.current_branch),
        })
    }

    fn refresh_default_branch(
        &self,
        repository: &Path,
        branch: &str,
    ) -> Result<GitRefreshOutcome, AppError> {
        let common_directory = self.common_directory(repository)?;
        let _lock = self.lock_repository(&common_directory)?;
        let preparation = match self.prepare_default_branch(repository, branch)? {
            Ok(preparation) => preparation,
            Err(block) => return Ok(GitRefreshOutcome::Blocked(block)),
        };
        let switched =
            match self.switch_default_branch(repository, branch, &preparation.current_branch) {
                Ok(switched) => switched,
                Err(error) => {
                    return Ok(GitRefreshOutcome::Failed {
                        message: error.to_string(),
                        previous_branch: None,
                    });
                }
            };

        if let Err(error) = self.fast_forward_default_branch(repository, branch) {
            return Ok(GitRefreshOutcome::Failed {
                message: error.to_string(),
                previous_branch: switched.then_some(preparation.current_branch),
            });
        }

        Ok(GitRefreshOutcome::Completed {
            update: preparation.update,
            previous_branch: switched.then_some(preparation.current_branch),
        })
    }
}

struct Preparation {
    current_branch: String,
    update: GitUpdate,
}

impl CommandGitClient {
    fn prepare_default_branch(
        &self,
        repository: &Path,
        branch: &str,
    ) -> Result<Result<Preparation, GitUpdateBlock>, AppError> {
        let branch = BranchName::new(branch)?;
        let status_args = ["status", "--porcelain=v2", "--branch", "--no-ahead-behind"];
        let status = self.worktree_status(repository)?.ok_or_else(|| {
            AppError::git_command_failed(
                format_probe(repository, &status_args),
                "destination is not a Git work tree",
            )
        })?;
        let Some(current_branch) = status.branch().map(str::to_string) else {
            return Ok(Err(GitUpdateBlock::DetachedHead));
        };
        if !status.is_clean() {
            return Ok(Err(GitUpdateBlock::DirtyWorkingTree));
        }

        let revisions = self.branch_revisions(repository, &branch)?;
        let Some(before) = revisions.local() else {
            return Ok(Err(GitUpdateBlock::MissingLocalBranch));
        };
        let Some(after) = revisions.remote() else {
            return Ok(Err(GitUpdateBlock::MissingRemoteBranch));
        };
        let (ahead, behind) = self.divergence_counts(repository, &branch)?;
        if ahead > 0 && behind > 0 {
            return Ok(Err(GitUpdateBlock::Diverged));
        }
        if ahead > 0 {
            return Ok(Err(GitUpdateBlock::AheadOfOrigin));
        }
        Ok(Ok(Preparation {
            current_branch,
            update: GitUpdate::new(before.to_string(), after.to_string()),
        }))
    }

    fn switch_default_branch(
        &self,
        repository: &Path,
        branch: &str,
        current_branch: &str,
    ) -> Result<bool, AppError> {
        let switched = current_branch != branch;
        if switched {
            self.git_required(repository, &["switch", "--", branch])?;
        }
        Ok(switched)
    }

    fn fast_forward_default_branch(&self, repository: &Path, branch: &str) -> Result<(), AppError> {
        let merge_target = format!("origin/{branch}");
        self.git_required(repository, &["merge", "--ff-only", "--", &merge_target])?;
        Ok(())
    }

    fn restore(&self, repository: &Path, switched: bool, branch: &str) -> Restoration {
        if !switched {
            return Restoration::NotNeeded;
        }

        match self.git_required(repository, &["switch", "--", branch]) {
            Ok(_) => Restoration::Restored,
            Err(err) => Restoration::Failed(err.to_string()),
        }
    }
}
