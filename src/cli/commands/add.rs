use std::io;
use std::path::PathBuf;

use clap::Args;

use crate::AppError;
use crate::app::add::{Entry, Event, Options, Outcome, Report};
use crate::app::api;

use crate::cli::Completion;
use crate::cli::output::{Output, terminal_text};
use crate::cli::tty::report::{entry_line, safe_message, write_line};
use crate::cli::tty::table::Paint;

#[derive(Args)]
pub(in crate::cli) struct AddCommand {
    #[arg(value_name = "path")]
    paths: Vec<PathBuf>,

    #[arg(short = 'o', long = "override")]
    override_file: bool,

    #[arg(long)]
    dry_run: bool,
}

pub(in crate::cli) fn run(
    config: Option<PathBuf>,
    command: AddCommand,
    output: &mut Output<'_>,
) -> Result<Completion, AppError> {
    let config = super::resolve_config(config, super::ConfigNotice::Never, output)?;
    let cwd = std::env::current_dir()?;
    let options = Options::new(command.override_file, command.dry_run);
    let report = api::add_with_events(config, cwd, command.paths, options, |event| match event {
        Event::Destination(destination) => {
            let destination = terminal_text(&destination.display().to_string());
            write_line(output, format_args!("Config: {destination}")).map_err(AppError::from)
        }
        Event::Entry(entry) => print_entry(entry, output).map_err(AppError::from),
    })?;
    print_summary(&report, output)?;

    if report.has_failure() { Ok(Completion::Failure) } else { Ok(Completion::Success) }
}

fn print_entry(entry: &Entry, output: &mut Output<'_>) -> io::Result<()> {
    let label = entry
        .name()
        .map(str::to_string)
        .unwrap_or_else(|| terminal_text(&entry.input().display().to_string()));
    match entry.outcome() {
        Outcome::Added => entry_line(
            output,
            "+",
            Paint::Green,
            &label,
            &terminal_text(entry.display_path().unwrap_or_default()),
        ),
        Outcome::Planned => entry_line(
            output,
            "+",
            Paint::Green,
            &label,
            &format!("would add {}", terminal_text(entry.display_path().unwrap_or_default())),
        ),
        Outcome::Unchanged => entry_line(
            output,
            "=",
            Paint::Dimmed,
            &label,
            &format!(
                "already configured at {}",
                terminal_text(entry.display_path().unwrap_or_default())
            ),
        ),
        Outcome::Failed(message) => {
            entry_line(output, "x", Paint::Red, &label, &safe_message(message))
        }
    }
}

fn print_summary(report: &Report, output: &mut Output<'_>) -> io::Result<()> {
    if !report.has_failure() {
        return Ok(());
    }
    let (changed, label) =
        if report.dry_run() { (report.planned(), "planned") } else { (report.added(), "written") };
    write_line(
        output,
        format_args!(
            "Stopped: {} {}, {} unchanged, {} failed, {} not attempted",
            changed,
            label,
            report.unchanged(),
            report.failed(),
            report.not_attempted()
        ),
    )
}
