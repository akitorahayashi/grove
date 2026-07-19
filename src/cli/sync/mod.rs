mod progress;

use std::fmt::{Arguments, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use clap::Args;
use owo_colors::OwoColorize;

use crate::AppError;
use crate::app::api;
use crate::app::sync::{
    BlockedReasonDetails, Outcome, Phase, PhaseSummary, Plan, Report, SyncOptions, ZoxideOutcome,
    ZoxideReport,
};
use crate::git::redact_url_for_display;

use self::progress::Display;
use super::printer::Printer;

#[derive(Args)]
pub(super) struct SyncCommand {
    #[arg(value_name = "repo")]
    repositories: Vec<String>,

    #[arg(long)]
    dry_run: bool,

    #[arg(short = 'z', long)]
    register_zoxide: bool,
}

pub(super) fn run(config: Option<PathBuf>, command: SyncCommand) -> Result<(), AppError> {
    let printer = Printer::Default;
    let options = SyncOptions::new(command.dry_run, command.register_zoxide);
    let report = if command.dry_run {
        api::sync_with_options(config, command.repositories, options)?
    } else {
        run_with_progress(config, command.repositories, options, printer)?
    };

    print_report(&report, command.dry_run, printer);

    if report.has_failures() {
        std::process::exit(1);
    }

    Ok(())
}

fn run_with_progress(
    config: Option<PathBuf>,
    repositories: Vec<String>,
    options: SyncOptions,
    printer: Printer,
) -> Result<Report, AppError> {
    let (sender, receiver) = mpsc::channel();

    std::thread::scope(|scope| {
        let execution =
            scope.spawn(move || api::sync_with_events(config, repositories, options, &sender));
        let mut progress = Display::new(printer);

        for event in receiver {
            if let Some(completion) = progress.handle(event) {
                print_phase_completion(completion.phase(), completion.summary(), printer);
            }
        }

        progress.finish();
        execution.join().expect("sync execution thread should not panic")
    })
}

fn print_report(report: &Report, dry_run: bool, printer: Printer) {
    if dry_run {
        print_dry_run_summary(report, printer);
    }

    print_count("Skipped", report.skipped(), printer);
    print_count("Blocked", report.blocked(), printer);
    print_entries(report, printer);
    print_zoxide_report(report.zoxide(), dry_run, printer);
}

fn print_phase_completion(phase: Phase, summary: PhaseSummary, printer: Printer) {
    match phase {
        Phase::Checking => print_phase("Checked", summary, printer),
        Phase::Preparing => print_count_with_elapsed("Prepared", summary, true, printer),
        Phase::Updating => print_count_with_elapsed("Updated", summary, false, printer),
    }
}

fn print_dry_run_summary(report: &Report, printer: Printer) {
    print_count("Would clone", report.planned_clones(), printer);
    print_count("Would fetch", report.planned_fetches(), printer);
    if report.planned_clones() == 0 && report.planned_fetches() == 0 && !report.has_failures() {
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
            Outcome::Planned(Plan::Fetch { .. }) | Outcome::Current { .. } => {}
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
                print_blocked_details(entry.blocked_details(), printer);
            }
        }
    }
}

fn print_blocked_details(details: Option<&BlockedReasonDetails>, printer: Printer) {
    if let Some(BlockedReasonDetails::RemoteUrlMismatch { actual, expected }) = details {
        write_line(
            printer,
            format_args!(
                "    {}",
                format!("actual:   {}", redact_url_for_display(actual)).dimmed()
            ),
        );
        write_line(
            printer,
            format_args!(
                "    {}",
                format!("expected: {}", redact_url_for_display(expected)).dimmed()
            ),
        );
    }
}

fn print_zoxide_report(report: Option<&ZoxideReport>, dry_run: bool, printer: Printer) {
    let Some(report) = report else {
        return;
    };

    write_line(printer, format_args!("Zoxide"));
    write_line(printer, format_args!(""));

    if let Some(message) = report.unavailable_message() {
        write_line(printer, format_args!(" {} {}", "x".red(), message.dimmed()));
        return;
    }

    if report.entries().is_empty() {
        let message = if dry_run {
            "No repositories would be registered"
        } else {
            "No repositories to register"
        };
        write_line(printer, format_args!("{message}"));
        return;
    }

    for entry in report.entries() {
        match entry.outcome() {
            ZoxideOutcome::WouldRegister => {
                write_line(
                    printer,
                    format_args!(
                        " {} {} {}",
                        "?".cyan(),
                        entry.repository().bold(),
                        "would register".dimmed()
                    ),
                );
            }
            ZoxideOutcome::Added => {
                write_line(
                    printer,
                    format_args!(
                        " {} {} {}",
                        "+".green(),
                        entry.repository().bold(),
                        "added".dimmed()
                    ),
                );
            }
            ZoxideOutcome::AlreadyRegistered => {
                write_line(
                    printer,
                    format_args!(
                        " {} {} {}",
                        "=".cyan(),
                        entry.repository().bold(),
                        "already registered".dimmed()
                    ),
                );
            }
            ZoxideOutcome::Failed(message) => {
                write_line(
                    printer,
                    format_args!(
                        " {} {} {}",
                        "x".red(),
                        entry.repository().bold(),
                        message.dimmed()
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
        print_count_with_elapsed(label, summary, true, printer);
    }
}

fn print_count_with_elapsed(label: &str, summary: PhaseSummary, show_zero: bool, printer: Printer) {
    if summary.count() == 0 && !show_zero {
        return;
    }

    write_line(
        printer,
        format_args!(
            "{}",
            format!(
                "{label} {} {}",
                repositories(summary.count()).bold(),
                format!("in {}", format_duration(summary.elapsed())).dimmed()
            )
            .dimmed()
        ),
    );
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
        Outcome::Planned(Plan::Fetch { .. }) | Outcome::Current { .. } => 4,
    }
}
