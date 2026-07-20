use std::path::PathBuf;

use clap::Args;

use crate::AppError;
use crate::app::api;
use crate::app::validate::Report;

#[derive(Args)]
pub(super) struct ValidateCommand {}

pub(super) fn run(config: Option<PathBuf>, _command: ValidateCommand) -> Result<(), AppError> {
    let report = api::validate(config)?;
    print_report(&report);
    Ok(())
}

fn print_report(report: &Report) {
    println!("Validated {}", repositories(report.repository_count()));
    println!("Config: {}", report.config_path().display());
}

fn repositories(count: usize) -> String {
    match count {
        1 => "1 repository".to_string(),
        _ => format!("{count} repositories"),
    }
}
