use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Output;

use super::command::{command_message, format_probe};
use super::{
    BranchTracking, CommandGitClient, GitProgressSink, RepositoryLock, RepositoryProbe,
    WorktreeStatus, default_branch, tracking, worktree,
};
use crate::AppError;
use crate::repositories::{BranchName, RemoteUrl, redact_urls_for_display};

impl RepositoryProbe for CommandGitClient {
    fn verify_available(&self) -> Result<(), AppError> {
        let mut command = self.command();
        command.arg("--version");
        let output = command.output().map_err(AppError::git_unavailable_source)?;
        if !output.status.success() {
            return Err(AppError::git_unavailable_status(
                "git --version",
                redact_urls_for_display(&command_message(&output)),
                output.status.code(),
            ));
        }

        let version = parse_git_version(&stdout(&output))
            .ok_or_else(|| AppError::git_unavailable("could not parse `git --version` output"))?;
        if version < (2, 23, 0) {
            return Err(AppError::git_unavailable(format!(
                "Git 2.23.0 or newer is required; found {}.{}.{}",
                version.0, version.1, version.2
            )));
        }
        Ok(())
    }

    fn lock_repository(&self, common_directory: &Path) -> Result<RepositoryLock, AppError> {
        RepositoryLock::acquire(common_directory)
    }

    fn fetch(&self, repository: &Path, progress: &mut dyn GitProgressSink) -> Result<(), AppError> {
        self.git_progress_required(
            repository,
            &["fetch", "--progress", "origin", "--prune"],
            progress,
        )
    }

    fn common_directory(&self, repository: &Path) -> Result<PathBuf, AppError> {
        let args = ["rev-parse", "--git-common-dir"];
        let output = self.git_required(repository, &args)?;
        let value = stdout(&output);
        let value = value.trim();
        if value.is_empty() {
            return Err(AppError::git_command_failed(
                format_probe(repository, &args),
                "Git returned an empty common directory",
            ));
        }

        let path = PathBuf::from(value);
        let path = if path.is_absolute() { path } else { repository.join(path) };
        fs::canonicalize(&path).map_err(|err| {
            io::Error::new(
                err.kind(),
                format!("failed to resolve Git common directory '{}': {err}", path.display()),
            )
            .into()
        })
    }

    fn is_work_tree(&self, repository: &Path) -> Result<bool, AppError> {
        let args = ["rev-parse", "--is-inside-work-tree"];
        let output = self.git_probe(repository, &args)?;
        if !output.status.success() {
            if output.status.code() == Some(128)
                && command_message(&output).contains("not a git repository")
            {
                return Ok(false);
            }
            return Err(probe_failure(repository, &args, &output));
        }
        match stdout(&output).trim() {
            "true" => Ok(true),
            "false" => Ok(false),
            value => Err(malformed_output(repository, &args, value)),
        }
    }

    fn worktree_status(&self, repository: &Path) -> Result<Option<WorktreeStatus>, AppError> {
        let args = ["status", "--porcelain=v2", "--branch", "--no-ahead-behind"];
        let output = self.git_probe(repository, &args)?;
        if !output.status.success() {
            let message = command_message(&output);
            if output.status.code() == Some(128)
                && (message.contains("not a git repository")
                    || message.contains("this operation must be run in a work tree"))
            {
                return Ok(None);
            }
            return Err(probe_failure(repository, &args, &output));
        }

        let output_text = stdout(&output);
        let parsed = worktree::parse(&output_text)
            .ok_or_else(|| malformed_output(repository, &args, &output_text))?;
        let branch = match parsed.head() {
            worktree::WorktreeHead::Branch(branch) => Some(branch.clone()),
            worktree::WorktreeHead::DetachedMarker => self.symbolic_head_branch(repository)?,
        };
        Ok(Some(WorktreeStatus::new(branch, parsed.is_clean())))
    }

    fn remote_url(&self, repository: &Path) -> Result<Option<RemoteUrl>, AppError> {
        let args = ["config", "--get", "remote.origin.url"];
        let output = self.git_probe(repository, &args)?;
        match optional_probe(repository, &args, output, 1)? {
            Some(output) => {
                let value = stdout(&output).trim().to_string();
                if value.is_empty() {
                    Err(malformed_output(repository, &args, "empty output"))
                } else {
                    Ok(Some(RemoteUrl::from_git(value)))
                }
            }
            None => Ok(None),
        }
    }

    fn default_branch(
        &self,
        repository: &Path,
        configured: Option<&BranchName>,
    ) -> Result<Option<String>, AppError> {
        if let Some(configured) = configured {
            return Ok(Some(configured.as_str().to_string()));
        }

        let args = ["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"];
        let output = self.git_probe(repository, &args)?;
        let Some(output) = optional_probe(repository, &args, output, 1)? else {
            return Ok(None);
        };
        let value = required_line(repository, &args, &output)?;
        let Some(parsed) = default_branch::parse_origin_head(&value) else {
            return Err(malformed_output(repository, &args, &value));
        };
        if BranchName::new(&parsed).is_err() {
            return Err(malformed_output(repository, &args, &value));
        }
        Ok(Some(parsed))
    }

    fn branch_tracking(
        &self,
        repository: &Path,
        branch: &BranchName,
    ) -> Result<BranchTracking, AppError> {
        let revisions = self.branch_revisions(repository, branch)?;
        if revisions.local().is_none() {
            return Ok(BranchTracking::MissingLocal);
        }
        if revisions.remote().is_none() {
            return Ok(BranchTracking::MissingRemote);
        }
        let (ahead, behind) = self.divergence_counts(repository, branch)?;
        Ok(BranchTracking::Divergence { ahead, behind })
    }
}

impl CommandGitClient {
    fn symbolic_head_branch(&self, repository: &Path) -> Result<Option<String>, AppError> {
        let args = ["symbolic-ref", "--quiet", "--short", "HEAD"];
        let output = self.git_probe(repository, &args)?;
        match optional_probe(repository, &args, output, 1)? {
            Some(output) => {
                let value = required_line(repository, &args, &output)?;
                BranchName::new(&value).map_err(|_| malformed_output(repository, &args, &value))?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    pub(super) fn branch_revisions(
        &self,
        repository: &Path,
        branch: &BranchName,
    ) -> Result<tracking::BranchRevisions, AppError> {
        let format = "--format=%(refname)%09%(objectname:short)";
        let local = format!("refs/heads/{branch}");
        let remote = format!("refs/remotes/origin/{branch}");
        let args = ["for-each-ref", format, &local, &remote];
        let output = self.git_required(repository, &args)?;
        let output_text = stdout(&output);
        tracking::parse(&output_text, branch)
            .ok_or_else(|| malformed_output(repository, &args, &output_text))
    }

    pub(super) fn divergence_counts(
        &self,
        repository: &Path,
        branch: &BranchName,
    ) -> Result<(u32, u32), AppError> {
        let range = format!("{branch}...origin/{branch}");
        let args = ["rev-list", "--left-right", "--count", &range];
        let output = self.git_required(repository, &args)?;
        let output_text = stdout(&output);
        let mut parts = output_text.split_whitespace();
        let Some((ahead, behind)) = parts.next().zip(parts.next()) else {
            return Err(malformed_output(repository, &args, &output_text));
        };
        if parts.next().is_some() {
            return Err(malformed_output(repository, &args, &output_text));
        }
        let ahead =
            ahead.parse::<u32>().map_err(|_| malformed_output(repository, &args, &output_text))?;
        let behind =
            behind.parse::<u32>().map_err(|_| malformed_output(repository, &args, &output_text))?;
        Ok((ahead, behind))
    }
}

fn optional_probe(
    repository: &Path,
    args: &[&str],
    output: Output,
    absent_status: i32,
) -> Result<Option<Output>, AppError> {
    if output.status.success() {
        Ok(Some(output))
    } else if output.status.code() == Some(absent_status) {
        Ok(None)
    } else {
        Err(probe_failure(repository, args, &output))
    }
}

fn required_line(repository: &Path, args: &[&str], output: &Output) -> Result<String, AppError> {
    let value = stdout(output);
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.split_whitespace().count() != 1 {
        return Err(malformed_output(repository, args, &value));
    }
    Ok(trimmed.to_string())
}

fn probe_failure(repository: &Path, args: &[&str], output: &Output) -> AppError {
    AppError::git_command_failed_status(
        format_probe(repository, args),
        redact_urls_for_display(&command_message(output)),
        output.status.code(),
    )
}

fn malformed_output(repository: &Path, args: &[&str], output: &str) -> AppError {
    let description = if output.trim().is_empty() {
        "Git returned empty output".to_string()
    } else {
        "Git returned malformed output".to_string()
    };
    AppError::git_command_failed(format_probe(repository, args), description)
}

pub(super) fn parse_git_version(output: &str) -> Option<(u32, u32, u32)> {
    let value = output.trim().strip_prefix("git version ")?.split_whitespace().next()?;
    let mut parts = value.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().and_then(|part| {
        let digits =
            part.chars().take_while(|character| character.is_ascii_digit()).collect::<String>();
        digits.parse().ok()
    })?;
    Some((major, minor, patch))
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::super::command::tests::{
        create_updatable_repository, git_wrapper, initialize_committed_repository, run_git,
    };
    use crate::git::{BranchTracking, CommandGitClient, RepositoryProbe};
    use crate::repositories::BranchName;

    #[test]
    fn linked_worktrees_resolve_to_the_same_common_directory() {
        let root = TempDir::new().unwrap();
        let main = root.path().join("main");
        let linked = root.path().join("linked");

        run_git(root.path(), &["init", "-b", "main", main.to_str().unwrap()]);
        std::fs::write(main.join("README.md"), "initial\n").unwrap();
        run_git(&main, &["add", "README.md"]);
        run_git(
            &main,
            &[
                "-c",
                "user.name=Grove Test",
                "-c",
                "user.email=grove@example.com",
                "commit",
                "-m",
                "initial",
            ],
        );
        run_git(&main, &["worktree", "add", "-b", "linked", linked.to_str().unwrap()]);

        let client = CommandGitClient::default();
        assert_eq!(
            client.common_directory(&main).unwrap(),
            client.common_directory(&linked).unwrap()
        );
    }

    #[test]
    fn worktree_status_returns_error_for_fatal_probe_failure() {
        let root = TempDir::new().unwrap();
        let repository = root.path().join("repo");
        run_git(root.path(), &["init", "-b", "main", repository.to_str().unwrap()]);
        std::fs::write(repository.join(".git").join("config"), "[bad\n").unwrap();

        let result = CommandGitClient::default().worktree_status(&repository);

        assert!(result.is_err_and(|err| {
            err.to_string().contains("git command failed") && err.to_string().contains("status")
        }));
    }

    #[test]
    fn remote_url_returns_error_for_fatal_probe_failure() {
        let root = TempDir::new().unwrap();
        let repository = root.path().join("repo");
        run_git(root.path(), &["init", "-b", "main", repository.to_str().unwrap()]);
        std::fs::write(repository.join(".git").join("config"), "[bad\n").unwrap();

        let result = CommandGitClient::default().remote_url(&repository);

        assert!(result.is_err_and(|err| {
            err.to_string().contains("git command failed") && err.to_string().contains("config")
        }));
    }

    #[test]
    fn expected_absence_is_distinct_from_fatal_probe_failure() {
        let root = TempDir::new().unwrap();
        let repository = root.path().join("repo");
        initialize_committed_repository(&repository);
        run_git(&repository, &["checkout", "--detach"]);
        let client = CommandGitClient::default();

        assert_eq!(client.worktree_status(&repository).unwrap().unwrap().branch(), None);
        assert_eq!(client.remote_url(&repository).unwrap(), None);
        assert_eq!(
            client.branch_tracking(&repository, &BranchName::new("missing").unwrap()).unwrap(),
            BranchTracking::MissingLocal
        );
    }

    #[test]
    fn worktree_status_distinguishes_detached_head_from_a_branch_named_detached() {
        let root = TempDir::new().unwrap();
        let repository = root.path().join("repo");
        initialize_committed_repository(&repository);
        let client = CommandGitClient::default();

        run_git(&repository, &["switch", "-c", "(detached)"]);
        let named = client.worktree_status(&repository).unwrap().unwrap();
        assert_eq!(named.branch(), Some("(detached)"));
        assert!(named.is_clean());

        run_git(&repository, &["checkout", "--detach"]);
        let detached = client.worktree_status(&repository).unwrap().unwrap();
        assert_eq!(detached.branch(), None);
    }

    #[test]
    fn worktree_status_reports_unborn_dirty_and_non_worktree_states() {
        let root = TempDir::new().unwrap();
        let repository = root.path().join("repo");
        run_git(root.path(), &["init", "-b", "main", repository.to_str().unwrap()]);
        let client = CommandGitClient::default();

        let unborn = client.worktree_status(&repository).unwrap().unwrap();
        assert_eq!(unborn.branch(), Some("main"));
        assert!(unborn.is_clean());

        std::fs::write(repository.join("untracked.txt"), "dirty\n").unwrap();
        assert!(!client.worktree_status(&repository).unwrap().unwrap().is_clean());
        assert_eq!(client.worktree_status(root.path()).unwrap(), None);
    }

    #[test]
    fn configured_default_branch_wins_without_probing_origin_head() {
        let root = TempDir::new().unwrap();
        let repository = root.path().join("repo");
        initialize_committed_repository(&repository);
        std::fs::write(repository.join(".git/config"), "[bad\n").unwrap();
        let configured = BranchName::new("release/stable").unwrap();

        let branch =
            CommandGitClient::default().default_branch(&repository, Some(&configured)).unwrap();

        assert_eq!(branch.as_deref(), Some("release/stable"));
    }

    #[cfg(unix)]
    #[test]
    fn divergence_rejects_partial_extra_and_nonnumeric_output() {
        let root = TempDir::new().unwrap();
        let repository = create_updatable_repository(root.path());
        for malformed in ["0", "0 1 extra", "zero 1"] {
            let wrapper = git_wrapper(
                root.path(),
                &format!("if [ \"$1\" = rev-list ]; then\n  echo '{malformed}'\n  exit 0\nfi"),
            );
            let result = CommandGitClient::with_executable(&wrapper)
                .branch_tracking(&repository, &BranchName::new("main").unwrap());
            assert!(result.is_err_and(|error| error.to_string().contains("malformed output")));
            std::fs::remove_file(wrapper).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn empty_reference_output_reports_missing_local() {
        let root = TempDir::new().unwrap();
        let repository = root.path().join("repo");
        initialize_committed_repository(&repository);
        let wrapper = git_wrapper(root.path(), "if [ \"$1\" = for-each-ref ]; then\n  exit 0\nfi");

        let result = CommandGitClient::with_executable(wrapper)
            .branch_tracking(&repository, &BranchName::new("main").unwrap());

        assert_eq!(result.unwrap(), BranchTracking::MissingLocal);
    }

    #[test]
    fn parses_supported_git_versions() {
        assert_eq!(super::parse_git_version("git version 2.23.0\n"), Some((2, 23, 0)));
        assert_eq!(
            super::parse_git_version("git version 2.39.5 (Apple Git-154)\n"),
            Some((2, 39, 5))
        );
        assert_eq!(super::parse_git_version("unexpected"), None);
    }
}
