use std::path::PathBuf;

use clap::Args;

use crate::AppError;
use crate::app::api;
use crate::app::validate::Report;

use crate::cli::Completion;
use crate::cli::output::{Output, terminal_text};
use crate::cli::tty::report::repositories;

#[derive(Args)]
pub(in crate::cli) struct ValidateCommand {}

pub(in crate::cli) fn run(
    config: Option<PathBuf>,
    _command: ValidateCommand,
    output: &mut Output<'_>,
) -> Result<Completion, AppError> {
    let report = api::validate(config)?;
    print_report(&report, output)?;
    Ok(Completion::Success)
}

fn print_report(report: &Report, output: &mut Output<'_>) -> std::io::Result<()> {
    output.stdout(format_args!("Validated {}\n", repositories(report.repository_count())))?;
    let path = terminal_text(&report.config_path().display().to_string());
    output.stdout(format_args!("Config: {path}\n"))
}
