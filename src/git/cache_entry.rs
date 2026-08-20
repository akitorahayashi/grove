use std::fs;
use std::path::Path;

use super::command::run_with_progress;
use super::probe::required_line;
use super::{CacheEntry, CommandGitClient, GitProgressSink};
use crate::AppError;
use crate::repositories::{RemoteUrl, redact_urls_for_display};

impl CacheEntry for CommandGitClient {
    fn cache_create(
        &self,
        url: &RemoteUrl,
        entry: &Path,
        branch: Option<&str>,
        reference: Option<&Path>,
        progress: &mut dyn GitProgressSink,
    ) -> Result<String, AppError> {
        if let Some(parent) = entry.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut command = self.command();
        command.arg("clone").arg("--bare").arg("--single-branch");
        if let Some(branch) = branch {
            command.arg("--branch").arg(branch);
        }
        if let Some(reference) = reference {
            command.arg("--reference").arg(reference).arg("--dissociate");
        }
        command.arg("--progress").arg("--").arg(url.as_process_argument()).arg(entry);

        let branch_display = branch.map(|branch| format!(" --branch {branch}")).unwrap_or_default();
        let reference_display = reference
            .map(|reference| format!(" --reference {} --dissociate", reference.display()))
            .unwrap_or_default();
        run_with_progress(
            command,
            redact_urls_for_display(&format!(
                "git clone --bare --single-branch{branch_display}{reference_display} --progress -- {url} {}",
                entry.display()
            )),
            progress,
        )?;

        let tracked = match branch {
            Some(branch) => branch.to_string(),
            None => self.head_branch(entry)?,
        };
        let refspec = format!("+refs/heads/{tracked}:refs/heads/{tracked}");
        self.git_required(entry, &["config", "remote.origin.fetch", &refspec])?;
        Ok(tracked)
    }

    fn cache_update(
        &self,
        entry: &Path,
        url: &RemoteUrl,
        progress: &mut dyn GitProgressSink,
    ) -> Result<(), AppError> {
        self.repoint_origin(entry, url)?;
        self.git_progress_required(entry, &["fetch", "--progress", "origin", "--prune"], progress)
    }

    fn cache_retarget(
        &self,
        entry: &Path,
        url: &RemoteUrl,
        branch: &str,
        progress: &mut dyn GitProgressSink,
    ) -> Result<(), AppError> {
        self.repoint_origin(entry, url)?;
        let refspec = format!("+refs/heads/{branch}:refs/heads/{branch}");
        self.git_required(entry, &["config", "remote.origin.fetch", &refspec])?;
        self.git_progress_required(entry, &["fetch", "--progress", "origin", "--prune"], progress)
    }

    fn cache_verify(&self, entry: &Path) -> Result<bool, AppError> {
        if !entry.exists() {
            return Ok(false);
        }
        let output = self.git_probe(entry, &["rev-parse", "--git-dir"])?;
        Ok(output.status.success())
    }

    fn clone_with_reference(
        &self,
        url: &RemoteUrl,
        destination: &Path,
        reference: &Path,
        progress: &mut dyn GitProgressSink,
    ) -> Result<(), AppError> {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut command = self.command();
        command
            .arg("clone")
            .arg("--reference")
            .arg(reference)
            .arg("--dissociate")
            .arg("--progress")
            .arg("--")
            .arg(url.as_process_argument())
            .arg(destination);
        run_with_progress(
            command,
            redact_urls_for_display(&format!(
                "git clone --reference {} --dissociate --progress -- {url} {}",
                reference.display(),
                destination.display()
            )),
            progress,
        )
    }
}

impl CommandGitClient {
    /// Entries are keyed by transport-independent identity, so a reused entry
    /// may still carry the origin of whichever URL form created it. Setting
    /// the URL unconditionally is idempotent and cheaper than reading first.
    fn repoint_origin(&self, entry: &Path, url: &RemoteUrl) -> Result<(), AppError> {
        self.git_required(
            entry,
            &["remote", "set-url", "--", "origin", url.as_process_argument()],
        )?;
        Ok(())
    }

    fn head_branch(&self, entry: &Path) -> Result<String, AppError> {
        let args = ["symbolic-ref", "--short", "HEAD"];
        let output = self.git_required(entry, &args)?;
        required_line(entry, &args, &output)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::git::{CacheEntry, CommandGitClient, NoopGitProgressSink};
    use crate::repositories::RemoteUrl;

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
}
