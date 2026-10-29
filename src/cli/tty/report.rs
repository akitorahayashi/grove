use std::fmt::Arguments;
use std::io;
use std::time::Duration;

use owo_colors::OwoColorize;

use crate::app::entry::BlockedReasonDetails;
use crate::cache::Outcome as CacheOutcome;
use crate::removal;
use crate::repositories::redact_urls_for_display;

use crate::cli::output::{Output, terminal_text};
use crate::cli::tty::table::Paint;

pub(in crate::cli) fn cache_annotation(cache: CacheOutcome) -> &'static str {
    match cache {
        CacheOutcome::Miss => "(cached)",
        CacheOutcome::Hit => "(from cache)",
        CacheOutcome::Rebuilt => "(cache rebuilt)",
        CacheOutcome::Retargeted => "(cache retargeted)",
    }
}

pub(in crate::cli) fn print_phase(
    label: &str,
    count: usize,
    elapsed: Duration,
    output: &mut Output<'_>,
) -> io::Result<()> {
    if count == 0 {
        write_line(
            output,
            format_args!("{}", format!("{label} in {}", format_duration(elapsed)).dimmed()),
        )
    } else {
        print_count_with_elapsed(label, count, elapsed, true, output)
    }
}

pub(in crate::cli) fn print_count_with_elapsed(
    label: &str,
    count: usize,
    elapsed: Duration,
    show_zero: bool,
    output: &mut Output<'_>,
) -> io::Result<()> {
    if count == 0 && !show_zero {
        return Ok(());
    }

    write_line(
        output,
        format_args!(
            "{}",
            format!(
                "{label} {} {}",
                repositories(count).bold(),
                format!("in {}", format_duration(elapsed)).dimmed()
            )
            .dimmed()
        ),
    )
}

pub(in crate::cli) fn print_count(
    label: &str,
    count: usize,
    output: &mut Output<'_>,
) -> io::Result<()> {
    if count > 0 {
        write_line(
            output,
            format_args!("{}", format!("{label} {}", repositories(count).bold()).dimmed()),
        )?;
    }
    Ok(())
}

pub(in crate::cli) fn write_line(
    output: &mut Output<'_>,
    arguments: Arguments<'_>,
) -> io::Result<()> {
    output.report(format_args!("{arguments}\n"))
}

pub(in crate::cli) fn safe_message(value: &str) -> String {
    terminal_text(&redact_urls_for_display(value))
}

/// One report line: a colored marker, the bold repository, and a dimmed
/// detail. Owns the escaping of the raw repository name; `detail` arrives
/// display-ready because callers choose between `terminal_text` (local text)
/// and `safe_message` (text that may echo remote URLs).
pub(in crate::cli) fn entry_line(
    output: &mut Output<'_>,
    marker: &str,
    paint: Paint,
    repository: &str,
    detail: &str,
) -> io::Result<()> {
    write_line(
        output,
        format_args!(
            " {} {} {}",
            paint.apply(marker),
            terminal_text(repository).bold(),
            detail.dimmed()
        ),
    )
}

pub(in crate::cli) fn print_blocked_details(
    details: Option<&BlockedReasonDetails>,
    output: &mut Output<'_>,
) -> io::Result<()> {
    if let Some(BlockedReasonDetails::RemoteUrlMismatch { actual, expected }) = details {
        write_line(
            output,
            format_args!("    {}", format!("actual:   {}", safe_message(actual)).dimmed()),
        )?;
        write_line(
            output,
            format_args!("    {}", format!("expected: {}", safe_message(expected)).dimmed()),
        )?;
    }
    Ok(())
}

pub(in crate::cli) fn print_removals(
    entries: &[removal::Entry],
    output: &mut Output<'_>,
) -> io::Result<()> {
    let mut entries = entries.iter().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.repository());

    for entry in entries {
        print_removal(entry, output)?;
    }
    Ok(())
}

fn print_removal(entry: &removal::Entry, output: &mut Output<'_>) -> io::Result<()> {
    let repository = entry.repository();
    match entry.outcome() {
        removal::Outcome::Planned(removal::Plan::Move { to }) => {
            let detail = terminal_text(&format!("would move to {to}"));
            entry_line(output, ">", Paint::Cyan, repository, &detail)?;
        }
        removal::Outcome::Planned(removal::Plan::Trash) => {
            entry_line(output, "-", Paint::Cyan, repository, "would move to the trash")?;
        }
        removal::Outcome::Moved { to } => {
            let detail = terminal_text(&format!("moved to {to}"));
            entry_line(output, ">", Paint::Green, repository, &detail)?;
        }
        removal::Outcome::Trashed => {
            entry_line(output, "-", Paint::Green, repository, "moved to the trash")?;
        }
        removal::Outcome::Blocked { reason } => {
            entry_line(output, "x", Paint::Red, repository, &safe_message(&reason.message()))?;
        }
        removal::Outcome::Absent => {
            write_line(
                output,
                format_args!(" {} {}", "=".cyan(), terminal_text(repository).bold()),
            )?;
        }
    }

    let cache = match entry.cache() {
        removal::CacheChange::Removed => Some("clone cache entry removed"),
        removal::CacheChange::WouldRemove => Some("clone cache entry would be removed"),
        _ => None,
    };
    if let Some(cache) = cache {
        write_line(output, format_args!("    {}", cache.dimmed()))?;
    }
    if let Some(warning) = entry.warning() {
        write_line(output, format_args!("    {}", safe_message(warning).dimmed()))?;
    }
    Ok(())
}

pub(in crate::cli) fn print_cache_drop(
    entry: &removal::CacheDrop,
    output: &mut Output<'_>,
) -> io::Result<()> {
    let identity = entry.identity();
    if let Some(warning) = entry.warning() {
        let detail = safe_message(&format!("clone cache entry not removed: {warning}"));
        return entry_line(output, "!", Paint::Yellow, identity, &detail);
    }
    match entry.change() {
        removal::CacheChange::WouldRemove => {
            entry_line(output, "-", Paint::Cyan, identity, "clone cache entry would be removed")
        }
        removal::CacheChange::Removed => {
            entry_line(output, "-", Paint::Green, identity, "clone cache entry removed")
        }
        _ => Ok(()),
    }
}

pub(in crate::cli) fn repositories(count: usize) -> String {
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
