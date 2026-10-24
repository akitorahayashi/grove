use std::path::{Path, PathBuf};

use super::probe::{malformed_output, optional_probe, stdout};
use super::{CommandGitClient, Removal, RepositoryProbe};
use crate::AppError;
use crate::repositories::RemoteUrl;

impl Removal for CommandGitClient {
    fn is_linked_worktree(&self, repository: &Path) -> Result<bool, AppError> {
        let args = ["rev-parse", "--absolute-git-dir"];
        let output = self.git_required(repository, &args)?;
        let value = stdout(&output);
        let git_directory = PathBuf::from(value.trim());
        if !git_directory.is_absolute() {
            return Err(malformed_output(repository, &args, &value));
        }
        Ok(git_directory.canonicalize()? != self.common_directory(repository)?)
    }

    fn has_linked_worktrees(&self, repository: &Path) -> Result<bool, AppError> {
        let output = self.git_required(repository, &["worktree", "list", "--porcelain"])?;
        let worktrees =
            stdout(&output).lines().filter(|line| line.starts_with("worktree ")).count();
        Ok(worktrees > 1)
    }

    fn has_stash(&self, repository: &Path) -> Result<bool, AppError> {
        let args = ["rev-parse", "--verify", "--quiet", "refs/stash"];
        let output = self.git_probe(repository, &args)?;
        Ok(optional_probe(repository, &args, output, 1)?.is_some())
    }

    fn has_unpushed_commits(&self, repository: &Path) -> Result<bool, AppError> {
        let head_args = ["rev-parse", "--verify", "--quiet", "HEAD"];
        let head = self.git_probe(repository, &head_args)?;
        let born = optional_probe(repository, &head_args, head, 1)?.is_some();

        let mut args = vec!["rev-list", "--max-count=1"];
        if born {
            args.push("HEAD");
        }
        args.extend(["--branches", "--not", "--remotes"]);
        let output = self.git_required(repository, &args)?;
        Ok(!stdout(&output).trim().is_empty())
    }

    fn set_origin_url(&self, repository: &Path, url: &RemoteUrl) -> Result<(), AppError> {
        CommandGitClient::set_origin_url(self, repository, url)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::TempDir;

    use super::super::command::tests::{commit, initialize_committed_repository, run_git};
    use crate::git::{CommandGitClient, Removal};

    fn clone_of(root: &Path) -> std::path::PathBuf {
        let origin = root.join("origin");
        initialize_committed_repository(&origin);
        let clone = root.join("clone");
        run_git(root, &["clone", "--quiet", origin.to_str().unwrap(), "clone"]);
        clone
    }

    #[test]
    fn a_fresh_clone_has_no_local_only_state() {
        let root = TempDir::new().unwrap();
        let clone = clone_of(root.path());
        let git = CommandGitClient::default();

        assert!(!git.is_linked_worktree(&clone).unwrap());
        assert!(!git.has_linked_worktrees(&clone).unwrap());
        assert!(!git.has_stash(&clone).unwrap());
        assert!(!git.has_unpushed_commits(&clone).unwrap());
    }

    #[test]
    fn detects_linked_worktrees_from_either_side() {
        let root = TempDir::new().unwrap();
        let clone = clone_of(root.path());
        run_git(&clone, &["worktree", "add", "--quiet", "-b", "side", "../linked"]);
        let git = CommandGitClient::default();

        assert!(git.has_linked_worktrees(&clone).unwrap());
        assert!(!git.is_linked_worktree(&clone).unwrap());
        assert!(git.is_linked_worktree(&root.path().join("linked")).unwrap());
    }

    #[test]
    fn detects_a_stash() {
        let root = TempDir::new().unwrap();
        let clone = clone_of(root.path());
        std::fs::write(clone.join("README.md"), "changed\n").unwrap();
        run_git(
            &clone,
            &[
                "-c",
                "user.name=Grove Test",
                "-c",
                "user.email=grove@example.com",
                "stash",
                "--quiet",
            ],
        );

        assert!(CommandGitClient::default().has_stash(&clone).unwrap());
    }

    #[test]
    fn detects_commits_on_unpushed_branches_and_detached_heads() {
        let root = TempDir::new().unwrap();
        let clone = clone_of(root.path());
        let git = CommandGitClient::default();

        run_git(&clone, &["switch", "--quiet", "-c", "local"]);
        std::fs::write(clone.join("local.txt"), "local\n").unwrap();
        run_git(&clone, &["add", "local.txt"]);
        commit(&clone, "local");
        assert!(git.has_unpushed_commits(&clone).unwrap());

        run_git(&clone, &["switch", "--quiet", "--detach", "HEAD"]);
        run_git(&clone, &["branch", "--quiet", "-D", "local"]);
        assert!(git.has_unpushed_commits(&clone).unwrap());
    }

    #[test]
    fn an_unborn_repository_has_no_unpushed_commits() {
        let root = TempDir::new().unwrap();
        run_git(root.path(), &["init", "--quiet", "empty"]);

        assert!(
            !CommandGitClient::default().has_unpushed_commits(&root.path().join("empty")).unwrap()
        );
    }
}
