use std::path::PathBuf;

use clap::Args;

use crate::AppError;
use crate::app::api;
use crate::app::sync::{SyncReport, SyncState};

#[derive(Args)]
pub(super) struct SyncCommand {
    #[arg(value_name = "repo")]
    repositories: Vec<String>,

    #[arg(long)]
    dry_run: bool,
}

pub(super) fn run(config: Option<PathBuf>, command: SyncCommand) -> Result<(), AppError> {
    let report = api::sync(config, command.repositories, command.dry_run)?;

    for row in report.rows() {
        println!("{:<8} {:<24} {}", row.state().as_str(), row.repository(), row.detail());
    }
    print_summary(&report);

    Ok(())
}

fn print_summary(report: &SyncReport) {
    println!();
    println!(
        "Planned {}, cloned {}, updated {}, current {}, skipped {}, blocked {}",
        report.count(SyncState::Planned),
        report.count(SyncState::Cloned),
        report.count(SyncState::Updated),
        report.count(SyncState::Current),
        report.count(SyncState::Skipped),
        report.count(SyncState::Blocked)
    );
}
