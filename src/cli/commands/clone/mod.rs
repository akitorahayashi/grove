use std::io;
use std::path::PathBuf;

use clap::Args;
use owo_colors::OwoColorize;

use crate::AppError;
use crate::app::api;
use crate::app::clone::{Phase, PhaseSummary, Report};

use crate::cli::Completion;
use crate::cli::output::{Output, terminal_text};
use crate::cli::tty::progress::{ProgressPhase, run_with_progress};
use crate::cli::tty::report::{cache_annotation, write_line};

#[derive(Args)]
pub(in crate::cli) struct CloneCommand {
    #[arg(value_name = "url")]
    url: String,

    #[arg(value_name = "dest")]
    dest: Option<PathBuf>,
}

pub(in crate::cli) fn run(
    config: Option<PathBuf>,
    command: CloneCommand,
    output: &mut Output<'_>,
) -> Result<Completion, AppError> {
    if config.is_some() {
        return Err(AppError::config_error("--config cannot be used with clone"));
    }

    let report = run_with_progress(
        output,
        "clone",
        move |sender| api::clone_with_events(command.url, command.dest, &sender),
        print_phase_completion,
    )?;

    print_report(&report, output)?;
    Ok(Completion::Success)
}

impl ProgressPhase for Phase {
    fn message(self) -> &'static str {
        match self {
            Phase::Cloning => "Cloning repository...",
        }
    }

    fn shows_git_progress(self) -> bool {
        true
    }
}

fn print_phase_completion(
    _phase: Phase,
    _summary: PhaseSummary,
    _output: &mut Output<'_>,
) -> io::Result<()> {
    Ok(())
}

fn print_report(report: &Report, output: &mut Output<'_>) -> io::Result<()> {
    let destination = terminal_text(&report.destination().display().to_string());
    write_line(
        output,
        format_args!(
            " {} {} {}",
            "+".green(),
            destination.bold(),
            format!("from {} {}", terminal_text(report.url()), cache_annotation(report.cache()))
                .dimmed()
        ),
    )
}
