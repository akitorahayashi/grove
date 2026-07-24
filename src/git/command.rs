use std::ffi::OsString;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Output, Stdio};

use super::{GitProgressSink, parse_git_progress};
use crate::AppError;
use crate::repositories::redact_urls_for_display;

#[derive(Debug, Clone)]
pub struct CommandGitClient {
    executable: OsString,
}

impl Default for CommandGitClient {
    fn default() -> Self {
        Self { executable: OsString::from("git") }
    }
}

impl CommandGitClient {
    #[cfg(test)]
    fn with_executable(executable: impl AsRef<std::ffi::OsStr>) -> Self {
        Self { executable: executable.as_ref().to_os_string() }
    }

    pub(super) fn command(&self) -> Command {
        Command::new(&self.executable)
    }

    pub(super) fn git_required(
        &self,
        repository: &Path,
        args: &[&str],
    ) -> Result<Output, AppError> {
        let mut command = self.command();
        command.current_dir(repository).args(args);
        let display =
            redact_urls_for_display(&format!("git -C {} {}", repository.display(), args.join(" ")));
        run_required(command, display)
    }

    pub(super) fn git_progress_required(
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

    pub(super) fn git_probe(&self, repository: &Path, args: &[&str]) -> Result<Output, AppError> {
        let mut command = self.command();
        command.current_dir(repository).env("LC_ALL", "C").args(args);
        command
            .output()
            .map_err(|err| AppError::git_command_failed_source(format_probe(repository, args), err))
    }
}

fn run_required(mut command: Command, display: String) -> Result<Output, AppError> {
    let output = command
        .output()
        .map_err(|err| AppError::git_command_failed_source(display.clone(), err))?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(AppError::git_command_failed_status(
            display,
            redact_urls_for_display(&command_message(&output)),
            output.status.code(),
        ))
    }
}

pub(super) fn run_with_progress(
    mut command: Command,
    display: String,
    progress: &mut dyn GitProgressSink,
) -> Result<(), AppError> {
    command.stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child =
        command.spawn().map_err(|err| AppError::git_command_failed_source(display.clone(), err))?;
    let mut stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let error = AppError::internal("Git progress stderr pipe was unavailable");
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let mut buffer = [0; 4096];
    let mut pending = Vec::new();
    let mut pending_truncated = false;
    let mut diagnostics = DiagnosticTail::default();
    let mut processing_error = None;

    loop {
        let read = match stderr.read(&mut buffer) {
            Ok(read) => read,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => {
                if processing_error.is_none() {
                    processing_error =
                        Some(AppError::git_command_failed_source(display.clone(), err));
                }
                break;
            }
        };
        if read == 0 {
            break;
        }
        for byte in &buffer[..read] {
            if *byte == b'\r' || *byte == b'\n' {
                if let Some(error) = process_progress_line(
                    &pending,
                    pending_truncated,
                    progress,
                    &mut diagnostics,
                    processing_error.is_none(),
                ) {
                    processing_error = Some(error);
                }
                pending.clear();
                pending_truncated = false;
            } else {
                pending.push(*byte);
                if pending.len() > MAX_DIAGNOSTIC_BYTES {
                    let excess = pending.len() - MAX_DIAGNOSTIC_BYTES;
                    pending.drain(..excess);
                    pending_truncated = true;
                }
            }
        }
    }
    if let Some(error) = process_progress_line(
        &pending,
        pending_truncated,
        progress,
        &mut diagnostics,
        processing_error.is_none(),
    ) {
        processing_error = Some(error);
    }

    drop(stderr);
    let status = child.wait().map_err(|err| {
        processing_error
            .take()
            .unwrap_or_else(|| AppError::git_command_failed_source(display.clone(), err))
    })?;
    if let Some(error) = processing_error {
        return Err(error);
    }
    if status.success() {
        Ok(())
    } else {
        Err(AppError::git_command_failed_status(
            display,
            redact_urls_for_display(&diagnostics.render()),
            status.code(),
        ))
    }
}

const MAX_DIAGNOSTIC_BYTES: usize = 64 * 1024;

#[derive(Default)]
struct DiagnosticTail {
    bytes: Vec<u8>,
    truncated: bool,
}

impl DiagnosticTail {
    fn push(&mut self, line: &[u8], line_truncated: bool) {
        if !self.bytes.is_empty() {
            self.bytes.push(b'\n');
        }
        self.bytes.extend_from_slice(line);
        if self.bytes.len() > MAX_DIAGNOSTIC_BYTES {
            let excess = self.bytes.len() - MAX_DIAGNOSTIC_BYTES;
            self.bytes.drain(..excess);
            self.truncated = true;
        }
        self.truncated |= line_truncated;
    }

    fn render(&self) -> String {
        let message = String::from_utf8_lossy(&self.bytes).trim().to_string();
        match (self.truncated, message.is_empty()) {
            (true, true) => "[Git diagnostic output truncated]".to_string(),
            (true, false) => format!("[Git diagnostic output truncated]\n{message}"),
            (false, true) => "Git command failed without diagnostic output".to_string(),
            (false, false) => message,
        }
    }
}

fn process_progress_line(
    line: &[u8],
    line_truncated: bool,
    progress: &mut dyn GitProgressSink,
    diagnostics: &mut DiagnosticTail,
    emit: bool,
) -> Option<AppError> {
    if line.is_empty() {
        return None;
    }
    let line = String::from_utf8_lossy(line);
    if let Some(parsed) = parse_git_progress(&line) {
        if emit {
            return progress.progress(parsed).err();
        }
    } else {
        diagnostics.push(line.as_bytes(), line_truncated);
    }
    None
}

pub(super) fn command_message(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        stderr
    }
}

pub(super) fn format_probe(repository: &Path, args: &[&str]) -> String {
    redact_urls_for_display(&format!("git -C {} {}", repository.display(), args.join(" ")))
}

#[cfg(test)]
mod tests {
    use std::fs::{OpenOptions, TryLockError};
    use std::path::Path;
    use std::process::Command;

    use tempfile::TempDir;

    use crate::AppError;
    use crate::git::{
        CacheEntry, CommandGitClient, DefaultBranch, GitProgress, GitProgressSink,
        GitRefreshOutcome, GitUpdateBlock, GitUpdateOutcome, NoopGitProgressSink, RepositoryProbe,
        Restoration,
    };
    use crate::repositories::{BranchName, RemoteUrl};

    #[cfg(unix)]
    #[test]
    fn progress_failures_keep_a_bounded_redacted_diagnostic_tail() {
        use std::os::unix::fs::PermissionsExt;

        let root = TempDir::new().unwrap();
        let script = root.path().join("diagnostic-script");
        std::fs::write(
            &script,
            "#!/bin/sh\ni=0\nwhile [ \"$i\" -lt 5000 ]; do\n  printf 'diagnostic-%04d-abcdefghijklmnopqrstuvwxyz\\n' \"$i\" >&2\n  i=$((i + 1))\ndone\nprintf 'final HTTPS://user:secret@example.com/repo.git?token=value\\n' >&2\nexit 42\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let error = super::run_with_progress(
            Command::new(script),
            "git diagnostic-test".to_string(),
            &mut NoopGitProgressSink,
        )
        .unwrap_err();
        let message = error.to_string();

        assert!(
            message.len() <= super::MAX_DIAGNOSTIC_BYTES + 2048,
            "diagnostic length was {} bytes",
            message.len()
        );
        assert!(message.contains("[Git diagnostic output truncated]"), "{message}");
        assert!(message.contains("final HTTPS://[redacted]@example.com/repo.git?token=[redacted]"));
        assert!(!message.contains("diagnostic-0000-"));
        assert!(!message.contains("secret"));
        assert!(!message.contains("value"));
    }

    #[test]
    fn repository_locks_coordinate_independent_file_handles() {
        let root = TempDir::new().unwrap();
        let client = CommandGitClient::default();
        let held = client.lock_repository(root.path()).unwrap();
        let competing = OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.path().join("grove-operation.lock"))
            .unwrap();

        assert!(matches!(competing.try_lock(), Err(TryLockError::WouldBlock)));
        drop(held);
        competing.try_lock().unwrap();
    }

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
            crate::git::BranchTracking::MissingLocal
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

        assert_eq!(result.unwrap(), crate::git::BranchTracking::MissingLocal);
    }

    #[test]
    fn parses_supported_git_versions() {
        assert_eq!(
            super::super::probe::parse_git_version("git version 2.23.0\n"),
            Some((2, 23, 0))
        );
        assert_eq!(
            super::super::probe::parse_git_version("git version 2.39.5 (Apple Git-154)\n"),
            Some((2, 39, 5))
        );
        assert_eq!(super::super::probe::parse_git_version("unexpected"), None);
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
    fn update_blocks_a_missing_remote_reference() {
        let root = TempDir::new().unwrap();
        let repository = root.path().join("repo");
        initialize_committed_repository(&repository);

        let result =
            CommandGitClient::default().update_default_branch(&repository, "main").unwrap();

        assert_eq!(result, GitUpdateOutcome::Blocked(GitUpdateBlock::MissingRemoteBranch));
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
    fn update_blocks_a_diverged_branch_before_switching() {
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
            "if [ \"$1\" = for-each-ref ] && [ \"$(git rev-parse main)\" = \"$(git rev-parse origin/main)\" ]; then\n  echo post-merge-probe-failed >&2\n  exit 42\nfi",
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
        let reference = workspace.join("cache");
        let url = RemoteUrl::new("--upload-pack=hostile").unwrap();

        CommandGitClient::with_executable(&wrapper)
            .clone_with_reference(&url, &destination, &reference, &mut NoopGitProgressSink)
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(log).unwrap().lines().collect::<Vec<_>>(),
            [
                "clone",
                "--reference",
                reference.to_str().unwrap(),
                "--dissociate",
                "--progress",
                "--",
                "--upload-pack=hostile",
                destination.to_str().unwrap()
            ]
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
