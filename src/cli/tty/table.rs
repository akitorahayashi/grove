use std::io;

use owo_colors::OwoColorize;

use crate::cli::output::Output;

const COLUMN_GAP: &str = "  ";

/// Color applied to a rendered cell when the destination is a styled terminal.
#[derive(Debug, Clone, Copy)]
pub(in crate::cli) enum Paint {
    Bold,
    Dimmed,
    Cyan,
    Blue,
    Green,
    Yellow,
    Red,
}

/// A single cell: display-ready text paired with the color it carries.
///
/// Text is expected to already be escaped for terminal display; the table only
/// measures, pads, and colors it.
#[derive(Debug)]
pub(in crate::cli) struct Cell {
    text: String,
    paint: Paint,
}

impl Cell {
    pub(in crate::cli) fn new(text: impl Into<String>, paint: Paint) -> Self {
        Self { text: text.into(), paint }
    }
}

/// A column-aligned table with a colored header and a dimmed rule.
#[derive(Debug)]
pub(in crate::cli) struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<Cell>>,
}

impl Table {
    pub(in crate::cli) fn new(headers: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self { headers: headers.into_iter().map(Into::into).collect(), rows: Vec::new() }
    }

    pub(in crate::cli) fn push_row(&mut self, cells: Vec<Cell>) {
        debug_assert_eq!(
            cells.len(),
            self.headers.len(),
            "table row must have one cell per header column"
        );
        self.rows.push(cells);
    }

    pub(in crate::cli) fn render(&self, styled: bool, output: &mut Output<'_>) -> io::Result<()> {
        let widths = self.column_widths();

        output.stdout(format_args!("\n"))?;
        self.render_header(&widths, styled, output)?;
        self.render_rule(&widths, styled, output)?;
        for row in &self.rows {
            render_line(
                row.iter().map(|cell| (cell.text.as_str(), cell.paint)),
                &widths,
                styled,
                output,
            )?;
        }
        Ok(())
    }

    fn column_widths(&self) -> Vec<usize> {
        let mut widths: Vec<usize> =
            self.headers.iter().map(|header| header.chars().count()).collect();
        for row in &self.rows {
            for (width, cell) in widths.iter_mut().zip(row) {
                *width = (*width).max(cell.text.chars().count());
            }
        }
        widths
    }

    fn render_header(
        &self,
        widths: &[usize],
        styled: bool,
        output: &mut Output<'_>,
    ) -> io::Result<()> {
        let cells = self.headers.iter().zip(widths).map(|(header, &width)| {
            let padded = format!("{header:<width$}");
            if styled { padded.yellow().bold().to_string() } else { padded }
        });
        write_row(cells, output)
    }

    fn render_rule(
        &self,
        widths: &[usize],
        styled: bool,
        output: &mut Output<'_>,
    ) -> io::Result<()> {
        let span =
            widths.iter().sum::<usize>() + COLUMN_GAP.len() * self.headers.len().saturating_sub(1);
        let rule = "-".repeat(span);
        if styled {
            output.stdout(format_args!("{}\n", rule.dimmed()))
        } else {
            output.stdout(format_args!("{rule}\n"))
        }
    }
}

fn render_line<'a>(
    cells: impl Iterator<Item = (&'a str, Paint)>,
    widths: &[usize],
    styled: bool,
    output: &mut Output<'_>,
) -> io::Result<()> {
    let rendered = cells.zip(widths).map(|((text, paint), &width)| {
        let padded = format!("{text:<width$}");
        apply(padded, paint, styled)
    });
    write_row(rendered, output)
}

fn write_row(cells: impl Iterator<Item = String>, output: &mut Output<'_>) -> io::Result<()> {
    let line = cells.collect::<Vec<_>>().join(COLUMN_GAP);
    output.stdout(format_args!("{line}\n"))
}

fn apply(text: String, paint: Paint, styled: bool) -> String {
    if !styled {
        return text;
    }
    match paint {
        Paint::Bold => text.bold().to_string(),
        Paint::Dimmed => text.dimmed().to_string(),
        Paint::Cyan => text.cyan().to_string(),
        Paint::Blue => text.blue().to_string(),
        Paint::Green => text.green().to_string(),
        Paint::Yellow => text.yellow().to_string(),
        Paint::Red => text.red().to_string(),
    }
}
