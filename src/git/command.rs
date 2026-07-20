use std::ffi::OsString;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use super::client::BranchDivergence;
use super::{
    GitClient, GitProgressParser, GitProgressSink, GitRefreshOutcome, GitUpdate, GitUpdateBlock,
    GitUpdateOutcome, Restoration, default_branch,
};
use crate::AppError;
use crate::repositories::redact_urls_for_display;
use crate::repositories::{BranchName, RemoteUrl, ResolutionError, resolve_operational_path};

#[derive(Debug, Clone)]
pub struct CommandGitClient {
    executable: OsString,
}

impl Default for CommandGitClient {
    fn default() -> Self {
        Self { executable: OsString::from("git") }
    }
}

impl GitClient for CommandGitClient {
    fn verify_available(&self) -> Result<(), AppError> {
        let mut command = self.command();
        command.arg("--version");
        let output = command.output().map_err(|err| AppError::GitUnavailable(err.to_string()))?;
        if !output.status.success() {
            return Err(AppError::GitUnavailable(redact_urls_for_display(&command_message(
                &output,
            ))));
        }

        let version = parse_git_version(&stdout(&output)).ok_or_else(|| {
            AppError::GitUnavailable("could not parse `git --version` output".to_string())
        })?;
        if version < (2, 23, 0) {
            return Err(AppError::GitUnavailable(format!(
                "Git 2.23.0 or newer is required; found {}.{}.{}",
                version.0, version.1, version.2
            )));
        }
        Ok(())
    }

    fn clone_repository(
        &self,
        url: &RemoteUrl,
        destination: &Path,
        grove_root: &Path,
        progress: &mut dyn GitProgressSink,
    ) -> Result<(), AppError> {
        match resolve_operational_path(destination, grove_root) {
            Ok(resolved) if resolved == destination => {}
            Ok(resolved) => {
                return Err(AppError::config_error(format!(
                    "clone destination changed after validation: '{}' resolves to '{}'",
                    destination.display(),
                    resolved.display()
                )));
            }
            Err(ResolutionError::OutsideRoot) => {
                return Err(AppError::config_error(format!(
                    "clone destination '{}' leaves the grove root",
                    destination.display()
                )));
            }
            Err(ResolutionError::Io(err)) => return Err(err.into()),
        }

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut command = self.command();
        command
            .arg("clone")
            .arg("--progress")
            .arg("--")
            .arg(url.as_process_argument())
            .arg(destination);
        run_with_progress(
            command,
            redact_urls_for_display(&format!(
                "git clone --progress -- {url} {}",
                destination.display()
            )),
            progress,
        )
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

    fn current_branch(&self, repository: &Path) -> Result<Option<String>, AppError> {
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

    fn working_tree_clean(&self, repository: &Path) -> Result<bool, AppError> {
        let output = self.git_required(repository, &["status", "--porcelain"])?;
        Ok(stdout(&output).trim().is_empty())
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

    fn local_branch_exists(&self, repository: &Path, branch: &str) -> Result<bool, AppError> {
        let reference = format!("refs/heads/{branch}");
        let args = ["show-ref", "--verify", "--quiet", &reference];
        let output = self.git_probe(repository, &args)?;
        Ok(optional_probe(repository, &args, output, 1)?.is_some())
    }

    fn remote_branch_exists(&self, repository: &Path, branch: &str) -> Result<bool, AppError> {
        let reference = format!("refs/remotes/origin/{branch}");
        let args = ["show-ref", "--verify", "--quiet", &reference];
        let output = self.git_probe(repository, &args)?;
        Ok(optional_probe(repository, &args, output, 1)?.is_some())
    }

    fn branch_divergence(
        &self,
        repository: &Path,
        branch: &str,
    ) -> Result<BranchDivergence, AppError> {
        let range = format!("{branch}...origin/{branch}");
        let output =
            self.git_required(repository, &["rev-list", "--left-right", "--count", &range])?;
        let stdout = stdout(&output);
        let mut parts = stdout.split_whitespace();
        let parsed = parts.next().zip(parts.next());
        let Some((ahead, behind)) = parsed else {
            return Err(malformed_output(
                repository,
                &["rev-list", "--left-right", "--count", &range],
                &stdout,
            ));
        };
        if parts.next().is_some() {
            return Err(malformed_output(
                repository,
                &["rev-list", "--left-right", "--count", &range],
                &stdout,
            ));
        }
        let ahead = ahead.parse::<u32>().map_err(|_| {
            malformed_output(repository, &["rev-list", "--left-right", "--count", &range], &stdout)
        })?;
        let behind = behind.parse::<u32>().map_err(|_| {
            malformed_output(repository, &["rev-list", "--left-right", "--count", &range], &stdout)
        })?;
        Ok(BranchDivergence::new(ahead, behind))
    }

    fn short_revision(&self, repository: &Path, reference: &str) -> Result<String, AppError> {
        let output = self.git_required(repository, &["rev-parse", "--short", reference])?;
        let value = required_line(repository, &["rev-parse", "--short", reference], &output)?;
        if !value.chars().all(|character| character.is_ascii_hexdigit()) {
            return Err(malformed_output(repository, &["rev-parse", "--short", reference], &value));
        }
        Ok(value)
    }

    fn update_default_branch(
        &self,
        repository: &Path,
        branch: &str,
    ) -> Result<GitUpdateOutcome, AppError> {
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

struct DefaultBranchPreparation {
    current_branch: String,
    update: GitUpdate,
}

impl CommandGitClient {
    #[cfg(test)]
    fn with_executable(executable: impl AsRef<std::ffi::OsStr>) -> Self {
        Self { executable: executable.as_ref().to_os_string() }
    }

    fn command(&self) -> Command {
        Command::new(&self.executable)
    }

    fn prepare_default_branch(
        &self,
        repository: &Path,
        branch: &str,
    ) -> Result<Result<DefaultBranchPreparation, GitUpdateBlock>, AppError> {
        let Some(current_branch) = self.current_branch(repository)? else {
            return Ok(Err(GitUpdateBlock::DetachedHead));
        };
        if !self.working_tree_clean(repository)? {
            return Ok(Err(GitUpdateBlock::DirtyWorkingTree));
        }

        let before = self.short_revision(repository, branch)?;
        let after = self.short_revision(repository, &format!("origin/{branch}"))?;
        Ok(Ok(DefaultBranchPreparation { current_branch, update: GitUpdate::new(before, after) }))
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

    fn git_required(&self, repository: &Path, args: &[&str]) -> Result<Output, AppError> {
        let mut command = self.command();
        command.current_dir(repository).args(args);
        let display =
            redact_urls_for_display(&format!("git -C {} {}", repository.display(), args.join(" ")));
        run_required(command, display)
    }

    fn git_progress_required(
        &self,
        repository: &Path,
        args: &[&str],
        progress: &mut dyn GitProgressSink,
    ) -> Result<(), AppError> {
        let mut command = self.command();
        command.current_dir(repository).args(args);
        let display =
            redact_urls_for_display(&format!("git -C {} {}", repository.display(), args.join(" ")));
        run_with_progress(command, display, progress)
    }

    fn git_probe(&self, repository: &Path, args: &[&str]) -> Result<Output, AppError> {
        let mut command = self.command();
        command.current_dir(repository).env("LC_ALL", "C").args(args);
        command.output().map_err(|err| {
            AppError::git_command_failed(format_probe(repository, args), err.to_string())
        })
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
    AppError::git_command_failed(
        format_probe(repository, args),
        redact_urls_for_display(&command_message(output)),
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

fn parse_git_version(output: &str) -> Option<(u32, u32, u32)> {
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

fn run_required(mut command: Command, display: String) -> Result<Output, AppError> {
    let output = command
        .output()
        .map_err(|err| AppError::git_command_failed(display.clone(), err.to_string()))?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(AppError::git_command_failed(
            display,
            redact_urls_for_display(&command_message(&output)),
        ))
    }
}

fn run_with_progress(
    mut command: Command,
    display: String,
    progress: &mut dyn GitProgressSink,
) -> Result<(), AppError> {
    command.stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|err| AppError::git_command_failed(display.clone(), err.to_string()))?;
    let mut stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let error = AppError::internal("Git progress stderr pipe was unavailable");
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let mut stderr_text = String::new();
    let mut buffer = [0; 4096];
    let mut pending = Vec::new();
    let mut processing_error = None;

    loop {
        let read = match stderr.read(&mut buffer) {
            Ok(read) => read,
            Err(err) => {
                if processing_error.is_none() {
                    processing_error =
                        Some(AppError::git_command_failed(display.clone(), err.to_string()));
                }
                break;
            }
        };
        if read == 0 {
            break;
        }
        stderr_text.push_str(&String::from_utf8_lossy(&buffer[..read]));
        for byte in &buffer[..read] {
            if *byte == b'\r' || *byte == b'\n' {
                if processing_error.is_none() {
                    processing_error = emit_progress(&pending, progress).err();
                }
                pending.clear();
            } else {
                pending.push(*byte);
            }
        }
    }
    if processing_error.is_none() {
        processing_error = emit_progress(&pending, progress).err();
    }

    drop(stderr);
    let status = child.wait().map_err(|err| {
        processing_error
            .take()
            .unwrap_or_else(|| AppError::git_command_failed(display.clone(), err.to_string()))
    })?;
    if let Some(error) = processing_error {
        return Err(error);
    }
    if status.success() {
        Ok(())
    } else {
        Err(AppError::git_command_failed(
            display,
            redact_urls_for_display(&progress_message(&stderr_text)),
        ))
    }
}

fn emit_progress(line: &[u8], progress: &mut dyn GitProgressSink) -> Result<(), AppError> {
    let line = String::from_utf8_lossy(line);
    if let Some(parsed) = GitProgressParser::parse(&line) {
        progress.progress(parsed)?;
    }
    Ok(())
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

fn progress_message(stderr: &str) -> String {
    let message = stderr
        .lines()
        .filter(|line| GitProgressParser::parse(line).is_none())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if message.is_empty() { stderr.trim().to_string() } else { message }
}

fn format_probe(repository: &Path, args: &[&str]) -> String {
    redact_urls_for_display(&format!("git -C {} {}", repository.display(), join_args(args)))
}

fn join_args(args: &[&str]) -> String {
    args.join(" ")
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use tempfile::TempDir;

    use crate::AppError;
    use crate::git::{
        CommandGitClient, GitClient, GitProgress, GitProgressSink, GitRefreshOutcome,
        GitUpdateBlock, GitUpdateOutcome, NoopGitProgressSink, Restoration,
    };
    use crate::repositories::{BranchName, RemoteUrl};

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
    fn current_branch_returns_error_for_fatal_probe_failure() {
        let root = TempDir::new().unwrap();
        let repository = root.path().join("repo");
        run_git(root.path(), &["init", "-b", "main", repository.to_str().unwrap()]);
        std::fs::write(repository.join(".git").join("config"), "[bad\n").unwrap();

        let result = CommandGitClient::default().current_branch(&repository);

        assert!(result.is_err_and(|err| {
            err.to_string().contains("git command failed")
                && err.to_string().contains("symbolic-ref")
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

        assert_eq!(client.current_branch(&repository).unwrap(), None);
        assert_eq!(client.remote_url(&repository).unwrap(), None);
        assert!(!client.local_branch_exists(&repository, "missing").unwrap());
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
            let result =
                CommandGitClient::with_executable(&wrapper).branch_divergence(&repository, "main");
            assert!(result.is_err_and(|error| error.to_string().contains("malformed output")));
            std::fs::remove_file(wrapper).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn short_revision_rejects_empty_output() {
        let root = TempDir::new().unwrap();
        let repository = root.path().join("repo");
        initialize_committed_repository(&repository);
        let wrapper = git_wrapper(root.path(), "if [ \"$1\" = rev-parse ]; then\n  exit 0\nfi");

        let result = CommandGitClient::with_executable(wrapper).short_revision(&repository, "main");

        assert!(result.is_err_and(|error| error.to_string().contains("empty output")));
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

    #[test]
    fn update_rechecks_detached_and_dirty_preconditions() {
        let root = TempDir::new().unwrap();
        let repository = root.path().join("repo");
        initialize_committed_repository(&repository);
        let client = CommandGitClient::default();

        std::fs::write(repository.join("dirty.txt"), "dirty\n").unwrap();
        assert_eq!(
            client.update_default_branch(&repository, "main").unwrap(),
            GitUpdateOutcome::Blocked(GitUpdateBlock::DirtyWorkingTree)
        );
        std::fs::remove_file(repository.join("dirty.txt")).unwrap();
        run_git(&repository, &["checkout", "--detach"]);
        assert_eq!(
            client.update_default_branch(&repository, "main").unwrap(),
            GitUpdateOutcome::Blocked(GitUpdateBlock::DetachedHead)
        );
    }

    #[test]
    fn update_from_feature_branch_fast_forwards_and_restores_feature() {
        let root = TempDir::new().unwrap();
        let repository = create_updatable_repository(root.path());

        let outcome =
            CommandGitClient::default().update_default_branch(&repository, "main").unwrap();

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
            client.refresh_default_branch(&repository, "main").unwrap(),
            GitRefreshOutcome::Blocked(GitUpdateBlock::DirtyWorkingTree)
        );
        std::fs::remove_file(repository.join("dirty.txt")).unwrap();
        run_git(&repository, &["checkout", "--detach"]);
        assert_eq!(
            client.refresh_default_branch(&repository, "main").unwrap(),
            GitRefreshOutcome::Blocked(GitUpdateBlock::DetachedHead)
        );
    }

    #[test]
    fn refresh_from_feature_branch_fast_forwards_and_stays_on_default_branch() {
        let root = TempDir::new().unwrap();
        let repository = create_updatable_repository(root.path());

        let outcome =
            CommandGitClient::default().refresh_default_branch(&repository, "main").unwrap();

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

        let outcome = CommandGitClient::with_executable(wrapper)
            .refresh_default_branch(&repository, "main")
            .unwrap();

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
    fn merge_failure_restores_original_branch_and_preserves_default_branch() {
        let root = TempDir::new().unwrap();
        let repository = create_updatable_repository(root.path());
        run_git(&repository, &["switch", "main"]);
        std::fs::write(repository.join("local.txt"), "local\n").unwrap();
        run_git(&repository, &["add", "local.txt"]);
        commit(&repository, "local");
        let before = git_stdout(&repository, &["rev-parse", "main"]);
        run_git(&repository, &["switch", "feature"]);

        let outcome =
            CommandGitClient::default().update_default_branch(&repository, "main").unwrap();

        assert!(matches!(
            outcome,
            GitUpdateOutcome::Failed { restoration: Restoration::Restored, .. }
        ));
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

        let outcome = CommandGitClient::with_executable(&wrapper)
            .update_default_branch(&repository, "main")
            .unwrap();

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
            "if [ \"$1\" = rev-parse ] && [ \"${3:-}\" = main ] && [ \"$(git rev-parse main)\" = \"$(git rev-parse origin/main)\" ]; then\n  echo post-merge-probe-failed >&2\n  exit 42\nfi",
        );

        let outcome = CommandGitClient::with_executable(wrapper)
            .update_default_branch(&repository, "main")
            .unwrap();

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

    #[cfg(unix)]
    #[test]
    fn progress_sink_failure_waits_for_git_child() {
        let root = TempDir::new().unwrap();
        let repository = root.path().join("repository");
        std::fs::create_dir(&repository).unwrap();
        let completed = root.path().join("completed");
        let wrapper = git_wrapper(
            root.path(),
            &format!(
                "if [ \"$1\" = fetch ]; then\n  printf 'Receiving objects: 50%% (1/2)\\r' >&2\n  sleep 0.2\n  touch \"{}\"\n  exit 0\nfi",
                completed.display()
            ),
        );
        let mut progress = FailingProgressSink;

        let result = CommandGitClient::with_executable(wrapper).fetch(&repository, &mut progress);

        assert!(result.is_err_and(|error| error.to_string().contains("progress sink failed")));
        assert!(completed.exists());
    }

    #[cfg(unix)]
    #[test]
    fn clone_passes_option_like_url_after_operand_terminator() {
        use std::os::unix::fs::PermissionsExt;

        let root = TempDir::new().unwrap();
        let log = root.path().join("args");
        let wrapper = root.path().join("git-wrapper");
        std::fs::write(
            &wrapper,
            format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\n", log.display()),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&wrapper).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&wrapper, permissions).unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let destination = workspace.join("repo");
        let url = RemoteUrl::new("--upload-pack=hostile").unwrap();

        CommandGitClient::with_executable(&wrapper)
            .clone_repository(&url, &destination, &workspace, &mut NoopGitProgressSink)
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(log).unwrap().lines().collect::<Vec<_>>(),
            ["clone", "--progress", "--", "--upload-pack=hostile", destination.to_str().unwrap()]
        );
    }

    fn create_updatable_repository(root: &Path) -> std::path::PathBuf {
        let remote = root.join("remote.git");
        let seed = root.join("seed");
        let repository = root.join("repository");
        run_git(root, &["init", "--bare", "--initial-branch=main", remote.to_str().unwrap()]);
        initialize_committed_repository(&seed);
        run_git(&seed, &["remote", "add", "origin", remote.to_str().unwrap()]);
        run_git(&seed, &["push", "-u", "origin", "main"]);
        run_git(root, &["clone", remote.to_str().unwrap(), repository.to_str().unwrap()]);
        run_git(&repository, &["switch", "-c", "feature"]);
        std::fs::write(seed.join("remote.txt"), "remote\n").unwrap();
        run_git(&seed, &["add", "remote.txt"]);
        commit(&seed, "remote");
        run_git(&seed, &["push", "origin", "main"]);
        run_git(&repository, &["fetch", "origin"]);
        repository
    }

    fn initialize_committed_repository(repository: &Path) {
        run_git(
            repository.parent().unwrap(),
            &["init", "-b", "main", repository.to_str().unwrap()],
        );
        std::fs::write(repository.join("README.md"), "initial\n").unwrap();
        run_git(repository, &["add", "README.md"]);
        commit(repository, "initial");
    }

    fn commit(repository: &Path, message: &str) {
        run_git(
            repository,
            &[
                "-c",
                "user.name=Grove Test",
                "-c",
                "user.email=grove@example.com",
                "commit",
                "-m",
                message,
            ],
        );
    }

    fn git_stdout(directory: &Path, args: &[&str]) -> String {
        let output = Command::new("git").current_dir(directory).args(args).output().unwrap();
        assert!(output.status.success(), "git {} failed", args.join(" "));
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[cfg(unix)]
    fn git_wrapper(directory: &Path, behavior: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let output = Command::new("sh").args(["-c", "command -v git"]).output().unwrap();
        assert!(output.status.success());
        let real_git = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let wrapper = directory.join("git-wrapper");
        std::fs::write(&wrapper, format!("#!/bin/sh\n{behavior}\nexec \"{real_git}\" \"$@\"\n"))
            .unwrap();
        let mut permissions = std::fs::metadata(&wrapper).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&wrapper, permissions).unwrap();
        wrapper
    }

    fn run_git(directory: &Path, args: &[&str]) {
        let output = Command::new("git").current_dir(directory).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    struct FailingProgressSink;

    impl GitProgressSink for FailingProgressSink {
        fn progress(&mut self, _progress: GitProgress) -> Result<(), AppError> {
            Err(AppError::internal("progress sink failed"))
        }
    }
}
