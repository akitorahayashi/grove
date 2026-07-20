use std::path::PathBuf;

use clap::Args;

use crate::AppError;
use crate::app::api;

use super::Completion;
use super::output::{Output, terminal_text};

#[derive(Args)]
pub(super) struct InitCommand;

pub(super) fn run(
    config: Option<PathBuf>,
    _command: InitCommand,
    output: &mut Output<'_>,
) -> Result<Completion, AppError> {
    if config.is_some() {
        return Err(AppError::config_error("--config cannot be used with init"));
    }

    let report = api::init(std::env::current_dir()?)?;

    let path = terminal_text(&report.created_path().display().to_string());
    output.stdout(format_args!("created {path}\n"))?;

    Ok(Completion::Success)
}
