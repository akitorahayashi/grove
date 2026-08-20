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
    pub(in crate::git) fn with_executable(executable: impl AsRef<std::ffi::OsStr>) -> Self {
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
        run_required(command, format_probe(repository, args))
    }

    pub(super) fn git_progress_required(
        &self,
        repository: &Path,
        args: &[&str],
        progress: &mut dyn GitProgressSink,
    ) -> Result<(), AppError> {
        let mut command = self.command();
        command.current_dir(repository).args(args);
        run_with_progress(command, format_probe(repository, args), progress)
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

/// Also owns the Git repository and wrapper fixtures that the probe,
/// cache-entry, branch-update, client, and cache-store test modules import:
/// they drive the same `git` process boundary this module owns, and each needs
/// nearly the identical set.
#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;
    use std::process::Command;

    use tempfile::TempDir;

    use crate::AppError;
    use crate::git::{
        CommandGitClient, GitProgress, GitProgressSink, NoopGitProgressSink, RepositoryProbe,
    };

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

    pub(crate) fn create_updatable_repository(root: &Path) -> std::path::PathBuf {
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

    pub(crate) fn initialize_committed_repository(repository: &Path) {
        run_git(
            repository.parent().unwrap(),
            &["init", "-b", "main", repository.to_str().unwrap()],
        );
        std::fs::write(repository.join("README.md"), "initial\n").unwrap();
        run_git(repository, &["add", "README.md"]);
        commit(repository, "initial");
    }

    pub(crate) fn commit(repository: &Path, message: &str) {
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

    pub(crate) fn git_stdout(directory: &Path, args: &[&str]) -> String {
        let output = Command::new("git").current_dir(directory).args(args).output().unwrap();
        assert!(output.status.success(), "git {} failed", args.join(" "));
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[cfg(unix)]
    pub(crate) fn git_wrapper(directory: &Path, behavior: &str) -> std::path::PathBuf {
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

    pub(crate) fn run_git(directory: &Path, args: &[&str]) {
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
