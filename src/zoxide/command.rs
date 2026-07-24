use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::ZoxideClient;
use crate::AppError;

#[derive(Debug, Clone, Copy, Default)]
pub struct CommandZoxideClient;

impl ZoxideClient for CommandZoxideClient {
    fn verify_available(&self) -> Result<(), AppError> {
        let mut command = Command::new("zoxide");
        command.arg("--version");
        let output = command.output().map_err(AppError::zoxide_unavailable_source)?;
        if !output.status.success() {
            return Err(AppError::zoxide_unavailable_status(
                "zoxide --version",
                command_message(&output),
                output.status.code(),
            ));
        }

        verify_capability(&["query", "--help"])?;
        verify_capability(&["add", "--help"])
    }

    fn resolve_symlinks(&self) -> bool {
        env::var_os("_ZO_RESOLVE_SYMLINKS").is_some_and(|value| value == "1")
    }

    fn entries(&self) -> Result<Vec<PathBuf>, AppError> {
        let output = run_required(
            Command::new("zoxide").args(["query", "--list", "--all"]),
            "zoxide query --list --all",
        )?;
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(PathBuf::from)
            .collect())
    }

    fn add(&self, path: &Path) -> Result<(), AppError> {
        run_required(
            Command::new("zoxide").arg("add").arg(path),
            format!("zoxide add {}", path.display()),
        )?;
        Ok(())
    }
}

fn verify_capability(args: &[&str]) -> Result<(), AppError> {
    let output =
        Command::new("zoxide").args(args).output().map_err(AppError::zoxide_unavailable_source)?;
    if output.status.success() {
        Ok(())
    } else {
        let command = format!("zoxide {}", args.join(" "));
        Err(AppError::zoxide_unavailable_status(
            command,
            format!(
                "required capability `zoxide {}` is unavailable: {}",
                args.join(" "),
                command_message(&output)
            ),
            output.status.code(),
        ))
    }
}

fn run_required(command: &mut Command, display: impl Into<String>) -> Result<Output, AppError> {
    let display = display.into();
    let output = command
        .output()
        .map_err(|err| AppError::zoxide_command_failed_source(display.clone(), err))?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(AppError::zoxide_command_failed_status(
            display,
            command_message(&output),
            output.status.code(),
        ))
    }
}

fn command_message(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_string();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stdout = stdout.trim();
    if !stdout.is_empty() {
        return stdout.to_string();
    }

    format!("process exited with {}", output.status)
}
