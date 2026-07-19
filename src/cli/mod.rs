//! CLI adapter.

mod item;
mod label;
mod labeling;

use crate::AppError;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rs-cli-tmpl")]
#[command(version)]
#[command(
    about = "Reference architecture for building Rust CLI tools",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Manage items", visible_alias = "i")]
    Item {
        #[command(subcommand)]
        command: item::ItemCommand,
    },
    #[command(about = "Manage labels", visible_alias = "l")]
    Label {
        #[command(subcommand)]
        command: label::LabelCommand,
    },
    #[command(about = "Manage item-label assignments", visible_alias = "ln")]
    Labeling {
        #[command(subcommand)]
        command: labeling::LabelingCommand,
    },
}

/// Entry point for the CLI.
pub fn run() {
    let cli = Cli::parse();

    let result: Result<(), AppError> = match cli.command {
        Commands::Item { command } => item::run(command),
        Commands::Label { command } => label::run(command),
        Commands::Labeling { command } => labeling::run(command),
    };

    if let Err(err) = result {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}
