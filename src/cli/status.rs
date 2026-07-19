use std::io::{self, IsTerminal};
use std::path::PathBuf;

use clap::Args;
use owo_colors::OwoColorize;

use crate::AppError;
use crate::app::api;
use crate::app::status::StatusRow;

#[derive(Args)]
pub(super) struct StatusCommand {
    #[arg(value_name = "repo")]
    repositories: Vec<String>,

    #[arg(long)]
    fetch: bool,
}

pub(super) fn run(config: Option<PathBuf>, command: StatusCommand) -> Result<(), AppError> {
    let report = api::status(config, command.repositories, command.fetch)?;
    let rows = report.rows();
    let widths = ColumnWidths::for_rows(rows);
    let styled = io::stdout().is_terminal();

    println!();
    print_header(widths, styled);
    print_separator(widths, styled);
    for row in rows {
        print_row(row, widths, styled);
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct ColumnWidths {
    repository: usize,
    branch: usize,
    state: usize,
    default_branch: usize,
}

impl ColumnWidths {
    fn for_rows(rows: &[StatusRow]) -> Self {
        let mut widths = Self {
            repository: "REPOSITORY".len(),
            branch: "BRANCH".len(),
            state: "STATE".len(),
            default_branch: "DEFAULT".len(),
        };

        for row in rows {
            widths.repository = widths.repository.max(row.repository().len());
            widths.branch = widths.branch.max(row.branch().len());
            widths.state = widths.state.max(row.state().len());
            widths.default_branch = widths.default_branch.max(row.default_branch().len());
        }

        widths
    }

    fn separator_len(self) -> usize {
        self.repository + self.branch + self.state + self.default_branch + 6
    }
}

fn print_header(widths: ColumnWidths, styled: bool) {
    let repository = format_cell("REPOSITORY", widths.repository);
    let branch = format_cell("BRANCH", widths.branch);
    let state = format_cell("STATE", widths.state);
    let default_branch = format_cell("DEFAULT", widths.default_branch);

    if styled {
        println!(
            "{}  {}  {}  {}",
            repository.yellow().bold(),
            branch.yellow().bold(),
            state.yellow().bold(),
            default_branch.yellow().bold()
        );
    } else {
        println!("{repository}  {branch}  {state}  {default_branch}");
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

fn print_row(row: &StatusRow, widths: ColumnWidths, styled: bool) {
    let repository = format_cell(row.repository(), widths.repository);
    let branch = format_cell(row.branch(), widths.branch);
    let state = format_cell(row.state(), widths.state);
    let default_branch = format_cell(row.default_branch(), widths.default_branch);

    if styled {
        println!(
            "{}  {}  {}  {}",
            repository.cyan(),
            branch.blue(),
            format_state(&state, row.state()),
            default_branch.dimmed()
        );
    } else {
        println!("{repository}  {branch}  {state}  {default_branch}");
    }
}

fn format_cell(value: &str, width: usize) -> String {
    format!("{value:<width$}")
}

fn format_state(padded: &str, state: &str) -> String {
    match state {
        "clean" => padded.green().to_string(),
        "dirty" => padded.yellow().to_string(),
        "missing" | "invalid" | "remote-mismatch" => padded.red().to_string(),
        state if state.starts_with("fetch-failed") => padded.red().to_string(),
        _ => padded.to_string(),
    }
}
