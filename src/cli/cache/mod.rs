use std::io;
use std::path::PathBuf;
use std::time::SystemTime;

use clap::{Args, Subcommand};
use owo_colors::OwoColorize;

use crate::AppError;
use crate::app::api;
use crate::app::cache::CacheEntryInfo;

use super::Completion;
use super::output::Output;
use super::terminal_report::safe_message;

#[derive(Args)]
pub(super) struct CacheCommand {
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

pub(super) fn run(
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
    if entries.is_empty() {
        output.stdout(format_args!("No cached repositories\n"))?;
        return Ok(Completion::Success);
    }

    for entry in &entries {
        print_entry(entry, output)?;
    }
    Ok(Completion::Success)
}

fn print_entry(entry: &CacheEntryInfo, output: &mut Output<'_>) -> io::Result<()> {
    let detail = match entry.modified().and_then(format_age) {
        Some(age) => format!("{}, updated {age} ago", format_size(entry.size_bytes())),
        None => format_size(entry.size_bytes()),
    };
    output.stdout(format_args!(" {} {}\n", safe_message(entry.url()).bold(), detail.dimmed()))
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
