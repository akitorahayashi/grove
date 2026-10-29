use std::collections::VecDeque;
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
    #[cfg(test)]
    isolated: bool,
}

impl Default for CommandGitClient {
    fn default() -> Self {
        Self {
            executable: OsString::from("git"),
            #[cfg(test)]
            isolated: false,
        }
    }
}

impl CommandGitClient {
    /// A client whose Git processes run under the fixtures' isolated
    /// configuration, so a developer's global hooks, signing, or inherited
    /// repository selectors cannot change what a unit test observes.
    #[cfg(test)]
    pub(crate) fn isolated() -> Self {
        Self::with_executable("git")
    }

    #[cfg(test)]
    pub(in crate::git) fn with_executable(executable: impl AsRef<std::ffi::OsStr>) -> Self {
        Self { executable: executable.as_ref().to_os_string(), isolated: true }
    }

    pub(super) fn command(&self) -> Command {
        #[cfg_attr(not(test), allow(unused_mut))]
        let mut command = Command::new(&self.executable);
        #[cfg(test)]
        if self.isolated {
            tests::isolate(&mut command);
        }
        command
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
    let mut pending = BoundedDiagnosticBytes::default();
    let mut overwritten = BoundedDiagnosticBytes::default();
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
            match *byte {
                b'\r' => {
                    if pending.is_empty() {
                        continue;
                    }
                    let emit = processing_error.is_none();
                    if let Some(error) = emit_progress(pending.as_slice(), progress, emit) {
                        processing_error = Some(error);
                    }
                    overwritten = std::mem::take(&mut pending);
                }
                b'\n' => {
                    let emit = processing_error.is_none();
                    if let Some(error) = finish_line(
                        &mut pending,
                        &mut overwritten,
                        progress,
                        &mut diagnostics,
                        emit,
                    ) {
                        processing_error = Some(error);
                    }
                }
                byte => pending.push(byte),
            }
        }
    }
    let emit = processing_error.is_none();
    if let Some(error) =
        finish_line(&mut pending, &mut overwritten, progress, &mut diagnostics, emit)
    {
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
struct BoundedDiagnosticBytes {
    bytes: VecDeque<u8>,
    truncated: bool,
}

impl BoundedDiagnosticBytes {
    fn push(&mut self, byte: u8) {
        if self.bytes.len() == MAX_DIAGNOSTIC_BYTES {
            self.bytes.pop_front();
            self.truncated = true;
        }
        self.bytes.push_back(byte);
    }

    fn extend(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.push(*byte);
        }
    }

    fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    fn truncated(&self) -> bool {
        self.truncated
    }

    fn include_truncation(&mut self, truncated: bool) {
        self.truncated |= truncated;
    }

    fn as_slice(&mut self) -> &[u8] {
        self.bytes.make_contiguous()
    }

    fn clear(&mut self) {
        self.bytes.clear();
        self.truncated = false;
    }
}

#[derive(Default)]
struct DiagnosticTail {
    bytes: BoundedDiagnosticBytes,
}

impl DiagnosticTail {
    fn push(&mut self, line: &[u8], line_truncated: bool) {
        if !self.bytes.is_empty() {
            self.bytes.push(b'\n');
        }
        self.bytes.extend(line);
        self.bytes.include_truncation(line_truncated);
    }

    fn render(&mut self) -> String {
        let truncated = self.bytes.truncated();
        let message = String::from_utf8_lossy(self.bytes.as_slice()).trim().to_string();
        match (truncated, message.is_empty()) {
            (true, true) => "[Git diagnostic output truncated]".to_string(),
            (true, false) => format!("[Git diagnostic output truncated]\n{message}"),
            (false, true) => "Git command failed without diagnostic output".to_string(),
            (false, false) => message,
        }
    }
}

/// Ends a line at `\n` or end of output. The diagnostic tail keeps what a
/// terminal would leave visible: every finished line, whatever its content,
/// so a fatal message that happens to look like progress survives. A segment
/// ended by `\r` and then overwritten by further text is a transient progress
/// refresh and is dropped; one followed directly by `\n` stays visible.
fn finish_line(
    pending: &mut BoundedDiagnosticBytes,
    overwritten: &mut BoundedDiagnosticBytes,
    progress: &mut dyn GitProgressSink,
    diagnostics: &mut DiagnosticTail,
    emit: bool,
) -> Option<AppError> {
    let mut error = None;
    let mut line = if pending.is_empty() {
        std::mem::take(overwritten)
    } else {
        error = emit_progress(pending.as_slice(), progress, emit);
        overwritten.clear();
        std::mem::take(pending)
    };
    if !line.is_empty() {
        let truncated = line.truncated();
        diagnostics.push(line.as_slice(), truncated);
    }
    error
}

fn emit_progress(line: &[u8], progress: &mut dyn GitProgressSink, emit: bool) -> Option<AppError> {
    if !emit {
        return None;
    }
    let parsed = parse_git_progress(&String::from_utf8_lossy(line))?;
    progress.progress(parsed).err()
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
    fn progress_failures_keep_finished_lines_that_resemble_progress() {
        use std::os::unix::fs::PermissionsExt;

        let root = TempDir::new().unwrap();
        let script = root.path().join("diagnostic-script");
        std::fs::write(
            &script,
            "#!/bin/sh\ni=0\nwhile [ \"$i\" -lt 500 ]; do\n  printf 'Receiving objects: %d%% (%d/500)\\r' $((i / 5)) \"$i\" >&2\n  i=$((i + 1))\ndone\nprintf 'Receiving objects: 100%% (500/500), done.\\n' >&2\nprintf 'fatal: server is 100%% unavailable\\n' >&2\nprintf 'error: 3/4 (75%%) of mirrors refused\\r\\n' >&2\nexit 42\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();
        let mut progress = RecordingProgressSink::default();

        let error =
            super::run_with_progress(Command::new(script), "git fetch".to_string(), &mut progress)
                .unwrap_err();

        let git = error.git_error().expect("a Git failure");
        assert_eq!(git.exit_code(), Some(42));
        assert!(git.diagnostic().contains("fatal: server is 100% unavailable"), "{error}");
        assert!(git.diagnostic().contains("error: 3/4 (75%) of mirrors refused"), "{error}");
        assert!(git.diagnostic().contains("Receiving objects: 100% (500/500), done."));
        assert!(!git.diagnostic().contains("(250/500)"), "{error}");
        assert!(progress.count > 500, "progress events were {}", progress.count);
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
        create_updatable_repository_on(root, "main")
    }

    /// A clone on `feature` whose default branch `branch` is one commit behind
    /// its fetched remote-tracking ref.
    pub(crate) fn create_updatable_repository_on(root: &Path, branch: &str) -> std::path::PathBuf {
        let remote = root.join("remote.git");
        let seed = root.join("seed");
        let repository = root.join("repository");
        run_git(
            root,
            &["init", "--bare", &format!("--initial-branch={branch}"), remote.to_str().unwrap()],
        );
        initialize_committed_repository(&seed);
        run_git(&seed, &["branch", "-m", branch]);
        run_git(&seed, &["remote", "add", "origin", remote.to_str().unwrap()]);
        run_git(&seed, &["push", "-u", "origin", branch]);
        run_git(root, &["clone", remote.to_str().unwrap(), repository.to_str().unwrap()]);
        run_git(&repository, &["switch", "-c", "feature"]);
        std::fs::write(seed.join("remote.txt"), "remote\n").unwrap();
        run_git(&seed, &["add", "remote.txt"]);
        commit(&seed, "remote");
        run_git(&seed, &["push", "origin", branch]);
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
        let output = git(directory).args(args).output().unwrap();
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
        let output = git(directory).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Strips every inherited `GIT_*` variable and replaces global and system
    /// configuration with nothing, so fixtures and isolated clients neither read
    /// a developer's signing or hook settings nor follow an ambient `GIT_DIR`.
    /// Applied per command; the test process environment is never mutated.
    pub(crate) fn isolate(command: &mut Command) {
        for (name, _) in std::env::vars_os() {
            if name.to_str().is_some_and(|name| name.starts_with("GIT_")) {
                command.env_remove(name);
            }
        }
        command.env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1");
    }

    fn git(directory: &Path) -> Command {
        let mut command = Command::new("git");
        command.current_dir(directory);
        isolate(&mut command);
        command
    }

    #[derive(Default)]
    struct RecordingProgressSink {
        count: usize,
    }

    impl GitProgressSink for RecordingProgressSink {
        fn progress(&mut self, _progress: GitProgress) -> Result<(), AppError> {
            self.count += 1;
            Ok(())
        }
    }

    struct FailingProgressSink;

    impl GitProgressSink for FailingProgressSink {
        fn progress(&mut self, _progress: GitProgress) -> Result<(), AppError> {
            Err(AppError::internal("progress sink failed"))
        }
    }
}
