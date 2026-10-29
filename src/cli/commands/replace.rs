use std::io;
use std::path::PathBuf;

use clap::Args;

use crate::AppError;
use crate::app::api;
use crate::app::replace::{ConfigurationOutcome, RepointEntry, RepointOutcome, Report};
use crate::removal;

use crate::cli::Completion;
use crate::cli::output::{Output, terminal_text};
use crate::cli::tty::report::{
    entry_line, print_blocked_details, print_cache_drop, print_count, print_removals, repositories,
    safe_message, write_line,
};
use crate::cli::tty::table::Paint;

#[derive(Args)]
pub(in crate::cli) struct ReplaceCommand {
    #[arg(
        value_name = "candidate",
        help = "The configuration to converge to and write in place of the root configuration"
    )]
    candidate: PathBuf,

    #[arg(long)]
    dry_run: bool,
}

pub(in crate::cli) fn run(
    config: Option<PathBuf>,
    command: ReplaceCommand,
    output: &mut Output<'_>,
) -> Result<Completion, AppError> {
    let report = api::replace(config, command.candidate, command.dry_run)?;
    let completion = if report.has_failures() { Completion::Failure } else { Completion::Success };

    // A provisioning caller acts on the exit status, which a reader closing
    // stdout early must not turn into the success a broken pipe otherwise maps to.
    match print_report(&report, &mut output.reporting_on_stdout()) {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(completion),
        result => result.map(|()| completion).map_err(AppError::from),
    }
}

fn print_report(report: &Report, output: &mut Output<'_>) -> io::Result<()> {
    let removals = |predicate: fn(&removal::Outcome) -> bool| {
        report.removals().iter().filter(|entry| predicate(entry.outcome())).count()
    };
    let repoints = |predicate: fn(&RepointOutcome) -> bool| {
        report.repoints().iter().filter(|entry| predicate(entry.outcome())).count()
    };
    print_count(
        "Would move",
        removals(|outcome| {
            matches!(outcome, removal::Outcome::Planned(removal::Plan::Move { .. }))
        }),
        output,
    )?;
    print_count(
        "Would trash",
        removals(|outcome| matches!(outcome, removal::Outcome::Planned(removal::Plan::Trash))),
        output,
    )?;
    print_count(
        "Would repoint",
        repoints(|outcome| matches!(outcome, RepointOutcome::Planned { .. })),
        output,
    )?;
    print_count(
        "Moved",
        removals(|outcome| matches!(outcome, removal::Outcome::Moved { .. })),
        output,
    )?;
    print_count(
        "Trashed",
        removals(|outcome| matches!(outcome, removal::Outcome::Trashed)),
        output,
    )?;
    print_count(
        "Repointed",
        repoints(|outcome| matches!(outcome, RepointOutcome::Repointed { .. })),
        output,
    )?;
    print_count(
        "Blocked",
        removals(|outcome| matches!(outcome, removal::Outcome::Blocked { .. }))
            + repoints(|outcome| matches!(outcome, RepointOutcome::Blocked { .. })),
        output,
    )?;
    print_removals(report.removals(), output)?;
    let mut entries = report.repoints().iter().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.repository());
    for entry in entries {
        print_repoint(entry, output)?;
    }
    for entry in report.cache() {
        print_cache_drop(entry, output)?;
    }
    print_configuration(report, output)?;
    if report.not_cloned() > 0 {
        write_line(
            output,
            format_args!("{} not cloned; run gv sync", repositories(report.not_cloned())),
        )?;
    }
    Ok(())
}

fn print_repoint(entry: &RepointEntry, output: &mut Output<'_>) -> io::Result<()> {
    let repository = entry.repository();
    match entry.outcome() {
        RepointOutcome::Planned { url } => {
            let detail = format!("would repoint origin to {}", terminal_text(url));
            entry_line(output, "~", Paint::Cyan, repository, &detail)
        }
        RepointOutcome::Repointed { url } => {
            let detail = format!("origin repointed to {}", terminal_text(url));
            entry_line(output, "~", Paint::Green, repository, &detail)
        }
        RepointOutcome::Blocked { reason } => {
            entry_line(output, "x", Paint::Red, repository, &safe_message(&reason.message()))?;
            print_blocked_details(entry.blocked_details(), output)
        }
    }
}

fn print_configuration(report: &Report, output: &mut Output<'_>) -> io::Result<()> {
    let path = report.destination().display().to_string();
    match report.configuration() {
        ConfigurationOutcome::Kept => {
            entry_line(output, "x", Paint::Red, &path, "kept until every repository converges")
        }
        ConfigurationOutcome::WouldWrite => {
            entry_line(output, "~", Paint::Cyan, &path, "would be written")
        }
        ConfigurationOutcome::Current => {
            entry_line(output, "=", Paint::Dimmed, &path, "already current")
        }
        ConfigurationOutcome::Written => entry_line(output, "~", Paint::Green, &path, "written"),
        ConfigurationOutcome::DurabilityUnconfirmed(message) => entry_line(
            output,
            "!",
            Paint::Yellow,
            &path,
            &format!("written; durability not confirmed: {}", safe_message(message)),
        ),
        ConfigurationOutcome::WriteFailed(message) => entry_line(
            output,
            "x",
            Paint::Red,
            &path,
            &safe_message(&format!("not written: {message}")),
        ),
    }
}
