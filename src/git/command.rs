use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use super::client::BranchDivergence;
use super::{GitClient, GitUpdate, default_branch, working_tree};
use crate::AppError;

#[derive(Debug, Clone, Copy, Default)]
pub struct CommandGitClient;

impl GitClient for CommandGitClient {
    fn verify_available(&self) -> Result<(), AppError> {
        let mut command = Command::new("git");
        command.arg("--version");
        let output = command.output().map_err(|err| AppError::GitUnavailable(err.to_string()))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(AppError::GitUnavailable(command_message(&output)))
        }
    }

    fn clone_repository(&self, url: &str, destination: &Path) -> Result<(), AppError> {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut command = Command::new("git");
        command.arg("clone").arg(url).arg(destination);
        run_required(command, format!("git clone {url} {}", destination.display())).map(|_| ())
    }

    fn fetch(&self, repository: &Path) -> Result<(), AppError> {
        self.git_required(repository, &["fetch", "origin", "--prune"]).map(|_| ())
    }

    fn is_work_tree(&self, repository: &Path) -> Result<bool, AppError> {
        let output = self.git_probe(repository, &["rev-parse", "--is-inside-work-tree"])?;
        Ok(output.status.success() && stdout(&output).trim() == "true")
    }

    fn current_branch(&self, repository: &Path) -> Result<Option<String>, AppError> {
        let output = self.git_probe(repository, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        if output.status.success() {
            Ok(Some(stdout(&output).trim().to_string()))
        } else {
            Ok(None)
        }
    }

    fn working_tree_clean(&self, repository: &Path) -> Result<bool, AppError> {
        let output = self.git_required(repository, &["status", "--porcelain"])?;
        Ok(working_tree::status_is_clean(&stdout(&output)))
    }

    fn remote_url(&self, repository: &Path) -> Result<Option<String>, AppError> {
        let output = self.git_probe(repository, &["config", "--get", "remote.origin.url"])?;
        if output.status.success() {
            Ok(Some(stdout(&output).trim().to_string()))
        } else {
            Ok(None)
        }
    }

    fn default_branch(
        &self,
        repository: &Path,
        configured: Option<&str>,
    ) -> Result<Option<String>, AppError> {
        let output = self.git_probe(
            repository,
            &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"],
        )?;
        if output.status.success() {
            let parsed = default_branch::parse_origin_head(&stdout(&output));
            if parsed.is_some() {
                return Ok(parsed);
            }
        }

        Ok(configured.map(str::to_string))
    }

    fn local_branch_exists(&self, repository: &Path, branch: &str) -> Result<bool, AppError> {
        let reference = format!("refs/heads/{branch}");
        let output =
            self.git_probe(repository, &["show-ref", "--verify", "--quiet", &reference])?;
        Ok(output.status.success())
    }

    fn remote_branch_exists(&self, repository: &Path, branch: &str) -> Result<bool, AppError> {
        let reference = format!("refs/remotes/origin/{branch}");
        let output =
            self.git_probe(repository, &["show-ref", "--verify", "--quiet", &reference])?;
        Ok(output.status.success())
    }

    fn branch_divergence(
        &self,
        repository: &Path,
        branch: &str,
    ) -> Result<Option<BranchDivergence>, AppError> {
        if !self.local_branch_exists(repository, branch)?
            || !self.remote_branch_exists(repository, branch)?
        {
            return Ok(None);
        }

        let range = format!("{branch}...origin/{branch}");
        let output =
            self.git_required(repository, &["rev-list", "--left-right", "--count", &range])?;
        let stdout = stdout(&output);
        let mut parts = stdout.split_whitespace();
        let ahead = parts.next().and_then(|value| value.parse().ok()).unwrap_or(0);
        let behind = parts.next().and_then(|value| value.parse().ok()).unwrap_or(0);
        Ok(Some(BranchDivergence::new(ahead, behind)))
    }

    fn short_revision(&self, repository: &Path, reference: &str) -> Result<String, AppError> {
        let output = self.git_required(repository, &["rev-parse", "--short", reference])?;
        Ok(stdout(&output).trim().to_string())
    }

    fn update_default_branch(
        &self,
        repository: &Path,
        branch: &str,
        current_branch: &str,
    ) -> Result<GitUpdate, AppError> {
        let before = self.short_revision(repository, branch)?;
        let switched = current_branch != branch;

        if switched {
            self.git_required(repository, &["switch", branch])?;
        }

        let merge_result =
            self.git_required(repository, &["merge", "--ff-only", &format!("origin/{branch}")]);
        if let Err(err) = merge_result {
            if switched {
                let _ = self.git_required(repository, &["switch", current_branch]);
            }
            return Err(err);
        }

        let after = self.short_revision(repository, branch)?;
        if switched {
            self.git_required(repository, &["switch", current_branch])?;
        }

        Ok(GitUpdate::new(before, after))
    }
}

impl CommandGitClient {
    fn git_required(&self, repository: &Path, args: &[&str]) -> Result<Output, AppError> {
        let mut command = Command::new("git");
        command.current_dir(repository).args(args);
        let display = format!("git -C {} {}", repository.display(), args.join(" "));
        run_required(command, display)
    }

    fn git_probe(&self, repository: &Path, args: &[&str]) -> Result<Output, AppError> {
        let mut command = Command::new("git");
        command.current_dir(repository).args(args);
        command.output().map_err(|err| {
            AppError::git_command_failed(format_probe(repository, args), err.to_string())
        })
    }
}

fn run_required(mut command: Command, display: String) -> Result<Output, AppError> {
    let output = command
        .output()
        .map_err(|err| AppError::git_command_failed(display.clone(), err.to_string()))?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(AppError::git_command_failed(display, command_message(&output)))
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn command_message(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        stderr
    }
}

fn format_probe(repository: &Path, args: &[&str]) -> String {
    format!("git -C {} {}", repository.display(), join_args(args))
}

fn join_args(args: &[&str]) -> String {
    args.join(" ")
}
