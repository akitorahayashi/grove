use std::path::PathBuf;

use clap::Args;

use crate::AppError;
use crate::app::api;

#[derive(Args)]
pub(super) struct ListCommand {
    #[arg(long)]
    json: bool,
}

pub(super) fn run(config: Option<PathBuf>, command: ListCommand) -> Result<(), AppError> {
    let report = api::list(config)?;

    if command.json {
        serde_json::to_writer_pretty(std::io::stdout(), &report)?;
        println!();
        return Ok(());
    }

    for repository in report.repositories {
        println!("{:<18} {}", repository.name, repository.path);
    }

    Ok(())
}
