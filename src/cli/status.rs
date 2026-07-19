use std::path::PathBuf;

use clap::Args;

use crate::AppError;
use crate::app::api;

#[derive(Args)]
pub(super) struct StatusCommand {
    #[arg(value_name = "repo")]
    repositories: Vec<String>,

    #[arg(long)]
    fetch: bool,
}

pub(super) fn run(config: Option<PathBuf>, command: StatusCommand) -> Result<(), AppError> {
    let report = api::status(config, command.repositories, command.fetch)?;

    println!("{:<24} {:<18} {:<16} DEFAULT", "REPOSITORY", "BRANCH", "STATE");
    for row in report.rows() {
        println!(
            "{:<24} {:<18} {:<16} {}",
            row.repository(),
            row.branch(),
            row.state(),
            row.default_branch()
        );
    }

    Ok(())
}
