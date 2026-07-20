use std::fmt::Arguments;
use std::io;
use std::time::Duration;

use owo_colors::OwoColorize;

use crate::git::redact_url_for_display;

use super::output::{Output, terminal_text};

pub(super) fn print_phase(
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

pub(super) fn print_count_with_elapsed(
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

pub(super) fn print_count(label: &str, count: usize, output: &mut Output<'_>) -> io::Result<()> {
    if count > 0 {
        write_line(
            output,
            format_args!("{}", format!("{label} {}", repositories(count).bold()).dimmed()),
        )?;
    }
    Ok(())
}

pub(super) fn write_line(output: &mut Output<'_>, arguments: Arguments<'_>) -> io::Result<()> {
    output.stderr(format_args!("{arguments}\n"))
}

pub(super) fn safe_message(value: &str) -> String {
    terminal_text(&redact_url_for_display(value))
}

fn repositories(count: usize) -> String {
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
