use std::io;
use std::path::PathBuf;

use clap::Args;
use owo_colors::OwoColorize;

use crate::AppError;
use crate::app::api;
use crate::app::refresh::{Outcome, Phase, PhaseSummary, RefreshOptions, Report};

use super::Completion;
use super::output::{Output, terminal_text};
use super::repository_progress::{ProgressPhase, run_with_progress};
use super::terminal_report::{
    print_blocked_details, print_count, print_count_with_elapsed, print_phase, safe_message,
    write_line,
};

#[derive(Args)]
pub(super) struct RefreshCommand {
    #[arg(value_name = "repo")]
    repositories: Vec<String>,

    #[arg(long)]
    dry_run: bool,
}

pub(super) fn run(
    config: Option<PathBuf>,
    command: RefreshCommand,
    output: &mut Output<'_>,
) -> Result<Completion, AppError> {
    let options = RefreshOptions::new(command.dry_run);
    let report = if command.dry_run {
        api::refresh_with_options(config, command.repositories, options)?
    } else {
        run_with_progress(
            output,
            "refresh",
            move |sender| api::refresh_with_events(config, command.repositories, options, &sender),
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
    entries.sort_by(|left, right| {
        left.repository()
            .cmp(right.repository())
            .then_with(|| change_rank(left.outcome()).cmp(&change_rank(right.outcome())))
    });

    for entry in entries {
        match entry.outcome() {
            Outcome::Planned(_) | Outcome::Current { .. } => {}
            Outcome::Refreshed { branch, before, after, previous_branch } => {
                let repository = terminal_text(entry.repository());
                let mut change = format!("{branch} {before} -> {after}");
                if let Some(previous_branch) = previous_branch {
                    change.push_str(&format!(" from {previous_branch}"));
                }
                write_line(
                    output,
                    format_args!(
                        " {} {} {}",
                        "~".yellow(),
                        repository.bold(),
                        terminal_text(&change).dimmed()
                    ),
                )?;
            }
            Outcome::Switched { branch, previous_branch } => {
                let repository = terminal_text(entry.repository());
                let change = terminal_text(&format!("{branch} from {previous_branch}"));
                write_line(
                    output,
                    format_args!(" {} {} {}", ">".cyan(), repository.bold(), change.dimmed()),
                )?;
            }
            Outcome::SwitchedAndBlocked { branch, previous_branch, reason } => {
                let repository = terminal_text(entry.repository());
                let message = safe_message(&format!(
                    "switched to {branch} from {previous_branch}; update failed: {}",
                    reason.message()
                ));
                write_line(
                    output,
                    format_args!(" {} {} {}", "x".red(), repository.bold(), message.dimmed()),
                )?;
            }
            Outcome::Skipped { reason } => {
                let repository = terminal_text(entry.repository());
                write_line(
                    output,
                    format_args!(
                        " {} {} {}",
                        "!".yellow(),
                        repository.bold(),
                        terminal_text(reason.message()).dimmed()
                    ),
                )?;
            }
            Outcome::Blocked { reason } => {
                let repository = terminal_text(entry.repository());
                let message = safe_message(&reason.message());
                write_line(
                    output,
                    format_args!(" {} {} {}", "x".red(), repository.bold(), message.dimmed()),
                )?;
                print_blocked_details(entry.blocked_details(), output)?;
            }
        }
    }
    Ok(())
}

fn change_rank(outcome: &Outcome) -> u8 {
    match outcome {
        Outcome::Refreshed { .. } => 0,
        Outcome::Switched { .. } => 1,
        Outcome::SwitchedAndBlocked { .. } => 2,
        Outcome::Skipped { .. } => 3,
        Outcome::Blocked { .. } => 4,
        Outcome::Planned(_) | Outcome::Current { .. } => 5,
    }
}
