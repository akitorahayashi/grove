//! CLI adapter.

mod commands;
mod output;
mod tty;

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, error::ErrorKind};

use crate::AppError;
use crate::repositories::redact_urls_for_display;
use output::{Output, terminal_multiline_text, terminal_text};

#[derive(Parser)]
#[command(name = "gv")]
#[command(version)]
#[command(about = "Manage multiple Git repositories from grove.toml", long_about = None)]
struct Cli {
    #[arg(long, global = true, value_name = "path")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(visible_alias = "c", about = "Inspect and clean the local clone cache")]
    Cache(commands::cache::CacheCommand),
    #[command(
        visible_alias = "cl",
        about = "Clone a repository through the local cache, without grove.toml"
    )]
    Clone(commands::clone::CloneCommand),
    #[command(visible_alias = "i", about = "Create grove.toml in the current directory")]
    Init(commands::init::InitCommand),
    #[command(
        visible_alias = "rf",
        about = "Update existing repositories and switch to their default branches"
    )]
    Refresh(commands::refresh::RefreshCommand),
    #[command(
        visible_alias = "s",
        about = "Clone missing repositories and safely update existing repositories"
    )]
    Sync(commands::sync::SyncCommand),
    #[command(visible_aliases = ["st", "ts"], about = "Show managed repository status")]
    Status(commands::status::StatusCommand),
    #[command(visible_alias = "vl", about = "Validate grove.toml without inspecting repositories")]
    Validate(commands::validate::ValidateCommand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::cli) enum Completion {
    Success,
    Failure,
}

/// Entry point for the CLI.
pub fn run() -> ExitCode {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let mut output = Output::terminal(&mut stdout, &mut stderr);
    run_with_args(std::env::args_os(), &mut output)
}

fn run_with_args(args: impl IntoIterator<Item = OsString>, output: &mut Output<'_>) -> ExitCode {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => return render_clap_error(error, output),
    };

    let result = match cli.command {
        Commands::Cache(command) => commands::cache::run(cli.config, command, output),
        Commands::Clone(command) => commands::clone::run(cli.config, command, output),
        Commands::Init(command) => commands::init::run(cli.config, command, output),
        Commands::Refresh(command) => commands::refresh::run(cli.config, command, output),
        Commands::Sync(command) => commands::sync::run(cli.config, command, output),
        Commands::Status(command) => commands::status::run(cli.config, command, output),
        Commands::Validate(command) => commands::validate::run(cli.config, command, output),
    };

    match result {
        Ok(Completion::Success) => ExitCode::SUCCESS,
        Ok(Completion::Failure) => ExitCode::FAILURE,
        Err(AppError::Io(error)) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            let message = terminal_text(&redact_urls_for_display(&error.to_string()));
            if output.stderr(format_args!("error: {message}\n")).is_err() {
                return ExitCode::FAILURE;
            }
            ExitCode::FAILURE
        }
    }
}

fn render_clap_error(error: clap::Error, output: &mut Output<'_>) -> ExitCode {
    let success = matches!(error.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion);
    let rendered = terminal_multiline_text(&error.to_string());
    let written = if success {
        output.stdout(format_args!("{rendered}"))
    } else {
        output.stderr(format_args!("{rendered}"))
    };
    if written.is_err() || !success { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}
