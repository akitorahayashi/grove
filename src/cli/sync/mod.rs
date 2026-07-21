use std::io;
use std::path::PathBuf;

use clap::Args;
use owo_colors::OwoColorize;

use crate::AppError;
use crate::app::api;
use crate::app::sync::{
    Outcome, Phase, PhaseSummary, Plan, Report, SyncOptions, ZoxideOutcome, ZoxideReport,
};

use super::Completion;
use super::output::{Output, terminal_text};
use super::repository_progress::{ProgressPhase, run_with_progress};
use super::terminal_report::{
    cache_annotation, print_blocked_details, print_count, print_count_with_elapsed, print_phase,
    safe_message, write_line,
};

#[derive(Args)]
pub(super) struct SyncCommand {
    #[arg(value_name = "repo")]
    repositories: Vec<String>,

    #[arg(long)]
    dry_run: bool,

    #[arg(short = 'z', long)]
    register_zoxide: bool,
}

pub(super) fn run(
    config: Option<PathBuf>,
    command: SyncCommand,
    output: &mut Output<'_>,
) -> Result<Completion, AppError> {
    let options = SyncOptions::new(command.dry_run, command.register_zoxide);
    let report = if command.dry_run {
        api::sync(config, command.repositories, options)?
    } else {
        run_with_progress(
            output,
            "sync",
            move |sender| api::sync_with_events(config, command.repositories, options, &sender),
            print_phase_completion,
        )?
    };

    print_report(&report, command.dry_run, output)?;

    if report.has_failures() {
        return Ok(Completion::Failure);
    }

    Ok(Completion::Success)
}

impl ProgressPhase for Phase {
    fn message(self) -> &'static str {
        match self {
            Phase::Checking => "Checking repositories...",
            Phase::Preparing => "Preparing repositories...",
            Phase::Updating => "Updating repositories...",
            Phase::Seeding => "Seeding cache...",
        }
    }

    fn shows_git_progress(self) -> bool {
        matches!(self, Phase::Preparing | Phase::Seeding)
    }
}

fn print_report(report: &Report, dry_run: bool, output: &mut Output<'_>) -> io::Result<()> {
    if dry_run {
        print_dry_run_summary(report, output)?;
    }

    print_count("Skipped", report.skipped(), output)?;
    print_count("Blocked", report.blocked(), output)?;
    print_entries(report, output)?;
    print_zoxide_report(report.zoxide(), dry_run, output)
}

fn print_phase_completion(
    phase: Phase,
    summary: PhaseSummary,
    output: &mut Output<'_>,
) -> io::Result<()> {
    match phase {
        Phase::Checking => print_phase("Checked", summary.count(), summary.elapsed(), output),
        Phase::Preparing => {
            print_count_with_elapsed("Prepared", summary.count(), summary.elapsed(), true, output)
        }
        Phase::Updating => {
            print_count_with_elapsed("Updated", summary.count(), summary.elapsed(), false, output)
        }
        Phase::Seeding => {
            print_count_with_elapsed("Seeded", summary.count(), summary.elapsed(), false, output)
        }
    }
}

fn print_dry_run_summary(report: &Report, output: &mut Output<'_>) -> io::Result<()> {
    print_count("Would clone", report.planned_clones(), output)?;
    print_count("Would fetch", report.planned_fetches(), output)?;
    if report.planned_clones() == 0 && report.planned_fetches() == 0 && !report.has_failures() {
        write_line(output, format_args!("Would make no changes"))?;
    }
    Ok(())
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
            Outcome::Planned(Plan::Clone { url }) => {
                let repository = terminal_text(entry.repository());
                write_line(
                    output,
                    format_args!(
                        " {} {} {}",
                        "+".green(),
                        repository.bold(),
                        format!("from {}", terminal_text(url)).dimmed()
                    ),
                )?;
            }
            Outcome::Planned(Plan::Fetch { .. }) | Outcome::Current { .. } => {}
            Outcome::Cloned { url, cache } => {
                let repository = terminal_text(entry.repository());
                write_line(
                    output,
                    format_args!(
                        " {} {} {}",
                        "+".green(),
                        repository.bold(),
                        format!("from {} {}", terminal_text(url), cache_annotation(*cache))
                            .dimmed()
                    ),
                )?;
            }
            Outcome::Updated { branch, before, after } => {
                let repository = terminal_text(entry.repository());
                write_line(
                    output,
                    format_args!(
                        " {} {} {}",
                        "~".yellow(),
                        repository.bold(),
                        terminal_text(&format!("{branch} {before} -> {after}")).dimmed()
                    ),
                )?;
            }
            Outcome::UpdatedButRestorationFailed { branch, before, after, message } => {
                let repository = terminal_text(entry.repository());
                let message = safe_message(message);
                write_line(
                    output,
                    format_args!(
                        " {} {} {}",
                        "x".red(),
                        repository.bold(),
                        terminal_text(&format!(
                            "{branch} {before} -> {after}; restoring the original branch failed: {message}"
                        ))
                        .dimmed()
                    ),
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

        if let Some(warning) = entry.warning() {
            // Outcomes that print no line of their own need a header so the
            // note has a subject.
            let has_line = !matches!(
                entry.outcome(),
                Outcome::Planned(Plan::Fetch { .. }) | Outcome::Current { .. }
            );
            if !has_line {
                let repository = terminal_text(entry.repository());
                write_line(output, format_args!(" {} {}", "=".cyan(), repository.bold()))?;
            }
            write_line(
                output,
                format_args!(
                    "    {}",
                    format!("clone cache not seeded: {}", safe_message(warning)).dimmed()
                ),
            )?;
        }
    }
    Ok(())
}

fn print_zoxide_report(
    report: Option<&ZoxideReport>,
    dry_run: bool,
    output: &mut Output<'_>,
) -> io::Result<()> {
    let Some(report) = report else {
        return Ok(());
    };

    write_line(output, format_args!("Zoxide"))?;
    write_line(output, format_args!(""))?;

    if let Some(message) = report.unavailable_message() {
        let message = safe_message(message);
        write_line(output, format_args!(" {} {}", "x".red(), message.dimmed()))?;
        return Ok(());
    }

    if report.entries().is_empty() {
        let message = if dry_run {
            "No repositories would be registered"
        } else {
            "No repositories to register"
        };
        write_line(output, format_args!("{message}"))?;
        return Ok(());
    }

    for entry in report.entries() {
        match entry.outcome() {
            ZoxideOutcome::WouldRegister => {
                let repository = terminal_text(entry.repository());
                write_line(
                    output,
                    format_args!(
                        " {} {} {}",
                        "?".cyan(),
                        repository.bold(),
                        "would register".dimmed()
                    ),
                )?;
            }
            ZoxideOutcome::Added => {
                let repository = terminal_text(entry.repository());
                write_line(
                    output,
                    format_args!(" {} {} {}", "+".green(), repository.bold(), "added".dimmed()),
                )?;
            }
            ZoxideOutcome::AlreadyRegistered => {
                let repository = terminal_text(entry.repository());
                write_line(
                    output,
                    format_args!(
                        " {} {} {}",
                        "=".cyan(),
                        repository.bold(),
                        "already registered".dimmed()
                    ),
                )?;
            }
            ZoxideOutcome::Failed(message) => {
                let repository = terminal_text(entry.repository());
                let message = safe_message(message);
                write_line(
                    output,
                    format_args!(" {} {} {}", "x".red(), repository.bold(), message.dimmed()),
                )?;
            }
        }
    }
    Ok(())
}

fn change_rank(outcome: &Outcome) -> u8 {
    match outcome {
        Outcome::Planned(Plan::Clone { .. }) | Outcome::Cloned { .. } => 0,
        Outcome::Updated { .. } | Outcome::UpdatedButRestorationFailed { .. } => 1,
        Outcome::Skipped { .. } => 2,
        Outcome::Blocked { .. } => 3,
        Outcome::Planned(Plan::Fetch { .. }) | Outcome::Current { .. } => 4,
    }
}
