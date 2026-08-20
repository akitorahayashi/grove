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
        branch: &BranchName,
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
        branch: &BranchName,
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
        branch: &BranchName,
    ) -> Result<Result<Preparation, GitUpdateBlock>, AppError> {
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

        let revisions = self.branch_revisions(repository, branch)?;
        let Some(before) = revisions.local() else {
            return Ok(Err(GitUpdateBlock::MissingLocalBranch));
        };
        let Some(after) = revisions.remote() else {
            return Ok(Err(GitUpdateBlock::MissingRemoteBranch));
        };
        let (ahead, behind) = self.divergence_counts(repository, branch)?;
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
        branch: &BranchName,
        current_branch: &str,
    ) -> Result<bool, AppError> {
        let switched = current_branch != branch.as_str();
        if switched {
            self.git_required(repository, &["switch", "--", branch.as_str()])?;
        }
        Ok(switched)
    }

    fn fast_forward_default_branch(
        &self,
        repository: &Path,
        branch: &BranchName,
    ) -> Result<(), AppError> {
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

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use tempfile::TempDir;

    use super::super::command::tests::{
        commit, create_updatable_repository, git_stdout, git_wrapper,
        initialize_committed_repository, run_git,
    };
    use crate::git::{
        CommandGitClient, DefaultBranch, GitRefreshOutcome, GitUpdateBlock, GitUpdateOutcome,
        Restoration,
    };
    use crate::repositories::BranchName;

    fn update(client: &CommandGitClient, repository: &Path) -> GitUpdateOutcome {
        client.update_default_branch(repository, &BranchName::new("main").unwrap()).unwrap()
    }

    fn refresh(client: &CommandGitClient, repository: &Path) -> GitRefreshOutcome {
        client.refresh_default_branch(repository, &BranchName::new("main").unwrap()).unwrap()
    }

    #[test]
    fn update_rechecks_detached_and_dirty_preconditions() {
        let root = TempDir::new().unwrap();
        let repository = root.path().join("repo");
        initialize_committed_repository(&repository);
        let client = CommandGitClient::default();

        std::fs::write(repository.join("dirty.txt"), "dirty\n").unwrap();
        assert_eq!(
            update(&client, &repository),
            GitUpdateOutcome::Blocked(GitUpdateBlock::DirtyWorkingTree)
        );
        std::fs::remove_file(repository.join("dirty.txt")).unwrap();
        run_git(&repository, &["checkout", "--detach"]);
        assert_eq!(
            update(&client, &repository),
            GitUpdateOutcome::Blocked(GitUpdateBlock::DetachedHead)
        );
    }

    #[test]
    fn update_blocks_a_missing_remote_reference() {
        let root = TempDir::new().unwrap();
        let repository = root.path().join("repo");
        initialize_committed_repository(&repository);

        let result = update(&CommandGitClient::default(), &repository);

        assert_eq!(result, GitUpdateOutcome::Blocked(GitUpdateBlock::MissingRemoteBranch));
    }

    #[test]
    fn update_from_feature_branch_fast_forwards_and_restores_feature() {
        let root = TempDir::new().unwrap();
        let repository = create_updatable_repository(root.path());

        let outcome = update(&CommandGitClient::default(), &repository);

        assert!(matches!(
            outcome,
            GitUpdateOutcome::Completed { ref update, restoration: Restoration::Restored }
                if update.changed()
        ));
        assert_eq!(git_stdout(&repository, &["branch", "--show-current"]), "feature");
        assert_eq!(
            git_stdout(&repository, &["rev-parse", "main"]),
            git_stdout(&repository, &["rev-parse", "origin/main"])
        );
    }

    #[test]
    fn refresh_rechecks_detached_and_dirty_preconditions() {
        let root = TempDir::new().unwrap();
        let repository = root.path().join("repo");
        initialize_committed_repository(&repository);
        let client = CommandGitClient::default();

        std::fs::write(repository.join("dirty.txt"), "dirty\n").unwrap();
        assert_eq!(
            refresh(&client, &repository),
            GitRefreshOutcome::Blocked(GitUpdateBlock::DirtyWorkingTree)
        );
        std::fs::remove_file(repository.join("dirty.txt")).unwrap();
        run_git(&repository, &["checkout", "--detach"]);
        assert_eq!(
            refresh(&client, &repository),
            GitRefreshOutcome::Blocked(GitUpdateBlock::DetachedHead)
        );
    }

    #[test]
    fn refresh_from_feature_branch_fast_forwards_and_stays_on_default_branch() {
        let root = TempDir::new().unwrap();
        let repository = create_updatable_repository(root.path());

        let outcome = refresh(&CommandGitClient::default(), &repository);

        assert!(matches!(
            outcome,
            GitRefreshOutcome::Completed {
                ref update,
                previous_branch: Some(ref branch),
            } if update.changed() && branch == "feature"
        ));
        assert_eq!(git_stdout(&repository, &["branch", "--show-current"]), "main");
        assert_eq!(
            git_stdout(&repository, &["rev-parse", "main"]),
            git_stdout(&repository, &["rev-parse", "origin/main"])
        );
        assert!(git_stdout(&repository, &["branch", "--list", "feature"]).contains("feature"));
    }

    #[cfg(unix)]
    #[test]
    fn refresh_merge_failure_stays_on_default_branch() {
        let root = TempDir::new().unwrap();
        let repository = create_updatable_repository(root.path());
        let wrapper = git_wrapper(
            root.path(),
            "if [ \"$1\" = merge ]; then echo merge-failed >&2; exit 42; fi",
        );

        let outcome = refresh(&CommandGitClient::with_executable(wrapper), &repository);

        assert!(matches!(
            outcome,
            GitRefreshOutcome::Failed {
                ref message,
                previous_branch: Some(ref branch),
            } if message.contains("merge-failed") && branch == "feature"
        ));
        assert_eq!(git_stdout(&repository, &["branch", "--show-current"]), "main");
    }

    #[test]
    fn update_blocks_a_diverged_branch_before_switching() {
        let root = TempDir::new().unwrap();
        let repository = create_updatable_repository(root.path());
        run_git(&repository, &["switch", "main"]);
        std::fs::write(repository.join("local.txt"), "local\n").unwrap();
        run_git(&repository, &["add", "local.txt"]);
        commit(&repository, "local");
        let before = git_stdout(&repository, &["rev-parse", "main"]);
        run_git(&repository, &["switch", "feature"]);

        let outcome = update(&CommandGitClient::default(), &repository);

        assert_eq!(outcome, GitUpdateOutcome::Blocked(GitUpdateBlock::Diverged));
        assert_eq!(git_stdout(&repository, &["branch", "--show-current"]), "feature");
        assert_eq!(git_stdout(&repository, &["rev-parse", "main"]), before);
    }

    #[cfg(unix)]
    #[test]
    fn completed_fast_forward_reports_restoration_failure() {
        use std::os::unix::fs::PermissionsExt;

        let root = TempDir::new().unwrap();
        let repository = create_updatable_repository(root.path());
        let output = Command::new("sh").args(["-c", "command -v git"]).output().unwrap();
        assert!(output.status.success());
        let real_git = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let wrapper = root.path().join("git-wrapper");
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\nif [ \"$1\" = switch ] && [ \"${{3:-}}\" = feature ]; then\n  echo restoration-failed >&2\n  exit 42\nfi\nexec \"{}\" \"$@\"\n",
                real_git
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&wrapper).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&wrapper, permissions).unwrap();

        let outcome = update(&CommandGitClient::with_executable(&wrapper), &repository);

        assert!(matches!(
            outcome,
            GitUpdateOutcome::Completed {
                ref update,
                restoration: Restoration::Failed(ref message),
            } if update.changed() && message.contains("restoration-failed")
        ));
        assert_eq!(git_stdout(&repository, &["branch", "--show-current"]), "main");
        assert_eq!(
            git_stdout(&repository, &["rev-parse", "main"]),
            git_stdout(&repository, &["rev-parse", "origin/main"])
        );
    }

    #[cfg(unix)]
    #[test]
    fn completed_fast_forward_does_not_require_a_post_merge_revision_probe() {
        let root = TempDir::new().unwrap();
        let repository = create_updatable_repository(root.path());
        let wrapper = git_wrapper(
            root.path(),
            "if [ \"$1\" = for-each-ref ] && [ \"$(git rev-parse main)\" = \"$(git rev-parse origin/main)\" ]; then\n  echo post-merge-probe-failed >&2\n  exit 42\nfi",
        );

        let outcome = update(&CommandGitClient::with_executable(wrapper), &repository);

        assert!(matches!(
            outcome,
            GitUpdateOutcome::Completed { ref update, restoration: Restoration::Restored }
                if update.changed()
        ));
        assert_eq!(git_stdout(&repository, &["branch", "--show-current"]), "feature");
        assert_eq!(
            git_stdout(&repository, &["rev-parse", "main"]),
            git_stdout(&repository, &["rev-parse", "origin/main"])
        );
    }
}
