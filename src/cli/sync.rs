use std::fmt::{Arguments, Write};
use std::path::PathBuf;
use std::time::Duration;

use clap::Args;
use owo_colors::OwoColorize;

use crate::AppError;
use crate::app::api;
use crate::app::sync::{Outcome, PhaseSummary, Plan, Report};

use super::printer::Printer;
use super::reporters::SyncReporter;

#[derive(Args)]
pub(super) struct SyncCommand {
    #[arg(value_name = "repo")]
    repositories: Vec<String>,

    #[arg(long)]
    dry_run: bool,
}

pub(super) fn run(config: Option<PathBuf>, command: SyncCommand) -> Result<(), AppError> {
    let printer = Printer::Default;
    let mut reporter = SyncReporter::new(printer);
    let report =
        api::sync_with_observer(config, command.repositories, command.dry_run, &mut reporter)?;
    reporter.finish();

    print_report(&report, command.dry_run, printer);

    if report.has_failures() {
        std::process::exit(1);
    }

    Ok(())
}

fn print_report(report: &Report, dry_run: bool, printer: Printer) {
    if dry_run {
        print_dry_run_summary(report, printer);
    } else {
        let phases = report.phases();
        print_phase("Checked", phases.checked(), printer);
        print_phase_if_nonzero("Fetched", phases.fetched(), printer);
        print_phase_if_nonzero("Cloned", phases.cloned(), printer);
        print_count_with_elapsed("Updated", report.updated(), phases.updated().elapsed(), printer);
    }

    print_count("Skipped", report.skipped(), printer);
    print_count("Blocked", report.blocked(), printer);
    print_entries(report, printer);
}

fn print_dry_run_summary(report: &Report, printer: Printer) {
    print_count("Would clone", report.planned_clones(), printer);
    print_count("Would fetch", report.planned_checks(), printer);
    if report.planned_clones() == 0 && report.planned_checks() == 0 && !report.has_failures() {
        write_line(printer, format_args!("Would make no changes"));
    }
}

fn print_entries(report: &Report, printer: Printer) {
    let mut entries = report.entries().iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        left.repository()
            .cmp(right.repository())
            .then_with(|| change_rank(left.outcome()).cmp(&change_rank(right.outcome())))
    });

    for entry in entries {
        match entry.outcome() {
            Outcome::Planned(Plan::Clone { url }) => {
                write_line(
                    printer,
                    format_args!(
                        " {} {} {}",
                        "+".green(),
                        entry.repository().bold(),
                        format!("from {url}").dimmed()
                    ),
                );
            }
            Outcome::Planned(Plan::CheckExisting { .. }) | Outcome::Current { .. } => {}
            Outcome::Cloned { url } => {
                write_line(
                    printer,
                    format_args!(
                        " {} {} {}",
                        "+".green(),
                        entry.repository().bold(),
                        format!("from {url}").dimmed()
                    ),
                );
            }
            Outcome::Updated { branch, before, after } => {
                write_line(
                    printer,
                    format_args!(
                        " {} {} {}",
                        "~".yellow(),
                        entry.repository().bold(),
                        format!("{branch} {before} -> {after}").dimmed()
                    ),
                );
            }
            Outcome::Skipped { reason } => {
                write_line(
                    printer,
                    format_args!(
                        " {} {} {}",
                        "!".yellow(),
                        entry.repository().bold(),
                        reason.message().dimmed()
                    ),
                );
            }
            Outcome::Blocked { reason } => {
                write_line(
                    printer,
                    format_args!(
                        " {} {} {}",
                        "x".red(),
                        entry.repository().bold(),
                        reason.message().dimmed()
                    ),
                );
            }
        }
    }
}

fn print_phase(label: &str, summary: PhaseSummary, printer: Printer) {
    if summary.count() == 0 {
        write_line(
            printer,
            format_args!(
                "{}",
                format!("{label} in {}", format_duration(summary.elapsed())).dimmed()
            ),
        );
    } else {
        print_count_with_elapsed(label, summary.count(), summary.elapsed(), printer);
    }
}

fn print_phase_if_nonzero(label: &str, summary: PhaseSummary, printer: Printer) {
    if summary.count() > 0 {
        print_count_with_elapsed(label, summary.count(), summary.elapsed(), printer);
    }
}

fn print_count_with_elapsed(label: &str, count: usize, elapsed: Duration, printer: Printer) {
    if count > 0 {
        write_line(
            printer,
            format_args!(
                "{}",
                format!(
                    "{label} {} {}",
                    repositories(count).bold(),
                    format!("in {}", format_duration(elapsed)).dimmed()
                )
                .dimmed()
            ),
        );
    }
}

fn print_count(label: &str, count: usize, printer: Printer) {
    if count > 0 {
        write_line(
            printer,
            format_args!("{}", format!("{label} {}", repositories(count).bold()).dimmed()),
        );
    }
}

fn write_line(printer: Printer, arguments: Arguments<'_>) {
    let mut stderr = printer.stderr();
    let _ = writeln!(stderr, "{arguments}");
}

fn repositories(count: usize) -> String {
    match count {
        1 => "1 repository".to_string(),
        _ => format!("{count} repositories"),
    }
}

fn format_duration(duration: Duration) -> String {
    if duration.as_secs() > 0 {
        format!("{:.2}s", duration.as_secs_f64())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

fn change_rank(outcome: &Outcome) -> u8 {
    match outcome {
        Outcome::Planned(Plan::Clone { .. }) | Outcome::Cloned { .. } => 0,
        Outcome::Updated { .. } => 1,
        Outcome::Skipped { .. } => 2,
        Outcome::Blocked { .. } => 3,
        Outcome::Planned(Plan::CheckExisting { .. }) | Outcome::Current { .. } => 4,
    }
}
