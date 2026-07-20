use std::io;
use std::path::PathBuf;

use clap::Args;
use owo_colors::OwoColorize;

use crate::AppError;
use crate::app::api;
use crate::app::status::{BranchTrackingStatus, DefaultBranchStatus, StatusCondition, StatusEntry};

use super::Completion;
use super::output::{Output, terminal_text};

#[derive(Args)]
pub(super) struct StatusCommand {
    #[arg(value_name = "repo")]
    repositories: Vec<String>,

    #[arg(long)]
    fetch: bool,
}

pub(super) fn run(
    config: Option<PathBuf>,
    command: StatusCommand,
    output: &mut Output<'_>,
) -> Result<Completion, AppError> {
    let show_detail = command.repositories.len() == 1;
    let report = api::status(config, command.repositories, command.fetch)?;
    let entries = report.entries();
    let styled = output.stdout_is_terminal();

    if show_detail && entries.len() == 1 {
        print_detail(&entries[0], styled, output)?;
    } else {
        print_table(entries, styled, output)?;
    }

    Ok(Completion::Success)
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
            widths.name = widths.name.max(terminal_text(entry.name()).len());
            widths.repository = widths.repository.max(terminal_text(entry.display_path()).len());
            widths.branch = widths.branch.max(branch(entry).len());
            widths.state = widths.state.max(table_state(entry).len());
            widths.default_branch = widths.default_branch.max(default_branch(entry).len());
        }

        widths
    }

    fn separator_len(self) -> usize {
        self.name + self.repository + self.branch + self.state + self.default_branch + 8
    }
}

fn print_table(entries: &[StatusEntry], styled: bool, output: &mut Output<'_>) -> io::Result<()> {
    let widths = ColumnWidths::for_entries(entries);

    output.stdout(format_args!("\n"))?;
    print_header(widths, styled, output)?;
    print_separator(widths, styled, output)?;
    for entry in entries {
        print_row(entry, widths, styled, output)?;
    }
    Ok(())
}

fn print_header(widths: ColumnWidths, styled: bool, output: &mut Output<'_>) -> io::Result<()> {
    let name = format_cell("NAME", widths.name);
    let repository = format_cell("REPOSITORY", widths.repository);
    let branch = format_cell("BRANCH", widths.branch);
    let state = format_cell("STATE", widths.state);
    let default_branch = format_cell("DEFAULT", widths.default_branch);

    if styled {
        output.stdout(format_args!(
            "{}  {}  {}  {}  {}",
            name.yellow().bold(),
            repository.yellow().bold(),
            branch.yellow().bold(),
            state.yellow().bold(),
            default_branch.yellow().bold()
        ))?;
    } else {
        output.stdout(format_args!("{name}  {repository}  {branch}  {state}  {default_branch}"))?;
    }
    output.stdout(format_args!("\n"))
}

fn print_separator(widths: ColumnWidths, styled: bool, output: &mut Output<'_>) -> io::Result<()> {
    let separator = "-".repeat(widths.separator_len());
    if styled {
        output.stdout(format_args!("{}\n", separator.dimmed()))
    } else {
        output.stdout(format_args!("{separator}\n"))
    }
}

fn print_row(
    entry: &StatusEntry,
    widths: ColumnWidths,
    styled: bool,
    output: &mut Output<'_>,
) -> io::Result<()> {
    let name = format_cell(&terminal_text(entry.name()), widths.name);
    let repository = format_cell(&terminal_text(entry.display_path()), widths.repository);
    let branch = format_cell(&branch(entry), widths.branch);
    let state = format_cell(&table_state(entry), widths.state);
    let default_branch = format_cell(&default_branch(entry), widths.default_branch);

    if styled {
        output.stdout(format_args!(
            "{}  {}  {}  {}  {}",
            name.bold(),
            repository.cyan(),
            branch.blue(),
            format_state(&state, entry.condition()),
            default_branch.dimmed()
        ))?;
    } else {
        output.stdout(format_args!("{name}  {repository}  {branch}  {state}  {default_branch}"))?;
    }
    output.stdout(format_args!("\n"))
}

fn print_detail(entry: &StatusEntry, styled: bool, output: &mut Output<'_>) -> io::Result<()> {
    let name = terminal_text(entry.name());
    output.stdout(format_args!("\n"))?;
    print_title(&name, styled, output)?;
    print_detail_separator(name.len(), styled, output)?;
    print_section("Repository", styled, output)?;
    print_field("Path", &terminal_text(entry.display_path()), styled, output)?;
    print_field("Absolute path", &terminal_text(entry.absolute_path()), styled, output)?;
    print_field("URL", &terminal_text(entry.url()), styled, output)?;
    print_field("Config", &terminal_text(entry.source_config()), styled, output)?;

    output.stdout(format_args!("\n"))?;
    print_section("Status", styled, output)?;
    print_field("State", entry.condition().as_str(), styled, output)?;
    print_field("Branch", &branch(entry), styled, output)?;
    print_field("Default", &default_branch_name(entry), styled, output)?;
    print_field("Tracking", &tracking(entry), styled, output)?;

    if entry.condition().message().is_some() || entry.remote_mismatch().is_some() {
        output.stdout(format_args!("\n"))?;
        print_section("Diagnostics", styled, output)?;
        if let Some(message) = entry.condition().message() {
            print_field("Reason", &terminal_text(message), styled, output)?;
        }
        if let Some(mismatch) = entry.remote_mismatch() {
            print_diagnostic_line("Remote URL does not match grove.toml", styled, output)?;
            print_diagnostic_field("Actual", &terminal_text(mismatch.actual()), styled, output)?;
            print_diagnostic_field(
                "Expected",
                &terminal_text(mismatch.expected()),
                styled,
                output,
            )?;
        }
    }
    Ok(())
}

fn print_title(title: &str, styled: bool, output: &mut Output<'_>) -> io::Result<()> {
    if styled {
        output.stdout(format_args!("{}\n", title.bold()))
    } else {
        output.stdout(format_args!("{title}\n"))
    }
}

fn print_detail_separator(width: usize, styled: bool, output: &mut Output<'_>) -> io::Result<()> {
    let separator = "-".repeat(width.max(4));
    if styled {
        output.stdout(format_args!("{}\n", separator.dimmed()))
    } else {
        output.stdout(format_args!("{separator}\n"))
    }
}

fn print_section(section: &str, styled: bool, output: &mut Output<'_>) -> io::Result<()> {
    if styled {
        output.stdout(format_args!("{}\n", section.yellow().bold()))
    } else {
        output.stdout(format_args!("{section}\n"))
    }
}

fn print_field(label: &str, value: &str, styled: bool, output: &mut Output<'_>) -> io::Result<()> {
    if styled {
        output.stdout(format_args!("  {}  {}\n", format!("{label}:").dimmed(), value))
    } else {
        output.stdout(format_args!("  {label:<14} {value}\n", label = format!("{label}:")))
    }
}

fn print_diagnostic_line(value: &str, styled: bool, output: &mut Output<'_>) -> io::Result<()> {
    if styled {
        output.stdout(format_args!("  {}\n", value.red()))
    } else {
        output.stdout(format_args!("  {value}\n"))
    }
}

fn print_diagnostic_field(
    label: &str,
    value: &str,
    styled: bool,
    output: &mut Output<'_>,
) -> io::Result<()> {
    if styled {
        output.stdout(format_args!("    {} {}\n", format!("{label}:").dimmed(), value))
    } else {
        output.stdout(format_args!("    {label:<9} {value}\n", label = format!("{label}:")))
    }
}

fn format_cell(value: &str, width: usize) -> String {
    format!("{value:<width$}")
}

fn branch(entry: &StatusEntry) -> String {
    terminal_text(entry.branch().unwrap_or("-"))
}

fn default_branch(entry: &StatusEntry) -> String {
    entry.default_branch().map(format_default_branch).unwrap_or_else(|| "-".to_string())
}

fn default_branch_name(entry: &StatusEntry) -> String {
    entry
        .default_branch()
        .map(|status| status.branch().to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn tracking(entry: &StatusEntry) -> String {
    let Some(status) = entry.default_branch() else {
        return "-".to_string();
    };
    format_tracking_status(status.tracking())
}

fn format_default_branch(status: &DefaultBranchStatus) -> String {
    let mut parts = vec![status.branch().to_string()];
    match status.tracking() {
        BranchTrackingStatus::Divergence { ahead, behind } => {
            if *ahead > 0 {
                parts.push(format!("ahead {ahead}"));
            }
            if *behind > 0 {
                parts.push(format!("behind {behind}"));
            }
        }
        BranchTrackingStatus::MissingLocalBranch => parts.push("local missing".to_string()),
        BranchTrackingStatus::MissingRemoteBranch => parts.push("remote missing".to_string()),
    }
    parts.join(" ")
}

fn format_tracking_status(status: &BranchTrackingStatus) -> String {
    match status {
        BranchTrackingStatus::Divergence { ahead: 0, behind: 0 } => "up to date".to_string(),
        BranchTrackingStatus::Divergence { ahead, behind } => {
            let mut parts = Vec::new();
            if *ahead > 0 {
                parts.push(format!("ahead {ahead}"));
            }
            if *behind > 0 {
                parts.push(format!("behind {behind}"));
            }
            parts.join(", ")
        }
        BranchTrackingStatus::MissingLocalBranch => "local branch missing".to_string(),
        BranchTrackingStatus::MissingRemoteBranch => "remote branch missing".to_string(),
    }
}

fn table_state(entry: &StatusEntry) -> String {
    match entry.condition() {
        StatusCondition::FetchFailed(message) => {
            format!("fetch-failed: {}", sanitize_summary(message))
        }
        condition => condition.as_str().to_string(),
    }
}

fn sanitize_summary(value: &str) -> String {
    let escaped = terminal_text(value);
    let single_line = escaped.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::git::redact_url_for_display(&single_line)
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
