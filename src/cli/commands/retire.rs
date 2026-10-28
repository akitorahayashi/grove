use std::io;
use std::path::PathBuf;

use clap::Args;

use crate::AppError;
use crate::app::api;
use crate::app::retire::{OverrideOutcome, Phase, Report};
use crate::phases::Summary as PhaseSummary;
use crate::removal;

use crate::cli::Completion;
use crate::cli::output::Output;
use crate::cli::tty::progress::{ProgressPhase, run_with_progress};
use crate::cli::tty::report::{
    entry_line, print_count, print_count_with_elapsed, print_removals, safe_message,
};
use crate::cli::tty::table::Paint;

#[derive(Args)]
pub(in crate::cli) struct RetireCommand {
    #[arg(
        value_name = "override",
        help = "The override file to retire; defaults to the root configuration's override"
    )]
    target: Option<PathBuf>,

    #[arg(long)]
    dry_run: bool,
}

pub(in crate::cli) fn run(
    config: Option<PathBuf>,
    command: RetireCommand,
    output: &mut Output<'_>,
) -> Result<Completion, AppError> {
    let config = super::resolve_config(config, super::ConfigNotice::WhenAscended, output)?;
    let report = if command.dry_run {
        api::retire(Some(config), command.target, true)?
    } else {
        run_with_progress(
            output,
            "retire",
            move |sender| api::retire_with_events(Some(config), command.target, false, &sender),
            print_phase_completion,
        )?
    };

    print_report(&report, output)?;

    if report.has_failures() {
        return Ok(Completion::Failure);
    }
    Ok(Completion::Success)
}

impl ProgressPhase for Phase {
    fn message(self) -> &'static str {
        match self {
            Phase::Assessing => "Assessing repositories...",
            Phase::Retiring => "Retiring repositories...",
        }
    }

    fn shows_git_progress(self) -> bool {
        false
    }
}

fn print_phase_completion(
    phase: Phase,
    summary: PhaseSummary,
    output: &mut Output<'_>,
) -> io::Result<()> {
    match phase {
        Phase::Assessing => Ok(()),
        Phase::Retiring => {
            print_count_with_elapsed("Retired", summary.count(), summary.elapsed(), true, output)
        }
    }
}

fn print_report(report: &Report, output: &mut Output<'_>) -> io::Result<()> {
    let count = |predicate: fn(&removal::Outcome) -> bool| {
        report.entries().iter().filter(|entry| predicate(entry.outcome())).count()
    };
    print_count(
        "Would move",
        count(|outcome| matches!(outcome, removal::Outcome::Planned(removal::Plan::Move { .. }))),
        output,
    )?;
    print_count(
        "Would trash",
        count(|outcome| matches!(outcome, removal::Outcome::Planned(removal::Plan::Trash))),
        output,
    )?;
    print_count(
        "Blocked",
        count(|outcome| matches!(outcome, removal::Outcome::Blocked { .. })),
        output,
    )?;
    print_removals(report.entries(), output)?;
    print_override(report, output)
}

fn print_override(report: &Report, output: &mut Output<'_>) -> io::Result<()> {
    let path = report.override_display_path();
    match report.override_outcome() {
        OverrideOutcome::Kept => {
            entry_line(output, "x", Paint::Red, path, "kept until every repository can be retired")
        }
        OverrideOutcome::WouldTrash => {
            entry_line(output, "-", Paint::Cyan, path, "would move to the trash")
        }
        OverrideOutcome::Trashed => {
            entry_line(output, "-", Paint::Green, path, "moved to the trash")
        }
        OverrideOutcome::TrashFailed(message) => entry_line(
            output,
            "x",
            Paint::Red,
            path,
            &safe_message(&format!("trash failed: {message}")),
        ),
    }
}
