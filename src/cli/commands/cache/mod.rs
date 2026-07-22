use std::path::PathBuf;
use std::time::SystemTime;

use clap::{Args, Subcommand};
use owo_colors::OwoColorize;

use crate::AppError;
use crate::app::api;
use crate::cache::EntryInfo;

use crate::cli::Completion;
use crate::cli::output::Output;
use crate::cli::tty::report::safe_message;
use crate::cli::tty::table::{Cell, Paint, Table};

#[derive(Args)]
pub(in crate::cli) struct CacheCommand {
    #[command(subcommand)]
    command: CacheSubcommand,
}

#[derive(Subcommand)]
enum CacheSubcommand {
    #[command(visible_alias = "ls", about = "List cached repositories")]
    List(ListCommand),
    #[command(visible_alias = "cln", about = "Remove cached repositories, or those named")]
    Clean(CleanCommand),
}

#[derive(Args)]
struct ListCommand;

#[derive(Args)]
struct CleanCommand {
    #[arg(value_name = "repo")]
    repositories: Vec<String>,
}

pub(in crate::cli) fn run(
    config: Option<PathBuf>,
    command: CacheCommand,
    output: &mut Output<'_>,
) -> Result<Completion, AppError> {
    match command.command {
        CacheSubcommand::List(_) => run_list(output),
        CacheSubcommand::Clean(clean) => run_clean(config, clean.repositories, output),
    }
}

fn run_list(output: &mut Output<'_>) -> Result<Completion, AppError> {
    let entries = api::cache_list()?;
    let mut table = Table::new(["URL", "SIZE", "UPDATED"]);
    for entry in &entries {
        table.push_row(vec![
            Cell::new(safe_message(entry.url()), Paint::Bold),
            Cell::new(format_size(entry.size_bytes()), Paint::Dimmed),
            Cell::new(updated(entry), Paint::Dimmed),
        ]);
    }
    table.render(output)?;
    Ok(Completion::Success)
}

fn updated(entry: &EntryInfo) -> String {
    match entry.modified().and_then(format_age) {
        Some(age) => format!("{age} ago"),
        None => "-".to_string(),
    }
}

fn run_clean(
    config: Option<PathBuf>,
    repositories: Vec<String>,
    output: &mut Output<'_>,
) -> Result<Completion, AppError> {
    let report = api::cache_clean(config, repositories)?;
    let removed = report.removed();
    let noun = if removed == 1 { "entry" } else { "entries" };
    output.stdout(format_args!("Removed {removed} cache {noun}\n"))?;
    for absent in report.absent() {
        output.stdout(format_args!(
            " {} {}\n",
            "=".cyan(),
            format!("{} was not cached", safe_message(absent)).dimmed()
        ))?;
    }
    Ok(Completion::Success)
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

fn format_age(modified: SystemTime) -> Option<String> {
    let elapsed = SystemTime::now().duration_since(modified).ok()?.as_secs();
    let age = if elapsed < 60 {
        format!("{elapsed}s")
    } else if elapsed < 3600 {
        format!("{}m", elapsed / 60)
    } else if elapsed < 86400 {
        format!("{}h", elapsed / 3600)
    } else {
        format!("{}d", elapsed / 86400)
    };
    Some(age)
}
