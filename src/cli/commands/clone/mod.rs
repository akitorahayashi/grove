use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitStatus;

use clap::Args;

use crate::AppError;
use crate::app::api;
use crate::app::clone::CommandCache;

use crate::cli::Completion;
use crate::cli::output::{Output, terminal_text};
use crate::cli::tty::report::{cache_annotation, safe_message};

#[derive(Args)]
#[command(disable_help_flag = true, trailing_var_arg = true)]
pub(in crate::cli) struct CloneCommand {
    #[arg(value_name = "git-clone-argument", num_args = 0.., allow_hyphen_values = true)]
    arguments: Vec<OsString>,
}

impl CloneCommand {
    pub(in crate::cli) fn from_raw(arguments: Vec<OsString>) -> Self {
        Self { arguments }
    }
}

pub(in crate::cli) fn run(
    config: Option<PathBuf>,
    command: CloneCommand,
    output: &mut Output<'_>,
) -> Result<Completion, AppError> {
    if config.is_some() {
        return Err(AppError::invalid_arguments("--config cannot be used with clone"));
    }

    let report = api::clone_command(command.arguments)?;
    match report.cache() {
        CommandCache::Used(_) | CommandCache::Bypassed(_) if report.quiet() => {}
        CommandCache::Used(outcome) => output.stderr(format_args!(
            "gv: clone cache {}\n",
            terminal_text(cache_annotation(*outcome))
        ))?,
        CommandCache::Bypassed(reason) => {
            output.stderr(format_args!("gv: clone cache bypassed: {}\n", terminal_text(reason)))?
        }
        CommandCache::Delegated => {}
        CommandCache::Unavailable(message) => output.stderr(format_args!(
            "gv: clone cache unavailable; cloned without cache: {}\n",
            safe_message(message)
        ))?,
    }

    if report.status().success() {
        Ok(Completion::Success)
    } else {
        Ok(Completion::Code(status_code(report.status())))
    }
}

fn status_code(status: ExitStatus) -> u8 {
    if let Some(code) = status.code().and_then(|code| u8::try_from(code).ok()) {
        return code;
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        if let Some(signal) = status.signal()
            && let Ok(code) = u8::try_from(128 + signal)
        {
            return code;
        }
    }

    1
}
