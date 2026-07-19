mod progress;

use std::fmt::{Arguments, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use clap::Args;
use owo_colors::OwoColorize;

use crate::AppError;
use crate::app::api;
use crate::app::sync::{BlockedReasonDetails, Outcome, Phase, PhaseSummary, Plan, Report};

use self::progress::Display;
use super::printer::Printer;

#[derive(Args)]
pub(super) struct SyncCommand {
    #[arg(value_name = "repo")]
    repositories: Vec<String>,

    #[arg(long)]
    dry_run: bool,
}

pub(super) fn run(config: Option<PathBuf>, command: SyncCommand) -> Result<(), AppError> {
    let printer = Printer::Default;
    let report = if command.dry_run {
        api::sync(config, command.repositories, true)?
    } else {
        run_with_progress(config, command.repositories, printer)?
    };

    print_report(&report, command.dry_run, printer);

    if report.has_failures() {
        std::process::exit(1);
    }

    Ok(())
}

fn run_with_progress(
    config: Option<PathBuf>,
    repositories: Vec<String>,
    printer: Printer,
) -> Result<Report, AppError> {
    let (sender, receiver) = mpsc::channel();

    std::thread::scope(|scope| {
        let execution =
            scope.spawn(move || api::sync_with_events(config, repositories, false, &sender));
        let mut progress = Display::new(printer);

        for event in receiver {
            if let Some(completion) = progress.handle(event) {
                print_phase_completion(completion.phase(), completion.summary(), printer);
            }
        }

        progress.finish();
        execution.join().expect("sync execution thread should not panic")
    })
}

fn print_report(report: &Report, dry_run: bool, printer: Printer) {
    if dry_run {
        print_dry_run_summary(report, printer);
    }

    print_count("Skipped", report.skipped(), printer);
    print_count("Blocked", report.blocked(), printer);
    print_entries(report, printer);
}

fn print_phase_completion(phase: Phase, summary: PhaseSummary, printer: Printer) {
    match phase {
        Phase::Checking => print_phase("Checked", summary, printer),
        Phase::Preparing => print_count_with_elapsed("Prepared", summary, true, printer),
        Phase::Updating => print_count_with_elapsed("Updated", summary, false, printer),
    }
}

fn print_dry_run_summary(report: &Report, printer: Printer) {
    print_count("Would clone", report.planned_clones(), printer);
    print_count("Would fetch", report.planned_fetches(), printer);
    if report.planned_clones() == 0 && report.planned_fetches() == 0 && !report.has_failures() {
        write_line(printer, format_args!("Would make no changes"));
    }
}

fn print_entries(report: &Report, printer: Printer) {
    let mut entries = report.entries().iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        left.repository()
            .cmp(right.repository())
            .then_with(|| change_rank(left.outcome()).cmp(&change_rank(right.outcome())))
    });

    for entry in entries {
        match entry.outcome() {
            Outcome::Planned(Plan::Clone { url }) => {
                write_line(
                    printer,
                    format_args!(
                        " {} {} {}",
                        "+".green(),
                        entry.repository().bold(),
                        format!("from {url}").dimmed()
                    ),
                );
            }
            Outcome::Planned(Plan::Fetch { .. }) | Outcome::Current { .. } => {}
            Outcome::Cloned { url } => {
                write_line(
                    printer,
                    format_args!(
                        " {} {} {}",
                        "+".green(),
                        entry.repository().bold(),
                        format!("from {url}").dimmed()
                    ),
                );
            }
            Outcome::Updated { branch, before, after } => {
                write_line(
                    printer,
                    format_args!(
                        " {} {} {}",
                        "~".yellow(),
                        entry.repository().bold(),
                        format!("{branch} {before} -> {after}").dimmed()
                    ),
                );
            }
            Outcome::Skipped { reason } => {
                write_line(
                    printer,
                    format_args!(
                        " {} {} {}",
                        "!".yellow(),
                        entry.repository().bold(),
                        reason.message().dimmed()
                    ),
                );
            }
            Outcome::Blocked { reason } => {
                write_line(
                    printer,
                    format_args!(
                        " {} {} {}",
                        "x".red(),
                        entry.repository().bold(),
                        reason.message().dimmed()
                    ),
                );
                print_blocked_details(entry.blocked_details(), printer);
            }
        }
    }
}

fn print_blocked_details(details: Option<&BlockedReasonDetails>, printer: Printer) {
    if let Some(BlockedReasonDetails::RemoteUrlMismatch { actual, expected }) = details {
        write_line(
            printer,
            format_args!("    {}", format!("actual:   {}", display_url(actual)).dimmed()),
        );
        write_line(
            printer,
            format_args!("    {}", format!("expected: {}", display_url(expected)).dimmed()),
        );
    }
}

fn display_url(url: &str) -> String {
    redact_secret_query_parameters(&redact_http_userinfo(&escape_control_characters(url)))
}

fn escape_control_characters(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        if character.is_control() {
            escaped.extend(character.escape_default());
        } else {
            escaped.push(character);
        }
    }
    escaped
}

fn redact_http_userinfo(value: &str) -> String {
    let Some(scheme_end) = value.find("://") else {
        return value.to_string();
    };
    let scheme = value[..scheme_end].to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return value.to_string();
    }

    let authority_start = scheme_end + 3;
    let authority = &value[authority_start..];
    let authority_end =
        authority.find(|character| ['/', '?', '#'].contains(&character)).unwrap_or(authority.len());
    let Some(userinfo_end) = authority[..authority_end].rfind('@') else {
        return value.to_string();
    };

    format!(
        "{}[redacted]@{}",
        &value[..authority_start],
        &value[authority_start + userinfo_end + 1..]
    )
}

fn redact_secret_query_parameters(value: &str) -> String {
    let Some(query_start) = value.find('?') else {
        return value.to_string();
    };
    let query_value_start = query_start + 1;
    let fragment_start = value[query_value_start..]
        .find('#')
        .map(|index| query_value_start + index)
        .unwrap_or(value.len());
    let query = &value[query_value_start..fragment_start];
    let mut redacted = String::from(&value[..query_value_start]);

    for (index, parameter) in query.split('&').enumerate() {
        if index > 0 {
            redacted.push('&');
        }

        let (key, has_value) =
            parameter.split_once('=').map_or((parameter, false), |(key, _)| (key, true));
        if has_value && is_secret_query_key(key) {
            redacted.push_str(key);
            redacted.push_str("=[redacted]");
        } else {
            redacted.push_str(parameter);
        }
    }

    redacted.push_str(&value[fragment_start..]);
    redacted
}

fn is_secret_query_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    normalized.contains("token")
        || normalized.contains("password")
        || normalized.contains("passwd")
        || normalized.contains("secret")
        || normalized.contains("auth")
        || normalized == "key"
        || normalized.ends_with("_key")
        || normalized.ends_with("-key")
}

fn print_phase(label: &str, summary: PhaseSummary, printer: Printer) {
    if summary.count() == 0 {
        write_line(
            printer,
            format_args!(
                "{}",
                format!("{label} in {}", format_duration(summary.elapsed())).dimmed()
            ),
        );
    } else {
        print_count_with_elapsed(label, summary, true, printer);
    }
}

fn print_count_with_elapsed(label: &str, summary: PhaseSummary, show_zero: bool, printer: Printer) {
    if summary.count() == 0 && !show_zero {
        return;
    }

    write_line(
        printer,
        format_args!(
            "{}",
            format!(
                "{label} {} {}",
                repositories(summary.count()).bold(),
                format!("in {}", format_duration(summary.elapsed())).dimmed()
            )
            .dimmed()
        ),
    );
}

fn print_count(label: &str, count: usize, printer: Printer) {
    if count > 0 {
        write_line(
            printer,
            format_args!("{}", format!("{label} {}", repositories(count).bold()).dimmed()),
        );
    }
}

fn write_line(printer: Printer, arguments: Arguments<'_>) {
    let mut stderr = printer.stderr();
    let _ = writeln!(stderr, "{arguments}");
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

fn change_rank(outcome: &Outcome) -> u8 {
    match outcome {
        Outcome::Planned(Plan::Clone { .. }) | Outcome::Cloned { .. } => 0,
        Outcome::Updated { .. } => 1,
        Outcome::Skipped { .. } => 2,
        Outcome::Blocked { .. } => 3,
        Outcome::Planned(Plan::Fetch { .. }) | Outcome::Current { .. } => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::display_url;

    #[test]
    fn display_url_redacts_http_userinfo_and_secret_query_values() {
        assert_eq!(
            display_url(
                "https://user:ghp_secret@example.com/org/repo.git?access_token=token&branch=main&api_key=key"
            ),
            "https://[redacted]@example.com/org/repo.git?access_token=[redacted]&branch=main&api_key=[redacted]"
        );
    }

    #[test]
    fn display_url_escapes_control_characters() {
        let displayed = display_url("https://example.com/org/repo.git\n\t\u{1b}[31m");

        assert_eq!(displayed, "https://example.com/org/repo.git\\n\\t\\u{1b}[31m");
        assert!(!displayed.chars().any(char::is_control));
    }
}
