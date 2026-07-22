use std::path::PathBuf;

use clap::Args;

use crate::AppError;
use crate::app::api;

use crate::cli::Completion;
use crate::cli::output::{Output, terminal_text};

#[derive(Args)]
pub(in crate::cli) struct InitCommand;

pub(in crate::cli) fn run(
    config: Option<PathBuf>,
    _command: InitCommand,
    output: &mut Output<'_>,
) -> Result<Completion, AppError> {
    if config.is_some() {
        return Err(AppError::invalid_arguments("--config cannot be used with init"));
    }

    let report = api::init(std::env::current_dir()?)?;

    let path = terminal_text(&report.created_path().display().to_string());
    output.stdout(format_args!("created {path}\n"))?;

    Ok(Completion::Success)
}
