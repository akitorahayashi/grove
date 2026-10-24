use std::io;
use std::path::PathBuf;

use clap::Args;
use owo_colors::OwoColorize;

use crate::AppError;
use crate::app::api;
use crate::app::sync::{
    CacheRemoval, Outcome, Phase, PhaseSummary, Plan, RemovalEntry, RemovalOutcome, RemovalPlan,
    Report, SyncOptions, ZoxideOutcome, ZoxideReport,
};

use crate::cli::Completion;
use crate::cli::output::{Output, terminal_text};
use crate::cli::tty::progress::{ProgressPhase, run_with_progress};
use crate::cli::tty::report::{
    cache_annotation, entry_line, print_blocked_details, print_count, print_count_with_elapsed,
    print_phase, safe_message, write_line,
};
use crate::cli::tty::table::Paint;

#[derive(Args)]
pub(in crate::cli) struct SyncCommand {
    #[arg(value_name = "repo")]
    repositories: Vec<String>,

    #[arg(long)]
    dry_run: bool,

    #[arg(short = 'z', long)]
    register_zoxide: bool,

    #[arg(short = 'i', long, help = "Ignore sibling override files when loading configuration")]
    ignore_overrides: bool,
}

pub(in crate::cli) fn run(
    config: Option<PathBuf>,
    command: SyncCommand,
    output: &mut Output<'_>,
) -> Result<Completion, AppError> {
    let config = super::resolve_config(config, super::ConfigNotice::WhenAscended, output)?;
    let options = SyncOptions::new(command.dry_run, command.register_zoxide)
        .ignore_overrides(command.ignore_overrides);
    let report = if command.dry_run {
        api::sync(Some(config), command.repositories, options)?
    } else {
        run_with_progress(
            output,
            "sync",
            move |sender| {
                api::sync_with_events(Some(config), command.repositories, options, &sender)
            },
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
            Phase::Removing => "Removing repositories...",
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
    print_removals(report, output)?;
    print_entries(report, output)?;
    print_zoxide_report(report.zoxide(), dry_run, output)
}

fn print_phase_completion(
    phase: Phase,
    summary: PhaseSummary,
    output: &mut Output<'_>,
) -> io::Result<()> {
    match phase {
        Phase::Removing => {
            print_count_with_elapsed("Removed", summary.count(), summary.elapsed(), true, output)
        }
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
    print_count("Would move", report.planned_moves(), output)?;
    print_count("Would trash", report.planned_trashes(), output)?;
    let planned = report.planned_clones()
        + report.planned_fetches()
        + report.planned_moves()
        + report.planned_trashes();
    if planned == 0 && !report.has_failures() {
        write_line(output, format_args!("Would make no changes"))?;
    }
    Ok(())
}

fn print_entries(report: &Report, output: &mut Output<'_>) -> io::Result<()> {
    let mut entries = report.entries().iter().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.repository());

    for entry in entries {
        match entry.outcome() {
            Outcome::Planned(Plan::Clone { url }) => {
                entry_line(
                    output,
                    "+",
                    Paint::Green,
                    entry.repository(),
                    &format!("from {}", terminal_text(url)),
                )?;
            }
            Outcome::Planned(Plan::Fetch { .. } | Plan::Move { .. }) | Outcome::Current { .. } => {}
            Outcome::Cloned { url, cache } => {
                entry_line(
                    output,
                    "+",
                    Paint::Green,
                    entry.repository(),
                    &format!("from {} {}", terminal_text(url), cache_annotation(*cache)),
                )?;
            }
            Outcome::Updated { branch, before, after } => {
                entry_line(
                    output,
                    "~",
                    Paint::Yellow,
                    entry.repository(),
                    &terminal_text(&format!("{branch} {before} -> {after}")),
                )?;
            }
            Outcome::UpdatedButRestorationFailed { branch, before, after, message } => {
                let message = safe_message(message);
                entry_line(
                    output,
                    "x",
                    Paint::Red,
                    entry.repository(),
                    &terminal_text(&format!(
                        "{branch} {before} -> {after}; restoring the original branch failed: {message}"
                    )),
                )?;
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

        if let Some(warning) = entry.warning() {
            // Outcomes that print no line of their own need a header so the
            // note has a subject.
            let has_line = !matches!(
                entry.outcome(),
                Outcome::Planned(Plan::Fetch { .. } | Plan::Move { .. }) | Outcome::Current { .. }
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

fn print_removals(report: &Report, output: &mut Output<'_>) -> io::Result<()> {
    let mut entries = report.removals().iter().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.repository());

    for entry in entries {
        print_removal(entry, output)?;
    }
    Ok(())
}

fn print_removal(entry: &RemovalEntry, output: &mut Output<'_>) -> io::Result<()> {
    let repository = entry.repository();
    match entry.outcome() {
        RemovalOutcome::Planned(RemovalPlan::Move { to }) => {
            let detail = terminal_text(&format!("would move to {to}"));
            entry_line(output, ">", Paint::Cyan, repository, &detail)?;
        }
        RemovalOutcome::Planned(RemovalPlan::Trash) => {
            entry_line(output, "-", Paint::Cyan, repository, "would move to the trash")?;
        }
        RemovalOutcome::Moved { to } => {
            let detail = terminal_text(&format!("moved to {to}"));
            entry_line(output, ">", Paint::Green, repository, &detail)?;
        }
        RemovalOutcome::Trashed => {
            entry_line(output, "-", Paint::Green, repository, "moved to the trash")?;
        }
        RemovalOutcome::Blocked { reason } => {
            entry_line(output, "x", Paint::Red, repository, &safe_message(&reason.message()))?;
        }
        RemovalOutcome::Absent => {
            write_line(
                output,
                format_args!(" {} {}", "=".cyan(), terminal_text(repository).bold()),
            )?;
        }
    }

    let cache = match entry.cache() {
        CacheRemoval::Removed => Some("clone cache entry removed"),
        CacheRemoval::WouldRemove => Some("clone cache entry would be removed"),
        _ => None,
    };
    if let Some(cache) = cache {
        write_line(output, format_args!("    {}", cache.dimmed()))?;
    }
    if let Some(warning) = entry.warning() {
        write_line(output, format_args!("    {}", safe_message(warning).dimmed()))?;
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
                entry_line(output, "?", Paint::Cyan, entry.repository(), "would register")?;
            }
            ZoxideOutcome::Added => {
                entry_line(output, "+", Paint::Green, entry.repository(), "added")?;
            }
            ZoxideOutcome::AlreadyRegistered => {
                entry_line(output, "=", Paint::Cyan, entry.repository(), "already registered")?;
            }
            ZoxideOutcome::Failed(message) => {
                entry_line(output, "x", Paint::Red, entry.repository(), &safe_message(message))?;
            }
        }
    }
    Ok(())
}
