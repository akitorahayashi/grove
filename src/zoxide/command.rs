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
        let output =
            command.output().map_err(|err| AppError::ZoxideUnavailable(err.to_string()))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(AppError::ZoxideUnavailable(command_message(&output)))
        }
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

fn run_required(command: &mut Command, display: impl Into<String>) -> Result<Output, AppError> {
    let display = display.into();
    let output = command
        .output()
        .map_err(|err| AppError::zoxide_command_failed(display.clone(), err.to_string()))?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(AppError::zoxide_command_failed(display, command_message(&output)))
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
