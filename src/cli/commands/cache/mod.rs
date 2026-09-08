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
    let total = entries.iter().try_fold(0_u64, |total, entry| {
        total
            .checked_add(entry.size())
            .ok_or_else(|| AppError::cache_state("total cache size exceeds the supported range"))
    })?;
    let mut table = Table::new(["URL", "UPDATED", "SIZE"]);
    for entry in &entries {
        table.push_row(vec![
            Cell::new(safe_message(entry.url()), Paint::Bold),
            Cell::new(updated(entry), Paint::Dimmed),
            Cell::new(format_size(entry.size()), Paint::Dimmed),
        ]);
    }
    table.render(output)?;
    let noun = if entries.len() == 1 { "entry" } else { "entries" };
    output.stdout(format_args!("\nTotal: {} {noun}, {}\n", entries.len(), format_size(total)))?;
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
    // Cleaning the whole cache reads no configuration, so it stays usable
    // outside a grove root.
    let config = match repositories.is_empty() {
        true => None,
        false => Some(super::resolve_config(config, output)?),
    };
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

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];

    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64;
    let mut unit = UNITS[0];
    for candidate in UNITS {
        value /= 1024.0;
        unit = candidate;
        if value < 1024.0 || candidate == UNITS[UNITS.len() - 1] {
            break;
        }
    }
    let formatted = format!("{value:.1}");
    format!("{} {unit}", formatted.strip_suffix(".0").unwrap_or(&formatted))
}

#[cfg(test)]
mod tests {
    use super::format_size;

    #[test]
    fn formats_sizes_with_binary_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1 KiB");
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(format_size(10 * 1024), "10 KiB");
        assert_eq!(format_size(1024 * 1024), "1 MiB");
        assert_eq!(format_size(1536 * 1024), "1.5 MiB");
        assert_eq!(format_size(1024_u64.pow(3)), "1 GiB");
        assert_eq!(format_size(1024_u64.pow(4)), "1 TiB");
    }
}
