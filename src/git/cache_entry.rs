use std::fs;
use std::path::Path;

use super::command::{format_probe, run_with_progress};
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
        progress: &mut dyn GitProgressSink,
    ) -> Result<(), AppError> {
        self.git_progress_required(entry, &["fetch", "--progress", "origin", "--prune"], progress)
    }

    fn cache_retarget(
        &self,
        entry: &Path,
        branch: &str,
        progress: &mut dyn GitProgressSink,
    ) -> Result<(), AppError> {
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
    fn head_branch(&self, entry: &Path) -> Result<String, AppError> {
        let args = ["symbolic-ref", "--short", "HEAD"];
        let output = self.git_required(entry, &args)?;
        let value = String::from_utf8_lossy(&output.stdout);
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed.split_whitespace().count() != 1 {
            return Err(AppError::git_command_failed(
                format_probe(entry, &args),
                if trimmed.is_empty() {
                    "Git returned empty output"
                } else {
                    "Git returned malformed output"
                },
            ));
        }
        Ok(trimmed.to_string())
    }
}
