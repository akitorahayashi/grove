//! CLI adapter.

mod init;
mod list;
mod printer;
mod status;
mod sync;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::AppError;

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
    #[command(visible_alias = "i", about = "Create grove.toml in the current directory")]
    Init(init::InitCommand),
    #[command(
        visible_alias = "s",
        about = "Clone missing repositories and safely update existing repositories"
    )]
    Sync(sync::SyncCommand),
    #[command(visible_alias = "st", about = "Show managed repository status")]
    Status(status::StatusCommand),
    #[command(visible_alias = "ls", about = "List managed repositories")]
    List(list::ListCommand),
}

/// Entry point for the CLI.
pub fn run() {
    let cli = Cli::parse();
    let result: Result<(), AppError> = match cli.command {
        Commands::Init(command) => init::run(cli.config, command),
        Commands::Sync(command) => sync::run(cli.config, command),
        Commands::Status(command) => status::run(cli.config, command),
        Commands::List(command) => list::run(cli.config, command),
    };

    if let Err(err) = result {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
