use std::io::{self, IsTerminal};
use std::path::PathBuf;

use clap::Args;
use owo_colors::OwoColorize;

use crate::AppError;
use crate::app::api;
use crate::app::status::{StatusCondition, StatusEntry};
use crate::git::redact_url_for_display;
use crate::repositories::BranchTracking;

#[derive(Args)]
pub(super) struct StatusCommand {
    #[arg(value_name = "repo")]
    repositories: Vec<String>,

    #[arg(long)]
    fetch: bool,
}

pub(super) fn run(config: Option<PathBuf>, command: StatusCommand) -> Result<(), AppError> {
    let show_detail = command.repositories.len() == 1;
    let report = api::status(config, command.repositories, command.fetch)?;
    let entries = report.entries();
    let styled = io::stdout().is_terminal();

    if show_detail && entries.len() == 1 {
        print_detail(&entries[0], styled);
    } else {
        print_table(entries, styled);
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct ColumnWidths {
    name: usize,
    repository: usize,
    branch: usize,
    state: usize,
    default_branch: usize,
}

impl ColumnWidths {
    fn for_entries(entries: &[StatusEntry]) -> Self {
        let mut widths = Self {
            name: "NAME".len(),
            repository: "REPOSITORY".len(),
            branch: "BRANCH".len(),
            state: "STATE".len(),
            default_branch: "DEFAULT".len(),
        };

        for entry in entries {
            widths.name = widths.name.max(entry.name().len());
            widths.repository = widths.repository.max(entry.display_path().len());
            widths.branch = widths.branch.max(branch(entry).len());
            widths.state = widths.state.max(entry.condition().as_str().len());
            widths.default_branch = widths.default_branch.max(default_branch(entry).len());
        }

        widths
    }

    fn separator_len(self) -> usize {
        self.name + self.repository + self.branch + self.state + self.default_branch + 8
    }
}

fn print_table(entries: &[StatusEntry], styled: bool) {
    let widths = ColumnWidths::for_entries(entries);

    println!();
    print_header(widths, styled);
    print_separator(widths, styled);
    for entry in entries {
        print_row(entry, widths, styled);
    }
}

fn print_header(widths: ColumnWidths, styled: bool) {
    let name = format_cell("NAME", widths.name);
    let repository = format_cell("REPOSITORY", widths.repository);
    let branch = format_cell("BRANCH", widths.branch);
    let state = format_cell("STATE", widths.state);
    let default_branch = format_cell("DEFAULT", widths.default_branch);

    if styled {
        println!(
            "{}  {}  {}  {}  {}",
            name.yellow().bold(),
            repository.yellow().bold(),
            branch.yellow().bold(),
            state.yellow().bold(),
            default_branch.yellow().bold()
        );
    } else {
        println!("{name}  {repository}  {branch}  {state}  {default_branch}");
    }
}

fn print_separator(widths: ColumnWidths, styled: bool) {
    let separator = "-".repeat(widths.separator_len());
    if styled {
        println!("{}", separator.dimmed());
    } else {
        println!("{separator}");
    }
}

fn print_row(entry: &StatusEntry, widths: ColumnWidths, styled: bool) {
    let name = format_cell(entry.name(), widths.name);
    let repository = format_cell(entry.display_path(), widths.repository);
    let branch = format_cell(&branch(entry), widths.branch);
    let state = format_cell(entry.condition().as_str(), widths.state);
    let default_branch = format_cell(&default_branch(entry), widths.default_branch);

    if styled {
        println!(
            "{}  {}  {}  {}  {}",
            name.bold(),
            repository.cyan(),
            branch.blue(),
            format_state(&state, entry.condition()),
            default_branch.dimmed()
        );
    } else {
        println!("{name}  {repository}  {branch}  {state}  {default_branch}");
    }
}

fn print_detail(entry: &StatusEntry, styled: bool) {
    println!();
    print_title(entry.name(), styled);
    print_detail_separator(entry.name().len(), styled);
    print_section("Repository", styled);
    print_field("Path", entry.display_path(), styled);
    print_field("Absolute path", entry.absolute_path(), styled);
    print_field("URL", &redact_url_for_display(entry.url()), styled);
    print_field("Config", entry.source_config(), styled);

    println!();
    print_section("Status", styled);
    print_field("State", entry.condition().as_str(), styled);
    print_field("Branch", &branch(entry), styled);
    print_field("Default", &default_branch_name(entry), styled);
    print_field("Tracking", &tracking(entry), styled);

    if entry.condition().message().is_some() || entry.remote_mismatch().is_some() {
        println!();
        print_section("Diagnostics", styled);
        if let Some(message) = entry.condition().message() {
            print_field("Reason", message, styled);
        }
        if let Some(mismatch) = entry.remote_mismatch() {
            print_diagnostic_line("Remote URL does not match grove.toml", styled);
            print_diagnostic_field("Actual", &redact_url_for_display(mismatch.actual()), styled);
            print_diagnostic_field(
                "Expected",
                &redact_url_for_display(mismatch.expected()),
                styled,
            );
        }
    }
}

fn print_title(title: &str, styled: bool) {
    if styled {
        println!("{}", title.bold());
    } else {
        println!("{title}");
    }
}

fn print_detail_separator(width: usize, styled: bool) {
    let separator = "-".repeat(width.max(4));
    if styled {
        println!("{}", separator.dimmed());
    } else {
        println!("{separator}");
    }
}

fn print_section(section: &str, styled: bool) {
    if styled {
        println!("{}", section.yellow().bold());
    } else {
        println!("{section}");
    }
}

fn print_field(label: &str, value: &str, styled: bool) {
    if styled {
        println!("  {}  {}", format!("{label}:").dimmed(), value);
    } else {
        println!("  {label:<14} {value}", label = format!("{label}:"));
    }
}

fn print_diagnostic_line(value: &str, styled: bool) {
    if styled {
        println!("  {}", value.red());
    } else {
        println!("  {value}");
    }
}

fn print_diagnostic_field(label: &str, value: &str, styled: bool) {
    if styled {
        println!("    {} {}", format!("{label}:").dimmed(), value);
    } else {
        println!("    {label:<9} {value}", label = format!("{label}:"));
    }
}

fn format_cell(value: &str, width: usize) -> String {
    format!("{value:<width$}")
}

fn branch(entry: &StatusEntry) -> String {
    entry.branch().unwrap_or("-").to_string()
}

fn default_branch(entry: &StatusEntry) -> String {
    entry.default_branch().map(format_tracking).unwrap_or_else(|| "-".to_string())
}

fn default_branch_name(entry: &StatusEntry) -> String {
    entry
        .default_branch()
        .map(|tracking| tracking.branch().to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn tracking(entry: &StatusEntry) -> String {
    let Some(tracking) = entry.default_branch() else {
        return "-".to_string();
    };
    if tracking.ahead() == 0 && tracking.behind() == 0 {
        return "up to date".to_string();
    }
    format_tracking(tracking)
}

fn format_tracking(tracking: &BranchTracking) -> String {
    let mut parts = vec![tracking.branch().to_string()];
    if tracking.ahead() > 0 {
        parts.push(format!("ahead {}", tracking.ahead()));
    }
    if tracking.behind() > 0 {
        parts.push(format!("behind {}", tracking.behind()));
    }
    parts.join(" ")
}

fn format_state(padded: &str, condition: &StatusCondition) -> String {
    match condition {
        StatusCondition::Clean => padded.green().to_string(),
        StatusCondition::Dirty => padded.yellow().to_string(),
        StatusCondition::Missing
        | StatusCondition::Invalid(_)
        | StatusCondition::RemoteMismatch
        | StatusCondition::FetchFailed(_) => padded.red().to_string(),
    }
}
