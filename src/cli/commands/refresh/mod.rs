use std::io;
use std::path::PathBuf;

use clap::Args;

use crate::AppError;
use crate::app::api;
use crate::app::refresh::{Outcome, Phase, PhaseSummary, RefreshOptions, Report};

use crate::cli::Completion;
use crate::cli::output::{Output, terminal_text};
use crate::cli::tty::progress::{ProgressPhase, run_with_progress};
use crate::cli::tty::report::{
    entry_line, print_blocked_details, print_count, print_count_with_elapsed, print_phase,
    safe_message, write_line,
};
use crate::cli::tty::table::Paint;

#[derive(Args)]
pub(in crate::cli) struct RefreshCommand {
    #[arg(value_name = "repo")]
    repositories: Vec<String>,

    #[arg(long)]
    dry_run: bool,
}

pub(in crate::cli) fn run(
    config: Option<PathBuf>,
    command: RefreshCommand,
    output: &mut Output<'_>,
) -> Result<Completion, AppError> {
    let config = super::resolve_config(config, output)?;
    let options = RefreshOptions::new(command.dry_run);
    let report = if command.dry_run {
        api::refresh(Some(config), command.repositories, options)?
    } else {
        run_with_progress(
            output,
            "refresh",
            move |sender| {
                api::refresh_with_events(Some(config), command.repositories, options, &sender)
            },
            print_phase_completion,
        )?
    };

    print_report(&report, command.dry_run, output)?;
    if report.has_failures() { Ok(Completion::Failure) } else { Ok(Completion::Success) }
}

impl ProgressPhase for Phase {
    fn message(self) -> &'static str {
        match self {
            Phase::Checking => "Checking repositories...",
            Phase::Fetching => "Fetching repositories...",
            Phase::Refreshing => "Refreshing repositories...",
        }
    }

    fn shows_git_progress(self) -> bool {
        self == Phase::Fetching
    }
}

fn print_report(report: &Report, dry_run: bool, output: &mut Output<'_>) -> io::Result<()> {
    if dry_run {
        if report.planned() > 0 {
            print_count("Would fetch and refresh", report.planned(), output)?;
        } else if !report.has_failures() {
            write_line(output, format_args!("Would make no changes"))?;
        }
    }

    print_count("Skipped", report.skipped(), output)?;
    print_count("Blocked", report.blocked(), output)?;
    print_entries(report, output)
}

fn print_phase_completion(
    phase: Phase,
    summary: PhaseSummary,
    output: &mut Output<'_>,
) -> io::Result<()> {
    match phase {
        Phase::Checking => print_phase("Checked", summary.count(), summary.elapsed(), output),
        Phase::Fetching => {
            print_count_with_elapsed("Fetched", summary.count(), summary.elapsed(), true, output)
        }
        Phase::Refreshing => {
            print_count_with_elapsed("Refreshed", summary.count(), summary.elapsed(), false, output)
        }
    }
}

fn print_entries(report: &Report, output: &mut Output<'_>) -> io::Result<()> {
    let mut entries = report.entries().iter().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.repository());

    for entry in entries {
        match entry.outcome() {
            Outcome::Planned(_) | Outcome::Current { .. } => {}
            Outcome::Refreshed { branch, before, after, previous_branch } => {
                let mut change = format!("{branch} {before} -> {after}");
                if let Some(previous_branch) = previous_branch {
                    change.push_str(&format!(" from {previous_branch}"));
                }
                entry_line(
                    output,
                    "~",
                    Paint::Yellow,
                    entry.repository(),
                    &terminal_text(&change),
                )?;
            }
            Outcome::Switched { branch, previous_branch } => {
                entry_line(
                    output,
                    ">",
                    Paint::Cyan,
                    entry.repository(),
                    &terminal_text(&format!("{branch} from {previous_branch}")),
                )?;
            }
            Outcome::SwitchedAndBlocked { branch, previous_branch, reason } => {
                let message = safe_message(&format!(
                    "switched to {branch} from {previous_branch}; update failed: {}",
                    reason.message()
                ));
                entry_line(output, "x", Paint::Red, entry.repository(), &message)?;
            }
            Outcome::Skipped { reason } => {
                entry_line(
                    output,
                    "!",
                    Paint::Yellow,
                    entry.repository(),
                    &terminal_text(reason.message()),
                )?;
            }
            Outcome::Blocked { reason } => {
                entry_line(
                    output,
                    "x",
                    Paint::Red,
                    entry.repository(),
                    &safe_message(&reason.message()),
                )?;
                print_blocked_details(entry.blocked_details(), output)?;
            }
        }
    }
    Ok(())
}
