use std::fmt::Arguments;
use std::io;
use std::time::Duration;

use owo_colors::OwoColorize;

use crate::app::cache::CacheOutcome;
use crate::app::report::BlockedReasonDetails;
use crate::repositories::redact_urls_for_display;

use crate::cli::output::{Output, terminal_text};

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
    output.stderr(format_args!("{arguments}\n"))
}

pub(in crate::cli) fn safe_message(value: &str) -> String {
    terminal_text(&redact_urls_for_display(value))
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
